//! Ventanas gestionadas y su clasificación (ADR 0004). Sin E/S: `wm.rs`
//! lee las propiedades del servidor y aplica las decisiones.

use x11rb::protocol::xproto::{Atom, Window};

use crate::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Forma parte del mosaico; su geometría la decide el WM.
    Tiled,
    /// Conserva la geometría que pide y queda por encima del mosaico.
    Floating,
}

#[derive(Debug)]
pub struct Client {
    pub window: Window,
    pub kind: Kind,
    /// Última geometría asignada por el WM (solo en mosaico).
    pub geometry: Option<Rect>,
}

/// Ventanas gestionadas, en orden de llegada. Las del mosaico, en ese
/// orden, forman el diseño master-stack: la primera es la maestra.
///
/// Un `Vec` con búsqueda lineal basta para el número de ventanas de un
/// escritorio y conserva el orden sin estructuras adicionales.
#[derive(Debug, Default)]
pub struct Clients {
    list: Vec<Client>,
}

impl Clients {
    /// Añade la ventana al final. Devuelve `false`, sin cambiar nada, si ya
    /// estaba gestionada.
    pub fn manage(&mut self, window: Window, kind: Kind) -> bool {
        if self.get(window).is_some() {
            return false;
        }
        self.list.push(Client {
            window,
            kind,
            geometry: None,
        });
        true
    }

    /// Deja de gestionar la ventana. Devuelve su tipo, o `None` si no estaba
    /// gestionada (eventos duplicados o de ventanas ajenas).
    pub fn unmanage(&mut self, window: Window) -> Option<Kind> {
        let index = self.list.iter().position(|c| c.window == window)?;
        Some(self.list.remove(index).kind)
    }

    pub fn get(&self, window: Window) -> Option<&Client> {
        self.list.iter().find(|c| c.window == window)
    }

    /// Ventanas del mosaico, en orden: la primera es la maestra.
    pub fn tiled(&self) -> impl Iterator<Item = Window> + '_ {
        self.of_kind(Kind::Tiled)
    }

    /// Ventanas flotantes, en orden de llegada.
    pub fn floating(&self) -> impl Iterator<Item = Window> + '_ {
        self.of_kind(Kind::Floating)
    }

    fn of_kind(&self, kind: Kind) -> impl Iterator<Item = Window> + '_ {
        self.list
            .iter()
            .filter(move |c| c.kind == kind)
            .map(|c| c.window)
    }

    /// Guarda la geometría asignada. Devuelve `true` si ha cambiado, es
    /// decir, si hay que enviarla al servidor.
    pub fn set_geometry(&mut self, window: Window, rect: Rect) -> bool {
        match self.list.iter_mut().find(|c| c.window == window) {
            Some(client) if client.geometry != Some(rect) => {
                client.geometry = Some(rect);
                true
            }
            _ => false,
        }
    }
}

/// Datos de una ventana que deciden si va en mosaico.
#[derive(Debug, Default)]
pub struct WindowInfo {
    pub override_redirect: bool,
    /// Tiene la propiedad `WM_TRANSIENT_FOR` (diálogo de otra ventana).
    pub transient: bool,
    /// Valores de `_NET_WM_WINDOW_TYPE`, en orden de preferencia.
    pub types: Vec<Atom>,
}

/// Átomos de `_NET_WM_WINDOW_TYPE` que reconoce la clasificación.
pub struct WindowTypes<'a> {
    pub normal: Atom,
    /// Tipos que no van en mosaico (diálogos, menús, paneles…).
    pub floating: &'a [Atom],
}

