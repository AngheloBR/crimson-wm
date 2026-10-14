//! Bucle de eventos (ADR 0004 y 0005): las ventanas normales se colocan en
//! mosaico master-stack y se reorganizan al abrirse, retirarse o destruirse;
//! los diálogos y demás ventanas auxiliares quedan flotantes. El foco se
//! gestiona con el teclado (`Mod4+j`, `Mod4+k`, `Mod4+Shift+Return`), los
//! atajos de lanzamiento abren aplicaciones (ADR 0006) y el WM termina con el
//! atajo provisional `Mod4+Shift+Escape`.
//!
//! Escritorios virtuales (ADR 0007): solo se ven las ventanas del escritorio
//! activo; las demás se desmapean, y sus UnmapNotify se distinguen de las
//! retiradas de los clientes por el número de secuencia de la petición
//! (`OwnUnmaps`). Todas las ventanas gestionadas están en el save-set del WM:
//! si su conexión se cierra por cualquier motivo, el servidor vuelve a
//! mapear las ocultas.

use x11rb::connection::{Connection, SequenceNumber};
use x11rb::errors::ReplyError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureNotifyEvent,
    ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt as _, EventMask, FocusInEvent,
    InputFocus, MapState, NotifyDetail, NotifyMode, PropMode, Screen, SetMode, StackMode,
    Timestamp, UnmapNotifyEvent, Window,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

use crate::clients::{self, Class, Direction, InputModel, Kind, WindowInfo, WindowTypes};
use crate::display::Target;
use crate::error::Fatal;
use crate::keys::{self, Action, Repeats};
use crate::layout::{self, Rect};
use crate::spawn::Launcher;
use crate::workspaces::{self, Switch, Workspaces};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_TAKE_FOCUS,
        WM_STATE,
        _NET_ACTIVE_WINDOW,
        _NET_NUMBER_OF_DESKTOPS,
        _NET_CURRENT_DESKTOP,
        _NET_WM_DESKTOP,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _NET_WM_WINDOW_TYPE_SPLASH,
        _NET_WM_WINDOW_TYPE_TOOLBAR,
        _NET_WM_WINDOW_TYPE_MENU,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,
        _NET_WM_WINDOW_TYPE_TOOLTIP,
        _NET_WM_WINDOW_TYPE_POPUP_MENU,
        _NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
        _NET_WM_WINDOW_TYPE_COMBO,
        _NET_WM_WINDOW_TYPE_DND,
    }
}

/// Máximo de tipos de `_NET_WM_WINDOW_TYPE` que se leen (unidades de 32 bits).
const MAX_WINDOW_TYPES: u32 = 16;
/// Máximo de protocolos de `WM_PROTOCOLS` que se leen.
const MAX_PROTOCOLS: u32 = 32;
/// Longitud de `WM_HINTS` en unidades de 32 bits (ICCCM §4.1.2.4).
const WM_HINTS_LEN: u32 = 9;
/// Valor especial de `SetInputFocus`: el foco sigue al puntero (el estado
/// inicial del servidor).
const POINTER_ROOT: Window = 1;
/// Valores de `WM_STATE.state` (ICCCM §4.1.3.1).
const NORMAL_STATE: u32 = 1;
const ICONIC_STATE: u32 = 3;

