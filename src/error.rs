//! Errores fatales y su código de salida (tabla del ADR 0003, ampliada con
//! el código 15 en el ADR 0014).

use std::fmt;

use x11rb::errors::{ConnectionError, ReplyError};
use x11rb::protocol::ErrorKind;

/// Causa por la que el proceso termina con un código distinto de 0.
#[derive(Debug)]
pub enum Fatal {
    /// Argumentos inválidos (código 2).
    Usage(String),
    /// Rechazo de las salvaguardas: display 0, no local, malformado o sin
    /// marca de sandbox válida (código 10).
    Rejected(String),
    /// `DISPLAY` ausente o vacío, socket inaccesible, cookie no disponible o
    /// autenticación denegada (código 11).
    Unreachable(String),
    /// Ya hay otro gestor de ventanas (código 12).
    OtherWm(String),
    /// Se perdió la conexión con el servidor X (código 13).
    ConnectionLost(String),
    /// Error de protocolo inesperado al arrancar (código 14).
    Startup(String),
    /// Configuración ilegible o inválida (código 15, ADR 0014). Se detecta
    /// antes de conectar con X.
    Config(String),
}

impl Fatal {
    pub fn exit_code(&self) -> u8 {
        match self {
            Fatal::Usage(_) => 2,
            Fatal::Rejected(_) => 10,
            Fatal::Unreachable(_) => 11,
            Fatal::OtherWm(_) => 12,
            Fatal::ConnectionLost(_) => 13,
            Fatal::Startup(_) => 14,
            Fatal::Config(_) => 15,
        }
    }

    /// Error de una petición hecha durante el arranque.
    pub fn startup(context: &str, error: ReplyError) -> Self {
        match error {
            ReplyError::ConnectionError(e) => Fatal::lost(e),
            ReplyError::X11Error(e) => Fatal::Startup(format!("{context}: {:?}", e.error_kind)),
        }
    }

    pub fn lost(error: ConnectionError) -> Self {
        Fatal::ConnectionLost(format!("se perdió la conexión con el servidor X: {error}"))
    }

    /// Indica si un error X es `BadAccess`.
    pub fn is_access(error: &ReplyError) -> bool {
        matches!(error, ReplyError::X11Error(e) if e.error_kind == ErrorKind::Access)
    }
}

impl fmt::Display for Fatal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fatal::Usage(msg)
            | Fatal::Rejected(msg)
            | Fatal::Unreachable(msg)
            | Fatal::OtherWm(msg)
            | Fatal::ConnectionLost(msg)
            | Fatal::Startup(msg)
            | Fatal::Config(msg) => f.write_str(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codigos_de_salida_del_adr_0003() {
        let casos = [
            (Fatal::Usage(String::new()), 2),
            (Fatal::Rejected(String::new()), 10),
            (Fatal::Unreachable(String::new()), 11),
            (Fatal::OtherWm(String::new()), 12),
            (Fatal::ConnectionLost(String::new()), 13),
            (Fatal::Startup(String::new()), 14),
            (Fatal::Config(String::new()), 15),
        ];
        for (fatal, codigo) in casos {
            assert_eq!(fatal.exit_code(), codigo, "{fatal:?}");
        }
    }
}
