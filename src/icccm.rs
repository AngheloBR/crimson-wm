//! ICCCM (ADR 0008 y 0010): interpretación de `WM_HINTS`, `WM_PROTOCOLS`,
//! `WM_TRANSIENT_FOR` y `WM_NORMAL_HINTS`, mensajes `WM_PROTOCOLS`
//! (`WM_DELETE_WINDOW` y `WM_TAKE_FOCUS`) y valor de `WM_STATE`. Sin E/S:
//! `wm.rs` lee las propiedades y envía los mensajes.

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

/// Valor de `WM_TRANSIENT_FOR` (ICCCM §4.1.2.6), ya interpretado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransientFor {
    /// Sin la propiedad, con un valor mal formado o 0.
    None,
    /// La raíz: transitoria de grupo. Los grupos (`WM_HINTS.window_group`)
    /// no se implementan, así que no tiene propietario (ADR 0010).
    Root,
    /// La propia ventana: inválido.
    Itself,
    /// Otra ventana: el propietario declarado.
    Window(Window),
}

impl TransientFor {
    /// A partir de los valores de 32 bits de la propiedad (cualquier tipo,
    /// como en la clasificación; vacío si falta o el formato no es 32).
    pub fn parse(values: &[u32], window: Window, root: Window) -> Self {
        match values.first().copied() {
            None | Some(0) => TransientFor::None,
            Some(v) if v == root => TransientFor::Root,
            Some(v) if v == window => TransientFor::Itself,
            Some(v) => TransientFor::Window(v),
        }
    }

    /// El propietario declarado válido, si lo hay.
    pub fn owner(self) -> Option<Window> {
        match self {
            TransientFor::Window(w) => Some(w),
            _ => None,
        }
    }
}

/// Longitud de `WM_NORMAL_HINTS` (`WM_SIZE_HINTS`) en unidades de 32 bits.
pub const WM_NORMAL_HINTS_LEN: u32 = 18;

/// `WM_NORMAL_HINTS` pide una posición explícita (`USPosition`, dada por el
/// usuario, o `PPosition`, por el programa; ICCCM §4.1.2.3): el WM la
/// respeta y no centra la ventana. Una propiedad ausente o incompleta no
/// pide nada.
pub fn explicit_position(normal_hints: &[u32]) -> bool {
    const US_POSITION: u32 = 1;
    const P_POSITION: u32 = 1 << 2;
    normal_hints
        .first()
        .is_some_and(|flags| flags & (US_POSITION | P_POSITION) != 0)
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

    // U19: WM_TRANSIENT_FOR y WM_NORMAL_HINTS.

    #[test]
    fn interpreta_wm_transient_for() {
        let (window, root) = (0x40_0001, 0x1ff);
        assert_eq!(TransientFor::parse(&[], window, root), TransientFor::None);
        assert_eq!(TransientFor::parse(&[0], window, root), TransientFor::None);
        assert_eq!(
            TransientFor::parse(&[root], window, root),
            TransientFor::Root
        );
        assert_eq!(
            TransientFor::parse(&[window], window, root),
            TransientFor::Itself
        );
        assert_eq!(
            TransientFor::parse(&[0x60_0001, 7], window, root),
            TransientFor::Window(0x60_0001)
        );
        assert_eq!(TransientFor::Window(9).owner(), Some(9));
        for invalid in [TransientFor::None, TransientFor::Root, TransientFor::Itself] {
            assert_eq!(invalid.owner(), None);
        }
    }

    #[test]
    fn posicion_explicita() {
        assert!(!explicit_position(&[]), "ausente");
        assert!(!explicit_position(&[0, 10, 20]));
        assert!(explicit_position(&[1]), "USPosition");
        assert!(explicit_position(&[4]), "PPosition");
        assert!(!explicit_position(&[2 | 8]), "solo tamaños (USSize, PSize)");
        assert!(explicit_position(&[1 | 2 | 0x200]));
    }

    #[test]
    fn valor_de_wm_state() {
        assert_eq!(wm_state(NORMAL_STATE), [1, 0]);
        assert_eq!(wm_state(ICONIC_STATE), [3, 0]);
    }
}