/// Se llama solo después de superar las capas 1 a 4 del ADR 0003: registrar
/// los átomos los crea en el servidor, y un servidor rechazado no debe
/// quedar modificado.
///
/// `target` es el display validado por la capa 1: los programas lanzados
/// reciben ese `DISPLAY`.
pub fn run(conn: &impl Connection, screen: &Screen, target: &Target) -> Result<(), Fatal> {
    let bindings = keys::grab_bindings(conn, screen.root)?;
    let launcher = Launcher::new(target);
    let mut repeats = Repeats::default();
    let atoms = Atoms::new(conn)
        .map_err(Fatal::lost)?
        .reply()
        .map_err(|e| Fatal::startup("InternAtom", e))?;

    let mut wm = Wm {
        conn,
        root: screen.root,
        area: Rect {
            x: 0,
            y: 0,
            width: screen.width_in_pixels,
            height: screen.height_in_pixels,
        },
        floating_types: [
            atoms._NET_WM_WINDOW_TYPE_DIALOG,
            atoms._NET_WM_WINDOW_TYPE_UTILITY,
            atoms._NET_WM_WINDOW_TYPE_TOOLBAR,
        ],
        unfocusable_types: [
            atoms._NET_WM_WINDOW_TYPE_SPLASH,
            atoms._NET_WM_WINDOW_TYPE_MENU,
            atoms._NET_WM_WINDOW_TYPE_DOCK,
            atoms._NET_WM_WINDOW_TYPE_DESKTOP,
            atoms._NET_WM_WINDOW_TYPE_NOTIFICATION,
            atoms._NET_WM_WINDOW_TYPE_TOOLTIP,
            atoms._NET_WM_WINDOW_TYPE_POPUP_MENU,
            atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
            atoms._NET_WM_WINDOW_TYPE_COMBO,
            atoms._NET_WM_WINDOW_TYPE_DND,
        ],
        atoms,
        workspaces: Workspaces::default(),
        unmaps: OwnUnmaps::default(),
        focus_request: 0,
    };
    wm.adopt_existing()?;
    wm.publish_desktops()?;

    loop {
        conn.flush().map_err(Fatal::lost)?;
        let (event, sequence) = conn.wait_for_event_with_sequence().map_err(Fatal::lost)?;
        match event {
            Event::MapRequest(event) => wm.on_map_request(event.window)?,
            Event::ConfigureRequest(event) => wm.on_configure_request(&event)?,
            // Con SubstructureNotify en la raíz, `event` es la raíz. Se
            // incluye el UnmapNotify sintético con el que un cliente se
            // retira (ICCCM §4.1.4).
            Event::UnmapNotify(event) if event.event == wm.root => {
                wm.on_unmap_notify(&event, sequence)?
            }
            Event::DestroyNotify(event) if event.event == wm.root => {
                wm.forget(event.window, Gone::Destroyed)?
            }
            // Una ventana que pasa a ser hija de otra ya no es de primer nivel.
            Event::ReparentNotify(event) if event.event == wm.root && event.parent != wm.root => {
                wm.forget(event.window, Gone::Reparented)?
            }
            Event::FocusIn(event) => wm.on_focus_in(&event, sequence)?,
            Event::KeyPress(event) => match bindings.action(&event) {
                Some(Action::Exit) => {
                    eprintln!("crimson-wm: atajo de salida recibido");
                    return wm.release();
                }
                Some(Action::FocusNext) => wm.cycle(Direction::Next, event.time)?,
                Some(Action::FocusPrev) => wm.cycle(Direction::Prev, event.time)?,
                Some(Action::Zoom) => wm.zoom()?,
                // El lanzamiento no espera al programa ni puede fallar.
                Some(Action::Launch(program)) => {
                    if repeats.accept(program, &event) {
                        launcher.launch(program);
                    }
                }
                // Repetir el cambio a un escritorio no tiene efecto.
                Some(Action::View(desk)) => wm.view(desk, event.time)?,
                Some(Action::MoveTo(desk)) => {
                    if !repeats.autorepeat(&event) {
                        wm.move_to(desk, event.time)?;
                    }
                }
                Some(Action::ViewPrevious) => {
                    if !repeats.autorepeat(&event) {
                        wm.view_previous(event.time)?;
                    }
                }
                None => {}
            },
            // Solo llegan las de las teclas capturadas, mientras dura la
            // captura activada por la pulsación.
            Event::KeyRelease(event) => repeats.released(&event),
            // Las condiciones de carrera (BadWindow, etc.) no terminan el WM.
            Event::Error(error) => {
                eprintln!(
                    "crimson-wm: error X ignorado: {:?} (petición {}.{}, valor 0x{:x})",
                    error.error_kind, error.major_opcode, error.minor_opcode, error.bad_value
                );
            }
            _ => {}
        }
    }
}

struct Wm<'c, C: Connection> {
    conn: &'c C,
    root: Window,
    /// Área del mosaico: la pantalla completa, fija desde el arranque.
    area: Rect,
    atoms: Atoms,
    /// Tipos flotantes que reciben el foco y tipos flotantes que nunca lo
    /// reciben.
    floating_types: [u32; 3],
    unfocusable_types: [u32; 10],
    workspaces: Workspaces,
    unmaps: OwnUnmaps,
    /// Número de secuencia de la última petición con la que el WM cambió el
    /// foco. Los `FocusIn` generados antes de que el servidor la procesara
    /// son anteriores a esa decisión y se ignoran.
    focus_request: SequenceNumber,
}

