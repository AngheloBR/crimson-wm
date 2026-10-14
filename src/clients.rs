//! Ventanas gestionadas, su clasificación (ADR 0004) y el foco (ADR 0005).
//! Sin E/S: `wm.rs` lee las propiedades del servidor y aplica las
//! decisiones.

use x11rb::protocol::xproto::{Atom, Window};

use crate::icccm::Protocols;
use crate::layout::{self, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Forma parte del mosaico; su geometría la decide el WM.
    Tiled,
    /// Conserva la geometría que pide y queda por encima del mosaico.
    Floating,
}

/// Modelo de entrada de ICCCM §4.1.7, según `WM_HINTS.input` y la presencia
/// de `WM_TAKE_FOCUS` en `WM_PROTOCOLS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputModel {
    /// `input` verdadero, sin `WM_TAKE_FOCUS`: el WM le da el foco.
    Passive,
    /// `input` verdadero y `WM_TAKE_FOCUS`: el WM le da el foco y se lo
    /// notifica.
    LocallyActive,
    /// `input` falso y `WM_TAKE_FOCUS`: el WM solo se lo notifica y el
    /// cliente decide si toma el foco.
    GloballyActive,
    /// `input` falso, sin `WM_TAKE_FOCUS`: nunca recibe el foco del teclado.
    NoInput,
}

impl InputModel {
    /// Sin `WM_HINTS` o sin el campo `input`, se supone `input` verdadero,
    /// como hacen los gestores habituales.
    pub fn new(input: Option<bool>, take_focus: bool) -> Self {
        match (input.unwrap_or(true), take_focus) {
            (true, false) => InputModel::Passive,
            (true, true) => InputModel::LocallyActive,
            (false, true) => InputModel::GloballyActive,
            (false, false) => InputModel::NoInput,
        }
    }

    /// El WM debe llamar a `SetInputFocus` con la ventana.
    pub fn sets_input_focus(self) -> bool {
        matches!(self, InputModel::Passive | InputModel::LocallyActive)
    }

    /// El WM debe enviar el mensaje `WM_TAKE_FOCUS`.
    pub fn takes_focus(self) -> bool {
        matches!(self, InputModel::LocallyActive | InputModel::GloballyActive)
    }
}

/// Cómo se gestiona una ventana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Class {
    pub kind: Kind,
    /// Puede recibir el foco y entra en la navegación por teclado.
    pub focusable: bool,
    pub input: InputModel,
    /// Anuncia `WM_DELETE_WINDOW`: admite el cierre cooperativo (ADR 0008).
    pub delete_window: bool,
    /// Propietario declarado en `WM_TRANSIENT_FOR`, ya validado (ni 0, ni
    /// la raíz, ni la propia ventana; ADR 0010).
    pub transient_for: Option<Window>,
}

#[derive(Debug)]
pub struct Client {
    pub window: Window,
    pub kind: Kind,
    pub focusable: bool,
    /// Modelo de entrada y soporte de `WM_DELETE_WINDOW`: se actualizan si
    /// cambian `WM_HINTS` o `WM_PROTOCOLS` (`PropertyNotify`, ADR 0008).
    pub input: InputModel,
    pub delete_window: bool,
    /// Última geometría asignada por el WM: en mosaico o a pantalla
    /// completa.
    pub geometry: Option<Rect>,
    /// `Some` mientras está a pantalla completa (ADR 0009). Es la única
    /// fuente de verdad: `_NET_WM_STATE` y la geometría se derivan de él.
    pub fullscreen: Option<Restore>,
    /// Orden de gestión en todos los escritorios, para `_NET_CLIENT_LIST`
    /// (lo asigna `Workspaces`).
    pub order: u64,
    /// Propietario declarado (`WM_TRANSIENT_FOR`, ADR 0010). Solo es un
    /// dato declarado: el propietario efectivo se resuelve al usarlo
    /// (`Workspaces::owner_of`), así que nunca queda una referencia colgante
    /// si el propietario desaparece. Se actualiza con `PropertyNotify`.
    pub transient_for: Option<Window>,
}

/// Geometría completa de una ventana: rectángulo y borde.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub rect: Rect,
    pub border: u16,
}

