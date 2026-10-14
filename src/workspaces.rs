//! Escritorios virtuales (ADR 0007): ocho conjuntos independientes de
//! ventanas, cada uno con su propio orden master-stack e historial de foco.
//! Sin E/S: `wm.rs` muestra, oculta y enfoca las ventanas según lo que
//! decide este módulo.
//!
//! Los escritorios se numeran internamente de 0 a 7 (como
//! `_NET_CURRENT_DESKTOP`) y se muestran al usuario de 1 a 8.

use x11rb::protocol::xproto::Window;

use crate::clients::{Class, Client, Clients, Kind, Removed};

/// Número fijo de escritorios.
pub const COUNT: usize = 8;

/// Cambio del escritorio activo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switch {
    pub from: usize,
    pub to: usize,
}

/// Ventana llevada de un escritorio a otro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    pub window: Window,
    pub kind: Kind,
    pub from: usize,
    pub to: usize,
    /// Era la enfocada de su escritorio de origen.
    pub was_focused: bool,
}

/// Invariantes:
/// - cada ventana gestionada está en un único escritorio;
/// - `current` es menor que `COUNT`;
/// - `previous`, si existe, es distinto de `current`;
/// - el historial de cada escritorio solo contiene ventanas de ese
///   escritorio que admiten foco (lo garantiza `Clients`).
#[derive(Debug, Default)]
pub struct Workspaces {
    desks: [Clients; COUNT],
    current: usize,
    previous: Option<usize>,
    /// Siguiente número de orden de gestión (`Client::order`).
    next_order: u64,
}

impl Workspaces {
    /// Índice del escritorio activo.
    pub fn index(&self) -> usize {
        self.current
    }

    pub fn current(&self) -> &Clients {
        &self.desks[self.current]
    }

    pub fn current_mut(&mut self) -> &mut Clients {
        &mut self.desks[self.current]
    }

    /// Ventanas de un escritorio. `None` fuera de rango.
    pub fn desk(&self, index: usize) -> Option<&Clients> {
        self.desks.get(index)
    }

    /// Escritorio de la ventana, si está gestionada.
    pub fn locate(&self, window: Window) -> Option<usize> {
        self.desks.iter().position(|d| d.get(window).is_some())
    }

    /// La ventana gestionada, esté en el escritorio que esté.
    pub fn get(&self, window: Window) -> Option<&Client> {
        self.desks.iter().find_map(|d| d.get(window))
    }

    pub fn get_mut(&mut self, window: Window) -> Option<&mut Client> {
        self.desks.iter_mut().find_map(|d| d.get_mut(window))
    }

    /// Gestiona la ventana en el escritorio activo. Devuelve `false`, sin
    /// cambiar nada, si ya estaba gestionada en cualquier escritorio.
    pub fn manage(&mut self, window: Window, class: Class) -> bool {
        if self.locate(window).is_some() || !self.current_mut().manage(window, class) {
            return false;
        }
        let order = self.next_order;
        self.next_order += 1;
        if let Some(client) = self.current_mut().get_mut(window) {
            client.order = order;
        }
        true
    }

    /// Todas las ventanas gestionadas, de todos los escritorios, en el orden
    /// en que empezaron a gestionarse (`_NET_CLIENT_LIST`).
    pub fn client_list(&self) -> Vec<Window> {
        let mut all: Vec<(u64, Window)> = self
            .desks
            .iter()
            .flat_map(|d| d.windows().filter_map(|w| d.get(w)))
            .map(|c| (c.order, c.window))
            .collect();
        all.sort_unstable();
        all.into_iter().map(|(_, w)| w).collect()
    }

    /// Deja de gestionar la ventana, esté en el escritorio que esté, e
    /// indica en cuál estaba.
    pub fn unmanage(&mut self, window: Window) -> Option<(usize, Removed)> {
        let index = self.locate(window)?;
        self.desks[index]
            .unmanage(window)
            .map(|removed| (index, removed))
    }

    /// Activa el escritorio `to`; el activo pasa a ser el anterior. `None`,
    /// sin cambiar nada, si `to` ya es el activo o está fuera de rango.
    pub fn switch(&mut self, to: usize) -> Option<Switch> {
        if to >= COUNT || to == self.current {
            return None;
        }
        let from = self.current;
        self.previous = Some(from);
        self.current = to;
        Some(Switch { from, to })
    }

    /// Vuelve al escritorio anterior. `None` si todavía no hay ninguno.
    pub fn back(&mut self) -> Option<Switch> {
        self.previous.and_then(|to| self.switch(to))
    }

