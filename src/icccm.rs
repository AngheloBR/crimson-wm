//! ICCCM (ADR 0008): interpretación de `WM_HINTS` y `WM_PROTOCOLS`, mensajes
//! `WM_PROTOCOLS` (`WM_DELETE_WINDOW` y `WM_TAKE_FOCUS`) y valor de
//! `WM_STATE`. Sin E/S: `wm.rs` lee las propiedades y envía los mensajes.

use x11rb::NONE;
use x11rb::protocol::xproto::{Atom, ClientMessageEvent, Timestamp, Window};

/// Valores de `WM_STATE.state` (ICCCM §4.1.3.1).
pub const NORMAL_STATE: u32 = 1;
pub const ICONIC_STATE: u32 = 3;

/// Longitud de `WM_HINTS` en unidades de 32 bits (ICCCM §4.1.2.4).
pub const WM_HINTS_LEN: u32 = 9;
/// Máximo de protocolos de `WM_PROTOCOLS` que se leen.
pub const MAX_PROTOCOLS: u32 = 32;

/// Campo `input` de `WM_HINTS` (valores de 32 bits): solo cuenta si la
/// bandera `InputHint` está activa. Una propiedad incompleta se trata como
/// ausente.
pub fn input_hint(wm_hints: &[u32]) -> Option<bool> {
    const INPUT_HINT: u32 = 1;
    match wm_hints {
        [flags, input, ..] if flags & INPUT_HINT != 0 => Some(*input != 0),
        _ => None,
    }
}

/// Protocolos de `WM_PROTOCOLS` que el WM utiliza.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Protocols {
    /// Admite el cierre cooperativo (`WM_DELETE_WINDOW`).
    pub delete_window: bool,
    /// Quiere el mensaje `WM_TAKE_FOCUS` (modelos de entrada activos).
    pub take_focus: bool,
}

/// Átomos con los que se compara `WM_PROTOCOLS`.
#[derive(Clone, Copy, Debug)]
pub struct ProtocolAtoms {
    pub delete_window: Atom,
    pub take_focus: Atom,
}

impl Protocols {
    /// A partir de los átomos de `WM_PROTOCOLS`. Los desconocidos se
    /// ignoran; una propiedad ausente o mal formada llega vacía.
    pub fn parse(values: &[Atom], atoms: ProtocolAtoms) -> Self {
        Protocols {
            delete_window: values.contains(&atoms.delete_window),
            take_focus: values.contains(&atoms.take_focus),
        }
    }
}

/// Mensaje `WM_PROTOCOLS` para la ventana (ICCCM §4.2.8): formato 32,
/// `data[0]` = protocolo y `data[1]` = marca de tiempo. Se usa con
/// `WM_DELETE_WINDOW` y `WM_TAKE_FOCUS`.
pub fn protocol_message(
    window: Window,
    wm_protocols: Atom,
    protocol: Atom,
    time: Timestamp,
) -> ClientMessageEvent {
    ClientMessageEvent::new(32, window, wm_protocols, [protocol, time, 0, 0, 0])
}

/// Valor de `WM_STATE`: estado y ventana de icono (ninguna).
pub fn wm_state(state: u32) -> [u32; 2] {
    [state, NONE]
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELETE: Atom = 301;
    const TAKE: Atom = 302;
    const ATOMS: ProtocolAtoms = ProtocolAtoms {
        delete_window: DELETE,
        take_focus: TAKE,
    };

    // U15: ICCCM.

    #[test]
    fn campo_input_de_wm_hints() {
        assert_eq!(input_hint(&[1, 1, 0, 0, 0, 0, 0, 0, 0]), Some(true));
        assert_eq!(input_hint(&[1, 0]), Some(false));
        assert_eq!(input_hint(&[3, 0]), Some(false));
        // Sin la bandera InputHint el campo no cuenta.
        assert_eq!(input_hint(&[2, 0, 1]), None);
        // Propiedad ausente o incompleta.
        assert_eq!(input_hint(&[]), None);
        assert_eq!(input_hint(&[1]), None);
    }

    #[test]
    fn protocolos() {
        assert_eq!(Protocols::parse(&[], ATOMS), Protocols::default());
        assert_eq!(
            Protocols::parse(&[DELETE], ATOMS),
            Protocols {
                delete_window: true,
                take_focus: false
            }
        );
        assert_eq!(
            Protocols::parse(&[999, TAKE, 0, DELETE], ATOMS),
            Protocols {
                delete_window: true,
                take_focus: true
            }
        );
        assert_eq!(
            Protocols::parse(&[TAKE, TAKE], ATOMS),
            Protocols {
                delete_window: false,
                take_focus: true
            }
        );
    }

    #[test]
    fn mensajes_wm_protocols() {
        let (protocols, time) = (300, 123_456);
        for protocol in [DELETE, TAKE] {
            let message = protocol_message(0x40_0003, protocols, protocol, time);
            let bytes: [u8; 32] = message.into();
            assert_eq!(bytes[0], 33, "ClientMessage");
            assert_eq!(bytes[1], 32, "formato");
            let word = |i: usize| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap());
            assert_eq!(word(4), 0x40_0003, "ventana");
            assert_eq!(word(8), protocols, "tipo");
            assert_eq!((word(12), word(16)), (protocol, time));
            assert_eq!((word(20), word(24), word(28)), (0, 0, 0));
        }
    }

    #[test]
    fn valor_de_wm_state() {
        assert_eq!(wm_state(NORMAL_STATE), [1, 0]);
        assert_eq!(wm_state(ICONIC_STATE), [3, 0]);
    }
}