impl<C: Connection> Wm<'_, C> {
    /// Gestiona las ventanas que ya estaban mapeadas al arrancar y da el foco
    /// a la maestra (o a la primera que lo admita).
    fn adopt_existing(&mut self) -> Result<(), Fatal> {
        let tree = self
            .conn
            .query_tree(self.root)
            .map_err(Fatal::lost)?
            .reply()
            .map_err(|e| Fatal::startup("QueryTree", e))?;
        for window in tree.children {
            let Some((info, map_state)) = self.window_info(window)? else {
                continue;
            };
            if map_state != MapState::VIEWABLE {
                continue;
            }
            if let Some(class) = clients::classify(&info, &self.window_types()) {
                self.manage(window, class)?;
            }
        }
        self.arrange()?;
        self.set_focus(self.workspaces.current().fallback(), CURRENT_TIME)
    }

    /// Una ventana nueva, también un diálogo, se gestiona en el escritorio
    /// activo. Una ventana gestionada de otro escritorio sigue oculta: ni se
    /// cambia de escritorio ni se le da el foco (ADR 0007).
    fn on_map_request(&mut self, window: Window) -> Result<(), Fatal> {
        if let Some(desk) = self.workspaces.locate(window) {
            if desk != self.workspaces.index() {
                eprintln!(
                    "crimson-wm: ventana 0x{window:x} del escritorio {} sigue oculta (MapRequest)",
                    desk + 1
                );
                return Ok(());
            }
        }
        let mut focus = false;
        if self.workspaces.get(window).is_none() {
            let Some((info, _)) = self.window_info(window)? else {
                // La ventana desapareció antes de poder clasificarla.
                return Ok(());
            };
            if let Some(class) = clients::classify(&info, &self.window_types()) {
                self.manage(window, class)?;
                if class.kind == Kind::Tiled {
                    // Se coloca antes de mapearla para que aparezca ya con
                    // su tamaño.
                    self.arrange()?;
                }
                focus = class.focusable;
            }
        }
        // Las flotantes quedan encima al mapearse: una ventana recién creada
        // está en lo alto de la pila de apilamiento.
        self.conn.map_window(window).map_err(Fatal::lost)?;
        // Después de MapWindow, en la misma conexión: la ventana ya es visible
        // cuando el servidor procesa SetInputFocus.
        if focus {
            self.set_focus(Some(window), CURRENT_TIME)?;
        }
        Ok(())
    }

    /// ICCCM §4.1.5: en mosaico no se concede ningún cambio, ni de geometría
    /// ni de apilamiento (las flotantes deben seguir encima), y se responde
    /// con un ConfigureNotify sintético con la geometría real. A las
    /// flotantes y a las no gestionadas se les aplica lo pedido; el servidor
    /// genera entonces el ConfigureNotify real.
    ///
    /// Vale también para las ventanas ocultas de otros escritorios: el
    /// servidor conserva la geometría de una ventana desmapeada.
    fn on_configure_request(&mut self, event: &ConfigureRequestEvent) -> Result<(), Fatal> {
        match self.workspaces.get(event.window) {
            Some(client) if client.kind == Kind::Tiled => {
                if let Some(rect) = client.geometry {
                    let notify = synthetic_configure_notify(event.window, rect);
                    self.conn
                        .send_event(false, event.window, EventMask::STRUCTURE_NOTIFY, notify)
                        .map_err(Fatal::lost)?;
                }
            }
            _ => {
                let aux = ConfigureWindowAux::from_configure_request(event);
                self.conn
                    .configure_window(event.window, &aux)
                    .map_err(Fatal::lost)?;
            }
        }
        Ok(())
    }

    /// UnmapNotify en la raíz: el de un desmapeo con el que el WM ocultó la
    /// ventana se ignora; cualquier otro es una retirada del cliente.
    fn on_unmap_notify(
        &mut self,
        event: &UnmapNotifyEvent,
        sequence: SequenceNumber,
    ) -> Result<(), Fatal> {
        // Bit alto del código: enviado con SendEvent por otro cliente.
        let synthetic = event.response_type & 0x80 != 0;
        if self.unmaps.is_own(event.window, sequence, synthetic) {
            return Ok(());
        }
        let gone = if synthetic {
            Gone::Withdrawn
        } else {
            Gone::Unmapped
        };
        self.forget(event.window, gone)
    }

    /// Deja de gestionar una ventana que se ha retirado, destruido o
    /// cambiado de padre, esté en el escritorio que esté. Los eventos
    /// repetidos o de ventanas no gestionadas no tienen efecto. Si estaba en
    /// el escritorio activo, se reorganiza y, si era la enfocada, el foco
    /// pasa a la más reciente del historial; un escritorio oculto se
    /// reorganiza al mostrarse.
    fn forget(&mut self, window: Window, gone: Gone) -> Result<(), Fatal> {
        let pending = self.unmaps.forget(window);
        let Some((desk, removed)) = self.workspaces.unmanage(window) else {
            return Ok(());
        };
        let reason = match gone {
            // El cliente la desmapeó antes de que el servidor procesara el
            // desmapeo con el que el WM la ocultaba.
            Gone::Unmapped if pending => "desmapeada por el cliente; desmapeo del WM pendiente",
            Gone::Unmapped => "desmapeada por el cliente",
            Gone::Withdrawn => "UnmapNotify sintético",
            Gone::Destroyed => "destruida",
            Gone::Reparented => "con otro padre",
        };
        eprintln!(
            "crimson-wm: ventana 0x{window:x} retirada ({}) del escritorio {}: {reason}",
            kind_name(removed.kind),
            desk + 1
        );
        let active = desk == self.workspaces.index();
        if gone != Gone::Destroyed {
            // La ventana deja de ser del WM: si este termina, el servidor no
            // debe volver a mapearla, y pierde WM_STATE (ICCCM §4.1.3.1) y
            // _NET_WM_DESKTOP. Puede haber sido destruida justo después (un
            // cliente que termina): los errores se descartan.
            self.conn
                .change_save_set(SetMode::DELETE, window)
                .map_err(Fatal::lost)?
                .ignore_error();
            for property in [self.atoms.WM_STATE, self.atoms._NET_WM_DESKTOP] {
                self.conn
                    .delete_property(window, property)
                    .map_err(Fatal::lost)?
                    .ignore_error();
            }
        }
        if gone == Gone::Withdrawn && active {
            // Retirada sintética de una ventana que el WM tiene mapeada
            // (p. ej., la mostró justo antes de recibirla): se desmapea para
            // que no quede visible sin gestionar.
            self.conn
                .unmap_window(window)
                .map_err(Fatal::lost)?
                .ignore_error();
        }
        if !active {
            return Ok(());
        }
        if removed.kind == Kind::Tiled {
            self.arrange()?;
        }
        if removed.was_focused {
            self.set_focus(self.workspaces.current().fallback(), CURRENT_TIME)?;
        }
        Ok(())
    }

    /// Sigue los cambios de foco hechos por los clientes (ICCCM permite que
    /// un cliente se dé el foco). Se ignoran:
    /// - los anteriores a la última decisión de foco del WM (por número de
    ///   secuencia), que ya no describen el estado actual;
    /// - los producidos por capturas de teclado (modos Grab, Ungrab y
    ///   WhileGrabbed) y los que no sitúan el foco en la ventana (detalles
    ///   Pointer, PointerRoot y None);
    /// - los de ventanas no gestionadas o retiradas.
    fn on_focus_in(&mut self, event: &FocusInEvent, sequence: SequenceNumber) -> Result<(), Fatal> {
        if sequence < self.focus_request
            || event.mode != NotifyMode::NORMAL
            || matches!(
                event.detail,
                NotifyDetail::POINTER | NotifyDetail::POINTER_ROOT | NotifyDetail::NONE
            )
            || self.workspaces.current().focused() == Some(event.event)
        {
            return Ok(());
        }
        // Solo cuentan las ventanas del escritorio activo.
        if self.workspaces.current_mut().focus(event.event) {
            eprintln!("crimson-wm: foco en 0x{:x} (cliente)", event.event);
            self.publish_active(event.event)?;
        }
        Ok(())
    }

    /// `Mod4+j` / `Mod4+k`: enfoca la ventana siguiente o anterior.
    fn cycle(&mut self, direction: Direction, time: Timestamp) -> Result<(), Fatal> {
        let desk = self.workspaces.current();
        match desk.neighbour(direction) {
            Some(target) if Some(target) != desk.focused() => self.set_focus(Some(target), time),
            _ => Ok(()),
        }
    }

    /// `Mod4+Shift+Return`: la ventana enfocada pasa a maestra. El foco no
    /// cambia.
    fn zoom(&mut self) -> Result<(), Fatal> {
        if self.workspaces.current_mut().zoom() {
            self.arrange()?;
        }
        Ok(())
    }

    /// `Mod4+1`…`Mod4+8`: activa ese escritorio. Sin efecto si ya es el
    /// activo.
    fn view(&mut self, desk: usize, time: Timestamp) -> Result<(), Fatal> {
        match self.workspaces.switch(desk) {
            Some(switch) => self.show(switch, time),
            None => Ok(()),
        }
    }

    /// `Mod4+Tab`: vuelve al escritorio anterior, si lo hay.
    fn view_previous(&mut self, time: Timestamp) -> Result<(), Fatal> {
        match self.workspaces.back() {
            Some(switch) => self.show(switch, time),
            None => Ok(()),
        }
    }

    /// Muestra el escritorio activo y oculta el anterior.
    ///
    /// Orden de las peticiones (una sola conexión, procesadas en orden):
    /// 1. la geometría del mosaico, para que las ventanas aparezcan ya
    ///    colocadas y las flotantes encima;
    /// 2. el mapeo de las nuevas, antes de ocultar las anteriores, para no
    ///    dejar ver la raíz;
    /// 3. el foco, antes de ocultar: la ventana enfocada no se desmapea con
    ///    el foco, que el servidor devolvería a la raíz;
    /// 4. el desmapeo de las anteriores.
    fn show(&mut self, Switch { from, to }: Switch, time: Timestamp) -> Result<(), Fatal> {
        eprintln!("crimson-wm: escritorio {} (antes {})", to + 1, from + 1);
        self.arrange()?;
        let shown: Vec<Window> = self.workspaces.current().windows().collect();
        for window in shown {
            self.set_wm_state(window, NORMAL_STATE)?;
            self.conn.map_window(window).map_err(Fatal::lost)?;
        }
        self.set_focus(self.workspaces.current().fallback(), time)?;
        let hidden: Vec<Window> = self
            .workspaces
            .desk(from)
            .map(|d| d.windows().collect())
            .unwrap_or_default();
        for window in hidden {
            self.hide(window)?;
        }
        self.publish_current_desktop()
    }

    /// `Mod4+Shift+1`…`8`: lleva la ventana enfocada a ese escritorio, donde
    /// será la enfocada, sin cambiar de escritorio. El foco pasa a la más
    /// reciente del historial del activo.
    fn move_to(&mut self, desk: usize, time: Timestamp) -> Result<(), Fatal> {
        let Some(moved) = self.workspaces.move_focused(desk) else {
            return Ok(());
        };
        eprintln!(
            "crimson-wm: ventana 0x{:x} llevada al escritorio {}",
            moved.window,
            moved.to + 1
        );
        self.publish_desktop(moved.window, moved.to)?;
        // Como en `show`: el foco cambia antes de ocultar la ventana.
        self.set_focus(self.workspaces.current().fallback(), time)?;
        self.hide(moved.window)?;
        if moved.kind == Kind::Tiled {
            self.arrange()?;
        }
        Ok(())
    }

    /// Oculta una ventana gestionada y anota el número de secuencia del
    /// desmapeo para reconocer su UnmapNotify (`OwnUnmaps`).
    fn hide(&mut self, window: Window) -> Result<(), Fatal> {
        let cookie = self.conn.unmap_window(window).map_err(Fatal::lost)?;
        self.unmaps.push(window, cookie.sequence_number());
        self.set_wm_state(window, ICONIC_STATE)
    }

    fn manage(&mut self, window: Window, class: Class) -> Result<(), Fatal> {
        if !self.workspaces.manage(window, class) {
            return Ok(());
        }
        let desk = self.workspaces.index();
        eprintln!(
            "crimson-wm: ventana 0x{window:x} gestionada ({}) en el escritorio {}",
            kind_name(class.kind),
            desk + 1
        );
        // Si la conexión del WM se cierra (salida, señal o fallo) con la
        // ventana oculta, el servidor la vuelve a mapear.
        self.conn
            .change_save_set(SetMode::INSERT, window)
            .map_err(Fatal::lost)?;
        self.set_wm_state(window, NORMAL_STATE)?;
        self.publish_desktop(window, desk)?;
        if class.focusable {
            // Para seguir los cambios de foco hechos por el cliente. No se
            // retira la selección al dejar de gestionarla: la ventana puede
            // haber sido destruida, y sus eventos se ignoran igualmente.
            let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::FOCUS_CHANGE);
            self.conn
                .change_window_attributes(window, &aux)
                .map_err(Fatal::lost)?;
        }
        Ok(())
    }

    /// Da el foco a `target` según su modelo de entrada (ICCCM §4.1.7) y
    /// publica `_NET_ACTIVE_WINDOW`. Sin ventana, el foco vuelve a seguir al
    /// puntero, como en un servidor sin WM.
    ///
    /// Si la ventana acaba de desaparecer, el error X se registra como
    /// evento y su UnmapNotify o DestroyNotify vuelve a colocar el foco.
    fn set_focus(&mut self, target: Option<Window>, time: Timestamp) -> Result<(), Fatal> {
        let target = target.and_then(|w| {
            self.workspaces
                .current()
                .get(w)
                .filter(|c| c.focusable)
                .map(|c| (w, c.input))
        });
        let Some((window, input)) = target else {
            let cookie = self
                .conn
                .set_input_focus(InputFocus::POINTER_ROOT, POINTER_ROOT, time)
                .map_err(Fatal::lost)?;
            self.focus_request = cookie.sequence_number();
            eprintln!("crimson-wm: sin foco");
            return self.publish_active(NONE);
        };

        self.workspaces.current_mut().focus(window);
        if input.sets_input_focus() {
            // Si la ventana desaparece, el servidor devuelve el foco a la raíz.
            let cookie = self
                .conn
                .set_input_focus(InputFocus::PARENT, window, time)
                .map_err(Fatal::lost)?;
            self.focus_request = cookie.sequence_number();
        } else if input == InputModel::NoInput {
            // La ventana no acepta el teclado: el foco va a la raíz para que
            // las pulsaciones no lleguen a la ventana enfocada antes.
            let cookie = self
                .conn
                .set_input_focus(InputFocus::POINTER_ROOT, self.root, time)
                .map_err(Fatal::lost)?;
            self.focus_request = cookie.sequence_number();
        }
        if input.takes_focus() {
            let message = ClientMessageEvent::new(
                32,
                window,
                self.atoms.WM_PROTOCOLS,
                [self.atoms.WM_TAKE_FOCUS, time, 0, 0, 0],
            );
            let cookie = self
                .conn
                .send_event(false, window, EventMask::NO_EVENT, message)
                .map_err(Fatal::lost)?;
            self.focus_request = cookie.sequence_number();
        }
        eprintln!("crimson-wm: foco en 0x{window:x} ({input:?})");
        self.publish_active(window)
    }

    /// `_NET_ACTIVE_WINDOW` en la raíz (EWMH), con None si no hay ninguna.
    /// Es compatibilidad parcial: el WM no declara soporte EWMH
    /// (`_NET_SUPPORTED`, `_NET_SUPPORTING_WM_CHECK`).
    fn publish_active(&self, window: Window) -> Result<(), Fatal> {
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW,
                &[window],
            )
            .map_err(Fatal::lost)?;
        Ok(())
    }

    /// `_NET_NUMBER_OF_DESKTOPS` y `_NET_CURRENT_DESKTOP` en la raíz, como
    /// información parcial: no se atienden los mensajes de cambio (ADR 0007).
    fn publish_desktops(&self) -> Result<(), Fatal> {
        let count = u32::try_from(workspaces::COUNT).unwrap_or(u32::MAX);
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_NUMBER_OF_DESKTOPS,
                AtomEnum::CARDINAL,
                &[count],
            )
            .map_err(Fatal::lost)?;
        self.publish_current_desktop()
    }

    fn publish_current_desktop(&self) -> Result<(), Fatal> {
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_CURRENT_DESKTOP,
                AtomEnum::CARDINAL,
                &[desktop_number(self.workspaces.index())],
            )
            .map_err(Fatal::lost)?;
        Ok(())
    }

    /// `_NET_WM_DESKTOP` de una ventana gestionada.
    fn publish_desktop(&self, window: Window, desk: usize) -> Result<(), Fatal> {
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms._NET_WM_DESKTOP,
                AtomEnum::CARDINAL,
                &[desktop_number(desk)],
            )
            .map_err(Fatal::lost)?;
        Ok(())
    }

    /// `WM_STATE` (ICCCM §4.1.3.1): NormalState si está visible e
    /// IconicState si está oculta en otro escritorio. Sin ventana de icono.
    fn set_wm_state(&self, window: Window, state: u32) -> Result<(), Fatal> {
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms.WM_STATE,
                self.atoms.WM_STATE,
                &[state, NONE],
            )
            .map_err(Fatal::lost)?;
        Ok(())
    }

    /// Al salir: muestra las ventanas ocultas de los demás escritorios (el
    /// save-set lo haría igualmente al cerrarse la conexión, pero así quedan
    /// en NormalState), borra las propiedades de la raíz y devuelve el foco
    /// al puntero, el estado inicial del servidor.
    ///
    /// Termina con una petición con respuesta: sin ella, el proceso podría
    /// salir antes de que el servidor procese las anteriores, y otro cliente
    /// vería todavía el estado antiguo.
    fn release(&self) -> Result<(), Fatal> {
        let hidden = (0..workspaces::COUNT)
            .filter(|&d| d != self.workspaces.index())
            .filter_map(|d| self.workspaces.desk(d))
            .flat_map(|d| d.windows());
        for window in hidden {
            self.set_wm_state(window, NORMAL_STATE)?;
            self.conn.map_window(window).map_err(Fatal::lost)?;
        }
        for property in [
            self.atoms._NET_ACTIVE_WINDOW,
            self.atoms._NET_CURRENT_DESKTOP,
            self.atoms._NET_NUMBER_OF_DESKTOPS,
        ] {
            self.conn
                .delete_property(self.root, property)
                .map_err(Fatal::lost)?;
        }
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, POINTER_ROOT, CURRENT_TIME)
            .map_err(Fatal::lost)?;
        match self.conn.get_input_focus().map_err(Fatal::lost)?.reply() {
            Err(ReplyError::ConnectionError(e)) => Err(Fatal::lost(e)),
            // Un error X aquí no impide salir: el WM ya ha terminado su trabajo.
            Ok(_) | Err(ReplyError::X11Error(_)) => Ok(()),
        }
    }

    /// Aplica el diseño master-stack al escritorio activo. Solo se envía la
    /// geometría de las ventanas que cambian; después se suben las flotantes
    /// por encima.
    fn arrange(&mut self) -> Result<(), Fatal> {
        let tiled: Vec<Window> = self.workspaces.current().tiled().collect();
        let rects = layout::master_stack(self.area, tiled.len());
        for (window, rect) in tiled.into_iter().zip(rects) {
            if self.workspaces.current_mut().set_geometry(window, rect) {
                let aux = ConfigureWindowAux::new()
                    .x(i32::from(rect.x))
                    .y(i32::from(rect.y))
                    .width(u32::from(rect.width))
                    .height(u32::from(rect.height))
                    .border_width(0);
                self.conn
                    .configure_window(window, &aux)
                    .map_err(Fatal::lost)?;
            }
        }
        let raise = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        for window in self.workspaces.current().floating() {
            self.conn
                .configure_window(window, &raise)
                .map_err(Fatal::lost)?;
        }
        Ok(())
    }

    /// Lee lo necesario para clasificar una ventana, con las cinco
    /// peticiones enviadas juntas. `None`: la ventana ya no existe.
    ///
    /// Todas las propiedades se leen sin interpretar y se analizan aquí: un
    /// valor mal formado se trata como ausente y nunca termina el WM.
    fn window_info(&self, window: Window) -> Result<Option<(WindowInfo, MapState)>, Fatal> {
        let conn = self.conn;
        let property = |name: u32, type_: AtomEnum, len: u32| {
            conn.get_property(false, window, name, type_, 0, len)
                .map_err(Fatal::lost)
        };
        let attributes = conn.get_window_attributes(window).map_err(Fatal::lost)?;
        let transient = property(AtomEnum::WM_TRANSIENT_FOR.into(), AtomEnum::ANY, 1)?;
        let types = property(
            self.atoms._NET_WM_WINDOW_TYPE,
            AtomEnum::ATOM,
            MAX_WINDOW_TYPES,
        )?;
        let hints = property(AtomEnum::WM_HINTS.into(), AtomEnum::WM_HINTS, WM_HINTS_LEN)?;
        let protocols = property(self.atoms.WM_PROTOCOLS, AtomEnum::ATOM, MAX_PROTOCOLS)?;

        let attributes = reply_or_gone(attributes.reply(), window, "GetWindowAttributes")?;
        let transient = reply_or_gone(transient.reply(), window, "GetProperty(WM_TRANSIENT_FOR)")?;
        let types = reply_or_gone(types.reply(), window, "GetProperty(_NET_WM_WINDOW_TYPE)")?;
        let hints = reply_or_gone(hints.reply(), window, "GetProperty(WM_HINTS)")?;
        let protocols = reply_or_gone(protocols.reply(), window, "GetProperty(WM_PROTOCOLS)")?;
        let (Some(attributes), Some(transient), Some(types), Some(hints), Some(protocols)) =
            (attributes, transient, types, hints, protocols)
        else {
            return Ok(None);
        };
        let values32 = |reply: &x11rb::protocol::xproto::GetPropertyReply| -> Vec<u32> {
            reply.value32().map(Iterator::collect).unwrap_or_default()
        };
        let info = WindowInfo {
            override_redirect: attributes.override_redirect,
            // Se acepta cualquier tipo de propiedad con un valor de 32 bits:
            // ICCCM espera WINDOW, pero la regla permisiva es inocua.
            transient: transient.format == 32 && transient.value_len >= 1,
            types: values32(&types),
            input: clients::input_hint(&values32(&hints)),
            take_focus: values32(&protocols).contains(&self.atoms.WM_TAKE_FOCUS),
        };
        Ok(Some((info, attributes.map_state)))
    }

    fn window_types(&self) -> WindowTypes<'_> {
        WindowTypes {
            normal: self.atoms._NET_WM_WINDOW_TYPE_NORMAL,
            floating: &self.floating_types,
            unfocusable: &self.unfocusable_types,
        }
    }
}

