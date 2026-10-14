//! EWMH (ADR 0008): lista exacta de lo que el WM declara en `_NET_SUPPORTED`,
//! decodificación y validación de las peticiones de los clientes
//! (`ClientMessage`) y orden de `_NET_CLIENT_LIST_STACKING`. Sin E/S.

use x11rb::protocol::xproto::{Atom, Timestamp, Window};

use crate::workspaces;

/// Nombre del WM en `_NET_WM_NAME` de la ventana de comprobación.
pub const WM_NAME: &str = "Crimson WM";

/// Lo que el WM implementa de verdad, en el orden en que se publica. No
/// incluye:
/// - `_NET_WM_NAME`: el WM la pone en su ventana de comprobación, pero no lee
///   la de los clientes;
/// - los tipos de ventana sin un tratamiento propio (paneles, escritorio,
///   avisos, menús…), que solo quedan flotantes;
/// - `_NET_WM_STATE` y la pantalla completa (F7), `_NET_WORKAREA` y los
///   struts, `_NET_FRAME_EXTENTS`, sticky, etc.
pub const SUPPORTED: &[&str] = &[
    "_NET_SUPPORTED",
    "_NET_SUPPORTING_WM_CHECK",
    "_NET_CLIENT_LIST",
    "_NET_CLIENT_LIST_STACKING",
    "_NET_NUMBER_OF_DESKTOPS",
    "_NET_CURRENT_DESKTOP",
    "_NET_ACTIVE_WINDOW",
    "_NET_CLOSE_WINDOW",
    "_NET_WM_DESKTOP",
    "_NET_WM_WINDOW_TYPE",
    "_NET_WM_WINDOW_TYPE_NORMAL",
    "_NET_WM_WINDOW_TYPE_DIALOG",
    "_NET_WM_WINDOW_TYPE_UTILITY",
    "_NET_WM_WINDOW_TYPE_TOOLBAR",
];

/// Valor de `_NET_WM_DESKTOP` que pide estar en todos los escritorios.
pub const STICKY: u32 = 0xFFFF_FFFF;

/// Tipos de los mensajes que el WM atiende.
#[derive(Clone, Copy, Debug)]
pub struct MessageTypes {
    pub active_window: Atom,
    pub current_desktop: Atom,
    pub wm_desktop: Atom,
    pub close_window: Atom,
}

/// Indicación de origen de una petición (EWMH, «Source indication»).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 0: clientes anteriores a la indicación. Se tratan como aplicaciones.
    Legacy,
    /// 1: una aplicación.
    Application,
    /// 2: un paginador, un panel o el usuario.
    Pager,
}

impl Source {
    fn decode(value: u32) -> Result<Self, Invalid> {
        match value {
            0 => Ok(Source::Legacy),
            1 => Ok(Source::Application),
            2 => Ok(Source::Pager),
            other => Err(Invalid::Source(other)),
        }
    }

    /// Solo un paginador o el usuario pueden cambiar de escritorio al
    /// activar una ventana; una aplicación, nunca (ADR 0008).
    pub fn may_switch_desktop(self) -> bool {
        self == Source::Pager
    }
}

/// Petición EWMH ya validada (salvo que la ventana esté gestionada, que solo
/// sabe `wm.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// `_NET_ACTIVE_WINDOW`. La marca de tiempo del solicitante no se usa
    /// para decidir: el WM aplica su propia política de origen.
    Activate { window: Window, source: Source },
    /// `_NET_CURRENT_DESKTOP`, con el índice ya comprobado.
    CurrentDesktop { desk: usize },
    /// `_NET_WM_DESKTOP`, con el índice ya comprobado.
    WindowDesktop { window: Window, desk: usize },
    /// `_NET_CLOSE_WINDOW`: siempre cooperativo.
    Close {
        window: Window,
        time: Timestamp,
        source: Source,
    },
}

/// Motivo por el que se rechaza una petición.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Los mensajes EWMH son de formato 32.
    Format(u8),
    Source(u32),
    /// Índice de escritorio fuera de rango.
    Desk(u32),
    /// `_NET_WM_DESKTOP` = 0xFFFFFFFF: sin ventanas sticky (ADR 0008).
    Sticky,
    /// `_NET_CURRENT_DESKTOP` debe enviarse sobre la raíz.
    NotRoot(Window),
}

/// Decodifica un `ClientMessage` recibido en la raíz. `None` si no es una
/// petición EWMH que el WM atienda (se ignora sin más).
pub fn decode(
    types: &MessageTypes,
    root: Window,
    message_type: Atom,
    format: u8,
    window: Window,
    data: [u32; 5],
) -> Option<Result<Request, Invalid>> {
    let known = [
        types.active_window,
        types.current_desktop,
        types.wm_desktop,
        types.close_window,
    ];
    if !known.contains(&message_type) {
        return None;
    }
    if format != 32 {
        return Some(Err(Invalid::Format(format)));
    }
    let request = if message_type == types.active_window {
        Source::decode(data[0]).map(|source| Request::Activate { window, source })
    } else if message_type == types.current_desktop {
        if window != root {
            Err(Invalid::NotRoot(window))
        } else {
            desk(data[0]).map(|desk| Request::CurrentDesktop { desk })
        }
    } else if message_type == types.wm_desktop {
        desk(data[0]).map(|desk| Request::WindowDesktop { window, desk })
    } else {
        Source::decode(data[1]).map(|source| Request::Close {
            window,
            time: data[0],
            source,
        })
    };
    Some(request)
}

