//! Capa 1 del ADR 0003: validación de `DISPLAY` antes de abrir la conexión.
//!
//! Se usa el parser público de x11rb para que la validación y la conexión
//! interpreten el valor del mismo modo. La conexión se hace después con el
//! número ya validado, nunca volviendo a leer `DISPLAY`.

use std::ffi::OsString;

use x11rb::reexports::x11rb_protocol::parse_display::parse_display;

use crate::error::Fatal;

/// Display local validado.
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub display: u16,
    pub screen: u16,
}

impl Target {
    /// Único destino al que se conecta: el socket Unix local del display.
    pub fn socket_path(&self) -> String {
        format!("/tmp/.X11-unix/X{}", self.display)
    }
}

/// Valida el valor de `DISPLAY` según las reglas de la capa 1.
pub fn validate(value: Option<OsString>) -> Result<Target, Fatal> {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return Err(Fatal::Unreachable(
            "DISPLAY no está definido o está vacío".into(),
        ));
    };
    let Some(value) = value.to_str() else {
        return Err(Fatal::Rejected("DISPLAY no es Unicode".into()));
    };

    let parsed = parse_display(Some(value))
        .map_err(|e| Fatal::Rejected(format!("DISPLAY malformado: {e}")))?;

    if let Some(protocol) = parsed.protocol.as_deref().filter(|p| *p != "unix") {
        return Err(Fatal::Rejected(format!(
            "DISPLAY={value:?}: protocolo {protocol:?} no permitido; solo se admite el socket Unix local"
        )));
    }
    if !parsed.host.is_empty() {
        return Err(Fatal::Rejected(format!(
            "DISPLAY={value:?}: solo se admiten displays locales de la forma :N; \
             las rutas y los destinos remotos se rechazan"
        )));
    }
    if parsed.display == 0 {
        return Err(Fatal::Rejected(format!(
            "DISPLAY={value:?}: el display 0 se rechaza siempre"
        )));
    }

    Ok(Target {
        display: parsed.display,
        screen: parsed.screen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(value: &str) -> Result<Target, Fatal> {
        validate(Some(OsString::from(value)))
    }

    fn assert_code(value: &str, code: u8) {
        match check(value) {
            Ok(target) => panic!("{value:?} aceptado como {target:?}"),
            Err(e) => assert_eq!(e.exit_code(), code, "{value:?}: {e}"),
        }
    }

    #[test]
    fn display_ausente_o_vacio_da_codigo_11() {
        assert_eq!(validate(None).unwrap_err().exit_code(), 11);
        assert_code("", 11);
    }

    #[test]
    fn display_0_se_rechaza_en_cualquier_forma() {
        for value in [
            ":0",
            ":0.0",
            ":0.1",
            ":00",
            ":+0",
            "unix/:0",
            "unix:0",
            "/tmp/.X11-unix/X0",
        ] {
            assert_code(value, 10);
        }
    }

    #[test]
    fn destinos_remotos_o_tcp_se_rechazan() {
        for value in [
            "localhost:1",
            "host:1",
            "tcp/localhost:1",
            "tcp/:1",
            "inet/:1",
            "[::1]:1",
            "unix/host:1",
            "unix/unix:1",
        ] {
            assert_code(value, 10);
        }
    }

    #[test]
    fn rutas_y_valores_malformados_se_rechazan() {
        for value in [
            "/tmp/.X11-unix/X1",
            "/tmp/.X11-unix/X1.0",
            "unix:1",
            "unix:/tmp/.X11-unix/X1",
            "1",
            ":",
            ":a",
            ":1.a",
            ":-1",
            ":65536",
            ": 1",
        ] {
            assert_code(value, 10);
        }
    }

    #[test]
    fn display_no_unicode_se_rechaza() {
        use std::os::unix::ffi::OsStringExt;
        let value = OsString::from_vec(vec![b':', 0xff]);
        assert_eq!(validate(Some(value)).unwrap_err().exit_code(), 10);
    }

    #[test]
    fn displays_locales_validos() {
        let casos = [
            (":1", 1, 0),
            (":1.0", 1, 0),
            (":99.2", 99, 2),
            ("unix/:5", 5, 0),
            (":05", 5, 0),
            (":+5", 5, 0),
            (":65535", 65535, 0),
        ];
        for (value, display, screen) in casos {
            assert_eq!(
                check(value).unwrap(),
                Target { display, screen },
                "{value:?}"
            );
        }
    }

    #[test]
    fn el_socket_usa_el_numero_validado() {
        let target = check(":+05.1").unwrap();
        assert_eq!(target.socket_path(), "/tmp/.X11-unix/X5");
    }
}
