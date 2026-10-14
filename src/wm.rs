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
//!
//! ICCCM/EWMH (ADR 0008): el WM se identifica con una ventana de
//! comprobación, la selección `WM_S{pantalla}` y `_NET_SUPPORTED`; cierra
//! ventanas (`WM_DELETE_WINDOW`, `KillClient`), atiende las peticiones EWMH
//! de activación, escritorios y cierre, sigue los cambios de `WM_HINTS` y
//! `WM_PROTOCOLS` y publica `_NET_CLIENT_LIST(_STACKING)`.
//!
//! Pantalla completa (ADR 0009): `set_fullscreen` es el único punto que
//! cambia `Client.fullscreen`; la geometría sale de `Clients::plan` y la
//! ventana a pantalla completa sube con el foco y baja cuando se enfoca otra
//! de su escritorio (`restack_fullscreen`).
//!
//! Diálogos (ADR 0010): `WM_TRANSIENT_FOR` se guarda como propietario
//! declarado y se resuelve al usarlo (`Workspaces::owner_of`). Un diálogo
//! nace en el escritorio de su principal, centrado sobre ella si no pide
//! posición, queda encima de ella (`Workspaces::floating_stack`), se mueve
//! con ella y, al cerrarse, devuelve el foco a ella.
//!
//! Tipos de ventana (ADR 0011): `clients::classify` decide, a partir del
//! primer tipo reconocido de `_NET_WM_WINDOW_TYPE`, si una ventana va a un
//! escritorio (mosaico o flotante) o es fija (`DESKTOP`, `DOCK` y
//! `NOTIFICATION`): en todos los escritorios, sin foco y en su capa de la
//! pila (`apply_layers`).

use std::collections::VecDeque;

use x11rb::connection::{Connection, SequenceNumber};
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureNotifyEvent,
    ConfigureRequestEvent, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask,
    FocusInEvent, InputFocus, MapState, NotifyDetail, NotifyMode, PropMode, PropertyNotifyEvent,
    Screen, SelectionNotifyEvent, SelectionRequestEvent, SetMode, StackMode, Timestamp,
    UnmapNotifyEvent, Window, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

use crate::clients::{
    self, Class, Direction, Disposition, Frame, InputModel, Kind, Layer, Restore, WindowInfo,
};
use crate::display::Target;
use crate::error::Fatal;
use crate::ewmh::{self, MessageTypes, Request, Source, StateRead, TypeAtoms, WindowType};
use crate::icccm::{self, ICONIC_STATE, NORMAL_STATE, ProtocolAtoms, Protocols, TransientFor};
use crate::keys::{self, Action, Repeats};
use crate::layout::{self, Rect};
use crate::safeguards;
use crate::spawn::Launcher;
use crate::workspaces::{self, Switch, Workspaces};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_TAKE_FOCUS,
        WM_DELETE_WINDOW,
        WM_STATE,
        MANAGER,
        UTF8_STRING,
        _NET_SUPPORTED,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_NAME,
        _NET_CLIENT_LIST,
        _NET_CLIENT_LIST_STACKING,
        _NET_CLOSE_WINDOW,
        _NET_ACTIVE_WINDOW,
        _NET_NUMBER_OF_DESKTOPS,
        _NET_CURRENT_DESKTOP,
        _NET_WM_DESKTOP,
        _NET_WM_STATE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_WINDOW_TYPE,
    }
}

/// Máximo de tipos de `_NET_WM_WINDOW_TYPE` que se leen (unidades de 32 bits).
const MAX_WINDOW_TYPES: u32 = 16;
/// Valor especial de `SetInputFocus`: el foco sigue al puntero (el estado
/// inicial del servidor).
const POINTER_ROOT: Window = 1;

/// Se llama solo después de superar las capas 1 a 4 del ADR 0003: registrar
/// los átomos los crea en el servidor, y un servidor rechazado no debe
/// quedar modificado.
///
/// `target` es el display validado por la capa 1: los programas lanzados
/// reciben ese `DISPLAY`.
pub fn run(conn: &impl Connection, screen: &Screen, target: &Target) -> Result<(), Fatal> {
    let bindings = keys::grab_bindings(conn, screen.root)?;
    let launcher = Launcher::new(target);
    let atoms = Atoms::new(conn)
        .map_err(Fatal::lost)?
        .reply()
        .map_err(|e| Fatal::startup("InternAtom", e))?;
    let supported = intern_all(conn, ewmh::SUPPORTED)?;
    // Los átomos de los 14 tipos salen de `WindowType::NAMES`, la misma
    // tabla que los traduce (ADR 0011).
    let types = intern_all(conn, &WindowType::NAMES)?;
    let types = TypeAtoms::new(std::array::from_fn(|i| types[i]));
    let selection_name = safeguards::manager_selection(target.screen);
    let selection = intern_all(conn, &[selection_name.as_str()])?[0];

    let mut wm = Wm {
        conn,
        root: screen.root,
        area: Rect {
            x: 0,
            y: 0,
            width: screen.width_in_pixels,
            height: screen.height_in_pixels,
        },
        types,
        atoms,
        workspaces: Workspaces::default(),
        unmaps: OwnUnmaps::default(),
        focus_request: 0,
        identity: None,
        pending: VecDeque::new(),
        dirty: Dirty {
            clients: true,
            stacking: true,
        },
    };
    wm.identify(selection, &selection_name, &supported)?;
    // Desde aquí el servidor tiene la identificación del WM: un error que
    // no sea la pérdida de la conexión limpia antes lo propio.
    let outcome = wm
        .adopt_existing()
        .and_then(|()| wm.publish_desktops())
        .and_then(|()| event_loop(&mut wm, &bindings, &launcher));
    match outcome {
        Err(Fatal::ConnectionLost(_)) | Ok(()) => outcome,
        Err(error) => {
            if let Err(cleanup) = wm.release() {
                eprintln!("crimson-wm: limpieza tras el error incompleta: {cleanup}");
            }
            Err(error)
        }
    }
}

/// Bucle de eventos. Termina con `Ok` tras el atajo de salida o al perder
/// la selección del gestor, después de limpiar.
fn event_loop<C: Connection>(
    wm: &mut Wm<'_, C>,
    bindings: &keys::Bindings,
    launcher: &Launcher,
) -> Result<(), Fatal> {
    let mut repeats = Repeats::default();
    loop {
        wm.publish_dirty()?;
        wm.conn.flush().map_err(Fatal::lost)?;
        let (event, sequence) = wm.next_event()?;
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
                Some(Action::Close) => {
                    if !repeats.autorepeat(&event) {
                        if let Some(window) = wm.workspaces.current().focused() {
                            wm.close(window, event.time, "Mod4+w")?;
                        }
                    }
                }
                Some(Action::Kill) => {
                    if !repeats.autorepeat(&event) {
                        wm.kill()?;
                    }
                }
                Some(Action::Fullscreen) => {
                    if !repeats.autorepeat(&event) {
                        if let Some(window) = wm.workspaces.current().focused() {
                            let on = wm
                                .workspaces
                                .get(window)
                                .is_some_and(|c| c.fullscreen.is_none());
                            wm.set_fullscreen(window, on, "Mod4+f", false)?;
                        }
                    }
                }
                None => {}
            },
            // Solo llegan las de las teclas capturadas, mientras dura la
            // captura activada por la pulsación.
            Event::KeyRelease(event) => repeats.released(&event),
            Event::PropertyNotify(event) => wm.on_property_notify(&event)?,
            // Peticiones EWMH: los clientes las envían a la raíz con
            // SubstructureRedirect|SubstructureNotify.
            Event::ClientMessage(event) => wm.on_client_message(&event)?,
            Event::SelectionClear(event) if wm.owns_selection(event.selection, event.owner) => {
                eprintln!(
                    "crimson-wm: otro cliente ha tomado la selección del gestor (0x{:x}); se termina sin intentar recuperarla",
                    event.owner
                );
                return wm.release();
            }
            Event::SelectionRequest(event) => wm.refuse_conversion(&event)?,
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

/// Ventana de comprobación EWMH, propietaria de la selección del gestor.
#[derive(Clone, Copy, Debug)]
struct Identity {
    window: Window,
    selection: Atom,
}

/// Propiedades de la raíz pendientes de publicar: se escriben una vez por
/// tanda de eventos, antes de enviar las peticiones (`publish_dirty`).
#[derive(Debug, Default)]
struct Dirty {
    /// `_NET_CLIENT_LIST`.
    clients: bool,
    /// `_NET_CLIENT_LIST_STACKING` (requiere un `QueryTree`).
    stacking: bool,
}

struct Wm<'c, C: Connection> {
    conn: &'c C,
    root: Window,
    /// Área del mosaico: la pantalla completa, fija desde el arranque.
    area: Rect,
    atoms: Atoms,
    /// Átomos de los tipos de `_NET_WM_WINDOW_TYPE` (ADR 0011).
    types: TypeAtoms,
    workspaces: Workspaces,
    unmaps: OwnUnmaps,
    /// Número de secuencia de la última petición con la que el WM cambió el
    /// foco. Los `FocusIn` generados antes de que el servidor la procesara
    /// son anteriores a esa decisión y se ignoran.
    focus_request: SequenceNumber,
    /// `None` hasta completar la identificación y si esta falla.
    identity: Option<Identity>,
    /// Eventos leídos mientras se esperaba la marca de tiempo del servidor
    /// (`server_time`), que se atienden antes que los nuevos.
    pending: VecDeque<(Event, SequenceNumber)>,
    dirty: Dirty,
}