/// Respuesta de una petición sobre una ventana que pudo desaparecer entre
/// el evento y la consulta: un error X se registra y da `None`; solo la
/// pérdida de la conexión es fatal (código 13).
fn reply_or_gone<T>(
    reply: Result<T, ReplyError>,
    window: Window,
    context: &str,
) -> Result<Option<T>, Fatal> {
    match reply {
        Ok(value) => Ok(Some(value)),
        Err(ReplyError::ConnectionError(e)) => Err(Fatal::lost(e)),
        Err(ReplyError::X11Error(e)) => {
            eprintln!(
                "crimson-wm: ventana 0x{window:x} no gestionada: {context}: {:?}",
                e.error_kind
            );
            Ok(None)
        }
    }
}

/// ConfigureNotify sintético con la geometría asignada, en coordenadas de
/// la raíz (no hay reparenting) y sin borde.
fn synthetic_configure_notify(window: Window, rect: Rect) -> ConfigureNotifyEvent {
    ConfigureNotifyEvent {
        response_type: x11rb::protocol::xproto::CONFIGURE_NOTIFY_EVENT,
        sequence: 0,
        event: window,
        window,
        above_sibling: NONE,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        border_width: 0,
        override_redirect: false,
    }
}

/// Por qué deja de gestionarse una ventana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gone {
    /// UnmapNotify generado por el servidor que no es de un desmapeo del WM.
    Unmapped,
    /// UnmapNotify sintético (ICCCM §4.1.4).
    Withdrawn,
    Destroyed,
    /// Ha pasado a ser hija de otra ventana.
    Reparented,
}

