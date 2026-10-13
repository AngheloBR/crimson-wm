//! Bucle de eventos (ADR 0004 y 0005): las ventanas normales se colocan en
//! mosaico master-stack y se reorganizan al abrirse, retirarse o destruirse;
//! los diálogos y demás ventanas auxiliares quedan flotantes. El foco se
//! gestiona con el teclado (`Mod4+j`, `Mod4+k`, `Mod4+Shift+Return`) y el WM
//! termina con el atajo provisional `Mod4+Shift+Escape`.

use x11rb::connection::{Connection, SequenceNumber};
use x11rb::errors::ReplyError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureNotifyEvent,
    ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt as _, EventMask, FocusInEvent,
    InputFocus, MapState, NotifyDetail, NotifyMode, PropMode, Screen, StackMode, Timestamp, Window,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

use crate::clients::{self, Class, Clients, Direction, InputModel, Kind, WindowInfo, WindowTypes};
use crate::error::Fatal;
use crate::keys::{self, Action};
use crate::layout::{self, Rect};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_TAKE_FOCUS,
        _NET_ACTIVE_WINDOW,
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

/// Se llama solo después de superar las capas 1 a 4 del ADR 0003: registrar
/// los átomos los crea en el servidor, y un servidor rechazado no debe
/// quedar modificado.
pub fn run(conn: &impl Connection, screen: &Screen) -> Result<(), Fatal> {
    let bindings = keys::grab_bindings(conn, screen.root)?;
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
        clients: Clients::default(),
        focus_request: 0,
    };
    wm.adopt_existing()?;

    loop {
        conn.flush().map_err(Fatal::lost)?;
        let (event, sequence) = conn.wait_for_event_with_sequence().map_err(Fatal::lost)?;
        match event {
            Event::MapRequest(event) => wm.on_map_request(event.window)?,
            Event::ConfigureRequest(event) => wm.on_configure_request(&event)?,
            // Con SubstructureNotify en la raíz, `event` es la raíz. Se
            // incluye el UnmapNotify sintético con el que un cliente se
            // retira (ICCCM §4.1.4).
            Event::UnmapNotify(event) if event.event == wm.root => wm.forget(event.window)?,
            Event::DestroyNotify(event) if event.event == wm.root => wm.forget(event.window)?,
            // Una ventana que pasa a ser hija de otra ya no es de primer nivel.
            Event::ReparentNotify(event) if event.event == wm.root && event.parent != wm.root => {
                wm.forget(event.window)?
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
                None => {}
            },
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
    clients: Clients,
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
        self.set_focus(self.clients.fallback(), CURRENT_TIME)
    }

    fn on_map_request(&mut self, window: Window) -> Result<(), Fatal> {
        let mut focus = false;
        if self.clients.get(window).is_none() {
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
    fn on_configure_request(&mut self, event: &ConfigureRequestEvent) -> Result<(), Fatal> {
        match self.clients.get(event.window) {
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

    /// Deja de gestionar una ventana que se ha retirado, destruido o
    /// cambiado de padre. Los eventos repetidos o de ventanas no gestionadas
    /// no tienen efecto. Si era la enfocada, el foco pasa a la más reciente
    /// del historial.
    fn forget(&mut self, window: Window) -> Result<(), Fatal> {
        let Some(removed) = self.clients.unmanage(window) else {
            return Ok(());
        };
        eprintln!(
            "crimson-wm: ventana 0x{window:x} retirada ({})",
            kind_name(removed.kind)
        );
        if removed.kind == Kind::Tiled {
            self.arrange()?;
        }
        if removed.was_focused {
            self.set_focus(self.clients.fallback(), CURRENT_TIME)?;
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
            || self.clients.focused() == Some(event.event)
        {
            return Ok(());
        }
        if self.clients.focus(event.event) {
            eprintln!("crimson-wm: foco en 0x{:x} (cliente)", event.event);
            self.publish_active(event.event)?;
        }
        Ok(())
    }

    /// `Mod4+j` / `Mod4+k`: enfoca la ventana siguiente o anterior.
    fn cycle(&mut self, direction: Direction, time: Timestamp) -> Result<(), Fatal> {
        match self.clients.neighbour(direction) {
            Some(target) if Some(target) != self.clients.focused() => {
                self.set_focus(Some(target), time)
            }
            _ => Ok(()),
        }
    }

    /// `Mod4+Shift+Return`: la ventana enfocada pasa a maestra. El foco no
    /// cambia.
    fn zoom(&mut self) -> Result<(), Fatal> {
        if self.clients.zoom() {
            self.arrange()?;
        }
        Ok(())
    }

    fn manage(&mut self, window: Window, class: Class) -> Result<(), Fatal> {
        if !self.clients.manage(window, class) {
            return Ok(());
        }
        eprintln!(
            "crimson-wm: ventana 0x{window:x} gestionada ({})",
            kind_name(class.kind)
        );
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
            self.clients
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

        self.clients.focus(window);
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

    /// Al salir: borra `_NET_ACTIVE_WINDOW` y devuelve el foco al puntero,
    /// el estado inicial del servidor.
    ///
    /// Termina con una petición con respuesta: sin ella, el proceso podría
    /// salir antes de que el servidor procese las anteriores, y otro cliente
    /// vería todavía el estado antiguo.
    fn release(&self) -> Result<(), Fatal> {
        self.conn
            .delete_property(self.root, self.atoms._NET_ACTIVE_WINDOW)
            .map_err(Fatal::lost)?;
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, POINTER_ROOT, CURRENT_TIME)
            .map_err(Fatal::lost)?;
        match self.conn.get_input_focus().map_err(Fatal::lost)?.reply() {
            Err(ReplyError::ConnectionError(e)) => Err(Fatal::lost(e)),
            // Un error X aquí no impide salir: el WM ya ha terminado su trabajo.
            Ok(_) | Err(ReplyError::X11Error(_)) => Ok(()),
        }
    }

    /// Aplica el diseño master-stack. Solo se envía la geometría de las
    /// ventanas que cambian; después se suben las flotantes por encima.
    fn arrange(&mut self) -> Result<(), Fatal> {
        let tiled: Vec<Window> = self.clients.tiled().collect();
        let rects = layout::master_stack(self.area, tiled.len());
        for (window, rect) in tiled.into_iter().zip(rects) {
            if self.clients.set_geometry(window, rect) {
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
        for window in self.clients.floating() {
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
