//! Capas 2, 3 y 4 del ADR 0003. Son salvaguardas contra accidentes, no una
//! frontera de seguridad.

use std::ffi::OsString;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, GetPropertyReply, Window,
};

use crate::error::Fatal;

pub const TOKEN_VAR: &str = "CRIMSON_SANDBOX_TOKEN";
const MARKER_ATOM: &[u8] = b"_CRIMSON_SANDBOX";
const WM_CHECK_ATOM: &[u8] = b"_NET_SUPPORTING_WM_CHECK";

/// Valor esperado de la marca de sandbox. Ausente, vacío o no Unicode se
/// rechaza: la variable es un requisito para arrancar, no una excepción.
pub fn sandbox_token(value: Option<OsString>) -> Result<String, Fatal> {
    value
        .filter(|v| !v.is_empty())
        .and_then(|v| v.into_string().ok())
        .ok_or_else(|| {
            Fatal::Rejected(format!(
                "{TOKEN_VAR} no está definida, está vacía o no es Unicode"
            ))
        })
}

/// Capa 2: la propiedad `_CRIMSON_SANDBOX` de la raíz debe coincidir con el
/// valor esperado.
pub fn check_sandbox_marker(
    conn: &impl Connection,
    root: Window,
    token: &str,
) -> Result<(), Fatal> {
    let rejected = || Fatal::Rejected("la raíz no tiene una marca de sandbox válida".into());

    let Some(atom) = existing_atom(conn, MARKER_ATOM)? else {
        return Err(rejected());
    };
    // Se pide una unidad de 32 bits más que el valor esperado para detectar
    // una propiedad más larga.
    let units = u32::try_from(token.len())
        .map_err(|_| rejected())?
        .div_ceil(4)
        + 1;
    let reply = conn
        .get_property(false, root, atom, AtomEnum::ANY, 0, units)
        .map_err(Fatal::lost)?
        .reply()
        .map_err(|e| Fatal::startup("GetProperty(_CRIMSON_SANDBOX)", e))?;

    if marker_matches(token, &reply) {
        Ok(())
    } else {
        Err(rejected())
    }
}

fn marker_matches(token: &str, reply: &GetPropertyReply) -> bool {
    reply.type_ == u32::from(AtomEnum::STRING)
        && reply.format == 8
        && reply.bytes_after == 0
        && reply.value == token.as_bytes()
}

/// Capa 3: rechazo conservador si hay, o hubo, otro gestor de ventanas.
/// Nunca crea átomos ni modifica nada del servidor.
///
/// - La raíz tiene `_NET_SUPPORTING_WM_CHECK`, aunque sea obsoleta: también
///   la que deja Crimson WM tras una terminación abrupta (ADR 0008). El
///   mensaje indica si la ventana a la que apunta existe, solo como
///   diagnóstico.
/// - Desde F6, la selección `WM_S{pantalla}` (ICCCM §2.8) tiene propietario:
///   detecta también los gestores que no usan EWMH.
pub fn check_no_other_wm(conn: &impl Connection, root: Window, screen: u16) -> Result<(), Fatal> {
    if let Some(atom) = existing_atom(conn, WM_CHECK_ATOM)? {
        let reply = conn
            .get_property(false, root, atom, AtomEnum::ANY, 0, 1)
            .map_err(Fatal::lost)?
            .reply()
            .map_err(|e| Fatal::startup("GetProperty(_NET_SUPPORTING_WM_CHECK)", e))?;
        if reply.type_ != u32::from(AtomEnum::NONE) {
            let target = check_target(&reply);
            let state = match target {
                Some(window) => format!("0x{window:x}, {}", window_state(conn, window)?),
                None => "valor no válido".into(),
            };
            return Err(Fatal::OtherWm(format!(
                "la raíz tiene _NET_SUPPORTING_WM_CHECK ({state}): hay, o hubo, otro gestor de ventanas"
            )));
        }
    }
    if let Some(selection) = existing_atom(conn, manager_selection(screen).as_bytes())? {
        let owner = conn
            .get_selection_owner(selection)
            .map_err(Fatal::lost)?
            .reply()
            .map_err(|e| Fatal::startup("GetSelectionOwner(WM_Sn)", e))?
            .owner;
        if owner != x11rb::NONE {
            return Err(Fatal::OtherWm(format!(
                "la selección {} tiene propietario (0x{owner:x}): hay otro gestor de ventanas",
                manager_selection(screen)
            )));
        }
    }
    Ok(())
}

/// Nombre de la selección del gestor de ventanas de la pantalla (ICCCM
/// §2.8).
pub fn manager_selection(screen: u16) -> String {
    format!("WM_S{screen}")
}