impl<C: Connection> Wm<'_, C> {
    /// Siguiente evento: primero los que se leyeron mientras se esperaba una
    /// marca de tiempo.
    fn next_event(&mut self) -> Result<(Event, SequenceNumber), Fatal> {
        match self.pending.pop_front() {
            Some(event) => Ok(event),
            None => self
                .conn
                .wait_for_event_with_sequence()
                .map_err(Fatal::lost),
        }
    }

    /// Identificación del WM (ADR 0008), después de las capas 1 a 4 del ADR
    /// 0003, de capturar los atajos y de registrar los átomos:
    /// 1. crea la ventana de comprobación (InputOnly, override-redirect, sin
    ///    mapear) con `_NET_SUPPORTING_WM_CHECK` y `_NET_WM_NAME`;
    /// 2. obtiene la hora del servidor (`server_time`);
    /// 3. con el servidor capturado (`GrabServer`), para que ningún otro
    ///    cliente se intercale: vuelve a comprobar que `WM_Sn` no tiene
    ///    propietario y que la raíz no tiene `_NET_SUPPORTING_WM_CHECK`,
    ///    adquiere la selección con esa hora, comprueba que es suya y solo
    ///    entonces publica `_NET_SUPPORTING_WM_CHECK` y `_NET_SUPPORTED` en la
    ///    raíz;
    /// 4. anuncia `MANAGER` (ICCCM §2.8).
    ///
    /// Si algo no cuadra, se detiene con el código 12 (otro gestor) o 14 y
    /// solo destruye su propia ventana, lo que libera la selección si llegó
    /// a adquirirla. No toca propiedades ajenas ni reemplaza a otro gestor.
    fn identify(
        &mut self,
        selection: Atom,
        selection_name: &str,
        supported: &[Atom],
    ) -> Result<(), Fatal> {
        let window = self.conn.generate_id().map_err(|e| match e {
            ReplyOrIdError::ConnectionError(e) => Fatal::lost(e),
            other => Fatal::Startup(format!(
                "identificador de la ventana de comprobación: {other}"
            )),
        })?;
        let aux = CreateWindowAux::new()
            .override_redirect(1)
            .event_mask(EventMask::PROPERTY_CHANGE);
        self.conn
            .create_window(
                0,
                window,
                self.root,
                -1,
                -1,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &aux,
            )
            .map_err(Fatal::lost)?
            .check()
            .map_err(|e| Fatal::startup("CreateWindow(comprobación)", e))?;
        self.identity = Some(Identity { window, selection });
        let outcome = self.claim(window, selection, selection_name, supported);
        if outcome.is_err() {
            self.identity = None;
            // Solo lo propio: la ventana (y con ella la selección, si llegó
            // a ser suya). Las propiedades de la raíz no se han escrito.
            self.conn
                .destroy_window(window)
                .map_err(Fatal::lost)?
                .ignore_error();
            self.conn.flush().map_err(Fatal::lost)?;
        }
        outcome
    }

    fn claim(
        &mut self,
        window: Window,
        selection: Atom,
        selection_name: &str,
        supported: &[Atom],
    ) -> Result<(), Fatal> {
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                AtomEnum::WINDOW,
                &[window],
            )
            .map_err(Fatal::lost)?;
        self.conn
            .change_property8(
                PropMode::REPLACE,
                window,
                self.atoms._NET_WM_NAME,
                self.atoms.UTF8_STRING,
                ewmh::WM_NAME.as_bytes(),
            )
            .map_err(Fatal::lost)?;
        let time = self.server_time()?;

        self.conn.grab_server().map_err(Fatal::lost)?;
        let claimed = self.claim_grabbed(window, selection, selection_name, supported, time);
        self.conn.ungrab_server().map_err(Fatal::lost)?;
        claimed?;

        let manager = ClientMessageEvent::new(
            32,
            self.root,
            self.atoms.MANAGER,
            [time, selection, window, 0, 0],
        );
        self.conn
            .send_event(false, self.root, EventMask::STRUCTURE_NOTIFY, manager)
            .map_err(Fatal::lost)?;
        eprintln!(
            "crimson-wm: identificación publicada: ventana de comprobación 0x{window:x}, selección {selection_name}, marca de tiempo {time}"
        );
        Ok(())
    }

    /// Paso 3 de `identify`, con el servidor capturado.
    fn claim_grabbed(
        &self,
        window: Window,
        selection: Atom,
        selection_name: &str,
        supported: &[Atom],
        time: Timestamp,
    ) -> Result<(), Fatal> {
        let owner = self.selection_owner(selection)?;
        if owner != NONE {
            return Err(Fatal::OtherWm(format!(
                "la selección {selection_name} ha pasado a tener propietario (0x{owner:x}) durante el arranque"
            )));
        }
        if self.root_check()?.is_some() {
            return Err(Fatal::OtherWm(
                "la raíz ha recibido _NET_SUPPORTING_WM_CHECK durante el arranque".into(),
            ));
        }
        self.conn
            .set_selection_owner(window, selection, time)
            .map_err(Fatal::lost)?;
        let owner = self.selection_owner(selection)?;
        if owner != window {
            return Err(Fatal::OtherWm(format!(
                "no se pudo adquirir la selección {selection_name} (propietario 0x{owner:x})"
            )));
        }
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                AtomEnum::WINDOW,
                &[window],
            )
            .map_err(Fatal::lost)?;
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_SUPPORTED,
                AtomEnum::ATOM,
                supported,
            )
            .map_err(Fatal::lost)?;
        Ok(())
    }

    fn selection_owner(&self, selection: Atom) -> Result<Window, Fatal> {
        Ok(self
            .conn
            .get_selection_owner(selection)
            .map_err(Fatal::lost)?
            .reply()
            .map_err(|e| Fatal::startup("GetSelectionOwner", e))?
            .owner)
    }

    /// Valor de `_NET_SUPPORTING_WM_CHECK` en la raíz: `None` si no existe;
    /// `Some(None)` si existe con un valor no válido.
    fn root_check(&self) -> Result<Option<Option<Window>>, Fatal> {
        let reply = self
            .conn
            .get_property(
                false,
                self.root,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                AtomEnum::ANY,
                0,
                1,
            )
            .map_err(Fatal::lost)?
            .reply()
            .map_err(|e| Fatal::startup("GetProperty(_NET_SUPPORTING_WM_CHECK)", e))?;
        if reply.type_ == u32::from(AtomEnum::NONE) {
            return Ok(None);
        }
        let value = reply
            .value32()
            .and_then(|mut v| v.next())
            .filter(|_| reply.format == 32);
        Ok(Some(value))
    }

    fn owns_selection(&self, selection: Atom, owner: Window) -> bool {
        self.identity
            .is_some_and(|i| i.selection == selection && i.window == owner)
    }

    /// Como propietario de la selección del gestor, el WM no ofrece ninguna
    /// conversión: responde que no (propiedad None, ICCCM §2.2) para que el
    /// solicitante no quede esperando.
    fn refuse_conversion(&self, event: &SelectionRequestEvent) -> Result<(), Fatal> {
        let notify = SelectionNotifyEvent {
            response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: event.time,
            requestor: event.requestor,
            selection: event.selection,
            target: event.target,
            property: NONE,
        };
        self.conn
            .send_event(false, event.requestor, EventMask::NO_EVENT, notify)
            .map_err(Fatal::lost)?
            .ignore_error();
        Ok(())
    }

    /// Hora actual del servidor: un `ChangeProperty` vacío (añadir cero
    /// bytes) en la ventana de comprobación genera un `PropertyNotify` con
    /// la marca de tiempo. Los eventos que llegan mientras tanto se guardan
    /// en `pending`, en orden.
    ///
    /// Se usa donde ICCCM exige una marca de tiempo real y no hay evento que
    /// la aporte: `SetSelectionOwner` y los mensajes `WM_TAKE_FOCUS` y
    /// `WM_DELETE_WINDOW` que no vienen de una pulsación.
    fn server_time(&mut self) -> Result<Timestamp, Fatal> {
        let Some(identity) = self.identity else {
            return Ok(CURRENT_TIME);
        };
        let request = self
            .conn
            .change_property8(
                PropMode::APPEND,
                identity.window,
                self.atoms._NET_WM_NAME,
                self.atoms.UTF8_STRING,
                &[],
            )
            .map_err(Fatal::lost)?
            .sequence_number();
        self.conn.flush().map_err(Fatal::lost)?;
        loop {
            let (event, sequence) = self
                .conn
                .wait_for_event_with_sequence()
                .map_err(Fatal::lost)?;
            if let Event::PropertyNotify(notify) = &event {
                if notify.window == identity.window
                    && notify.atom == self.atoms._NET_WM_NAME
                    && sequence >= request
                {
                    return Ok(notify.time);
                }
            }
            self.pending.push_back((event, sequence));
        }
    }

    /// Escribe las propiedades marcadas en `dirty`. `_NET_CLIENT_LIST_STACKING`
    /// sale del orden real del servidor (`QueryTree`, de abajo arriba).
    fn publish_dirty(&mut self) -> Result<(), Fatal> {
        if std::mem::take(&mut self.dirty.clients) {
            let list = self.workspaces.client_list();
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    self.root,
                    self.atoms._NET_CLIENT_LIST,
                    AtomEnum::WINDOW,
                    &list,
                )
                .map_err(Fatal::lost)?;
        }
        if std::mem::take(&mut self.dirty.stacking) {
            let tree = self
                .conn
                .query_tree(self.root)
                .map_err(Fatal::lost)?
                .reply();
            let children = match tree {
                Ok(tree) => tree.children,
                Err(ReplyError::ConnectionError(e)) => return Err(Fatal::lost(e)),
                Err(ReplyError::X11Error(e)) => {
                    eprintln!("crimson-wm: QueryTree de la raíz: {:?}", e.error_kind);
                    return Ok(());
                }
            };
            let stacking = ewmh::stacking(&children, |w| self.workspaces.is_managed(w));
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    self.root,
                    self.atoms._NET_CLIENT_LIST_STACKING,
                    AtomEnum::WINDOW,
                    &stacking,
                )
                .map_err(Fatal::lost)?;
        }
        Ok(())
    }

    /// Gestiona las ventanas que ya estaban mapeadas al arrancar y da el foco
    /// a la maestra (o a la primera que lo admita).
    fn adopt_existing(&mut self) -> Result<(), Fatal> {
        let tree = self
            .conn
            .query_tree(self.root)
            .map_err(Fatal::lost)?
            .reply()
            .map_err(|e| Fatal::startup("QueryTree", e))?;
        let mut fullscreen = Vec::new();
        for window in tree.children {
            let Some((info, map_state)) = self.window_info(window)? else {
                continue;
            };
            if map_state != MapState::VIEWABLE {
                continue;
            }
            match clients::classify(&info) {
                Disposition::Unmanaged => {}
                Disposition::Fixed(layer) => self.manage_fixed(window, layer, info.fullscreen)?,
                Disposition::Managed(class) => {
                    self.manage(window, class)?;
                    if info.fullscreen {
                        fullscreen.push(window);
                    }
                }
            }
        }
        // De abajo arriba: si varias piden pantalla completa, la más alta de
        // la pila es la última y gana (ADR 0009).
        for window in fullscreen {
            self.set_fullscreen(window, true, "adopción", true)?;
        }
        // También coloca las fijas en su capa (ADR 0011).
        self.arrange()?;
        self.set_focus(self.workspaces.current().fallback(), CURRENT_TIME)
    }

    /// Una ventana nueva se gestiona en el escritorio activo, salvo un
    /// diálogo cuyo propietario efectivo está en otro: va oculto a ese
    /// escritorio, sin cambiar el activo ni el foco (ADR 0010). Una ventana
    /// gestionada de otro escritorio sigue oculta: ni se cambia de escritorio
    /// ni se le da el foco (ADR 0007). Una fija se coloca en su capa antes
    /// de mapearla y no recibe el foco (ADR 0011).
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
        if !self.workspaces.is_managed(window) {
            let Some((info, _)) = self.window_info(window)? else {
                // La ventana desapareció antes de poder clasificarla.
                return Ok(());
            };
            let class = match clients::classify(&info) {
                Disposition::Managed(class) => Some(class),
                Disposition::Fixed(layer) => {
                    self.manage_fixed(window, layer, info.fullscreen)?;
                    self.apply_layers()?;
                    None
                }
                // Una override-redirect no genera MapRequest; si llegara,
                // se mapea sin gestionarla, como antes de F9.
                Disposition::Unmanaged => None,
            };
            if let Some(class) = class {
                self.manage(window, class)?;
                self.log_relation(window, info.transient);
                let hidden = self.place_with_owner(window)?;
                // Diálogos y demás flotantes con foco; nunca los menús ni
                // las emergentes, que conservan su posición (ADR 0011).
                if class.centers {
                    self.center_on_owner(window)?;
                }
                if info.fullscreen {
                    // Pedida antes del mapeo (EWMH): ya aparece a pantalla
                    // completa.
                    self.set_fullscreen(window, true, "estado inicial", false)?;
                }
                if hidden {
                    // Oculta con su principal: no se mapea ni recibe el foco.
                    return Ok(());
                }
                // Se coloca antes de mapearla para que aparezca ya con su
                // tamaño y en su sitio de la pila (los diálogos, encima de su
                // principal).
                self.arrange()?;
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
    ///
    /// A pantalla completa (en mosaico o flotante) tampoco se concede nada:
    /// el `ConfigureNotify` sintético lleva el área completa y el marco
    /// guardado no cambia (ADR 0009).
    ///
    /// Una petición de apilamiento concedida a una flotante (que queda
    /// dentro de las capas 2 y 3) va seguida de `apply_layers`, que
    /// restablece las capas 1, 4 y 5 (ADR 0011). Las fijas tienen su propio
    /// tratamiento (`configure_fixed`).
    fn on_configure_request(&mut self, event: &ConfigureRequestEvent) -> Result<(), Fatal> {
        if let Some(fixed) = self.workspaces.fixed(event.window) {
            return self.configure_fixed(event, fixed.layer);
        }
        match self.workspaces.get(event.window) {
            Some(client) if client.fullscreen.is_some() => {
                let notify = synthetic_configure_notify(event.window, self.area, 0);
                self.conn
                    .send_event(false, event.window, EventMask::STRUCTURE_NOTIFY, notify)
                    .map_err(Fatal::lost)?;
            }
            Some(client) if client.kind == Kind::Tiled => {
                if let Some(rect) = client.geometry {
                    let notify = synthetic_configure_notify(event.window, rect, 0);
                    self.conn
                        .send_event(false, event.window, EventMask::STRUCTURE_NOTIFY, notify)
                        .map_err(Fatal::lost)?;
                }
            }
            managed => {
                let managed = managed.is_some();
                let aux = ConfigureWindowAux::from_configure_request(event);
                self.conn
                    .configure_window(event.window, &aux)
                    .map_err(Fatal::lost)?;
                // Puede haber cambiado el apilamiento.
                self.dirty.stacking = true;
                if managed && aux.stack_mode.is_some() {
                    self.apply_layers()?;
                }
            }
        }
        Ok(())
    }

    /// ConfigureRequest de una ventana fija (ADR 0011): se concede la
    /// geometría pedida, pero no el apilamiento (`stack_mode` y `sibling`),
    /// porque su sitio en la pila lo decide su capa. Si no queda nada que
    /// conceder, se responde con un ConfigureNotify sintético con la
    /// geometría real (ICCCM §4.1.5).
    fn configure_fixed(
        &mut self,
        event: &ConfigureRequestEvent,
        layer: Layer,
    ) -> Result<(), Fatal> {
        let window = event.window;
        let mut aux = ConfigureWindowAux::from_configure_request(event);
        if aux.stack_mode.is_some() || aux.sibling.is_some() {
            eprintln!(
                "crimson-wm: ventana 0x{window:x} (fija, {}): apilamiento pedido ignorado",
                layer_name(layer)
            );
            aux = aux.stack_mode(None).sibling(None);
        }
        let geometry = aux.x.is_some()
            || aux.y.is_some()
            || aux.width.is_some()
            || aux.height.is_some()
            || aux.border_width.is_some();
        if geometry {
            self.conn
                .configure_window(window, &aux)
                .map_err(Fatal::lost)?;
        } else if let Some(frame) = self.frame_of(window)? {
            let notify = synthetic_configure_notify(window, frame.rect, frame.border);
            self.conn
                .send_event(false, window, EventMask::STRUCTURE_NOTIFY, notify)
                .map_err(Fatal::lost)?;
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
        let reason = match gone {
            // El cliente la desmapeó antes de que el servidor procesara el
            // desmapeo con el que el WM la ocultaba.
            Gone::Unmapped if pending => "desmapeada por el cliente; desmapeo del WM pendiente",
            Gone::Unmapped => "desmapeada por el cliente",
            Gone::Withdrawn => "UnmapNotify sintético",
            Gone::Destroyed => "destruida",
            Gone::Reparented => "con otro padre",
        };
        // Una fija: siempre visible y nunca enfocada por el WM (ADR 0011).
        if let Some(fixed) = self.workspaces.unmanage_fixed(window) {
            eprintln!(
                "crimson-wm: ventana 0x{window:x} retirada (fija, {}): {reason}",
                layer_name(fixed.layer)
            );
            self.dirty.clients = true;
            self.dirty.stacking = true;
            return self.disown(window, gone, false, true);
        }
        // Antes de olvidarla: a su propietario vuelve el foco (ADR 0010).
        let owner = self.workspaces.owner_of(window);
        let Some((desk, removed)) = self.workspaces.unmanage(window) else {
            return Ok(());
        };
        eprintln!(
            "crimson-wm: ventana 0x{window:x} retirada ({}) del escritorio {}: {reason}",
            kind_name(removed.kind),
            desk + 1
        );
        self.dirty.clients = true;
        self.dirty.stacking = true;
        let active = desk == self.workspaces.index();
        self.disown(window, gone, removed.fullscreen, active)?;
        let back = if removed.was_focused {
            self.workspaces.focus_return(owner, desk)
        } else {
            None
        };
        if !active {
            // Al mostrar ese escritorio, el foco irá a la propietaria.
            if let (Some(owner), Some(clients)) = (back, self.workspaces.desk_mut(desk)) {
                clients.focus(owner);
            }
            return Ok(());
        }
        if removed.kind == Kind::Tiled || removed.fullscreen {
            self.arrange()?;
        }
        if removed.was_focused {
            let target = back.or_else(|| self.workspaces.current().fallback());
            self.set_focus(target, CURRENT_TIME)?;
        }
        Ok(())
    }

    /// Lo que el WM retira de una ventana que deja de gestionar sin que
    /// haya sido destruida: el save-set, `WM_STATE`, `_NET_WM_DESKTOP` y, si
    /// estaba a pantalla completa, su FULLSCREEN. Una retirada sintética de
    /// una ventana que el WM tiene mapeada (`visible`) se desmapea.
    fn disown(
        &mut self,
        window: Window,
        gone: Gone,
        fullscreen: bool,
        visible: bool,
    ) -> Result<(), Fatal> {
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
            // El WM solo retira lo suyo de `_NET_WM_STATE`: FULLSCREEN.
            if fullscreen {
                self.write_fullscreen_state(window, false)?;
            }
        }
        if gone == Gone::Withdrawn && visible {
            // Retirada sintética de una ventana que el WM tiene mapeada
            // (p. ej., la mostró justo antes de recibirla): se desmapea para
            // que no quede visible sin gestionar.
            self.conn
                .unmap_window(window)
                .map_err(Fatal::lost)?
                .ignore_error();
        }
        Ok(())
    }

    /// Registra por qué una ventana con `WM_TRANSIENT_FOR` no tiene
    /// propietario efectivo, si es el caso (ADR 0010).
    fn log_relation(&self, window: Window, transient: bool) {
        let Some(client) = self.workspaces.get(window) else {
            return;
        };
        let reason = match client.transient_for {
            None if transient => "valor 0, la raíz o la propia ventana",
            None => return,
            Some(owner) if self.workspaces.owner_of(window).is_some() => {
                eprintln!("crimson-wm: ventana 0x{window:x}: diálogo de 0x{owner:x}");
                return;
            }
            Some(owner) if self.workspaces.get(owner).is_none() => {
                "propietario pendiente o no gestionado"
            }
            Some(_) => "ciclo o cadena de más de 16 propietarios",
        };
        eprintln!(
            "crimson-wm: ventana 0x{window:x}: WM_TRANSIENT_FOR sin propietario efectivo ({reason})"
        );
    }

    /// Un diálogo recién gestionado va al escritorio de su propietario
    /// efectivo (ADR 0010). Si es otro que el activo, queda oculto allí,
    /// como la más reciente de su historial, sin cambiar el escritorio
    /// activo ni el foco. Devuelve `true` si queda oculto.
    fn place_with_owner(&mut self, window: Window) -> Result<bool, Fatal> {
        let current = self.workspaces.index();
        let Some(desk) = self
            .workspaces
            .owner_of(window)
            .and_then(|o| self.workspaces.locate(o))
            .filter(|&d| d != current)
        else {
            return Ok(false);
        };
        if self.workspaces.move_window(window, desk, true).is_none() {
            return Ok(false);
        }
        self.publish_desktop(window, desk)?;
        self.set_wm_state(window, ICONIC_STATE)?;
        self.dirty.stacking = true;
        eprintln!(
            "crimson-wm: ventana 0x{window:x} al escritorio {} con su principal (oculta)",
            desk + 1
        );
        Ok(true)
    }

    /// Centra un diálogo flotante recién gestionado sobre la geometría real
    /// de su propietario efectivo, dentro del área (ADR 0010). No se centra:
    /// sin propietario efectivo; si `WM_NORMAL_HINTS` pide una posición
    /// (`USPosition` o `PPosition`); o si alguna de las dos ventanas ya no
    /// existe. Solo se hace al gestionarla, nunca al reorganizar.
    fn center_on_owner(&mut self, window: Window) -> Result<(), Fatal> {
        let Some(owner) = self.workspaces.owner_of(window) else {
            return Ok(());
        };
        let hints = self
            .conn
            .get_property(
                false,
                window,
                AtomEnum::WM_NORMAL_HINTS,
                AtomEnum::WM_SIZE_HINTS,
                0,
                icccm::WM_NORMAL_HINTS_LEN,
            )
            .map_err(Fatal::lost)?
            .reply();
        let Some(hints) = reply_or_gone(hints, window, "GetProperty(WM_NORMAL_HINTS)")? else {
            return Ok(());
        };
        let flags: Vec<u32> = hints.value32().map(Iterator::collect).unwrap_or_default();
        if icccm::explicit_position(&flags) {
            eprintln!("crimson-wm: ventana 0x{window:x}: posición del cliente (WM_NORMAL_HINTS)");
            return Ok(());
        }
        let (Some(over), Some(own)) = (self.frame_of(owner)?, self.frame_of(window)?) else {
            return Ok(());
        };
        let (x, y) = layout::center(
            own.rect.width,
            own.rect.height,
            own.border,
            over.rect,
            self.area,
        );
        self.conn
            .configure_window(
                window,
                &ConfigureWindowAux::new().x(i32::from(x)).y(i32::from(y)),
            )
            .map_err(Fatal::lost)?
            .ignore_error();
        eprintln!("crimson-wm: ventana 0x{window:x} centrada sobre 0x{owner:x} en {x},{y}");
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
            self.restack_fullscreen()?;
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
        self.dirty.stacking = true;
        self.publish_current_desktop()
    }

    /// `Mod4+Shift+1`…`8`: lleva la ventana enfocada a ese escritorio, donde
    /// será la enfocada, sin cambiar de escritorio. El foco pasa a la más
    /// reciente del historial del activo.
    ///
    /// Sus dependientes del mismo escritorio (diálogos) van con ella, en su
    /// orden (ADR 0010).
    fn move_to(&mut self, desk: usize, time: Timestamp) -> Result<(), Fatal> {
        let family = self.workspaces.move_focused(desk);
        if family.is_empty() {
            return Ok(());
        }
        for moved in &family {
            eprintln!(
                "crimson-wm: ventana 0x{:x} llevada al escritorio {}",
                moved.window,
                moved.to + 1
            );
            self.publish_desktop(moved.window, moved.to)?;
            self.restore_displaced(moved.displaced, moved.to)?;
        }
        // Como en `show`: el foco cambia antes de ocultar las ventanas.
        self.set_focus(self.workspaces.current().fallback(), time)?;
        for moved in &family {
            self.hide(moved.window)?;
        }
        self.arrange()
    }

    /// La ventana que deja la pantalla completa en el escritorio de destino
    /// de un movimiento (la última gana). Si el destino es el activo, se
    /// reorganiza; si no, al mostrarse.
    fn restore_displaced(
        &mut self,
        displaced: Option<(Window, Restore)>,
        desk: usize,
    ) -> Result<(), Fatal> {
        let Some((window, restore)) = displaced else {
            return Ok(());
        };
        eprintln!(
            "crimson-wm: ventana 0x{window:x} sale de pantalla completa: llega otra al escritorio {}",
            desk + 1
        );
        self.restore_window(window, restore)?;
        if desk == self.workspaces.index() {
            self.arrange()?;
        }
        Ok(())
    }

    /// Petición `_NET_WM_DESKTOP`: lleva una ventana de cualquier escritorio
    /// a otro sin cambiar el activo ni darle el foco. Si sale del activo, se
    /// oculta (y el foco pasa a la siguiente del historial si la tenía); si
    /// entra en él, se muestra.
    ///
    /// Sus dependientes del mismo escritorio van con ella (ADR 0010).
    fn move_window(&mut self, window: Window, desk: usize) -> Result<(), Fatal> {
        let family = self.workspaces.move_family(window, desk, false);
        if family.is_empty() {
            eprintln!(
                "crimson-wm: _NET_WM_DESKTOP sin efecto: 0x{window:x} no está gestionada o ya está en el escritorio {}",
                desk + 1
            );
            return Ok(());
        }
        let current = self.workspaces.index();
        for moved in &family {
            eprintln!(
                "crimson-wm: ventana 0x{:x} llevada al escritorio {} (_NET_WM_DESKTOP)",
                moved.window,
                moved.to + 1
            );
            self.publish_desktop(moved.window, moved.to)?;
            self.restore_displaced(moved.displaced, moved.to)?;
        }
        if family.iter().any(|m| m.from == current && m.was_focused) {
            self.set_focus(self.workspaces.current().fallback(), CURRENT_TIME)?;
        }
        for moved in &family {
            if moved.from == current {
                self.hide(moved.window)?;
            }
        }
        // Las que entran en el activo se colocan antes de mapearlas.
        self.arrange()?;
        for moved in family.iter().filter(|m| m.to == current) {
            self.set_wm_state(moved.window, NORMAL_STATE)?;
            self.conn.map_window(moved.window).map_err(Fatal::lost)?;
        }
        self.dirty.stacking = true;
        Ok(())
    }

    /// `Mod4+w` y `_NET_CLOSE_WINDOW`: cierre cooperativo con
    /// `WM_DELETE_WINDOW` (ICCCM §4.2.8.1). Si la ventana no anuncia el
    /// protocolo, no se hace nada: una petición externa nunca termina un
    /// cliente a la fuerza (ADR 0008).
    fn close(&mut self, window: Window, time: Timestamp, origin: &str) -> Result<(), Fatal> {
        let Some(client) = self.workspaces.get(window) else {
            eprintln!("crimson-wm: {origin}: 0x{window:x} no está gestionada");
            return Ok(());
        };
        if !client.delete_window {
            eprintln!(
                "crimson-wm: {origin}: 0x{window:x} no anuncia WM_DELETE_WINDOW; no se cierra"
            );
            return Ok(());
        }
        let time = if time == CURRENT_TIME {
            self.server_time()?
        } else {
            time
        };
        let message = icccm::protocol_message(
            window,
            self.atoms.WM_PROTOCOLS,
            self.atoms.WM_DELETE_WINDOW,
            time,
        );
        self.conn
            .send_event(false, window, EventMask::NO_EVENT, message)
            .map_err(Fatal::lost)?;
        eprintln!(
            "crimson-wm: {origin}: WM_DELETE_WINDOW enviado a 0x{window:x} (marca de tiempo {time})"
        );
        Ok(())
    }

    /// `Mod4+Shift+w`: cierre forzado de la ventana enfocada, solo por
    /// acción explícita del usuario. `KillClient` cierra la conexión X11 del
    /// cliente: **todas** sus ventanas desaparecen, no solo esta.
    fn kill(&mut self) -> Result<(), Fatal> {
        let Some(window) = self.workspaces.current().focused() else {
            return Ok(());
        };
        self.conn.kill_client(window).map_err(Fatal::lost)?;
        eprintln!(
            "crimson-wm: Mod4+Shift+w: cierre forzado de 0x{window:x} (KillClient: se cierran todas las ventanas de su conexión)"
        );
        Ok(())
    }

    /// `PropertyNotify` de `WM_HINTS` o `WM_PROTOCOLS` de una ventana
    /// gestionada: se vuelven a leer ambas y se actualizan el modelo de
    /// entrada y el soporte de `WM_DELETE_WINDOW`. Las demás propiedades
    /// (también las que escribe el propio WM) se ignoran, así que no hay
    /// bucles. El cambio se aplica en la siguiente decisión de foco.
    ///
    /// `WM_TRANSIENT_FOR` se trata aparte (`on_transient_for`). Un cambio
    /// de `_NET_WM_WINDOW_TYPE` de una ventana gestionada solo se registra:
    /// la clasificación no cambia mientras siga gestionada y se repite
    /// cuando se retira y vuelve a mapearse (ADR 0011).
    fn on_property_notify(&mut self, event: &PropertyNotifyEvent) -> Result<(), Fatal> {
        if event.atom == u32::from(AtomEnum::WM_TRANSIENT_FOR) {
            return self.on_transient_for(event.window);
        }
        if event.atom == self.atoms._NET_WM_WINDOW_TYPE {
            if self.workspaces.is_managed(event.window) {
                eprintln!(
                    "crimson-wm: ventana 0x{:x}: cambio de _NET_WM_WINDOW_TYPE ignorado mientras siga gestionada (se reclasifica al volver a mapearse)",
                    event.window
                );
            }
            return Ok(());
        }
        let relevant = [u32::from(AtomEnum::WM_HINTS), self.atoms.WM_PROTOCOLS];
        if !relevant.contains(&event.atom) || self.workspaces.get(event.window).is_none() {
            return Ok(());
        }
        let Some((input, protocols)) = self.read_input(event.window)? else {
            return Ok(());
        };
        let model = InputModel::new(input, protocols.take_focus);
        let Some(client) = self.workspaces.get_mut(event.window) else {
            return Ok(());
        };
        if client.input != model || client.delete_window != protocols.delete_window {
            client.input = model;
            client.delete_window = protocols.delete_window;
            eprintln!(
                "crimson-wm: ventana 0x{:x}: modelo de entrada {model:?}, WM_DELETE_WINDOW {} (PropertyNotify)",
                event.window,
                if protocols.delete_window { "sí" } else { "no" }
            );
        }
        Ok(())
    }

    /// Cambio de `WM_TRANSIENT_FOR` de una ventana gestionada (ADR 0010): se
    /// vuelve a leer y validar y se actualiza el propietario declarado. No
    /// se reclasifica la ventana, no se mueve de escritorio ni cambia el
    /// foco; solo se recoloca la pila. El WM nunca escribe esta propiedad,
    /// así que no hay bucles.
    fn on_transient_for(&mut self, window: Window) -> Result<(), Fatal> {
        let Some(desk) = self.workspaces.locate(window) else {
            return Ok(());
        };
        let reply = self
            .conn
            .get_property(
                false,
                window,
                AtomEnum::WM_TRANSIENT_FOR,
                AtomEnum::ANY,
                0,
                1,
            )
            .map_err(Fatal::lost)?
            .reply();
        let Some(reply) = reply_or_gone(reply, window, "GetProperty(WM_TRANSIENT_FOR)")? else {
            return Ok(());
        };
        let values: Vec<u32> = reply.value32().map(Iterator::collect).unwrap_or_default();
        let declared = TransientFor::parse(&values, window, self.root);
        let Some(client) = self.workspaces.get_mut(window) else {
            return Ok(());
        };
        if client.transient_for == declared.owner() {
            return Ok(());
        }
        client.transient_for = declared.owner();
        eprintln!(
            "crimson-wm: ventana 0x{window:x}: WM_TRANSIENT_FOR cambiado a {declared:?} (PropertyNotify)"
        );
        self.log_relation(window, declared != TransientFor::None);
        if desk == self.workspaces.index() {
            self.arrange()?;
        } else {
            self.dirty.stacking = true;
        }
        Ok(())
    }

    /// Peticiones EWMH. Se validan (formato, origen, índices) antes de
    /// actuar; las rechazadas se registran y se ignoran.
    fn on_client_message(&mut self, event: &ClientMessageEvent) -> Result<(), Fatal> {
        let types = MessageTypes {
            active_window: self.atoms._NET_ACTIVE_WINDOW,
            current_desktop: self.atoms._NET_CURRENT_DESKTOP,
            wm_desktop: self.atoms._NET_WM_DESKTOP,
            close_window: self.atoms._NET_CLOSE_WINDOW,
            wm_state: self.atoms._NET_WM_STATE,
            fullscreen: self.atoms._NET_WM_STATE_FULLSCREEN,
        };
        let decoded = ewmh::decode(
            &types,
            self.root,
            event.type_,
            event.format,
            event.window,
            event.data.as_data32(),
        );
        match decoded {
            None => Ok(()),
            Some(Err(invalid)) => {
                eprintln!(
                    "crimson-wm: petición EWMH rechazada para 0x{:x}: {invalid:?}",
                    event.window
                );
                Ok(())
            }
            // Las fijas no cambian de escritorio, ni reciben el foco, ni se
            // cierran o ponen a pantalla completa por petición (ADR 0011).
            Some(Ok(request))
                if request
                    .window()
                    .is_some_and(|w| self.workspaces.fixed(w).is_some()) =>
            {
                eprintln!(
                    "crimson-wm: petición EWMH rechazada para 0x{:x}: ventana fija ({request:?})",
                    event.window
                );
                Ok(())
            }
            Some(Ok(Request::Activate { window, source })) => self.activate(window, source),
            Some(Ok(Request::CurrentDesktop { desk })) => {
                eprintln!("crimson-wm: _NET_CURRENT_DESKTOP: escritorio {}", desk + 1);
                self.view(desk, CURRENT_TIME)
            }
            Some(Ok(Request::WindowDesktop { window, desk })) => self.move_window(window, desk),
            Some(Ok(Request::Close {
                window,
                time,
                source,
            })) => self.close(window, time, &format!("_NET_CLOSE_WINDOW ({source:?})")),
            Some(Ok(Request::Fullscreen {
                window,
                action,
                source,
            })) => {
                let Some(client) = self.workspaces.get(window) else {
                    eprintln!(
                        "crimson-wm: _NET_WM_STATE ignorada: 0x{window:x} no está gestionada"
                    );
                    return Ok(());
                };
                let on = action.apply(client.fullscreen.is_some());
                let origin = format!("_NET_WM_STATE {action:?} ({source:?})");
                self.set_fullscreen(window, on, &origin, false)
            }
        }
    }

    /// `_NET_ACTIVE_WINDOW`: enfoca la ventana. Si está en otro escritorio,
    /// solo un paginador o el usuario (fuente 2) pueden cambiar a él; una
    /// aplicación, no (ADR 0008). La marca de tiempo del solicitante no se
    /// usa: el WM enfoca con la suya.
    fn activate(&mut self, window: Window, source: Source) -> Result<(), Fatal> {
        let Some(desk) = self.workspaces.locate(window) else {
            eprintln!("crimson-wm: _NET_ACTIVE_WINDOW ignorada: 0x{window:x} no está gestionada");
            return Ok(());
        };
        if !self.workspaces.get(window).is_some_and(|c| c.focusable) {
            eprintln!("crimson-wm: _NET_ACTIVE_WINDOW ignorada: 0x{window:x} no admite foco");
            return Ok(());
        }
        if desk == self.workspaces.index() {
            eprintln!("crimson-wm: _NET_ACTIVE_WINDOW ({source:?}): 0x{window:x}");
            return self.set_focus(Some(window), CURRENT_TIME);
        }
        if !source.may_switch_desktop() {
            eprintln!(
                "crimson-wm: _NET_ACTIVE_WINDOW ({source:?}) ignorada: 0x{window:x} está en el escritorio {}",
                desk + 1
            );
            return Ok(());
        }
        let Some(switch) = self.workspaces.switch(desk) else {
            return Ok(());
        };
        eprintln!(
            "crimson-wm: _NET_ACTIVE_WINDOW ({source:?}): 0x{window:x}, escritorio {}",
            desk + 1
        );
        // Pasa a ser la más reciente de su escritorio: `show` la enfoca.
        self.workspaces.current_mut().focus(window);
        self.show(switch, CURRENT_TIME)
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
        // PropertyChange, para seguir WM_HINTS y WM_PROTOCOLS; FocusChange,
        // para seguir los cambios de foco hechos por el cliente. No se
        // retira la selección al dejar de gestionarla: la ventana puede
        // haber sido destruida, y sus eventos se ignoran igualmente.
        let mut mask = EventMask::PROPERTY_CHANGE;
        if class.focusable {
            mask |= EventMask::FOCUS_CHANGE;
        }
        let aux = ChangeWindowAttributesAux::new().event_mask(mask);
        self.conn
            .change_window_attributes(window, &aux)
            .map_err(Fatal::lost)?;
        self.dirty.clients = true;
        self.dirty.stacking = true;
        Ok(())
    }

    /// Gestiona una ventana fija (ADR 0011): sin escritorio
    /// (`_NET_WM_DESKTOP` = 0xFFFFFFFF), en NormalState, en el save-set como
    /// las demás y sin foco. Si pide pantalla completa en `_NET_WM_STATE`, se
    /// rechaza y se retira de la propiedad. La capa se aplica después
    /// (`apply_layers`).
    fn manage_fixed(
        &mut self,
        window: Window,
        layer: Layer,
        fullscreen: bool,
    ) -> Result<(), Fatal> {
        if !self.workspaces.manage_fixed(window, layer) {
            return Ok(());
        }
        eprintln!(
            "crimson-wm: ventana 0x{window:x} gestionada (fija, {}) en todos los escritorios",
            layer_name(layer)
        );
        self.conn
            .change_save_set(SetMode::INSERT, window)
            .map_err(Fatal::lost)?;
        self.set_wm_state(window, NORMAL_STATE)?;
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms._NET_WM_DESKTOP,
                AtomEnum::CARDINAL,
                &[ewmh::STICKY],
            )
            .map_err(Fatal::lost)?;
        // PropertyChange, solo para registrar los cambios de tipo, que se
        // ignoran. Sin FocusChange: el WM nunca la enfoca.
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
        self.conn
            .change_window_attributes(window, &aux)
            .map_err(Fatal::lost)?;
        if fullscreen {
            eprintln!(
                "crimson-wm: ventana 0x{window:x} no admite pantalla completa (estado inicial): petición rechazada"
            );
            self.write_fullscreen_state(window, false)?;
        }
        self.dirty.clients = true;
        self.dirty.stacking = true;
        Ok(())
    }

    /// Da el foco a `target` según su modelo de entrada (ICCCM §4.1.7) y
    /// publica `_NET_ACTIVE_WINDOW`. Sin ventana, el foco vuelve a seguir al
    /// puntero, como en un servidor sin WM.
    ///
    /// Si la ventana acaba de desaparecer, el error X se registra como
    /// evento y su UnmapNotify o DestroyNotify vuelve a colocar el foco.
    ///
    /// `time` es la marca de la pulsación que lo provoca, o `CurrentTime` si
    /// no hay ninguna (ventana nueva o retirada, peticiones EWMH). En ese caso
    /// `SetInputFocus` sigue usando `CurrentTime`: el WM decide el foco y una
    /// marca anterior al último cambio haría que el servidor lo ignorara.
    /// Pero `WM_TAKE_FOCUS` siempre lleva una marca real (ICCCM §4.2.7), la
    /// hora del servidor si hace falta.
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
            self.publish_active(NONE)?;
            return self.restack_fullscreen();
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
            let time = if time == CURRENT_TIME {
                self.server_time()?
            } else {
                time
            };
            let message = icccm::protocol_message(
                window,
                self.atoms.WM_PROTOCOLS,
                self.atoms.WM_TAKE_FOCUS,
                time,
            );
            let cookie = self
                .conn
                .send_event(false, window, EventMask::NO_EVENT, message)
                .map_err(Fatal::lost)?;
            self.focus_request = cookie.sequence_number();
        }
        eprintln!("crimson-wm: foco en 0x{window:x} ({input:?})");
        self.publish_active(window)?;
        self.restack_fullscreen()
    }

    /// Pantalla completa (ADR 0009): entra o sale, de forma idempotente,
    /// para cualquier ventana gestionada, también de un escritorio oculto
    /// (sin cambiar de escritorio ni el foco). Es el único punto que cambia
    /// `Client.fullscreen`.
    ///
    /// - Al entrar, una flotante guarda su marco real (`GetGeometry`). Con
    ///   `adopting`, un marco que ya ocupa el área es el de una pantalla
    ///   completa anterior y no se guarda: al salir se usa el de respaldo.
    /// - Si otra ventana de su escritorio estaba a pantalla completa, la
    ///   deja (la última gana).
    /// - `_NET_WM_STATE` solo cambia en `_NET_WM_STATE_FULLSCREEN`.
    /// - Si la ventana desaparece entre medias, no se cambia nada: su
    ///   `DestroyNotify` la olvidará.
    fn set_fullscreen(
        &mut self,
        window: Window,
        on: bool,
        origin: &str,
        adopting: bool,
    ) -> Result<(), Fatal> {
        let Some(client) = self.workspaces.get(window) else {
            return Ok(());
        };
        if client.fullscreen.is_some() == on {
            return Ok(());
        }
        if on && !client.fullscreen_allowed {
            eprintln!(
                "crimson-wm: ventana 0x{window:x} no admite pantalla completa ({origin}): petición rechazada"
            );
            // Si el cliente la pidió en `_NET_WM_STATE` antes de mapearla, se
            // retira: la propiedad describe el estado real.
            self.write_fullscreen_state(window, false)?;
            return Ok(());
        }
        let kind = client.kind;
        let desk = if on {
            let frame = match kind {
                Kind::Tiled => None,
                Kind::Floating => {
                    let Some(frame) = self.frame_of(window)? else {
                        return Ok(());
                    };
                    (!adopting || frame.rect != self.area).then_some(frame)
                }
            };
            // Primero la propiedad: si no se puede actualizar sin perder
            // estados ajenos, la ventana no entra (el modelo y lo publicado
            // siguen coherentes).
            if self.write_fullscreen_state(window, true)? != StateWrite::Done {
                return Ok(());
            }
            let Some(entered) = self.workspaces.enter_fullscreen(window, Restore { frame }) else {
                return Ok(());
            };
            if let Some((other, restore)) = entered.displaced {
                eprintln!(
                    "crimson-wm: ventana 0x{other:x} sale de pantalla completa: la ocupa 0x{window:x}"
                );
                self.restore_window(other, restore)?;
            }
            eprintln!(
                "crimson-wm: ventana 0x{window:x} a pantalla completa ({origin}) en el escritorio {}",
                entered.desk + 1
            );
            entered.desk
        } else {
            let Some((desk, restore)) = self.workspaces.leave_fullscreen(window) else {
                return Ok(());
            };
            self.restore_window(window, restore)?;
            eprintln!(
                "crimson-wm: ventana 0x{window:x} sale de pantalla completa ({origin}) en el escritorio {}",
                desk + 1
            );
            desk
        };
        if desk == self.workspaces.index() {
            self.arrange()?;
        }
        self.dirty.stacking = true;
        Ok(())
    }

    /// Devuelve una ventana que ya ha salido de pantalla completa en el
    /// modelo a su estado anterior: retira FULLSCREEN de `_NET_WM_STATE` y,
    /// si es flotante, le devuelve su marco (o el de respaldo si se
    /// desconoce). Una en mosaico recupera su hueco con `arrange` (o al
    /// mostrarse su escritorio).
    fn restore_window(&mut self, window: Window, restore: Restore) -> Result<(), Fatal> {
        self.write_fullscreen_state(window, false)?;
        if self.workspaces.get(window).map(|c| c.kind) != Some(Kind::Floating) {
            return Ok(());
        }
        let frame = restore.frame.unwrap_or_else(|| Frame::fallback(self.area));
        let aux = ConfigureWindowAux::new()
            .x(i32::from(frame.rect.x))
            .y(i32::from(frame.rect.y))
            .width(u32::from(frame.rect.width))
            .height(u32::from(frame.rect.height))
            .border_width(u32::from(frame.border));
        self.conn
            .configure_window(window, &aux)
            .map_err(Fatal::lost)?
            .ignore_error();
        Ok(())
    }

    /// Marco real de una ventana (`GetGeometry`). `None` si ya no existe.
    fn frame_of(&self, window: Window) -> Result<Option<Frame>, Fatal> {
        let reply = self.conn.get_geometry(window).map_err(Fatal::lost)?.reply();
        Ok(reply_or_gone(reply, window, "GetGeometry")?.map(|g| Frame {
            rect: Rect {
                x: g.x,
                y: g.y,
                width: g.width,
                height: g.height,
            },
            border: g.border_width,
        }))
    }

    /// Activa o retira `_NET_WM_STATE_FULLSCREEN` en `_NET_WM_STATE` de la
    /// ventana, conservando todos los demás átomos en su orden (ADR 0009).
    ///
    /// Se lee la propiedad completa, de cualquier tipo, hasta
    /// `ewmh::MAX_STATE_ATOMS` átomos, y solo se reescribe si es una lista de
    /// átomos leída entera. La lectura, la decisión y la escritura se hacen
    /// con el servidor capturado (`GrabServer`), así que ningún cliente puede
    /// cambiarla entre medias. Si es inválida o demasiado grande, no se
    /// escribe nada y se registra (`Refused`); si la ventana ya no existe,
    /// `Gone`.
    fn write_fullscreen_state(&self, window: Window, on: bool) -> Result<StateWrite, Fatal> {
        self.conn.grab_server().map_err(Fatal::lost)?;
        let outcome = self.update_fullscreen_state(window, on);
        self.conn.ungrab_server().map_err(Fatal::lost)?;
        outcome
    }

    fn update_fullscreen_state(&self, window: Window, on: bool) -> Result<StateWrite, Fatal> {
        let Some(read) = self.read_state(window)? else {
            return Ok(StateWrite::Gone);
        };
        let atom_type = u32::from(AtomEnum::ATOM);
        match ewmh::update_state(&read, atom_type, self.atoms._NET_WM_STATE_FULLSCREEN, on) {
            Ok(None) => Ok(StateWrite::Done),
            Ok(Some(states)) => {
                self.conn
                    .change_property32(
                        PropMode::REPLACE,
                        window,
                        self.atoms._NET_WM_STATE,
                        AtomEnum::ATOM,
                        &states,
                    )
                    .map_err(Fatal::lost)?
                    .ignore_error();
                Ok(StateWrite::Done)
            }
            Err(error) => {
                eprintln!(
                    "crimson-wm: _NET_WM_STATE de 0x{window:x} no se modifica ({error:?}): se conserva tal cual"
                );
                Ok(StateWrite::Refused)
            }
        }
    }

    /// `_NET_WM_STATE` completo, de cualquier tipo, hasta
    /// `ewmh::MAX_STATE_ATOMS` átomos. `None` si la ventana ya no existe.
    fn read_state(&self, window: Window) -> Result<Option<StateRead>, Fatal> {
        let reply = self
            .conn
            .get_property(
                false,
                window,
                self.atoms._NET_WM_STATE,
                AtomEnum::ANY,
                0,
                ewmh::MAX_STATE_ATOMS,
            )
            .map_err(Fatal::lost)?
            .reply();
        match reply {
            Ok(reply) => Ok(Some(state_read(&reply))),
            Err(ReplyError::ConnectionError(e)) => Err(Fatal::lost(e)),
            Err(ReplyError::X11Error(_)) => Ok(None),
        }
    }

    /// La ventana a pantalla completa del escritorio activo sube por encima
    /// de todo si tiene el foco y baja al fondo si no: así la enfocada, en
    /// mosaico o flotante, queda visible (EWMH, ADR 0009). Desde F9, el
    /// fondo está justo encima de las fijas `DESKTOP` (ADR 0011).
    ///
    /// Desde F8 cuenta su familia (ADR 0010): si el foco lo tiene uno de sus
    /// dependientes del mismo escritorio (un diálogo), también sube, y sus
    /// dependientes quedan encima de ella, en su orden de pila.
    fn restack_fullscreen(&mut self) -> Result<(), Fatal> {
        let index = self.workspaces.index();
        let desk = self.workspaces.current();
        let Some(window) = desk.fullscreen() else {
            return Ok(());
        };
        let focused = desk.focused();
        let family: Vec<Window> = self
            .workspaces
            .dependents(window)
            .into_iter()
            .filter(|&d| self.workspaces.locate(d) == Some(index))
            .collect();
        let family_focused =
            focused == Some(window) || focused.is_some_and(|f| family.contains(&f));
        let mode = if family_focused {
            StackMode::ABOVE
        } else {
            StackMode::BELOW
        };
        self.conn
            .configure_window(window, &ConfigureWindowAux::new().stack_mode(mode))
            .map_err(Fatal::lost)?;
        if !family_focused {
            // Al fondo, pero por encima de las DESKTOP (ADR 0011).
            self.lower_desktops()?;
        }
        if family_focused {
            let raise = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
            let order = self.workspaces.floating_stack(index);
            let mut above: Vec<Window> = order.into_iter().filter(|w| family.contains(w)).collect();
            // Las de mosaico de la familia (raro), al final.
            let rest: Vec<Window> = family
                .iter()
                .copied()
                .filter(|w| !above.contains(w))
                .collect();
            above.extend(rest);
            for dependent in above {
                self.conn
                    .configure_window(dependent, &raise)
                    .map_err(Fatal::lost)?;
            }
        }
        self.dirty.stacking = true;
        Ok(())
    }

    /// `_NET_ACTIVE_WINDOW` en la raíz (EWMH), con None si no hay ninguna.
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

    /// `_NET_NUMBER_OF_DESKTOPS` y `_NET_CURRENT_DESKTOP` en la raíz (ADR 0007
    /// y 0008).
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
                &icccm::wm_state(state),
            )
            .map_err(Fatal::lost)?;
        Ok(())
    }

    /// Al salir (atajo, pérdida de la selección del gestor o error tras la
    /// identificación): muestra las ventanas ocultas de los demás
    /// escritorios (el save-set lo haría igualmente al cerrarse la conexión,
    /// pero así quedan en NormalState), limpia lo propio y devuelve el foco
    /// al puntero, el estado inicial del servidor.
    ///
    /// Solo borra las propiedades de la raíz si `_NET_SUPPORTING_WM_CHECK`
    /// sigue apuntando a su ventana de comprobación: si otro gestor las ha
    /// sustituido, son suyas. Después destruye esa ventana, lo que libera la
    /// selección si aún es suya. `_NET_WM_DESKTOP` y `WM_STATE` se quedan en
    /// las ventanas, como recomienda EWMH para un gestor posterior.
    ///
    /// Termina con una petición con respuesta: sin ella, el proceso podría
    /// salir antes de que el servidor procese las anteriores, y otro cliente
    /// vería todavía el estado antiguo.
    ///
    /// Antes, todas las ventanas a pantalla completa, de todos los
    /// escritorios, vuelven a su hueco o a su marco y pierden FULLSCREEN
    /// (ADR 0009).
    fn release(&mut self) -> Result<(), Fatal> {
        for desk in 0..workspaces::COUNT {
            let Some(window) = self.workspaces.desk(desk).and_then(|d| d.fullscreen()) else {
                continue;
            };
            if let Some((_, restore)) = self.workspaces.leave_fullscreen(window) {
                self.restore_window(window, restore)?;
                self.place(desk)?;
                eprintln!("crimson-wm: ventana 0x{window:x} sale de pantalla completa (salida)");
            }
        }
        let hidden = (0..workspaces::COUNT)
            .filter(|&d| d != self.workspaces.index())
            .filter_map(|d| self.workspaces.desk(d))
            .flat_map(|d| d.windows());
        for window in hidden {
            self.set_wm_state(window, NORMAL_STATE)?;
            self.conn.map_window(window).map_err(Fatal::lost)?;
        }
        if let Some(identity) = self.identity {
            if self.root_check()? == Some(Some(identity.window)) {
                for property in [
                    self.atoms._NET_SUPPORTING_WM_CHECK,
                    self.atoms._NET_SUPPORTED,
                    self.atoms._NET_CLIENT_LIST,
                    self.atoms._NET_CLIENT_LIST_STACKING,
                    self.atoms._NET_ACTIVE_WINDOW,
                    self.atoms._NET_CURRENT_DESKTOP,
                    self.atoms._NET_NUMBER_OF_DESKTOPS,
                ] {
                    self.conn
                        .delete_property(self.root, property)
                        .map_err(Fatal::lost)?;
                }
            } else {
                eprintln!(
                    "crimson-wm: la raíz ya no apunta a la ventana de comprobación; se conservan sus propiedades"
                );
            }
            self.conn
                .destroy_window(identity.window)
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

    /// Aplica al escritorio activo su plan de geometría (master-stack y
    /// pantalla completa, `Clients::plan`). Solo se envía la geometría de las
    /// ventanas que cambian; después se suben las flotantes y se coloca en la
    /// pila la de pantalla completa según el foco.
    fn arrange(&mut self) -> Result<(), Fatal> {
        self.place(self.workspaces.index())?;
        let raise = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        // Pila jerárquica (ADR 0010): cada diálogo, encima de su principal.
        let floating: Vec<Window> = self
            .workspaces
            .floating_stack(self.workspaces.index())
            .into_iter()
            .filter(|&w| {
                self.workspaces
                    .get(w)
                    .is_some_and(|c| c.fullscreen.is_none())
            })
            .collect();
        for window in floating {
            self.conn
                .configure_window(window, &raise)
                .map_err(Fatal::lost)?;
        }
        self.dirty.stacking = true;
        self.apply_layers()
    }

    /// Capas de apilamiento (ADR 0011), de abajo arriba:
    /// 1. las fijas `DESKTOP`;
    /// 2. el mosaico;
    /// 3. las flotantes (las sube `arrange`, en el orden de F8);
    /// 4. las fijas `DOCK` y `NOTIFICATION`;
    /// 5. la ventana a pantalla completa enfocada con sus dependientes
    ///    (`restack_fullscreen`).
    ///
    /// Aquí se aplican las capas 4, 5 y 1, en ese orden. Ninguna de estas
    /// peticiones genera eventos que vuelvan a llamar a esta función (el WM
    /// no recibe ConfigureRequest de lo que configura él mismo), así que no
    /// hay ciclos. Las override-redirect no se tocan: una subida del WM
    /// puede dejar alguna por debajo hasta que su cliente la vuelva a subir.
    fn apply_layers(&mut self) -> Result<(), Fatal> {
        let raise = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        for window in self.workspaces.fixed_in(Layer::Above) {
            self.conn
                .configure_window(window, &raise)
                .map_err(Fatal::lost)?;
        }
        self.dirty.stacking = true;
        self.restack_fullscreen()?;
        self.lower_desktops()
    }

    /// Capa 1 (ADR 0011): las fijas `DESKTOP` al fondo de la pila; la más
    /// antigua, la última de todas.
    fn lower_desktops(&mut self) -> Result<(), Fatal> {
        let lower = ConfigureWindowAux::new().stack_mode(StackMode::BELOW);
        for window in self.workspaces.fixed_in(Layer::Below).into_iter().rev() {
            self.conn
                .configure_window(window, &lower)
                .map_err(Fatal::lost)?;
            self.dirty.stacking = true;
        }
        Ok(())
    }

    /// Geometría del plan de un escritorio, sin tocar el apilamiento. Sirve
    /// también para los ocultos (salida normal).
    fn place(&mut self, desk: usize) -> Result<(), Fatal> {
        let area = self.area;
        let Some(clients) = self.workspaces.desk_mut(desk) else {
            return Ok(());
        };
        let mut changed = Vec::new();
        for (window, rect) in clients.plan(area) {
            if clients.set_geometry(window, rect) {
                changed.push((window, rect));
            }
        }
        for (window, rect) in changed {
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
        Ok(())
    }

    /// Lee lo necesario para clasificar una ventana, con las seis
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
        let hints = property(
            AtomEnum::WM_HINTS.into(),
            AtomEnum::WM_HINTS,
            icccm::WM_HINTS_LEN,
        )?;
        let protocols = property(
            self.atoms.WM_PROTOCOLS,
            AtomEnum::ATOM,
            icccm::MAX_PROTOCOLS,
        )?;
        let states = property(
            self.atoms._NET_WM_STATE,
            AtomEnum::ANY,
            ewmh::MAX_STATE_ATOMS,
        )?;

        let attributes = reply_or_gone(attributes.reply(), window, "GetWindowAttributes")?;
        let transient = reply_or_gone(transient.reply(), window, "GetProperty(WM_TRANSIENT_FOR)")?;
        let types = reply_or_gone(types.reply(), window, "GetProperty(_NET_WM_WINDOW_TYPE)")?;
        let hints = reply_or_gone(hints.reply(), window, "GetProperty(WM_HINTS)")?;
        let protocols = reply_or_gone(protocols.reply(), window, "GetProperty(WM_PROTOCOLS)")?;
        let states = reply_or_gone(states.reply(), window, "GetProperty(_NET_WM_STATE)")?;
        let (
            Some(attributes),
            Some(transient),
            Some(types),
            Some(hints),
            Some(protocols),
            Some(states),
        ) = (attributes, transient, types, hints, protocols, states)
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
            owner: TransientFor::parse(&values32(&transient), window, self.root).owner(),
            window_type: self.types.window_type(&values32(&types)),
            input: icccm::input_hint(&values32(&hints)),
            protocols: Protocols::parse(&values32(&protocols), self.protocol_atoms()),
            fullscreen: match state_read(&states)
                .requests_fullscreen(AtomEnum::ATOM.into(), self.atoms._NET_WM_STATE_FULLSCREEN)
            {
                Ok(fullscreen) => fullscreen,
                Err(error) => {
                    eprintln!(
                        "crimson-wm: _NET_WM_STATE de 0x{window:x} no se interpreta ({error:?}): sin pantalla completa inicial"
                    );
                    false
                }
            },
        };
        Ok(Some((info, attributes.map_state)))
    }

    /// `WM_HINTS.input` y `WM_PROTOCOLS`, leídos juntos. `None`: la ventana
    /// ya no existe. Un valor mal formado cuenta como ausente.
    fn read_input(&self, window: Window) -> Result<Option<(Option<bool>, Protocols)>, Fatal> {
        let hints = self
            .conn
            .get_property(
                false,
                window,
                AtomEnum::WM_HINTS,
                AtomEnum::WM_HINTS,
                0,
                icccm::WM_HINTS_LEN,
            )
            .map_err(Fatal::lost)?;
        let protocols = self
            .conn
            .get_property(
                false,
                window,
                self.atoms.WM_PROTOCOLS,
                AtomEnum::ATOM,
                0,
                icccm::MAX_PROTOCOLS,
            )
            .map_err(Fatal::lost)?;
        let hints = reply_or_gone(hints.reply(), window, "GetProperty(WM_HINTS)")?;
        let protocols = reply_or_gone(protocols.reply(), window, "GetProperty(WM_PROTOCOLS)")?;
        let (Some(hints), Some(protocols)) = (hints, protocols) else {
            return Ok(None);
        };
        let values32 = |reply: &x11rb::protocol::xproto::GetPropertyReply| -> Vec<u32> {
            reply.value32().map(Iterator::collect).unwrap_or_default()
        };
        Ok(Some((
            icccm::input_hint(&values32(&hints)),
            Protocols::parse(&values32(&protocols), self.protocol_atoms()),
        )))
    }

    fn protocol_atoms(&self) -> ProtocolAtoms {
        ProtocolAtoms {
            delete_window: self.atoms.WM_DELETE_WINDOW,
            take_focus: self.atoms.WM_TAKE_FOCUS,
        }
    }
}