/// Desmapeos con los que el WM ha ocultado ventanas, pendientes de su
/// UnmapNotify (ADR 0007).
///
/// El servidor marca cada evento con el número de secuencia de la última
/// petición del WM que ha procesado, sea quien sea el cliente que lo causa;
/// x11rb reconstruye los 64 bits a partir de los 16 del protocolo. Una
/// ventana gestionada ya desmapeada solo puede volver a mapearse a través
/// del WM (SubstructureRedirect), así que entre el desmapeo del WM y su
/// siguiente petición ningún otro cliente puede generar un UnmapNotify real
/// de esa ventana. Por eso:
/// - el UnmapNotify real con la ventana y el número de secuencia exactos de
///   un desmapeo pendiente es el del WM;
/// - si el cliente desmapea la ventana antes de que el servidor procese el
///   desmapeo del WM, su evento lleva un número anterior (es una retirada) y
///   el del WM no genera ninguno;
/// - un UnmapNotify sintético (SendEvent) también lleva el número de
///   secuencia del WM, por lo que nunca se compara: siempre es una retirada.
///
/// Los eventos llegan en orden de número de secuencia: un pendiente anterior
/// al evento actual ya no recibirá el suyo y se descarta.
#[derive(Debug, Default)]
struct OwnUnmaps {
    pending: Vec<(Window, SequenceNumber)>,
}