impl Frame {
    /// Marco para una flotante que sale de pantalla completa sin marco
    /// conocido (adoptada tras una terminación abrupta): centrado, con dos
    /// tercios del área y sin borde. No es su geometría original, que se
    /// desconoce (ADR 0009).
    pub fn fallback(area: Rect) -> Frame {
        let width = (u32::from(area.width) * 2 / 3).max(1);
        let height = (u32::from(area.height) * 2 / 3).max(1);
        let offset =
            |total: u16, size: u32| i16::try_from((u32::from(total) - size) / 2).unwrap_or(0);
        Frame {
            rect: Rect {
                x: area.x.saturating_add(offset(area.width, width)),
                y: area.y.saturating_add(offset(area.height, height)),
                width: u16::try_from(width).unwrap_or(u16::MAX),
                height: u16::try_from(height).unwrap_or(u16::MAX),
            },
            border: 0,
        }
    }
}

/// Estado de pantalla completa de una ventana (ADR 0009): lo necesario para
/// devolverla a su estado anterior. En mosaico no hace falta nada: su hueco
/// sale de su posición en la lista, que no cambia. Una flotante guarda su
/// marco real; `None` si se desconoce (adoptada tras una terminación
/// abrupta).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Restore {
    pub frame: Option<Frame>,
}

/// Resultado de dejar de gestionar una ventana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Removed {
    pub kind: Kind,
    /// Era la ventana enfocada: hay que dar el foco a otra.
    pub was_focused: bool,
    /// Estaba a pantalla completa.
    pub fullscreen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Next,
    Prev,
}

/// Ventanas gestionadas, en orden de llegada. Las del mosaico, en ese
/// orden, forman el diseño master-stack: la primera es la maestra.
///
/// `history` guarda las ventanas que han tenido el foco, de la más reciente
/// a la más antigua; la primera es la enfocada. Solo contiene ventanas
/// gestionadas que admiten foco.
///
/// Un `Vec` con búsqueda lineal basta para el número de ventanas de un
/// escritorio y conserva el orden sin estructuras adicionales.
#[derive(Debug, Default)]
pub struct Clients {
    list: Vec<Client>,
    history: Vec<Window>,
}

impl Clients {
    /// Comprueba los invariantes: ventanas sin repetir y un historial sin
    /// repetir que solo contiene ventanas gestionadas que admiten foco.
    #[cfg(test)]
    pub fn assert_consistent(&self) {
        for (i, c) in self.list.iter().enumerate() {
            assert!(
                self.list[i + 1..].iter().all(|o| o.window != c.window),
                "ventana repetida {}",
                c.window
            );
        }
        for (i, &w) in self.history.iter().enumerate() {
            assert!(
                !self.history[i + 1..].contains(&w),
                "historial repetido {w}"
            );
            assert!(
                self.get(w).is_some_and(|c| c.focusable),
                "historial con una ventana ajena o sin foco {w}"
            );
        }
    }

    /// Añade la ventana al final. Devuelve `false`, sin cambiar nada, si ya
    /// estaba gestionada.
    pub fn manage(&mut self, window: Window, class: Class) -> bool {
        if self.get(window).is_some() {
            return false;
        }
        self.list.push(Client {
            window,
            kind: class.kind,
            focusable: class.focusable,
            input: class.input,
            delete_window: class.delete_window,
            geometry: None,
            fullscreen: None,
            order: 0,
            transient_for: class.transient_for,
        });
        true
    }

    /// Deja de gestionar la ventana y la quita del historial de foco.
    /// `None` si no estaba gestionada (eventos duplicados o de ventanas
    /// ajenas).
    pub fn unmanage(&mut self, window: Window) -> Option<Removed> {
        self.take(window).map(|(client, was_focused)| Removed {
            kind: client.kind,
            was_focused,
            fullscreen: client.fullscreen.is_some(),
        })
    }

    /// Saca la ventana de la lista y del historial, para llevarla a otro
    /// conjunto (ADR 0007). Indica también si era la enfocada.
    pub fn take(&mut self, window: Window) -> Option<(Client, bool)> {
        let index = self.list.iter().position(|c| c.window == window)?;
        let was_focused = self.focused() == Some(window);
        self.history.retain(|&w| w != window);
        Some((self.list.remove(index), was_focused))
    }