    /// Lleva la ventana enfocada del escritorio activo al final de `to`, donde
    /// pasa a ser la enfocada. El escritorio activo no cambia. `None`, sin
    /// cambiar nada, si no hay ventana enfocada o `to` es el activo o está
    /// fuera de rango.
    pub fn move_focused(&mut self, to: usize) -> Option<Moved> {
        let window = self.current().focused()?;
        self.move_window(window, to, true)
    }

    /// Lleva una ventana de cualquier escritorio al final de `to`, sin
    /// cambiar el escritorio activo (`_NET_WM_DESKTOP`, ADR 0008). Con
    /// `focus`, pasa a ser la enfocada de `to`; si no, no entra en su
    /// historial. `None`, sin cambiar nada, si no está gestionada o `to` es su
    /// escritorio o está fuera de rango.
    pub fn move_window(&mut self, window: Window, to: usize, focus: bool) -> Option<Moved> {
        let from = self.locate(window)?;
        if to >= COUNT || to == from {
            return None;
        }
        let (client, was_focused) = self.desks[from].take(window)?;
        let kind = client.kind;
        self.desks[to].adopt(client, focus);
        Some(Moved {
            window,
            kind,
            from,
            to,
            was_focused,
        })
    }

    #[cfg(test)]
    fn assert_consistent(&self) {
        assert!(self.current < COUNT);
        assert_ne!(self.previous, Some(self.current));
        assert!(self.previous.is_none_or(|p| p < COUNT));
        let list = self.client_list();
        let total: usize = self.desks.iter().map(|d| d.windows().count()).sum();
        assert_eq!(list.len(), total, "_NET_CLIENT_LIST incompleta");
        for (i, w) in list.iter().enumerate() {
            assert!(!list[i + 1..].contains(w), "_NET_CLIENT_LIST repetida");
        }
        for (i, desk) in self.desks.iter().enumerate() {
            desk.assert_consistent();
            for w in desk.windows() {
                assert_eq!(self.locate(w), Some(i), "ventana {w} en varios escritorios");
                let others = self.desks.iter().filter(|d| d.get(w).is_some()).count();
                assert_eq!(others, 1, "ventana {w} en {others} escritorios");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::InputModel;

    const TILED: Class = Class {
        kind: Kind::Tiled,
        focusable: true,
        input: InputModel::Passive,
        delete_window: false,
    };
    const FLOATING: Class = Class {
        kind: Kind::Floating,
        ..TILED
    };
    const PANEL: Class = Class {
        kind: Kind::Floating,
        focusable: false,
        input: InputModel::Passive,
        delete_window: false,
    };

    fn tiled(ws: &Workspaces, desk: usize) -> Vec<Window> {
        ws.desk(desk).unwrap().tiled().collect()
    }

    #[test]
    fn las_ventanas_nuevas_van_al_activo() {
        let mut ws = Workspaces::default();
        assert_eq!(ws.index(), 0);
        assert!(ws.manage(1, TILED));
        ws.switch(3);
        assert!(ws.manage(2, TILED));
        assert!(ws.manage(3, FLOATING));
        assert_eq!(ws.locate(1), Some(0));
        assert_eq!(ws.locate(2), Some(3));
        assert_eq!(ws.locate(3), Some(3));
        assert_eq!(ws.locate(9), None);
        // Una ventana ya gestionada en otro escritorio no se duplica.
        assert!(!ws.manage(1, TILED));
        assert_eq!(ws.locate(1), Some(0));
        assert_eq!(ws.get(1).map(|c| c.kind), Some(Kind::Tiled));
        assert_eq!(ws.current().windows().collect::<Vec<_>>(), vec![2, 3]);
        ws.assert_consistent();
    }

    #[test]
    fn cambiar_de_escritorio() {
        let mut ws = Workspaces::default();
        assert_eq!(ws.switch(0), None, "mismo escritorio");
        assert_eq!(ws.switch(COUNT), None, "fuera de rango");
        assert_eq!(ws.back(), None, "sin escritorio anterior");
        assert_eq!(ws.previous, None);
        assert_eq!(ws.switch(2), Some(Switch { from: 0, to: 2 }));
        assert_eq!(ws.switch(2), None);
        assert_eq!(ws.previous, Some(0));
        assert_eq!(ws.switch(7), Some(Switch { from: 2, to: 7 }));
        // Volver alterna entre los dos últimos.
        assert_eq!(ws.back(), Some(Switch { from: 7, to: 2 }));
        assert_eq!(ws.back(), Some(Switch { from: 2, to: 7 }));
        assert_eq!(ws.back(), Some(Switch { from: 7, to: 2 }));
        assert_eq!(ws.index(), 2);
        ws.assert_consistent();
        // Todos los escritorios son accesibles.
        for to in 0..COUNT {
            ws.switch(to);
            assert_eq!(ws.index(), to);
            ws.assert_consistent();
        }
    }

    #[test]
    fn mosaicos_e_historiales_independientes() {
        let mut ws = Workspaces::default();
        for w in [1, 2, 3] {
            ws.manage(w, TILED);
            ws.current_mut().focus(w);
        }
        ws.switch(1);
        for w in [11, 12] {
            ws.manage(w, TILED);
            ws.current_mut().focus(w);
        }
        ws.current_mut().focus(11);
        // La enfocada es la maestra: se intercambia con la segunda.
        assert!(ws.current_mut().zoom());
        assert_eq!(tiled(&ws, 1), vec![12, 11]);
        assert!(ws.current_mut().zoom());
        assert_eq!(tiled(&ws, 1), vec![11, 12]);
        // El escritorio 0 conserva su orden y su foco.
        assert_eq!(tiled(&ws, 0), vec![1, 2, 3]);
        assert_eq!(ws.desk(0).unwrap().focused(), Some(3));
        ws.switch(0);
        assert_eq!(ws.current().focused(), Some(3));
        assert_eq!(
            ws.current().neighbour(crate::clients::Direction::Next),
            Some(1)
        );
        ws.back();
        assert_eq!(ws.current().focused(), Some(11));
        ws.assert_consistent();
    }

    #[test]
    fn mover_la_enfocada() {
        let mut ws = Workspaces::default();
        for w in [1, 2, 3] {
            ws.manage(w, TILED);
            ws.current_mut().focus(w);
        }
        ws.current_mut().focus(2);
        ws.switch(4);
        ws.manage(40, TILED);
        ws.current_mut().focus(40);
        ws.switch(0);

        assert_eq!(
            ws.move_focused(4),
            Some(Moved {
                window: 2,
                kind: Kind::Tiled,
                from: 0,
                to: 4,
                was_focused: true,
            })
        );
        // Sale del origen, que vuelve a la anterior del historial.
        assert_eq!(tiled(&ws, 0), vec![1, 3]);
        assert_eq!(ws.current().focused(), Some(3));
        // Entra al final del destino, como enfocada.
        assert_eq!(tiled(&ws, 4), vec![40, 2]);
        assert_eq!(ws.desk(4).unwrap().focused(), Some(2));
        assert_eq!(ws.locate(2), Some(4));
        // El activo no cambia.
        assert_eq!(ws.index(), 0);
        ws.assert_consistent();
    }

    #[test]
    fn mover_sin_efecto() {
        let mut ws = Workspaces::default();
        assert_eq!(ws.move_focused(1), None, "sin ventanas");
        ws.manage(1, TILED);
        ws.manage(2, PANEL);
        assert_eq!(ws.move_focused(1), None, "sin ventana enfocada");
        ws.current_mut().focus(1);
        assert_eq!(ws.move_focused(0), None, "mismo escritorio");
        assert_eq!(ws.move_focused(COUNT), None, "fuera de rango");
        assert_eq!(ws.current().windows().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(ws.current().focused(), Some(1));
        ws.assert_consistent();
    }

    #[test]
    fn mover_una_flotante() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(2, FLOATING);
        ws.current_mut().focus(1);
        ws.current_mut().focus(2);
        let moved = ws.move_focused(7).unwrap();
        assert_eq!((moved.window, moved.kind), (2, Kind::Floating));
        assert_eq!(ws.desk(7).unwrap().floating().collect::<Vec<_>>(), vec![2]);
        assert_eq!(ws.current().fallback(), Some(1));
        ws.assert_consistent();
    }

    #[test]
    fn retirar_desde_un_escritorio_inactivo() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(2, TILED);
        ws.current_mut().focus(2);
        ws.switch(5);
        ws.manage(50, TILED);
        ws.current_mut().focus(50);

        let (desk, removed) = ws.unmanage(2).unwrap();
        assert_eq!(desk, 0);
        assert!(removed.was_focused);
        // El escritorio activo no se ve afectado.
        assert_eq!(ws.current().focused(), Some(50));
        assert_eq!(ws.desk(0).unwrap().fallback(), Some(1));
        assert_eq!(ws.unmanage(2), None, "evento repetido");
        assert_eq!(ws.unmanage(9), None, "ventana ajena");
        ws.assert_consistent();
    }

    #[test]
    fn mover_cualquier_ventana() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(2, TILED);
        ws.current_mut().focus(1);
        ws.switch(3);
        ws.manage(30, FLOATING);
        ws.current_mut().focus(30);

        // Desde un escritorio oculto a otro oculto, sin tocar el activo.
        let moved = ws.move_window(1, 5, false).unwrap();
        assert_eq!((moved.from, moved.to, moved.was_focused), (0, 5, true));
        assert_eq!(ws.locate(1), Some(5));
        assert_eq!(
            ws.desk(5).unwrap().focused(),
            None,
            "sin foco en el destino"
        );
        assert_eq!(ws.desk(0).unwrap().fallback(), Some(2));
        assert_eq!(ws.index(), 3);
        assert_eq!(ws.current().focused(), Some(30));

        // Hacia el escritorio activo.
        let moved = ws.move_window(2, 3, false).unwrap();
        assert_eq!((moved.from, moved.to, moved.was_focused), (0, 3, false));
        assert_eq!(ws.current().windows().collect::<Vec<_>>(), vec![30, 2]);
        assert_eq!(ws.current().focused(), Some(30));

        assert_eq!(ws.move_window(2, 3, false), None, "mismo escritorio");
        assert_eq!(ws.move_window(2, COUNT, false), None, "fuera de rango");
        assert_eq!(ws.move_window(9, 1, false), None, "no gestionada");
        ws.assert_consistent();
    }

    #[test]
    fn lista_de_clientes_en_orden_de_gestion() {
        let mut ws = Workspaces::default();
        ws.manage(7, TILED);
        ws.switch(2);
        ws.manage(3, FLOATING);
        ws.manage(5, TILED);
        ws.switch(0);
        ws.manage(1, PANEL);
        assert_eq!(ws.client_list(), vec![7, 3, 5, 1]);
        // Mover no cambia el orden; retirar quita la ventana; volver a
        // gestionarla la pone al final.
        ws.current_mut().focus(7);
        ws.move_focused(6);
        assert_eq!(ws.client_list(), vec![7, 3, 5, 1]);
        ws.unmanage(3);
        assert_eq!(ws.client_list(), vec![7, 5, 1]);
        ws.manage(3, TILED);
        assert_eq!(ws.client_list(), vec![7, 5, 1, 3]);
        assert!(!ws.manage(5, TILED));
        assert_eq!(ws.client_list(), vec![7, 5, 1, 3]);
        ws.assert_consistent();
    }

    /// Secuencia larga de operaciones pseudoaleatorias (generador
    /// congruencial con semilla fija): los invariantes se mantienen siempre.
    #[test]
    fn invariantes_tras_muchas_operaciones() {
        let mut ws = Workspaces::default();
        let mut state: u64 = 0x5eed;
        let mut next = |n: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % n
        };
        let classes = [TILED, FLOATING, PANEL];
        for _ in 0..5_000 {
            let window = Window::try_from(next(24)).unwrap() + 1;
            let desk = usize::try_from(next(9)).unwrap();
            match next(9) {
                0 | 1 => {
                    let class = classes[usize::try_from(next(3)).unwrap()];
                    let was_managed = ws.locate(window).is_some();
                    assert_eq!(ws.manage(window, class), !was_managed);
                }
                2 => {
                    let was = ws.locate(window);
                    assert_eq!(ws.unmanage(window).map(|(d, _)| d), was);
                }
                3 => {
                    ws.current_mut().focus(window);
                }
                4 => {
                    let before = ws.index();
                    let switched = ws.switch(desk);
                    assert_eq!(switched.is_some(), desk < COUNT && desk != before);
                }
                5 => {
                    ws.back();
                }
                6 => {
                    let focused = ws.current().focused();
                    match ws.move_focused(desk) {
                        Some(moved) => {
                            assert_eq!(Some(moved.window), focused);
                            assert_eq!(ws.locate(moved.window), Some(desk));
                            assert_eq!(ws.desk(desk).unwrap().focused(), focused);
                        }
                        None => assert!(focused.is_none() || desk >= COUNT || desk == ws.index()),
                    }
                }
                7 => {
                    ws.current_mut().zoom();
                }
                8 if next(2) == 0 => {
                    let from = ws.locate(window);
                    match ws.move_window(window, desk, next(2) == 0) {
                        Some(moved) => {
                            assert_eq!(Some(moved.from), from);
                            assert_eq!(ws.locate(window), Some(desk));
                        }
                        None => assert!(from.is_none() || from == Some(desk) || desk >= COUNT),
                    }
                }
                _ => {
                    if let Some(w) = ws.current().neighbour(crate::clients::Direction::Next) {
                        assert!(ws.current_mut().focus(w));
                    }
                }
            }
            ws.assert_consistent();
        }
    }
}
