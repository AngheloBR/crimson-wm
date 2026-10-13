//! Bucle de eventos de F2 (ADR 0004): las ventanas normales se colocan en
//! mosaico master-stack y se reorganizan al abrirse, retirarse o destruirse;
//! los diálogos y demás ventanas auxiliares quedan flotantes. Termina con el
//! atajo provisional `Mod4+Shift+Escape`.

use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureNotifyEvent, ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt as _,
    EventMask, MapState, Screen, StackMode, Window,
};

use crate::clients::{self, Clients, Kind, WindowInfo, WindowTypes};
use crate::error::Fatal;
use crate::keys;
use crate::layout::{self, Rect};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
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

/// Se llama solo después de superar las capas 1 a 4 del ADR 0003: registrar
/// los átomos los crea en el servidor, y un servidor rechazado no debe
/// quedar modificado.
pub fn run(conn: &impl Connection, screen: &Screen) -> Result<(), Fatal> {
    let exit_key = keys::grab_exit_key(conn, screen.root)?;
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
            atoms._NET_WM_WINDOW_TYPE_SPLASH,
            atoms._NET_WM_WINDOW_TYPE_TOOLBAR,
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
    };
    wm.adopt_existing()?;

    loop {
        conn.flush().map_err(Fatal::lost)?;
        match conn.wait_for_event().map_err(Fatal::lost)? {
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
            Event::KeyPress(event) if exit_key.matches(&event) => {
                eprintln!("crimson-wm: atajo de salida recibido");
                return Ok(());
            }
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
    floating_types: [u32; 13],
    clients: Clients,
}

impl<C: Connection> Wm<'_, C> {
    /// Gestiona las ventanas que ya estaban mapeadas al arrancar.
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
            if let Some(kind) = clients::classify(&info, &self.window_types()) {
                self.manage(window, kind);
            }
        }
        self.arrange()
    }

    fn on_map_request(&mut self, window: Window) -> Result<(), Fatal> {
        if self.clients.get(window).is_none() {
            let Some((info, _)) = self.window_info(window)? else {
                // La ventana desapareció antes de poder clasificarla.
                return Ok(());
            };
            if let Some(kind) = clients::classify(&info, &self.window_types()) {
                self.manage(window, kind);
                if kind == Kind::Tiled {
                    // Se coloca antes de mapearla para que aparezca ya con
                    // su tamaño.
                    self.arrange()?;
                }
            }
        }
        // Las flotantes quedan encima al mapearse: una ventana recién creada
        // está en lo alto de la pila de apilamiento.
        self.conn.map_window(window).map_err(Fatal::lost)?;
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
    /// no tienen efecto.
    fn forget(&mut self, window: Window) -> Result<(), Fatal> {
        match self.clients.unmanage(window) {
            Some(kind) => {
                eprintln!(
                    "crimson-wm: ventana 0x{window:x} retirada ({})",
                    kind_name(kind)
                );
                if kind == Kind::Tiled {
                    self.arrange()?;
                }
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn manage(&mut self, window: Window, kind: Kind) {
        if self.clients.manage(window, kind) {
            eprintln!(
                "crimson-wm: ventana 0x{window:x} gestionada ({})",
                kind_name(kind)
            );
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

    /// Lee lo necesario para clasificar una ventana, con las tres peticiones
    /// enviadas juntas. `None`: la ventana ya no existe.
    fn window_info(&self, window: Window) -> Result<Option<(WindowInfo, MapState)>, Fatal> {
        let conn = self.conn;
        let attributes = conn.get_window_attributes(window).map_err(Fatal::lost)?;
        let transient = conn
            .get_property(
                false,
                window,
                AtomEnum::WM_TRANSIENT_FOR,
                AtomEnum::ANY,
                0,
                1,
            )
            .map_err(Fatal::lost)?;
        let types = conn
            .get_property(
                false,
                window,
                self.atoms._NET_WM_WINDOW_TYPE,
                AtomEnum::ATOM,
                0,
                MAX_WINDOW_TYPES,
            )
            .map_err(Fatal::lost)?;

        let attributes = reply_or_gone(attributes.reply(), window, "GetWindowAttributes")?;
        let transient = reply_or_gone(transient.reply(), window, "GetProperty(WM_TRANSIENT_FOR)")?;
        let types = reply_or_gone(types.reply(), window, "GetProperty(_NET_WM_WINDOW_TYPE)")?;
        let (Some(attributes), Some(transient), Some(types)) = (attributes, transient, types)
        else {
            return Ok(None);
        };

        let info = WindowInfo {
            override_redirect: attributes.override_redirect,
            // Se acepta cualquier tipo de propiedad con un valor de 32 bits:
            // ICCCM espera WINDOW, pero la regla permisiva es inocua.
            transient: transient.format == 32 && transient.value_len >= 1,
            types: types.value32().map(Iterator::collect).unwrap_or_default(),
        };
        Ok(Some((info, attributes.map_state)))
    }

    fn window_types(&self) -> WindowTypes<'_> {
        WindowTypes {
            normal: self.atoms._NET_WM_WINDOW_TYPE_NORMAL,
            floating: &self.floating_types,
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
        above_sibling: x11rb::NONE,
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
}