    /// Añade al final una ventana sacada de otro conjunto con `take`,
    /// conservando su clasificación y su última geometría, que sigue siendo
    /// la real. Con `focused`, pasa al principio del historial. Devuelve
    /// `false`, sin cambiar nada, si ya estaba gestionada.
    pub fn adopt(&mut self, client: Client, focused: bool) -> bool {
        if self.get(client.window).is_some() {
            return false;
        }
        let window = client.window;
        self.list.push(client);
        if focused {
            self.focus(window);
        }
        true
    }

    pub fn get(&self, window: Window) -> Option<&Client> {
        self.list.iter().find(|c| c.window == window)
    }

    pub fn get_mut(&mut self, window: Window) -> Option<&mut Client> {
        self.list.iter_mut().find(|c| c.window == window)
    }

    /// Todas las ventanas, en orden de llegada.
    pub fn windows(&self) -> impl Iterator<Item = Window> + '_ {
        self.list.iter().map(|c| c.window)
    }

    /// La ventana a pantalla completa, si la hay (como mucho una por
    /// escritorio; lo garantiza `Workspaces`).
    pub fn fullscreen(&self) -> Option<Window> {
        self.list
            .iter()
            .find(|c| c.fullscreen.is_some())
            .map(|c| c.window)
    }

    /// Geometría de cada ventana colocada por el WM en `area`: las del
    /// mosaico que no están a pantalla completa, en orden, con
    /// `master_stack`, y la de pantalla completa, sea del tipo que sea, con
    /// el área entera. La ventana a pantalla completa sale del cálculo del
    /// mosaico pero conserva su posición en la lista: al salir recupera su
    /// hueco.
    pub fn plan(&self, area: Rect) -> Vec<(Window, Rect)> {
        let tiled: Vec<Window> = self
            .list
            .iter()
            .filter(|c| c.kind == Kind::Tiled && c.fullscreen.is_none())
            .map(|c| c.window)
            .collect();
        let rects = layout::master_stack(area, tiled.len());
        let mut plan: Vec<(Window, Rect)> = tiled.into_iter().zip(rects).collect();
        plan.extend(self.fullscreen().map(|w| (w, area)));
        plan
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

    /// Ventana enfocada, si la hay.
    pub fn focused(&self) -> Option<Window> {
        self.history.first().copied()
    }

    /// Marca la ventana como enfocada y la pone al principio del historial.
    /// Devuelve `false`, sin cambiar nada, si no está gestionada o no admite
    /// foco.
    pub fn focus(&mut self, window: Window) -> bool {
        if !self.get(window).is_some_and(|c| c.focusable) {
            return false;
        }
        self.history.retain(|&w| w != window);
        self.history.insert(0, window);
        true
    }

    /// Orden de navegación: el mosaico (maestra y pila de arriba abajo) y
    /// después las flotantes en orden de llegada, solo las que admiten foco.
    pub fn focus_order(&self) -> Vec<Window> {
        let focusable = |kind| {
            self.list
                .iter()
                .filter(move |c| c.kind == kind && c.focusable)
                .map(|c| c.window)
        };
        focusable(Kind::Tiled)
            .chain(focusable(Kind::Floating))
            .collect()
    }

    /// Ventana siguiente o anterior a la enfocada en el orden de navegación,
    /// con vuelta al principio o al final. Sin ventana enfocada, la primera
    /// (o la última, hacia atrás).
    pub fn neighbour(&self, direction: Direction) -> Option<Window> {
        let order = self.focus_order();
        let len = order.len();
        if len == 0 {
            return None;
        }
        let current = self
            .focused()
            .and_then(|f| order.iter().position(|&w| w == f));
        let index = match (current, direction) {
            (Some(i), Direction::Next) => (i + 1) % len,
            (Some(i), Direction::Prev) => (i + len - 1) % len,
            (None, Direction::Next) => 0,
            (None, Direction::Prev) => len - 1,
        };
        Some(order[index])
    }

    /// Ventana que debe recibir el foco cuando falta la enfocada: la más
    /// reciente del historial o, si está vacío, la primera del orden de
    /// navegación.
    pub fn fallback(&self) -> Option<Window> {
        self.focused()
            .or_else(|| self.focus_order().first().copied())
    }

    /// Convierte la ventana enfocada en la maestra; si ya lo es, la
    /// intercambia con la segunda. Las demás conservan su orden. Devuelve
    /// `false`, sin cambiar nada, si la enfocada no está en mosaico, no hay
    /// ninguna enfocada o hay menos de dos ventanas en mosaico.
    pub fn zoom(&mut self) -> bool {
        let Some(focused) = self.focused() else {
            return false;
        };
        let tiled: Vec<Window> = self.tiled().collect();
        if tiled.len() < 2 || !tiled.contains(&focused) {
            return false;
        }
        let target = if tiled[0] == focused {
            tiled[1]
        } else {
            focused
        };
        let from = self.list.iter().position(|c| c.window == target);
        let Some(from) = from else {
            return false;
        };
        let client = self.list.remove(from);
        // La maestra actual sigue en la lista: `target` nunca es ella.
        let to = self
            .list
            .iter()
            .position(|c| c.window == tiled[0])
            .unwrap_or(0);
        self.list.insert(to, client);
        true
    }
}