impl OwnUnmaps {
    fn push(&mut self, window: Window, sequence: SequenceNumber) {
        self.pending.push((window, sequence));
    }

    /// Indica si el UnmapNotify de `window` con número de secuencia
    /// `sequence` es el de un desmapeo del WM; en ese caso lo consume.
    fn is_own(&mut self, window: Window, sequence: SequenceNumber, synthetic: bool) -> bool {
        self.pending.retain(|&(_, s)| s >= sequence);
        if synthetic {
            return false;
        }
        match self.pending.iter().position(|&p| p == (window, sequence)) {
            Some(index) => {
                self.pending.swap_remove(index);
                true
            }
            None => false,
        }
    }

    /// La ventana ha dejado de gestionarse: sus desmapeos pendientes ya no
    /// importan. Indica si tenía alguno.
    fn forget(&mut self, window: Window) -> bool {
        let before = self.pending.len();
        self.pending.retain(|&(w, _)| w != window);
        self.pending.len() != before
    }
}

/// Número de escritorio de las propiedades EWMH: el índice, de 0 a 7.
fn desktop_number(desk: usize) -> u32 {
    u32::try_from(desk).unwrap_or(u32::MAX)
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Tiled => "mosaico",
        Kind::Floating => "flotante",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configure_notify_sintetico() {
        let rect = Rect {
            x: 640,
            y: 360,
            width: 640,
            height: 360,
        };
        let event = synthetic_configure_notify(0x20_0001, rect);
        assert_eq!(event.response_type, 22);
        assert_eq!((event.event, event.window), (0x20_0001, 0x20_0001));
        assert_eq!(
            (event.x, event.y, event.width, event.height),
            (640, 360, 640, 360)
        );
        assert_eq!((event.border_width, event.above_sibling), (0, 0));
        assert!(!event.override_redirect);
        // Se serializa en los 32 bytes de un evento X11.
        let bytes: [u8; 32] = event.into();
        assert_eq!(bytes[0], 22);
    }

    // U14: desmapeos propios.

    #[test]
    fn reconoce_el_desmapeo_propio_por_ventana_y_secuencia() {
        let mut own = OwnUnmaps::default();
        own.push(0xa, 100);
        own.push(0xb, 101);
        assert!(
            !own.is_own(0xb, 100, false),
            "otra ventana con la misma secuencia"
        );
        assert!(own.is_own(0xa, 100, false));
        assert!(
            !own.is_own(0xa, 100, false),
            "evento repetido: ya consumido"
        );
        assert!(own.is_own(0xb, 101, false));
        assert!(own.pending.is_empty());
    }

    #[test]
    fn la_retirada_del_cliente_antes_del_desmapeo_propio() {
        // El cliente desmapea la ventana (evento con la secuencia 99, la
        // última petición del WM procesada) antes de que el servidor procese
        // el desmapeo 100 del WM, que no genera ningún evento.
        let mut own = OwnUnmaps::default();
        own.push(0xa, 100);
        assert!(!own.is_own(0xa, 99, false), "es una retirada");
        // El pendiente sigue hasta que llega un evento posterior.
        assert_eq!(own.pending, vec![(0xa, 100)]);
        assert!(!own.is_own(0xc, 105, false));
        assert!(
            own.pending.is_empty(),
            "descartado: su evento ya no llegará"
        );
    }

    #[test]
    fn un_unmapnotify_sintetico_siempre_es_una_retirada() {
        let mut own = OwnUnmaps::default();
        own.push(0xa, 100);
        // Con la misma ventana y secuencia que el desmapeo pendiente.
        assert!(!own.is_own(0xa, 100, true));
        // El evento real del desmapeo del WM sigue reconociéndose.
        assert!(own.is_own(0xa, 100, false));
    }

    #[test]
    fn varios_desmapeos_pendientes_de_la_misma_ventana() {
        // Cambios rápidos 1→2→1→2: la ventana se oculta dos veces antes de
        // que llegue el primer evento.
        let mut own = OwnUnmaps::default();
        own.push(0xa, 100);
        own.push(0xa, 140);
        assert!(own.is_own(0xa, 100, false));
        assert!(!own.is_own(0xa, 120, false), "desmapeo ajeno entre los dos");
        assert!(own.is_own(0xa, 140, false));
        assert!(own.pending.is_empty());
    }

    #[test]
    fn olvidar_una_ventana_descarta_sus_pendientes() {
        let mut own = OwnUnmaps::default();
        own.push(0xa, 100);
        own.push(0xb, 101);
        own.push(0xa, 102);
        assert!(own.forget(0xa));
        assert!(!own.forget(0xa));
        assert_eq!(own.pending, vec![(0xb, 101)]);
        // Un identificador reutilizado no hereda pendientes.
        assert!(!own.is_own(0xa, 102, false));
    }

    #[test]
    fn secuencias_de_mas_de_16_bits() {
        // x11rb da números de 64 bits: los que coinciden en los 16 bits del
        // protocolo no se confunden.
        let mut own = OwnUnmaps::default();
        own.push(0xa, 0x1_0005);
        assert!(!own.is_own(0xa, 0x5, false));
        assert!(own.is_own(0xa, 0x1_0005, false));
        own.push(0xa, 0x2_0005);
        assert!(!own.is_own(0xa, 0x3_0005, false));
        assert!(own.pending.is_empty());
    }

    #[test]
    fn mensaje_wm_take_focus() {
        // ICCCM §4.2.8: tipo WM_PROTOCOLS, formato 32, data[0] = WM_TAKE_FOCUS
        // y data[1] = marca de tiempo.
        let (protocols, take_focus, time) = (301, 302, 123_456);
        let message =
            ClientMessageEvent::new(32, 0x40_0003, protocols, [take_focus, time, 0, 0, 0]);
        let bytes: [u8; 32] = message.into();
        assert_eq!(bytes[0], 33, "ClientMessage");
        assert_eq!(bytes[1], 32, "formato");
        let word = |i: usize| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap());
        assert_eq!(word(4), 0x40_0003, "ventana");
        assert_eq!(word(8), protocols, "tipo");
        assert_eq!((word(12), word(16)), (take_focus, time));
    }
}
