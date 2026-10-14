//! Escritorios virtuales (ADR 0007): ocho conjuntos independientes de
//! ventanas, cada uno con su propio orden master-stack e historial de foco.
//! Sin E/S: `wm.rs` muestra, oculta y enfoca las ventanas según lo que
//! decide este módulo.
//!
//! Los escritorios se numeran internamente de 0 a 7 (como
//! `_NET_CURRENT_DESKTOP`) y se muestran al usuario de 1 a 8.

use x11rb::protocol::xproto::Window;

use crate::clients::{Class, Client, Clients, Kind, Removed, Restore};

/// Número fijo de escritorios.
pub const COUNT: usize = 8;

/// Máximo de eslabones de una cadena de propietarios (`WM_TRANSIENT_FOR`,
/// ADR 0010). Una ventana cuya cadena es más larga no tiene propietario
/// efectivo.
pub const MAX_CHAIN: usize = 16;

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
    /// Ventana del destino que deja la pantalla completa porque llega otra
    /// que la tiene (la última gana, ADR 0009).
    pub displaced: Option<(Window, Restore)>,
}

/// Una ventana ha entrado en pantalla completa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entered {
    pub desk: usize,
    /// La que la tenía en ese escritorio y la deja.
    pub displaced: Option<(Window, Restore)>,
}