/// Datos de una ventana que deciden cómo se gestiona.
#[derive(Debug, Default)]
pub struct WindowInfo {
    pub override_redirect: bool,
    /// Tiene la propiedad `WM_TRANSIENT_FOR` (diálogo de otra ventana), con
    /// cualquier valor: decide la clasificación.
    pub transient: bool,
    /// Propietario declarado válido de `WM_TRANSIENT_FOR` (ADR 0010).
    pub owner: Option<Window>,
    /// Valores de `_NET_WM_WINDOW_TYPE`, en orden de preferencia.
    pub types: Vec<Atom>,
    /// Campo `input` de `WM_HINTS`, si está presente.
    pub input: Option<bool>,
    /// Protocolos de `WM_PROTOCOLS` que usa el WM.
    pub protocols: Protocols,
    /// `_NET_WM_STATE` contiene `_NET_WM_STATE_FULLSCREEN`: la ventana pide
    /// empezar a pantalla completa (ADR 0009). No afecta a la clasificación.
    pub fullscreen: bool,
}

/// Átomos de `_NET_WM_WINDOW_TYPE` que reconoce la clasificación.
pub struct WindowTypes<'a> {
    pub normal: Atom,
    /// Flotantes que reciben el foco (diálogos, utilidades, barras de
    /// herramientas).
    pub floating: &'a [Atom],
    /// Flotantes que nunca reciben el foco (paneles, escritorio, avisos,
    /// menús…).
    pub unfocusable: &'a [Atom],
}