/// Resultado de actualizar `_NET_WM_STATE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StateWrite {
    /// Escrita, o ya tenía el valor pedido.
    Done,
    /// Inválida o demasiado grande: no se ha tocado.
    Refused,
    /// La ventana ya no existe.
    Gone,
}

fn state_read(reply: &x11rb::protocol::xproto::GetPropertyReply) -> StateRead {
    StateRead {
        type_: reply.type_,
        format: reply.format,
        bytes_after: reply.bytes_after,
        values: reply.value32().map(Iterator::collect).unwrap_or_default(),
    }
}

/// Registra los átomos de `names`, con todas las peticiones enviadas juntas.
fn intern_all(conn: &impl Connection, names: &[&str]) -> Result<Vec<Atom>, Fatal> {
    let cookies = names
        .iter()
        .map(|name| {
            conn.intern_atom(false, name.as_bytes())
                .map_err(Fatal::lost)
        })
        .collect::<Result<Vec<_>, _>>()?;
    cookies
        .into_iter()
        .map(|cookie| {
            cookie
                .reply()
                .map(|reply| reply.atom)
                .map_err(|e| Fatal::startup("InternAtom", e))
        })
        .collect()
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
/// la raíz (no hay reparenting): sin borde en las que coloca el WM y con el
/// suyo en las fijas.
fn synthetic_configure_notify(window: Window, rect: Rect, border: u16) -> ConfigureNotifyEvent {
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
        border_width: border,
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

fn layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::Below => "capa inferior",
        Layer::Above => "capa superior",
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
        let event = synthetic_configure_notify(0x20_0001, rect, 0);
        assert_eq!(event.response_type, 22);
        assert_eq!((event.event, event.window), (0x20_0001, 0x20_0001));
        assert_eq!(
            (event.x, event.y, event.width, event.height),
            (640, 360, 640, 360)
        );
        assert_eq!((event.border_width, event.above_sibling), (0, 0));
        assert!(!event.override_redirect);
        // Una fija conserva su borde (ADR 0011).
        assert_eq!(synthetic_configure_notify(1, rect, 3).border_width, 3);
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
}