/// Decide cómo se gestiona una ventana. `None`: no se gestiona
/// (override-redirect).
///
/// Según EWMH, cuenta el primer tipo reconocido de `_NET_WM_WINDOW_TYPE`;
/// sin ninguno reconocido, una ventana con `WM_TRANSIENT_FOR` se trata como
/// un diálogo y las demás como normales.
pub fn classify(info: &WindowInfo, types: &WindowTypes) -> Option<Kind> {
    if info.override_redirect {
        return None;
    }
    let declared = info.types.iter().find_map(|&t| {
        if t == types.normal {
            Some(Kind::Tiled)
        } else if types.floating.contains(&t) {
            Some(Kind::Floating)
        } else {
            None
        }
    });
    Some(declared.unwrap_or(if info.transient {
        Kind::Floating
    } else {
        Kind::Tiled
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORMAL: Atom = 100;
    const DIALOG: Atom = 101;
    const DOCK: Atom = 102;
    const UNKNOWN: Atom = 200;
    const TYPES: WindowTypes = WindowTypes {
        normal: NORMAL,
        floating: &[DIALOG, DOCK],
    };

    fn rect(width: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width,
            height: 10,
        }
    }

    #[test]
    fn conserva_el_orden_de_llegada() {
        let mut clients = Clients::default();
        assert!(clients.manage(1, Kind::Tiled));
        assert!(clients.manage(2, Kind::Floating));
        assert!(clients.manage(3, Kind::Tiled));
        assert!(clients.manage(4, Kind::Tiled));
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1, 3, 4]);
        assert_eq!(clients.floating().collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn al_retirar_la_maestra_la_siguiente_pasa_a_serlo() {
        let mut clients = Clients::default();
        for w in [1, 2, 3] {
            clients.manage(w, Kind::Tiled);
        }
        assert_eq!(clients.unmanage(1), Some(Kind::Tiled));
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn retirar_es_idempotente_y_no_afecta_a_ventanas_ajenas() {
        let mut clients = Clients::default();
        clients.manage(1, Kind::Tiled);
        clients.manage(2, Kind::Floating);
        assert_eq!(clients.unmanage(9), None);
        assert_eq!(clients.unmanage(2), Some(Kind::Floating));
        assert_eq!(clients.unmanage(2), None);
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn gestionar_dos_veces_no_duplica() {
        let mut clients = Clients::default();
        assert!(clients.manage(1, Kind::Tiled));
        assert!(!clients.manage(1, Kind::Floating));
        assert_eq!(clients.get(1).map(|c| c.kind), Some(Kind::Tiled));
        assert_eq!(clients.tiled().count(), 1);
    }

    #[test]
    fn una_ventana_retirada_vuelve_al_final() {
        let mut clients = Clients::default();
        for w in [1, 2, 3] {
            clients.manage(w, Kind::Tiled);
        }
        clients.unmanage(2);
        clients.manage(2, Kind::Tiled);
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1, 3, 2]);
        assert_eq!(clients.get(2).and_then(|c| c.geometry), None);
    }

    #[test]
    fn geometria_solo_cambia_si_es_distinta() {
        let mut clients = Clients::default();
        clients.manage(1, Kind::Tiled);
        assert!(clients.set_geometry(1, rect(10)));
        assert!(!clients.set_geometry(1, rect(10)));
        assert!(clients.set_geometry(1, rect(20)));
        assert_eq!(clients.get(1).and_then(|c| c.geometry), Some(rect(20)));
        assert!(!clients.set_geometry(9, rect(10)));
    }

    fn info(override_redirect: bool, transient: bool, types: &[Atom]) -> WindowInfo {
        WindowInfo {
            override_redirect,
            transient,
            types: types.to_vec(),
        }
    }

    #[test]
    fn clasificacion() {
        let casos = [
            (info(false, false, &[]), Some(Kind::Tiled)),
            (info(false, false, &[NORMAL]), Some(Kind::Tiled)),
            (info(false, false, &[DIALOG]), Some(Kind::Floating)),
            (info(false, false, &[DOCK]), Some(Kind::Floating)),
            (info(false, true, &[]), Some(Kind::Floating)),
            // El tipo explícito prevalece sobre WM_TRANSIENT_FOR.
            (info(false, true, &[NORMAL]), Some(Kind::Tiled)),
            // Cuenta el primer tipo reconocido.
            (
                info(false, false, &[UNKNOWN, DIALOG, NORMAL]),
                Some(Kind::Floating),
            ),
            (info(false, false, &[NORMAL, DIALOG]), Some(Kind::Tiled)),
            (info(false, false, &[UNKNOWN]), Some(Kind::Tiled)),
            (info(false, true, &[UNKNOWN]), Some(Kind::Floating)),
            (info(true, false, &[NORMAL]), None),
        ];
        for (info, esperado) in casos {
            assert_eq!(classify(&info, &TYPES), esperado, "{info:?}");
        }
    }
}