/// Decide cómo se gestiona una ventana. `None`: no se gestiona
/// (override-redirect).
///
/// Según EWMH, cuenta el primer tipo reconocido de `_NET_WM_WINDOW_TYPE`;
/// sin ninguno reconocido, una ventana con `WM_TRANSIENT_FOR` se trata como
/// un diálogo y las demás como normales.
pub fn classify(info: &WindowInfo, types: &WindowTypes) -> Option<Class> {
    if info.override_redirect {
        return None;
    }
    let declared = info.types.iter().find_map(|&t| {
        if t == types.normal {
            Some((Kind::Tiled, true))
        } else if types.floating.contains(&t) {
            Some((Kind::Floating, true))
        } else if types.unfocusable.contains(&t) {
            Some((Kind::Floating, false))
        } else {
            None
        }
    });
    let (kind, focusable) = declared.unwrap_or(if info.transient {
        (Kind::Floating, true)
    } else {
        (Kind::Tiled, true)
    });
    Some(Class {
        kind,
        focusable,
        input: InputModel::new(info.input, info.protocols.take_focus),
        delete_window: info.protocols.delete_window,
        transient_for: info.owner,
    })
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
        floating: &[DIALOG],
        unfocusable: &[DOCK],
    };

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

    fn rect(width: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width,
            height: 10,
        }
    }

    fn with(classes: &[(Window, Class)]) -> Clients {
        let mut clients = Clients::default();
        for &(w, class) in classes {
            assert!(clients.manage(w, class));
        }
        clients
    }

    #[test]
    fn conserva_el_orden_de_llegada() {
        let clients = with(&[(1, TILED), (2, FLOATING), (3, TILED), (4, TILED)]);
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1, 3, 4]);
        assert_eq!(clients.floating().collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn al_retirar_la_maestra_la_siguiente_pasa_a_serlo() {
        let mut clients = with(&[(1, TILED), (2, TILED), (3, TILED)]);
        assert_eq!(
            clients.unmanage(1),
            Some(Removed {
                kind: Kind::Tiled,
                was_focused: false,
                fullscreen: false
            })
        );
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn retirar_es_idempotente_y_no_afecta_a_ventanas_ajenas() {
        let mut clients = with(&[(1, TILED), (2, FLOATING)]);
        assert_eq!(clients.unmanage(9), None);
        assert_eq!(clients.unmanage(2).map(|r| r.kind), Some(Kind::Floating));
        assert_eq!(clients.unmanage(2), None);
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn gestionar_dos_veces_no_duplica() {
        let mut clients = with(&[(1, TILED)]);
        assert!(!clients.manage(1, FLOATING));
        assert_eq!(clients.get(1).map(|c| c.kind), Some(Kind::Tiled));
        assert_eq!(clients.tiled().count(), 1);
    }

    #[test]
    fn una_ventana_retirada_vuelve_al_final() {
        let mut clients = with(&[(1, TILED), (2, TILED), (3, TILED)]);
        clients.unmanage(2);
        clients.manage(2, TILED);
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1, 3, 2]);
        assert_eq!(clients.get(2).and_then(|c| c.geometry), None);
    }

    #[test]
    fn geometria_solo_cambia_si_es_distinta() {
        let mut clients = with(&[(1, TILED)]);
        assert!(clients.set_geometry(1, rect(10)));
        assert!(!clients.set_geometry(1, rect(10)));
        assert!(clients.set_geometry(1, rect(20)));
        assert_eq!(clients.get(1).and_then(|c| c.geometry), Some(rect(20)));
        assert!(!clients.set_geometry(9, rect(10)));
    }

    // U17: plan de geometría con pantalla completa.

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    };

    fn r(x: i16, y: i16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn plan_sin_pantalla_completa() {
        let clients = with(&[(1, TILED), (9, FLOATING), (2, TILED), (3, TILED)]);
        assert_eq!(
            clients.plan(AREA),
            vec![
                (1, r(0, 0, 640, 720)),
                (2, r(640, 0, 640, 360)),
                (3, r(640, 360, 640, 360)),
            ]
        );
        assert_eq!(clients.fullscreen(), None);
    }

    #[test]
    fn plan_con_pantalla_completa_en_mosaico() {
        let mut clients = with(&[(1, TILED), (2, TILED), (3, TILED)]);
        clients.get_mut(2).unwrap().fullscreen = Some(Restore { frame: None });
        // Las demás se reparten el mosaico; la de pantalla completa, el área.
        assert_eq!(
            clients.plan(AREA),
            vec![(1, r(0, 0, 640, 720)), (3, r(640, 0, 640, 720)), (2, AREA)]
        );
        // Al salir recupera su hueco: su posición en la lista no ha cambiado.
        clients.get_mut(2).unwrap().fullscreen = None;
        assert_eq!(clients.plan(AREA)[1], (2, r(640, 0, 640, 360)));
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn plan_con_pantalla_completa_flotante() {
        let mut clients = with(&[(1, TILED), (9, FLOATING)]);
        clients.get_mut(9).unwrap().fullscreen = Some(Restore {
            frame: Some(Frame {
                rect: r(10, 20, 300, 200),
                border: 2,
            }),
        });
        assert_eq!(clients.plan(AREA), vec![(1, AREA), (9, AREA)]);
        assert_eq!(clients.fullscreen(), Some(9));
        let removed = clients.unmanage(9).unwrap();
        assert!(removed.fullscreen);
        assert_eq!(clients.fullscreen(), None);
    }

    #[test]
    fn marco_de_respaldo() {
        let frame = Frame::fallback(AREA);
        assert_eq!(frame.rect, r(213, 120, 853, 480));
        assert_eq!(frame.border, 0);
        // Centrado también en áreas impares o mínimas.
        assert_eq!(
            Frame::fallback(r(0, 0, 1279, 719)).rect,
            r(213, 120, 852, 479)
        );
        assert_eq!(Frame::fallback(r(0, 0, 1, 1)).rect, r(0, 0, 1, 1));
    }

    // U8: foco.

    #[test]
    fn historial_de_foco() {
        let mut clients = with(&[(1, TILED), (2, TILED), (3, FLOATING), (4, PANEL)]);
        assert_eq!(clients.focused(), None);
        assert!(clients.focus(1));
        assert!(clients.focus(2));
        assert!(clients.focus(3));
        assert!(clients.focus(1));
        assert_eq!(clients.focused(), Some(1));
        assert_eq!(clients.history, vec![1, 3, 2]);
        // Ni ventanas sin foco ni ajenas.
        assert!(!clients.focus(4));
        assert!(!clients.focus(9));
        assert_eq!(clients.history, vec![1, 3, 2]);
    }

    #[test]
    fn al_retirar_la_enfocada_vuelve_la_anterior() {
        let mut clients = with(&[(1, TILED), (2, TILED), (3, FLOATING)]);
        for w in [1, 2, 3] {
            clients.focus(w);
        }
        // Se cierra el diálogo: vuelve la ventana usada antes.
        let removed = clients.unmanage(3).unwrap();
        assert!(removed.was_focused);
        assert_eq!(clients.fallback(), Some(2));
        // Retirar una ventana no enfocada no cambia el foco.
        clients.focus(2);
        let removed = clients.unmanage(1).unwrap();
        assert!(!removed.was_focused);
        assert_eq!(clients.focused(), Some(2));
        // Al retirar la última no queda ninguna.
        assert!(clients.unmanage(2).unwrap().was_focused);
        assert_eq!(clients.fallback(), None);
        assert!(clients.history.is_empty());
    }

    #[test]
    fn sin_historial_el_foco_va_a_la_primera_del_orden() {
        let clients = with(&[(5, FLOATING), (1, TILED), (2, TILED)]);
        assert_eq!(clients.fallback(), Some(1));
        let only_panel = with(&[(4, PANEL)]);
        assert_eq!(only_panel.fallback(), None);
    }

    #[test]
    fn orden_de_navegacion_ciclico() {
        let mut clients = with(&[
            (10, FLOATING),
            (1, TILED),
            (11, PANEL),
            (2, TILED),
            (3, TILED),
        ]);
        assert_eq!(clients.focus_order(), vec![1, 2, 3, 10]);
        assert_eq!(clients.neighbour(Direction::Next), Some(1));
        assert_eq!(clients.neighbour(Direction::Prev), Some(10));
        clients.focus(3);
        assert_eq!(clients.neighbour(Direction::Next), Some(10));
        assert_eq!(clients.neighbour(Direction::Prev), Some(2));
        clients.focus(10);
        assert_eq!(clients.neighbour(Direction::Next), Some(1));
        clients.focus(1);
        assert_eq!(clients.neighbour(Direction::Prev), Some(10));

        let single = {
            let mut c = with(&[(1, TILED)]);
            c.focus(1);
            c
        };
        assert_eq!(single.neighbour(Direction::Next), Some(1));
        assert_eq!(Clients::default().neighbour(Direction::Next), None);
    }

    #[test]
    fn zoom_lleva_la_enfocada_a_maestra() {
        let mut clients = with(&[(1, TILED), (9, FLOATING), (2, TILED), (3, TILED)]);
        clients.focus(3);
        assert!(clients.zoom());
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![3, 1, 2]);
        assert_eq!(clients.focused(), Some(3));
        // Sobre la maestra, la intercambia con la segunda.
        assert!(clients.zoom());
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1, 3, 2]);
        assert_eq!(clients.focused(), Some(3));
        assert_eq!(clients.floating().collect::<Vec<_>>(), vec![9]);
    }

    #[test]
    fn zoom_sin_efecto() {
        let mut clients = with(&[(1, TILED), (9, FLOATING), (2, TILED)]);
        assert!(!clients.zoom(), "sin ventana enfocada");
        clients.focus(9);
        assert!(!clients.zoom(), "enfocada flotante");
        let mut single = with(&[(1, TILED)]);
        single.focus(1);
        assert!(!single.zoom(), "una sola ventana en mosaico");
        assert_eq!(clients.tiled().collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn sacar_y_adoptar() {
        let mut origin = with(&[(1, TILED), (2, FLOATING), (3, TILED)]);
        origin.set_geometry(3, rect(30));
        for w in [1, 3] {
            origin.focus(w);
        }
        let (client, was_focused) = origin.take(3).unwrap();
        assert!(was_focused);
        assert_eq!(origin.tiled().collect::<Vec<_>>(), vec![1]);
        assert_eq!(origin.history, vec![1]);
        assert!(origin.take(3).is_none());

        let mut target = with(&[(5, TILED)]);
        target.focus(5);
        assert!(target.adopt(client, true));
        // Al final, con su clasificación y su geometría, y enfocada.
        assert_eq!(target.tiled().collect::<Vec<_>>(), vec![5, 3]);
        assert_eq!(target.get(3).and_then(|c| c.geometry), Some(rect(30)));
        assert_eq!(target.history, vec![3, 5]);

        // Una flotante sin foco conserva su tipo y no entra en el historial.
        let (client, was_focused) = origin.take(2).unwrap();
        assert!(!was_focused);
        assert!(target.adopt(client, false));
        assert_eq!(target.floating().collect::<Vec<_>>(), vec![2]);
        assert_eq!(target.history, vec![3, 5]);
        assert_eq!(target.windows().collect::<Vec<_>>(), vec![5, 3, 2]);

        // No se duplica.
        let (dup, _) = with(&[(5, TILED)]).take(5).unwrap();
        assert!(!target.adopt(dup, true));
        assert_eq!(target.windows().count(), 3);
    }

    #[test]
    fn modelos_de_entrada() {
        let casos = [
            (None, false, InputModel::Passive),
            (Some(true), false, InputModel::Passive),
            (None, true, InputModel::LocallyActive),
            (Some(true), true, InputModel::LocallyActive),
            (Some(false), true, InputModel::GloballyActive),
            (Some(false), false, InputModel::NoInput),
        ];
        for (input, take, esperado) in casos {
            assert_eq!(InputModel::new(input, take), esperado, "{input:?} {take}");
        }
        assert!(InputModel::Passive.sets_input_focus() && !InputModel::Passive.takes_focus());
        assert!(InputModel::LocallyActive.sets_input_focus());
        assert!(InputModel::LocallyActive.takes_focus());
        assert!(!InputModel::GloballyActive.sets_input_focus());
        assert!(InputModel::GloballyActive.takes_focus());
        assert!(!InputModel::NoInput.sets_input_focus() && !InputModel::NoInput.takes_focus());
    }

    fn info(override_redirect: bool, transient: bool, types: &[Atom]) -> WindowInfo {
        WindowInfo {
            override_redirect,
            transient,
            types: types.to_vec(),
            ..WindowInfo::default()
        }
    }

    #[test]
    fn clasificacion() {
        let casos = [
            (info(false, false, &[]), Some(TILED)),
            (info(false, false, &[NORMAL]), Some(TILED)),
            (info(false, false, &[DIALOG]), Some(FLOATING)),
            (info(false, false, &[DOCK]), Some(PANEL)),
            (info(false, true, &[]), Some(FLOATING)),
            // El tipo explícito prevalece sobre WM_TRANSIENT_FOR.
            (info(false, true, &[NORMAL]), Some(TILED)),
            // Cuenta el primer tipo reconocido.
            (
                info(false, false, &[UNKNOWN, DIALOG, NORMAL]),
                Some(FLOATING),
            ),
            (info(false, false, &[NORMAL, DIALOG]), Some(TILED)),
            (info(false, false, &[UNKNOWN]), Some(TILED)),
            (info(false, true, &[UNKNOWN]), Some(FLOATING)),
            (info(true, false, &[NORMAL]), None),
        ];
        for (info, esperado) in casos {
            assert_eq!(classify(&info, &TYPES), esperado, "{info:?}");
        }
        let no_input = WindowInfo {
            input: Some(false),
            ..WindowInfo::default()
        };
        assert_eq!(
            classify(&no_input, &TYPES).map(|c| c.input),
            Some(InputModel::NoInput)
        );
        // WM_PROTOCOLS: modelo de entrada y cierre cooperativo.
        let protocols = WindowInfo {
            protocols: Protocols {
                delete_window: true,
                take_focus: true,
            },
            ..WindowInfo::default()
        };
        let class = classify(&protocols, &TYPES).unwrap();
        assert_eq!(class.input, InputModel::LocallyActive);
        assert!(class.delete_window);
    }
}
