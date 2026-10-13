//! Conexión explícita al socket Unix local, con autenticación obligatoria.
//!
//! No se usa `RustConnection::connect`: vuelve a leer `DISPLAY`, prueba TCP
//! si el socket falla e ignora los errores de autenticación (ADR 0002).

use x11rb::errors::ConnectError;
use x11rb::reexports::x11rb_protocol::parse_display::ConnectAddress;
use x11rb::reexports::x11rb_protocol::xauth::get_auth;
use x11rb::rust_connection::{DefaultStream, RustConnection};

use crate::display::Target;
use crate::error::Fatal;

pub fn connect(target: &Target) -> Result<RustConnection, Fatal> {
    let path = target.socket_path();
    let (stream, (family, address)) = DefaultStream::connect(&ConnectAddress::Socket(path.clone()))
        .map_err(|e| Fatal::Unreachable(format!("no se pudo conectar al socket {path}: {e}")))?;

    // Fallo cerrado: sin cookie no se envía la petición de conexión.
    let (auth_name, auth_data) = match get_auth(family, &address, target.display) {
        Ok(Some(auth)) => auth,
        Ok(None) => {
            return Err(Fatal::Unreachable(format!(
                "no hay cookie MIT-MAGIC-COOKIE-1 para el display :{} \
                 (XAUTHORITY o ~/.Xauthority)",
                target.display
            )));
        }
        Err(e) => {
            return Err(Fatal::Unreachable(format!(
                "no se pudo leer el archivo de autorización X11: {e}"
            )));
        }
    };

    RustConnection::connect_to_stream_with_auth_info(
        stream,
        usize::from(target.screen),
        auth_name,
        auth_data,
    )
    .map_err(|e| connect_error(target, e))
}

fn connect_error(target: &Target, error: ConnectError) -> Fatal {
    match error {
        ConnectError::SetupAuthenticate(_)
        | ConnectError::SetupFailed(_)
        | ConnectError::IoError(_) => Fatal::Unreachable(format!(
            "el servidor del display :{} rechazó la conexión: {error}",
            target.display
        )),
        ConnectError::InvalidScreen => Fatal::Startup(format!(
            "la pantalla {} no existe en el display :{}",
            target.screen, target.display
        )),
        other => Fatal::Startup(format!("error al establecer la conexión X: {other}")),
    }
}