fn desk(value: u32) -> Result<usize, Invalid> {
    if value == STICKY {
        return Err(Invalid::Sticky);
    }
    usize::try_from(value)
        .ok()
        .filter(|&d| d < workspaces::COUNT)
        .ok_or(Invalid::Desk(value))
}

/// `_NET_CLIENT_LIST_STACKING`: las ventanas gestionadas en el orden de
/// apilamiento real (`QueryTree` las da de abajo arriba, como pide EWMH).
pub fn stacking(children: &[Window], managed: impl Fn(Window) -> bool) -> Vec<Window> {
    children.iter().copied().filter(|&w| managed(w)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: Window = 0x1ff;
    const W: Window = 0x40_0001;
    const TYPES: MessageTypes = MessageTypes {
        active_window: 10,
        current_desktop: 11,
        wm_desktop: 12,
        close_window: 13,
    };

    fn decode_at(
        message_type: Atom,
        window: Window,
        data: [u32; 5],
    ) -> Option<Result<Request, Invalid>> {
        decode(&TYPES, ROOT, message_type, 32, window, data)
    }

    // U16: EWMH.

    #[test]
    fn lista_soportada_exacta() {
        assert_eq!(SUPPORTED.len(), 14);
        for (i, name) in SUPPORTED.iter().enumerate() {
            assert!(name.starts_with("_NET_"), "{name}");
            assert!(!SUPPORTED[i + 1..].contains(name), "repetido: {name}");
        }
        // Nada aplazado se declara.
        for deferred in [
            "_NET_WM_STATE",
            "_NET_WM_STATE_FULLSCREEN",
            "_NET_FRAME_EXTENTS",
            "_NET_WORKAREA",
            "_NET_WM_STRUT",
            "_NET_WM_STRUT_PARTIAL",
            "_NET_WM_NAME",
            "_NET_DESKTOP_NAMES",
            "_NET_WM_WINDOW_TYPE_DOCK",
            "_NET_WM_PING",
        ] {
            assert!(
                !SUPPORTED.contains(&deferred),
                "{deferred} no está implementada"
            );
        }
    }

    #[test]
    fn activar_ventana() {
        for (value, source) in [
            (0, Source::Legacy),
            (1, Source::Application),
            (2, Source::Pager),
        ] {
            assert_eq!(
                decode_at(10, W, [value, 1234, 0, 0, 0]),
                Some(Ok(Request::Activate { window: W, source }))
            );
        }
        assert_eq!(
            decode_at(10, W, [3, 0, 0, 0, 0]),
            Some(Err(Invalid::Source(3)))
        );
        // Solo un paginador puede cambiar de escritorio.
        assert!(Source::Pager.may_switch_desktop());
        assert!(!Source::Application.may_switch_desktop());
        assert!(!Source::Legacy.may_switch_desktop());
    }

    #[test]
    fn escritorio_actual() {
        assert_eq!(
            decode_at(11, ROOT, [7, 99, 0, 0, 0]),
            Some(Ok(Request::CurrentDesktop { desk: 7 }))
        );
        assert_eq!(
            decode_at(11, ROOT, [8, 0, 0, 0, 0]),
            Some(Err(Invalid::Desk(8)))
        );
        assert_eq!(
            decode_at(11, ROOT, [STICKY, 0, 0, 0, 0]),
            Some(Err(Invalid::Sticky))
        );
        assert_eq!(
            decode_at(11, W, [1, 0, 0, 0, 0]),
            Some(Err(Invalid::NotRoot(W)))
        );
    }

    #[test]
    fn escritorio_de_una_ventana() {
        assert_eq!(
            decode_at(12, W, [0, 2, 0, 0, 0]),
            Some(Ok(Request::WindowDesktop { window: W, desk: 0 }))
        );
        assert_eq!(
            decode_at(12, W, [STICKY, 2, 0, 0, 0]),
            Some(Err(Invalid::Sticky))
        );
        assert_eq!(
            decode_at(12, W, [8, 2, 0, 0, 0]),
            Some(Err(Invalid::Desk(8)))
        );
        assert_eq!(
            decode_at(12, W, [u32::MAX - 1, 2, 0, 0, 0]),
            Some(Err(Invalid::Desk(u32::MAX - 1)))
        );
    }

    #[test]
    fn cerrar_ventana() {
        assert_eq!(
            decode_at(13, W, [5678, 2, 0, 0, 0]),
            Some(Ok(Request::Close {
                window: W,
                time: 5678,
                source: Source::Pager
            }))
        );
        assert_eq!(
            decode_at(13, W, [0, 7, 0, 0, 0]),
            Some(Err(Invalid::Source(7)))
        );
    }

    #[test]
    fn formato_y_tipos_desconocidos() {
        for message_type in [10, 11, 12, 13] {
            assert_eq!(
                decode(&TYPES, ROOT, message_type, 8, ROOT, [0; 5]),
                Some(Err(Invalid::Format(8)))
            );
        }
        // Otros mensajes (p. ej. `_NET_WM_STATE`, aplazado) se ignoran.
        assert_eq!(decode_at(99, W, [1, 2, 3, 4, 5]), None);
    }

    #[test]
    fn orden_de_apilamiento() {
        let children = [5, 1, 9, 2, 7, 3];
        let managed = [1, 2, 3];
        assert_eq!(stacking(&children, |w| managed.contains(&w)), vec![1, 2, 3]);
        assert_eq!(stacking(&[], |_| true), Vec::<Window>::new());
        assert_eq!(stacking(&children, |_| false), Vec::<Window>::new());
    }
}