/// Invariantes:
/// - cada ventana gestionada está en un único escritorio;
/// - `current` es menor que `COUNT`;
/// - `previous`, si existe, es distinto de `current`;
/// - el historial de cada escritorio solo contiene ventanas de ese
///   escritorio que admiten foco (lo garantiza `Clients`);
/// - como mucho una ventana a pantalla completa por escritorio (ADR 0009).
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

    pub fn desk_mut(&mut self, index: usize) -> Option<&mut Clients> {
        self.desks.get_mut(index)
    }

    /// Pone la ventana a pantalla completa con lo necesario para
    /// restaurarla. Si otra la tenía en su escritorio, la deja (la última
    /// gana) y se devuelve para restaurarla. `None`, sin cambiar nada, si no
    /// está gestionada o ya estaba a pantalla completa (idempotente).
    pub fn enter_fullscreen(&mut self, window: Window, restore: Restore) -> Option<Entered> {
        let desk = self.locate(window)?;
        if self.desks[desk].get(window)?.fullscreen.is_some() {
            return None;
        }
        let displaced = self.desks[desk]
            .fullscreen()
            .and_then(|other| self.leave_fullscreen(other).map(|(_, r)| (other, r)));
        self.desks[desk].get_mut(window)?.fullscreen = Some(restore);
        Some(Entered { desk, displaced })
    }

    /// Quita la pantalla completa y devuelve el escritorio y lo necesario
    /// para restaurarla. Una flotante pierde la geometría de pantalla
    /// completa (`geometry` solo describe lo que coloca el WM). `None`, sin
    /// cambiar nada, si no estaba a pantalla completa (idempotente).
    pub fn leave_fullscreen(&mut self, window: Window) -> Option<(usize, Restore)> {
        let desk = self.locate(window)?;
        let client = self.desks[desk].get_mut(window)?;
        let restore = client.fullscreen.take()?;
        if client.kind == Kind::Floating {
            client.geometry = None;
        }
        Some((desk, restore))
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

    /// Propietario declarado de una ventana, si está gestionado.
    fn declared_managed(&self, window: Window) -> Option<Window> {
        self.get(window)?
            .transient_for
            .filter(|&owner| self.get(owner).is_some())
    }

    /// Propietario efectivo (ADR 0010): el declarado en `WM_TRANSIENT_FOR`,
    /// solo si está gestionado (en cualquier escritorio) y la cadena de
    /// propietarios declarados que empieza en él no vuelve a la ventana ni
    /// tiene más de `MAX_CHAIN` eslabones. Las ventanas de un ciclo no
    /// tienen propietario efectivo; una que apunta a un ciclo sin formar
    /// parte de él, sí (el miembro del ciclo es su raíz). Se resuelve cada
    /// vez, así que una ventana destruida deja de ser propietaria sin
    /// limpiar nada.
    pub fn owner_of(&self, window: Window) -> Option<Window> {
        let declared = self.declared_managed(window)?;
        let mut seen = vec![window];
        let mut current = declared;
        loop {
            if seen.contains(&current) {
                // Ciclo: inválido si pasa por la ventana.
                return (current != window).then_some(declared);
            }
            if seen.len() > MAX_CHAIN {
                return None;
            }
            seen.push(current);
            match self.declared_managed(current) {
                Some(next) => current = next,
                None => return Some(declared),
            }
        }
    }

    /// Dependientes transitivos (diálogos, sus diálogos…) de una ventana, en
    /// todos los escritorios y en orden de gestión.
    pub fn dependents(&self, window: Window) -> Vec<Window> {
        let all = self.client_list();
        let mut family = vec![window];
        loop {
            let before = family.len();
            for &w in &all {
                if !family.contains(&w) && self.owner_of(w).is_some_and(|o| family.contains(&o)) {
                    family.push(w);
                }
            }
            if family.len() == before {
                break;
            }
        }
        all.into_iter()
            .filter(|w| *w != window && family.contains(w))
            .collect()
    }

    /// A quién vuelve el foco al cerrarse un diálogo enfocado de `desk`: a su
    /// propietario efectivo si sigue en ese escritorio y admite foco (ADR
    /// 0010). `None`: se usa el historial.
    pub fn focus_return(&self, owner: Option<Window>, desk: usize) -> Option<Window> {
        owner.filter(|&o| self.locate(o) == Some(desk) && self.get(o).is_some_and(|c| c.focusable))
    }

    /// Orden en que se suben las flotantes de un escritorio (ADR 0010): cada
    /// familia (una flotante sin propietario efectivo entre ellas y sus
    /// dependientes flotantes) en orden de árbol, de modo que cada diálogo
    /// queda justo encima de su propietario; las familias, de la que tiene
    /// la ventana más antigua a la que tiene la más reciente, para que un
    /// diálogo nuevo quede visible. Las de mosaico nunca se reordenan.
    pub fn floating_stack(&self, desk: usize) -> Vec<Window> {
        let Some(clients) = self.desks.get(desk) else {
            return Vec::new();
        };
        let order = |w: Window| self.get(w).map_or(0, |c| c.order);
        let mut floating: Vec<Window> = clients.floating().collect();
        floating.sort_by_key(|&w| order(w));
        let parent = |w: Window| self.owner_of(w).filter(|o| floating.contains(o));
        let children = |p: Window| -> Vec<Window> {
            floating
                .iter()
                .copied()
                .filter(|&w| parent(w) == Some(p))
                .collect()
        };
        let mut roots: Vec<(u64, Window)> = Vec::new();
        for &root in floating.iter().filter(|&&w| parent(w).is_none()) {
            let mut newest = order(root);
            let mut stack = vec![root];
            let mut visited = Vec::new();
            while let Some(w) = stack.pop() {
                if visited.contains(&w) {
                    continue;
                }
                visited.push(w);
                newest = newest.max(order(w));
                stack.extend(children(w));
            }
            roots.push((newest, root));
        }
        roots.sort_by_key(|&(newest, root)| (newest, order(root)));
        let mut result = Vec::new();
        for (_, root) in roots {
            let mut stack = vec![root];
            while let Some(w) = stack.pop() {
                if result.contains(&w) {
                    continue;
                }
                result.push(w);
                // Al revés: el primer hijo sale antes de la pila.
                stack.extend(children(w).into_iter().rev());
            }
        }
        result
    }

    /// Lleva una ventana y sus dependientes transitivos que estén en su
    /// mismo escritorio a `to`, conservando su orden; el escritorio activo
    /// no cambia (ADR 0010). Con `focus`, la ventana pasa a ser la enfocada
    /// del destino. Vacío si no se mueve nada.
    pub fn move_family(&mut self, window: Window, to: usize, focus: bool) -> Vec<Moved> {
        let Some(from) = self.locate(window) else {
            return Vec::new();
        };
        let family: Vec<Window> = self
            .dependents(window)
            .into_iter()
            .filter(|&d| self.locate(d) == Some(from))
            .collect();
        let Some(main) = self.move_window(window, to, focus) else {
            return Vec::new();
        };
        let mut moved = vec![main];
        moved.extend(
            family
                .into_iter()
                .filter_map(|d| self.move_window(d, to, false)),
        );
        moved
    }

    /// Lleva la ventana enfocada del escritorio activo, con su familia del
    /// mismo escritorio (ADR 0010), al final de `to`, donde pasa a ser la
    /// enfocada. El escritorio activo no cambia. Vacío, sin cambiar nada, si
    /// no hay ventana enfocada o `to` es el activo o está fuera de rango.
    pub fn move_focused(&mut self, to: usize) -> Vec<Moved> {
        match self.current().focused() {
            Some(window) => self.move_family(window, to, true),
            None => Vec::new(),
        }
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
        // Una ventana a pantalla completa la conserva; si el destino ya
        // tenía otra, esa la deja (la última gana).
        let displaced = match (client.fullscreen, self.desks[to].fullscreen()) {
            (Some(_), Some(other)) => self.leave_fullscreen(other).map(|(_, r)| (other, r)),
            _ => None,
        };
        self.desks[to].adopt(client, focus);
        Some(Moved {
            window,
            kind,
            from,
            to,
            was_focused,
            displaced,
        })
    }

    #[cfg(test)]
    fn assert_consistent(&self) {
        assert!(self.current < COUNT);
        assert_ne!(self.previous, Some(self.current));
        assert!(self.previous.is_none_or(|p| p < COUNT));
        let list = self.client_list();
        for &w in &list {
            // La relación efectiva no tiene ciclos y siempre apunta a una
            // ventana gestionada distinta.
            let mut current = w;
            for _ in 0..=list.len() {
                match self.owner_of(current) {
                    Some(owner) => {
                        assert_ne!(owner, current, "propia propietaria {w}");
                        assert!(self.get(owner).is_some(), "propietario no gestionado");
                        current = owner;
                    }
                    None => break,
                }
            }
            assert!(self.owner_of(current).is_none(), "ciclo efectivo desde {w}");
        }
        let total: usize = self.desks.iter().map(|d| d.windows().count()).sum();
        assert_eq!(list.len(), total, "_NET_CLIENT_LIST incompleta");
        for (i, w) in list.iter().enumerate() {
            assert!(!list[i + 1..].contains(w), "_NET_CLIENT_LIST repetida");
        }
        for (i, desk) in self.desks.iter().enumerate() {
            let fullscreen = desk
                .windows()
                .filter(|&w| desk.get(w).is_some_and(|c| c.fullscreen.is_some()))
                .count();
            assert!(fullscreen <= 1, "{fullscreen} a pantalla completa en {i}");
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
        transient_for: None,
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
        transient_for: None,
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
            vec![Moved {
                window: 2,
                kind: Kind::Tiled,
                from: 0,
                to: 4,
                was_focused: true,
                displaced: None,
            }]
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
        assert!(ws.move_focused(1).is_empty(), "sin ventanas");
        ws.manage(1, TILED);
        ws.manage(2, PANEL);
        assert!(ws.move_focused(1).is_empty(), "sin ventana enfocada");
        ws.current_mut().focus(1);
        assert!(ws.move_focused(0).is_empty(), "mismo escritorio");
        assert!(ws.move_focused(COUNT).is_empty(), "fuera de rango");
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
        let moved = ws.move_focused(7)[0];
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

    // U17: pantalla completa por escritorio.

    const TILED_RESTORE: Restore = Restore { frame: None };

    #[test]
    fn una_pantalla_completa_por_escritorio() {
        let mut ws = Workspaces::default();
        for w in [1, 2, 3] {
            ws.manage(w, TILED);
        }
        let entered = ws.enter_fullscreen(2, TILED_RESTORE).unwrap();
        assert_eq!((entered.desk, entered.displaced), (0, None));
        assert_eq!(ws.current().fullscreen(), Some(2));
        // Idempotente.
        assert_eq!(ws.enter_fullscreen(2, TILED_RESTORE), None);
        // La última gana: la anterior sale y se devuelve para restaurarla.
        let entered = ws.enter_fullscreen(3, TILED_RESTORE).unwrap();
        assert_eq!(entered.displaced, Some((2, TILED_RESTORE)));
        assert_eq!(ws.current().fullscreen(), Some(3));
        assert!(ws.get(2).unwrap().fullscreen.is_none());
        assert_eq!(ws.leave_fullscreen(3), Some((0, TILED_RESTORE)));
        assert_eq!(ws.leave_fullscreen(3), None, "idempotente");
        assert_eq!(ws.enter_fullscreen(9, TILED_RESTORE), None, "no gestionada");
        ws.assert_consistent();
    }

    #[test]
    fn pantalla_completa_independiente_por_escritorio() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.enter_fullscreen(1, TILED_RESTORE);
        ws.switch(2);
        ws.manage(20, FLOATING);
        let restore = Restore {
            frame: Some(crate::clients::Frame {
                rect: crate::layout::Rect {
                    x: 5,
                    y: 6,
                    width: 70,
                    height: 80,
                },
                border: 1,
            }),
        };
        // Una ventana de un escritorio oculto también puede entrar.
        assert_eq!(ws.enter_fullscreen(20, restore).unwrap().displaced, None);
        assert_eq!(ws.desk(0).unwrap().fullscreen(), Some(1));
        assert_eq!(ws.desk(2).unwrap().fullscreen(), Some(20));
        // Una flotante pierde la geometría de pantalla completa al salir.
        ws.desk_mut(2).unwrap().set_geometry(
            20,
            crate::layout::Rect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
        );
        assert_eq!(ws.leave_fullscreen(20), Some((2, restore)));
        assert_eq!(ws.get(20).unwrap().geometry, None);
        ws.assert_consistent();
    }

    #[test]
    fn mover_una_ventana_a_pantalla_completa() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(2, TILED);
        ws.current_mut().focus(1);
        ws.enter_fullscreen(1, TILED_RESTORE);
        ws.switch(3);
        ws.manage(30, TILED);
        ws.enter_fullscreen(30, TILED_RESTORE);
        ws.switch(0);
        // Conserva el estado; en el destino, la anterior lo deja.
        let moved = ws.move_focused(3)[0];
        assert_eq!(moved.displaced, Some((30, TILED_RESTORE)));
        assert_eq!(ws.desk(3).unwrap().fullscreen(), Some(1));
        assert_eq!(ws.current().fullscreen(), None);
        // Sin otra en el destino, no hay desplazada.
        ws.enter_fullscreen(2, TILED_RESTORE);
        assert_eq!(ws.move_window(2, 5, false).unwrap().displaced, None);
        assert_eq!(ws.desk(5).unwrap().fullscreen(), Some(2));
        // Una ventana normal no desplaza a la de pantalla completa.
        ws.manage(4, TILED);
        assert_eq!(ws.move_window(4, 5, false).unwrap().displaced, None);
        assert_eq!(ws.desk(5).unwrap().fullscreen(), Some(2));
        ws.assert_consistent();
    }

    // U20: relaciones entre ventanas (WM_TRANSIENT_FOR).

    fn transient(owner: Window) -> Class {
        Class {
            transient_for: Some(owner),
            ..FLOATING
        }
    }

    #[test]
    fn propietario_efectivo() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(2, transient(1));
        ws.manage(3, transient(2));
        ws.manage(4, transient(99));
        assert_eq!(ws.owner_of(2), Some(1));
        assert_eq!(ws.owner_of(3), Some(2), "cadena");
        assert_eq!(ws.owner_of(4), None, "propietario pendiente");
        assert_eq!(ws.owner_of(1), None);
        // El propietario pendiente llega: la relación empieza a valer.
        ws.manage(99, TILED);
        assert_eq!(ws.owner_of(4), Some(99));
        // En otro escritorio también es propietario.
        ws.move_window(99, 5, false);
        assert_eq!(ws.owner_of(4), Some(99));
        // Al desaparecer, deja de serlo sin limpiar nada.
        ws.unmanage(1);
        assert_eq!(ws.owner_of(2), None);
        assert_eq!(ws.owner_of(3), Some(2));
        ws.assert_consistent();
    }

    #[test]
    fn ciclos_y_cadenas_largas() {
        let mut ws = Workspaces::default();
        // Ciclo de dos: 1 → 2 → 1.
        ws.manage(1, transient(2));
        ws.manage(2, transient(1));
        assert_eq!((ws.owner_of(1), ws.owner_of(2)), (None, None));
        // Ciclo de tres: 3 → 4 → 5 → 3.
        ws.manage(3, transient(4));
        ws.manage(4, transient(5));
        ws.manage(5, transient(3));
        for w in [3, 4, 5] {
            assert_eq!(ws.owner_of(w), None, "{w}");
        }
        // Una ventana que apunta a un ciclo sin formar parte de él.
        ws.manage(6, transient(3));
        assert_eq!(ws.owner_of(6), Some(3));
        // Cadena de 16 eslabones: vale; de 17: no.
        ws.manage(100, TILED);
        for w in 101..=117 {
            ws.manage(w, transient(w - 1));
        }
        assert_eq!(ws.owner_of(116), Some(115), "16 eslabones hasta la raíz");
        assert_eq!(ws.owner_of(117), None, "17 eslabones");
        assert_eq!(ws.owner_of(101), Some(100));
        ws.assert_consistent();
    }

    #[test]
    fn dependientes_y_retorno_del_foco() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(2, transient(1));
        ws.manage(3, TILED);
        ws.manage(4, transient(2));
        ws.manage(5, transient(1));
        assert_eq!(ws.dependents(1), vec![2, 4, 5], "transitivos, en orden");
        assert_eq!(ws.dependents(2), vec![4]);
        assert_eq!(ws.dependents(3), Vec::<Window>::new());
        // El foco vuelve al propietario del mismo escritorio que admite foco.
        assert_eq!(ws.focus_return(ws.owner_of(4), 0), Some(2));
        assert_eq!(ws.focus_return(None, 0), None);
        ws.move_window(2, 3, false);
        assert_eq!(ws.focus_return(ws.owner_of(4), 0), None, "otro escritorio");
        ws.manage(7, PANEL);
        ws.manage(8, transient(7));
        assert_eq!(ws.focus_return(ws.owner_of(8), 0), None, "sin foco");
    }

    #[test]
    fn pila_jerarquica_de_flotantes() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(10, FLOATING); // flotante sin propietario (antigua)
        ws.manage(2, transient(1)); // diálogo de una de mosaico
        ws.manage(20, FLOATING); // otra flotante sin propietario
        ws.manage(3, transient(2)); // subdiálogo
        assert_eq!(ws.floating_stack(0), vec![10, 20, 2, 3]);
        // Un diálogo nuevo de la flotante más antigua: su familia sube.
        ws.manage(11, transient(10));
        assert_eq!(ws.floating_stack(0), vec![20, 2, 3, 10, 11]);
        // Cada diálogo, justo encima de su propietario y de los anteriores.
        ws.manage(12, transient(10));
        ws.manage(13, transient(11));
        assert_eq!(ws.floating_stack(0), vec![20, 2, 3, 10, 11, 13, 12]);
        // Los ciclos no rompen el orden ni lo repiten.
        ws.manage(30, transient(31));
        ws.manage(31, transient(30));
        let stack = ws.floating_stack(0);
        assert_eq!(stack.len(), 9);
        assert_eq!(&stack[7..], &[30, 31]);
        assert_eq!(ws.floating_stack(COUNT), Vec::<Window>::new());
    }

    #[test]
    fn mover_una_familia() {
        let mut ws = Workspaces::default();
        ws.manage(1, TILED);
        ws.manage(2, transient(1));
        ws.manage(3, transient(2));
        ws.manage(4, TILED);
        ws.manage(5, transient(1));
        ws.move_window(5, 6, false); // diálogo ya en otro escritorio
        ws.current_mut().focus(1);
        let moved = ws.move_family(1, 3, true);
        let windows: Vec<Window> = moved.iter().map(|m| m.window).collect();
        assert_eq!(
            windows,
            vec![1, 2, 3],
            "solo los del mismo escritorio, en orden"
        );
        assert_eq!(
            ws.desk(3).unwrap().windows().collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(ws.desk(3).unwrap().focused(), Some(1));
        assert_eq!(ws.locate(5), Some(6));
        assert_eq!(ws.locate(4), Some(0));
        // Mover solo un diálogo lleva sus subdiálogos, no a su propietario.
        let moved = ws.move_family(2, 7, false);
        assert_eq!(
            moved.iter().map(|m| m.window).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(ws.locate(1), Some(3));
        assert_eq!(ws.owner_of(2), Some(1), "la relación se mantiene");
        assert!(ws.move_family(2, 7, false).is_empty(), "mismo escritorio");
        assert!(ws.move_family(99, 1, false).is_empty(), "no gestionada");
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
                    match ws.move_focused(desk).first().copied() {
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
                8 if next(4) == 0 => {
                    // Relaciones arbitrarias, también ciclos y la propia
                    // ventana: los invariantes deben mantenerse.
                    let owner = Window::try_from(next(26)).unwrap();
                    if let Some(client) = ws.get_mut(window) {
                        client.transient_for = (owner != 0).then_some(owner);
                    }
                    if next(2) == 0 {
                        ws.move_family(window, desk, false);
                    }
                    let _ = ws.floating_stack(ws.index());
                }
                8 if next(3) == 0 => {
                    if next(2) == 0 {
                        let entered = ws.enter_fullscreen(window, Restore { frame: None });
                        if let Some(e) = entered {
                            assert_eq!(ws.desk(e.desk).unwrap().fullscreen(), Some(window));
                        }
                    } else {
                        ws.leave_fullscreen(window);
                    }
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