/// Ventana a la que apunta `_NET_SUPPORTING_WM_CHECK`, si el valor tiene la
/// forma esperada (formato 32, al menos un elemento).
fn check_target(reply: &GetPropertyReply) -> Option<Window> {
    reply
        .value32()
        .and_then(|mut values| values.next())
        .filter(|_| reply.format == 32)
}

/// Solo para el diagnóstico: si la ventana existe (lectura sin efectos).
fn window_state(conn: &impl Connection, window: Window) -> Result<&'static str, Fatal> {
    match conn
        .get_window_attributes(window)
        .map_err(Fatal::lost)?
        .reply()
    {
        Ok(_) => Ok("existente"),
        Err(x11rb::errors::ReplyError::ConnectionError(e)) => Err(Fatal::lost(e)),
        Err(x11rb::errors::ReplyError::X11Error(_)) => Ok("inexistente: obsoleta"),
    }
}

/// Capa 4: selección *checked* de `SubstructureRedirect`, con `BadAccess`
/// tratado explícitamente. En la misma petición se selecciona
/// `SubstructureNotify`, que el mosaico necesita para saber cuándo se
/// retira o destruye una ventana; ese bit no puede causar `BadAccess`.
pub fn take_substructure_redirect(conn: &impl Connection, root: Window) -> Result<(), Fatal> {
    let attributes = ChangeWindowAttributesAux::new()
        .event_mask(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY);
    conn.change_window_attributes(root, &attributes)
        .map_err(Fatal::lost)?
        .check()
        .map_err(|e| {
            if Fatal::is_access(&e) {
                Fatal::OtherWm("otro cliente ya tiene SubstructureRedirect (BadAccess)".into())
            } else {
                Fatal::startup("ChangeWindowAttributes(SubstructureRedirect)", e)
            }
        })
}

/// Consulta un átomo sin crearlo: devuelve `None` si no existe en el servidor.
fn existing_atom(conn: &impl Connection, name: &[u8]) -> Result<Option<u32>, Fatal> {
    let atom = conn
        .intern_atom(true, name)
        .map_err(Fatal::lost)?
        .reply()
        .map_err(|e| Fatal::startup("InternAtom", e))?
        .atom;
    Ok((atom != u32::from(AtomEnum::NONE)).then_some(atom))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property(type_: AtomEnum, format: u8, value: &[u8], bytes_after: u32) -> GetPropertyReply {
        GetPropertyReply {
            format,
            sequence: 0,
            length: 0,
            type_: type_.into(),
            bytes_after,
            value_len: value.len() as u32,
            value: value.to_vec(),
        }
    }

    #[test]
    fn token_ausente_vacio_o_no_unicode_se_rechaza() {
        use std::os::unix::ffi::OsStringExt;
        assert_eq!(sandbox_token(None).unwrap_err().exit_code(), 10);
        assert_eq!(sandbox_token(Some("".into())).unwrap_err().exit_code(), 10);
        let no_unicode = OsString::from_vec(vec![0xff]);
        assert_eq!(sandbox_token(Some(no_unicode)).unwrap_err().exit_code(), 10);
        assert_eq!(sandbox_token(Some("abc".into())).unwrap(), "abc");
    }

    #[test]
    fn seleccion_del_gestor() {
        assert_eq!(manager_selection(0), "WM_S0");
        assert_eq!(manager_selection(3), "WM_S3");
    }

    #[test]
    fn ventana_de_la_comprobacion() {
        let window = 0x60_0001u32.to_ne_bytes();
        let mut reply = property(AtomEnum::WINDOW, 32, &window, 0);
        assert_eq!(check_target(&reply), Some(0x60_0001));
        reply.format = 8;
        assert_eq!(check_target(&reply), None, "formato 8");
        assert_eq!(check_target(&property(AtomEnum::WINDOW, 32, &[], 0)), None);
    }

    #[test]
    fn marca_valida() {
        let token = "0123456789abcdef0123456789abcdef";
        assert!(marker_matches(
            token,
            &property(AtomEnum::STRING, 8, token.as_bytes(), 0)
        ));
    }

    #[test]
    fn marca_invalida() {
        let token = "0123456789abcdef";
        let casos = [
            property(AtomEnum::STRING, 8, b"0123456789abcdee", 0),
            property(AtomEnum::STRING, 8, b"0123456789abcde", 0),
            property(AtomEnum::STRING, 8, b"0123456789abcdef0", 0),
            property(AtomEnum::STRING, 8, b"0123456789abcdef", 4),
            property(AtomEnum::STRING, 8, b"", 0),
            property(AtomEnum::ATOM, 8, token.as_bytes(), 0),
            property(AtomEnum::STRING, 32, token.as_bytes(), 0),
            property(AtomEnum::NONE, 0, b"", 0),
        ];
        for reply in casos {
            assert!(!marker_matches(token, &reply), "{reply:?}");
        }
    }
}
