//! EWMH (ADR 0008, 0009 y 0011): lista exacta de lo que el WM declara en
//! `_NET_SUPPORTED`, tipos de ventana (`_NET_WM_WINDOW_TYPE`), decodificación
//! y validación de las peticiones de los clientes (`ClientMessage`), orden de
//! `_NET_CLIENT_LIST_STACKING` y valor de `_NET_WM_STATE`. Sin E/S.

use x11rb::protocol::xproto::{Atom, Timestamp, Window};

use crate::workspaces;

/// Nombre del WM en `_NET_WM_NAME` de la ventana de comprobación.
pub const WM_NAME: &str = "Crimson WM";

/// Lo que el WM implementa de verdad, en el orden en que se publica. No
/// incluye:
/// - `_NET_WM_NAME`: el WM la pone en su ventana de comprobación, pero no lee
///   la de los clientes;
/// - los tipos de ventana que EWMH describe para ventanas override-redirect
///   (`DROPDOWN_MENU`, `POPUP_MENU`, `TOOLTIP`, `COMBO` y `DND`): el WM los
///   reconoce para no centrarlos, enfocarlos ni ponerlos a pantalla
///   completa si alguno llega a gestionarse, pero no tiene un tratamiento
///   que anunciar (ADR 0011);
/// - los estados de `_NET_WM_STATE` distintos de la pantalla completa
///   (maximizar, sticky, above, below, etc.), `_NET_FRAME_EXTENTS`, etc.
///
/// Desde F10, `_NET_WM_STRUT`, `_NET_WM_STRUT_PARTIAL` y `_NET_WORKAREA`
/// (ADR 0012): solo los paneles (`DOCK`) reservan espacio.
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
    "_NET_WM_STATE",
    "_NET_WM_STATE_FULLSCREEN",
    "_NET_WM_WINDOW_TYPE",
    "_NET_WM_WINDOW_TYPE_NORMAL",
    "_NET_WM_WINDOW_TYPE_DIALOG",
    "_NET_WM_WINDOW_TYPE_UTILITY",
    "_NET_WM_WINDOW_TYPE_TOOLBAR",
    "_NET_WM_WINDOW_TYPE_SPLASH",
    "_NET_WM_WINDOW_TYPE_MENU",
    "_NET_WM_WINDOW_TYPE_DOCK",
    "_NET_WM_WINDOW_TYPE_DESKTOP",
    "_NET_WM_WINDOW_TYPE_NOTIFICATION",
    "_NET_WM_STRUT",
    "_NET_WM_STRUT_PARTIAL",
    "_NET_WORKAREA",
];

/// Valor de `_NET_WM_DESKTOP` que pide estar en todos los escritorios. El
/// WM solo lo publica en las ventanas fijas (ADR 0011); como petición de un
/// cliente se rechaza.
pub const STICKY: u32 = 0xFFFF_FFFF;

/// Tipos de ventana de EWMH (`_NET_WM_WINDOW_TYPE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowType {
    Normal,
    Dialog,
    Utility,
    Toolbar,
    Splash,
    Menu,
    DropdownMenu,
    PopupMenu,
    Tooltip,
    Notification,
    Dock,
    Desktop,
    Combo,
    Dnd,
}

impl WindowType {
    /// Los 14 tipos, en el orden de `NAMES`.
    pub const ALL: [WindowType; 14] = [
        WindowType::Normal,
        WindowType::Dialog,
        WindowType::Utility,
        WindowType::Toolbar,
        WindowType::Splash,
        WindowType::Menu,
        WindowType::DropdownMenu,
        WindowType::PopupMenu,
        WindowType::Tooltip,
        WindowType::Notification,
        WindowType::Dock,
        WindowType::Desktop,
        WindowType::Combo,
        WindowType::Dnd,
    ];

    /// Nombres de los átomos, en el orden de `ALL`.
    pub const NAMES: [&'static str; 14] = [
        "_NET_WM_WINDOW_TYPE_NORMAL",
        "_NET_WM_WINDOW_TYPE_DIALOG",
        "_NET_WM_WINDOW_TYPE_UTILITY",
        "_NET_WM_WINDOW_TYPE_TOOLBAR",
        "_NET_WM_WINDOW_TYPE_SPLASH",
        "_NET_WM_WINDOW_TYPE_MENU",
        "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
        "_NET_WM_WINDOW_TYPE_POPUP_MENU",
        "_NET_WM_WINDOW_TYPE_TOOLTIP",
        "_NET_WM_WINDOW_TYPE_NOTIFICATION",
        "_NET_WM_WINDOW_TYPE_DOCK",
        "_NET_WM_WINDOW_TYPE_DESKTOP",
        "_NET_WM_WINDOW_TYPE_COMBO",
        "_NET_WM_WINDOW_TYPE_DND",
    ];
}

/// Átomos de los 14 tipos en este servidor, en el orden de
/// `WindowType::ALL`. Es la única tabla que traduce átomos a tipos.
#[derive(Clone, Copy, Debug)]
pub struct TypeAtoms([Atom; 14]);

impl TypeAtoms {
    /// Con los átomos de `WindowType::NAMES`, en su orden.
    pub fn new(atoms: [Atom; 14]) -> Self {
        TypeAtoms(atoms)
    }

    /// Tipo de una ventana según los valores de `_NET_WM_WINDOW_TYPE`: el
    /// primero que se reconoce, porque la lista va en orden de preferencia
    /// (EWMH). Los desconocidos (también 0) se saltan; `None` si no se
    /// reconoce ninguno o la lista está vacía (propiedad ausente o mal
    /// formada).
    pub fn window_type(&self, values: &[Atom]) -> Option<WindowType> {
        values.iter().find_map(|&value| {
            self.0
                .iter()
                .position(|&atom| atom == value && atom != 0)
                .map(|i| WindowType::ALL[i])
        })
    }
}

/// Tipos de los mensajes que el WM atiende.
#[derive(Clone, Copy, Debug)]
pub struct MessageTypes {
    pub active_window: Atom,
    pub current_desktop: Atom,
    pub wm_desktop: Atom,
    pub close_window: Atom,
    pub wm_state: Atom,
    /// `_NET_WM_STATE_FULLSCREEN`: el único estado que el WM gestiona.
    pub fullscreen: Atom,
}

/// Acción de una petición `_NET_WM_STATE` (`data[0]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateAction {
    Remove,
    Add,
    Toggle,
}

impl StateAction {
    fn decode(value: u32) -> Result<Self, Invalid> {
        match value {
            0 => Ok(StateAction::Remove),
            1 => Ok(StateAction::Add),
            2 => Ok(StateAction::Toggle),
            other => Err(Invalid::Action(other)),
        }
    }

    /// Estado resultante a partir del actual. Añadir lo que ya está o quitar
    /// lo que no está no cambia nada (idempotente).
    pub fn apply(self, current: bool) -> bool {
        match self {
            StateAction::Remove => false,
            StateAction::Add => true,
            StateAction::Toggle => !current,
        }
    }
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
    /// `_NET_WM_STATE` sobre `_NET_WM_STATE_FULLSCREEN` (en `data[1]` o
    /// `data[2]`). El otro estado de la petición, si lo hay, se ignora.
    Fullscreen {
        window: Window,
        action: StateAction,
        source: Source,
    },
}

impl Request {
    /// La ventana a la que se refiere la petición; `None` para el cambio de
    /// escritorio activo.
    pub fn window(&self) -> Option<Window> {
        match *self {
            Request::Activate { window, .. }
            | Request::WindowDesktop { window, .. }
            | Request::Close { window, .. }
            | Request::Fullscreen { window, .. } => Some(window),
            Request::CurrentDesktop { .. } => None,
        }
    }
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
    /// Acción de `_NET_WM_STATE` distinta de 0, 1 o 2.
    Action(u32),
    /// `_NET_WM_STATE` sin `_NET_WM_STATE_FULLSCREEN`: estados que el WM no
    /// gestiona; no se aplican ni se añaden.
    UnsupportedState,
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
        types.wm_state,
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
    } else if message_type == types.wm_state {
        StateAction::decode(data[0]).and_then(|action| {
            if data[1] != types.fullscreen && data[2] != types.fullscreen {
                return Err(Invalid::UnsupportedState);
            }
            Source::decode(data[3]).map(|source| Request::Fullscreen {
                window,
                action,
                source,
            })
        })
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

/// Máximo de átomos de `_NET_WM_STATE` que el WM lee y reescribe (4 KiB).
/// EWMH define una docena de estados; una propiedad mayor se rechaza sin
/// escribir nada, para no agotar memoria ni truncarla.
pub const MAX_STATE_ATOMS: u32 = 1024;

/// Lo que devolvió `GetProperty(_NET_WM_STATE, AnyPropertyType, 0,
/// MAX_STATE_ATOMS)`: tipo y formato reales, bytes que quedaron sin leer y
/// valores de 32 bits (vacío si el formato no es 32).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateRead {
    /// 0 (None) si la propiedad no existe.
    pub type_: Atom,
    pub format: u8,
    pub bytes_after: u32,
    pub values: Vec<Atom>,
}

/// Por qué no se puede actualizar `_NET_WM_STATE` sin riesgo de destruir
/// estados ajenos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    /// No es una lista de átomos (tipo `ATOM`, formato 32).
    Malformed { type_: Atom, format: u8 },
    /// Más de `MAX_STATE_ATOMS` átomos: la lectura quedó incompleta.
    TooLarge { bytes_after: u32 },
}

impl StateRead {
    /// Los átomos de la propiedad completa, o por qué no se puede usar. Una
    /// propiedad ausente es una lista vacía.
    pub fn atoms(&self, atom_type: Atom) -> Result<&[Atom], StateError> {
        if self.type_ == 0 {
            return Ok(&[]);
        }
        if self.type_ != atom_type || self.format != 32 {
            return Err(StateError::Malformed {
                type_: self.type_,
                format: self.format,
            });
        }
        if self.bytes_after != 0 {
            return Err(StateError::TooLarge {
                bytes_after: self.bytes_after,
            });
        }
        Ok(&self.values)
    }

    /// La ventana pide la pantalla completa. Una propiedad inválida o
    /// demasiado grande no cuenta como petición.
    pub fn requests_fullscreen(
        &self,
        atom_type: Atom,
        fullscreen: Atom,
    ) -> Result<bool, StateError> {
        self.atoms(atom_type)
            .map(|atoms| atoms.contains(&fullscreen))
    }
}

/// Valor que hay que escribir en `_NET_WM_STATE` para activar o no la
/// pantalla completa: `Ok(None)` si no hay que cambiar nada; `Err` si la
/// propiedad no se puede reescribir sin perder información (no se escribe
/// nada, ni siquiera en parte). Los átomos ajenos se conservan todos, en su
/// orden (`with_fullscreen`).
pub fn update_state(
    read: &StateRead,
    atom_type: Atom,
    fullscreen: Atom,
    on: bool,
) -> Result<Option<Vec<Atom>>, StateError> {
    let current = read.atoms(atom_type)?;
    let updated = with_fullscreen(current, fullscreen, on);
    Ok((updated != current).then_some(updated))
}

/// Nuevo valor de `_NET_WM_STATE` con la pantalla completa activada o no.
/// El WM solo gestiona `_NET_WM_STATE_FULLSCREEN`: los demás átomos (los
/// que puso el cliente antes de mapear la ventana, como
/// `_NET_WM_STATE_SKIP_TASKBAR`) se conservan en su orden, sin
/// interpretarlos ni declararlos (ADR 0009). Al activarlo, si ya estaba se
/// queda en su sitio (sin repeticiones) y, si no, se añade al final; al
/// desactivarlo se quitan todas sus apariciones.
pub fn with_fullscreen(states: &[Atom], fullscreen: Atom, on: bool) -> Vec<Atom> {
    let mut seen = false;
    let mut result: Vec<Atom> = states
        .iter()
        .copied()
        .filter(|&a| {
            if a != fullscreen {
                return true;
            }
            let keep = on && !seen;
            seen = true;
            keep
        })
        .collect();
    if on && !seen {
        result.push(fullscreen);
    }
    result
}

/// `_NET_CLIENT_LIST_STACKING`: las ventanas gestionadas en el orden de
/// apilamiento real (`QueryTree` las da de abajo arriba, como pide EWMH).
pub fn stacking(children: &[Window], managed: impl Fn(Window) -> bool) -> Vec<Window> {
    children.iter().copied().filter(|&w| managed(w)).collect()
}

/// Valores de `_NET_WM_STRUT` (ADR 0012).
pub const STRUT_LEN: u32 = 4;
/// Valores de `_NET_WM_STRUT_PARTIAL` (ADR 0012).
pub const STRUT_PARTIAL_LEN: u32 = 12;

/// Borde de la pantalla, en el orden de los valores de los struts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

    fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
            Side::Top => 2,
            Side::Bottom => 3,
        }
    }
}

/// Reserva declarada por un cliente en un borde: grosor desde el borde de
/// la raíz y tramo del borde que ocupa, en coordenadas de la raíz, con el
/// final incluido (EWMH). Un grosor 0 no reserva nada.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reservation {
    pub thickness: u32,
    pub start: u32,
    pub end: u32,
}

/// Struts de una ventana, ya validados, con los valores declarados por el
/// cliente (sin recortar). La reserva efectiva se calcula aparte, con la
/// geometría de la pantalla (`Strut::effective`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Strut {
    sides: [Reservation; 4],
}

impl Strut {
    pub fn side(&self, side: Side) -> Reservation {
        self.sides[side.index()]
    }

    /// No reserva nada en ningún borde.
    pub fn is_empty(&self) -> bool {
        self.sides.iter().all(|r| r.thickness == 0)
    }

    /// Grosor efectivo de cada borde (izquierda, derecha, arriba, abajo)
    /// en una raíz de `width` × `height`:
    /// - un tramo que no toca la raíz (`start` fuera de ella) no reserva
    ///   nada (correspondería a un monitor que no existe);
    /// - un tramo que se sale de la raíz cuenta solo en la parte que la toca,
    ///   así que basta con que la toque;
    /// - el grosor se limita a la dimensión de la raíz perpendicular al
    ///   borde.
    ///
    /// El área de trabajo resultante la limita después `layout::work_area`.
    pub fn effective(&self, width: u16, height: u16) -> [u32; 4] {
        Side::ALL.map(|side| {
            let r = self.side(side);
            let (along, across) = match side {
                Side::Left | Side::Right => (height, width),
                Side::Top | Side::Bottom => (width, height),
            };
            if r.thickness == 0 || r.start >= u32::from(along) {
                0
            } else {
                r.thickness.min(u32::from(across))
            }
        })
    }

    /// Algún borde con grosor tiene un tramo que no toca la raíz.
    pub fn outside(&self, width: u16, height: u16) -> bool {
        let effective = self.effective(width, height);
        Side::ALL
            .iter()
            .zip(effective)
            .any(|(&side, e)| self.side(side).thickness > 0 && e == 0)
    }
}

/// Lo que devolvió `GetProperty(propiedad, AnyPropertyType, 0, longitud)`
/// de un strut, como `StateRead`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StrutRead {
    /// 0 (None) si la propiedad no existe.
    pub type_: Atom,
    pub format: u8,
    pub bytes_after: u32,
    pub values: Vec<u32>,
}

/// Qué propiedad da la reserva de una ventana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrutSource {
    /// Ninguna de las dos existe.
    None,
    /// `_NET_WM_STRUT_PARTIAL`, válida.
    Partial,
    /// `_NET_WM_STRUT`, válida, sin `_NET_WM_STRUT_PARTIAL`.
    Legacy,
}

/// Defecto de una propiedad de strut presente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrutError {
    /// No es `CARDINAL` de formato 32.
    Type { type_: Atom, format: u8 },
    /// No tiene exactamente los valores que define EWMH (4 o 12).
    Length { values: usize, bytes_after: u32 },
    /// Un borde con grosor cuyo tramo empieza después de terminar.
    Range(Side),
}

/// Propiedad de strut defectuosa: cuál y por qué.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrutDefect {
    pub partial: bool,
    pub error: StrutError,
}

/// Interpreta las dos propiedades de strut de una ventana (EWMH):
/// - `_NET_WM_STRUT_PARTIAL` válida: se usa, y `_NET_WM_STRUT` se ignora;
/// - `_NET_WM_STRUT_PARTIAL` ausente: se usa `_NET_WM_STRUT` si es válida,
///   con cada tramo igual al borde completo;
/// - `_NET_WM_STRUT_PARTIAL` presente pero mal formada: es un defecto y no
///   se recurre a `_NET_WM_STRUT` (ADR 0012);
/// - las dos ausentes: sin reserva.
///
/// `width` y `height` son los de la raíz, para los tramos completos de
/// `_NET_WM_STRUT`.
pub fn struts(
    partial: &StrutRead,
    legacy: &StrutRead,
    cardinal: Atom,
    width: u16,
    height: u16,
) -> Result<(StrutSource, Strut), StrutDefect> {
    if partial.type_ != 0 {
        return parse_strut(partial, cardinal, STRUT_PARTIAL_LEN)
            .map(|values| {
                let mut sides = [Reservation::default(); 4];
                for (i, side) in sides.iter_mut().enumerate() {
                    *side = Reservation {
                        thickness: values[i],
                        start: values[4 + 2 * i],
                        end: values[5 + 2 * i],
                    };
                }
                (StrutSource::Partial, sides)
            })
            .and_then(|(source, sides)| checked(sides).map(|strut| (source, strut)))
            .map_err(|error| StrutDefect {
                partial: true,
                error,
            });
    }
    if legacy.type_ == 0 {
        return Ok((StrutSource::None, Strut::default()));
    }
    let full = |side: Side| {
        let along = match side {
            Side::Left | Side::Right => height,
            Side::Top | Side::Bottom => width,
        };
        u32::from(along).saturating_sub(1)
    };
    parse_strut(legacy, cardinal, STRUT_LEN)
        .map(|values| {
            let sides = Side::ALL.map(|side| Reservation {
                thickness: values[side.index()],
                start: 0,
                end: full(side),
            });
            (StrutSource::Legacy, Strut { sides })
        })
        .map_err(|error| StrutDefect {
            partial: false,
            error,
        })
}

/// Valores de una propiedad de strut presente: `CARDINAL` de formato 32,
/// con exactamente `len` valores leídos enteros.
fn parse_strut(read: &StrutRead, cardinal: Atom, len: u32) -> Result<Vec<u32>, StrutError> {
    if read.type_ != cardinal || read.format != 32 {
        return Err(StrutError::Type {
            type_: read.type_,
            format: read.format,
        });
    }
    if read.bytes_after != 0 || u32::try_from(read.values.len()) != Ok(len) {
        return Err(StrutError::Length {
            values: read.values.len(),
            bytes_after: read.bytes_after,
        });
    }
    Ok(read.values.clone())
}

/// Un borde con grosor necesita un tramo con `start <= end`. Los bordes sin
/// grosor no reservan nada y su tramo no importa.
fn checked(sides: [Reservation; 4]) -> Result<Strut, StrutError> {
    for side in Side::ALL {
        let r = sides[side.index()];
        if r.thickness > 0 && r.start > r.end {
            return Err(StrutError::Range(side));
        }
    }
    Ok(Strut { sides })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: Window = 0x1ff;
    const W: Window = 0x40_0001;
    const FULLSCREEN: Atom = 50;
    const TYPES: MessageTypes = MessageTypes {
        active_window: 10,
        current_desktop: 11,
        wm_desktop: 12,
        close_window: 13,
        wm_state: 14,
        fullscreen: FULLSCREEN,
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
        assert_eq!(SUPPORTED.len(), 24);
        // U22: struts y área de trabajo, al final (ADR 0012).
        assert_eq!(
            &SUPPORTED[21..],
            ["_NET_WM_STRUT", "_NET_WM_STRUT_PARTIAL", "_NET_WORKAREA"]
        );
        assert!(SUPPORTED.contains(&"_NET_WM_STATE"));
        assert!(SUPPORTED.contains(&"_NET_WM_STATE_FULLSCREEN"));
        for (i, name) in SUPPORTED.iter().enumerate() {
            assert!(name.starts_with("_NET_"), "{name}");
            assert!(!SUPPORTED[i + 1..].contains(name), "repetido: {name}");
        }
        // Nada aplazado se declara.
        for deferred in [
            "_NET_WM_STATE_MAXIMIZED_VERT",
            "_NET_WM_STATE_MAXIMIZED_HORZ",
            "_NET_WM_STATE_STICKY",
            "_NET_WM_STATE_ABOVE",
            "_NET_WM_STATE_BELOW",
            "_NET_FRAME_EXTENTS",
            "_NET_WM_NAME",
            "_NET_DESKTOP_NAMES",
            "_NET_WM_PING",
        ] {
            assert!(
                !SUPPORTED.contains(&deferred),
                "{deferred} no está implementada"
            );
        }
        // U21: tipos declarados (ADR 0011): los que EWMH describe para
        // ventanas gestionadas, más NOTIFICATION, que tiene tratamiento
        // propio. Los de ventanas override-redirect se reconocen sin
        // declararse.
        let declared: Vec<&str> = SUPPORTED
            .iter()
            .copied()
            .filter(|n| n.starts_with("_NET_WM_WINDOW_TYPE_"))
            .collect();
        assert_eq!(
            declared,
            [
                "_NET_WM_WINDOW_TYPE_NORMAL",
                "_NET_WM_WINDOW_TYPE_DIALOG",
                "_NET_WM_WINDOW_TYPE_UTILITY",
                "_NET_WM_WINDOW_TYPE_TOOLBAR",
                "_NET_WM_WINDOW_TYPE_SPLASH",
                "_NET_WM_WINDOW_TYPE_MENU",
                "_NET_WM_WINDOW_TYPE_DOCK",
                "_NET_WM_WINDOW_TYPE_DESKTOP",
                "_NET_WM_WINDOW_TYPE_NOTIFICATION",
            ]
        );
        for name in declared {
            assert!(WindowType::NAMES.contains(&name), "{name} sin tipo");
        }
    }

    // U21: _NET_WM_WINDOW_TYPE.

    /// Átomos ficticios: 300 + índice de `WindowType::ALL`.
    fn type_atoms() -> TypeAtoms {
        let mut atoms = [0; 14];
        for (i, atom) in (300..).zip(atoms.iter_mut()) {
            *atom = i;
        }
        TypeAtoms::new(atoms)
    }

    fn atom_of(t: WindowType) -> Atom {
        let i = WindowType::ALL.iter().position(|&x| x == t).unwrap();
        300 + Atom::try_from(i).unwrap()
    }

    #[test]
    fn los_14_tipos() {
        let atoms = type_atoms();
        for (t, name) in WindowType::ALL.iter().zip(WindowType::NAMES) {
            assert_eq!(atoms.window_type(&[atom_of(*t)]), Some(*t), "{name}");
            let suffix = format!("{t:?}").to_uppercase();
            assert_eq!(
                name.trim_start_matches("_NET_WM_WINDOW_TYPE_")
                    .replace('_', ""),
                suffix,
                "nombre y tipo en el mismo orden"
            );
        }
        for (i, name) in WindowType::NAMES.iter().enumerate() {
            assert!(
                !WindowType::NAMES[i + 1..].contains(name),
                "repetido: {name}"
            );
        }
    }

    #[test]
    fn el_primer_tipo_reconocido_gana() {
        let atoms = type_atoms();
        let (normal, dialog, dock) = (
            atom_of(WindowType::Normal),
            atom_of(WindowType::Dialog),
            atom_of(WindowType::Dock),
        );
        assert_eq!(
            atoms.window_type(&[normal, dialog]),
            Some(WindowType::Normal)
        );
        assert_eq!(
            atoms.window_type(&[dialog, normal]),
            Some(WindowType::Dialog)
        );
        // Desconocidos y 0 se saltan.
        assert_eq!(atoms.window_type(&[999, 0, dock]), Some(WindowType::Dock));
        assert_eq!(atoms.window_type(&[dock, dock]), Some(WindowType::Dock));
        // Ausente, vacía o sin ninguno reconocido.
        assert_eq!(atoms.window_type(&[]), None);
        assert_eq!(atoms.window_type(&[999, 0, 1]), None);
        // Un átomo 0 en la tabla nunca coincide con un valor 0.
        let mut with_zero = [0; 14];
        with_zero[1] = 5;
        assert_eq!(
            TypeAtoms::new(with_zero).window_type(&[0, 5]),
            Some(WindowType::Dialog)
        );
    }

    #[test]
    fn ventana_de_cada_peticion() {
        let requests = [
            Request::Activate {
                window: W,
                source: Source::Pager,
            },
            Request::WindowDesktop { window: W, desk: 1 },
            Request::Close {
                window: W,
                time: 0,
                source: Source::Pager,
            },
            Request::Fullscreen {
                window: W,
                action: StateAction::Add,
                source: Source::Application,
            },
        ];
        for request in requests {
            assert_eq!(request.window(), Some(W), "{request:?}");
        }
        assert_eq!(Request::CurrentDesktop { desk: 2 }.window(), None);
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
        for message_type in [10, 11, 12, 13, 14] {
            assert_eq!(
                decode(&TYPES, ROOT, message_type, 8, ROOT, [0; 5]),
                Some(Err(Invalid::Format(8)))
            );
        }
        // Otros mensajes se ignoran.
        assert_eq!(decode_at(99, W, [1, 2, 3, 4, 5]), None);
    }

    // U18: _NET_WM_STATE.

    #[test]
    fn estado_de_pantalla_completa() {
        let fullscreen = |action, source| {
            Some(Ok(Request::Fullscreen {
                window: W,
                action,
                source,
            }))
        };
        assert_eq!(
            decode_at(14, W, [0, FULLSCREEN, 0, 1, 0]),
            fullscreen(StateAction::Remove, Source::Application)
        );
        assert_eq!(
            decode_at(14, W, [1, FULLSCREEN, 0, 2, 0]),
            fullscreen(StateAction::Add, Source::Pager)
        );
        // En data[2], con otro estado (que se ignora) en data[1].
        assert_eq!(
            decode_at(14, W, [2, 77, FULLSCREEN, 0, 0]),
            fullscreen(StateAction::Toggle, Source::Legacy)
        );
        assert_eq!(
            decode_at(14, W, [3, FULLSCREEN, 0, 1, 0]),
            Some(Err(Invalid::Action(3)))
        );
        assert_eq!(
            decode_at(14, W, [1, 77, 78, 1, 0]),
            Some(Err(Invalid::UnsupportedState))
        );
        assert_eq!(
            decode_at(14, W, [1, FULLSCREEN, 0, 9, 0]),
            Some(Err(Invalid::Source(9)))
        );
    }

    #[test]
    fn acciones_idempotentes() {
        for current in [false, true] {
            assert!(!StateAction::Remove.apply(current));
            assert!(StateAction::Add.apply(current));
            assert_eq!(StateAction::Toggle.apply(current), !current);
        }
    }

    #[test]
    fn valor_de_net_wm_state() {
        // Los estados ajenos se conservan, en su orden.
        assert_eq!(
            with_fullscreen(&[7, 8], FULLSCREEN, true),
            vec![7, 8, FULLSCREEN]
        );
        assert_eq!(
            with_fullscreen(&[7, FULLSCREEN, 8], FULLSCREEN, false),
            vec![7, 8]
        );
        // Si ya estaba, se queda en su sitio y sin repeticiones; vacío si no
        // queda nada.
        assert_eq!(
            with_fullscreen(&[FULLSCREEN, 7, FULLSCREEN], FULLSCREEN, true),
            vec![FULLSCREEN, 7]
        );
        assert_eq!(
            with_fullscreen(&[7, FULLSCREEN, 8], FULLSCREEN, true),
            vec![7, FULLSCREEN, 8]
        );
        assert_eq!(with_fullscreen(&[], FULLSCREEN, false), Vec::<Atom>::new());
        assert_eq!(
            with_fullscreen(&[FULLSCREEN], FULLSCREEN, true),
            vec![FULLSCREEN]
        );
    }

    // U18: lectura y escritura completas de _NET_WM_STATE.

    const ATOM_TYPE: Atom = 4;

    fn read(values: &[Atom]) -> StateRead {
        StateRead {
            type_: ATOM_TYPE,
            format: 32,
            bytes_after: 0,
            values: values.to_vec(),
        }
    }

    fn update(read: &StateRead, on: bool) -> Result<Option<Vec<Atom>>, StateError> {
        update_state(read, ATOM_TYPE, FULLSCREEN, on)
    }

    #[test]
    fn propiedad_vacia_o_ausente() {
        let absent = StateRead {
            type_: 0,
            format: 0,
            bytes_after: 0,
            values: vec![],
        };
        assert_eq!(update(&absent, true), Ok(Some(vec![FULLSCREEN])));
        assert_eq!(update(&absent, false), Ok(None), "nada que quitar");
        assert_eq!(update(&read(&[]), true), Ok(Some(vec![FULLSCREEN])));
        assert_eq!(update(&read(&[]), false), Ok(None));
        assert_eq!(absent.requests_fullscreen(ATOM_TYPE, FULLSCREEN), Ok(false));
    }

    #[test]
    fn solo_fullscreen_e_idempotencia() {
        let only = read(&[FULLSCREEN]);
        assert_eq!(update(&only, true), Ok(None), "ya está");
        assert_eq!(update(&only, false), Ok(Some(vec![])));
        assert_eq!(update(&read(&[7, 8]), false), Ok(None), "no estaba");
        assert_eq!(only.requests_fullscreen(ATOM_TYPE, FULLSCREEN), Ok(true));
    }

    #[test]
    fn fullscreen_al_inicio_en_medio_y_al_final() {
        for states in [[FULLSCREEN, 7, 8], [7, FULLSCREEN, 8], [7, 8, FULLSCREEN]] {
            let r = read(&states);
            assert_eq!(update(&r, false), Ok(Some(vec![7, 8])), "{states:?}");
            assert!(r.requests_fullscreen(ATOM_TYPE, FULLSCREEN).unwrap());
        }
        // Añadir cuando ya está, pero no al final: no se reordena nada.
        assert_eq!(update(&read(&[FULLSCREEN, 7]), true), Ok(None));
        // Repetido: queda una sola vez.
        assert_eq!(
            update(&read(&[FULLSCREEN, 7, FULLSCREEN]), false),
            Ok(Some(vec![7]))
        );
    }

    #[test]
    fn mas_de_64_estados_ajenos() {
        let many: Vec<Atom> = (1000..1200).collect();
        let mut with = many.clone();
        with.push(FULLSCREEN);
        assert_eq!(update(&read(&many), true), Ok(Some(with.clone())));
        assert_eq!(update(&read(&with), false), Ok(Some(many.clone())));
        // Justo en el límite de lectura.
        let limit: Vec<Atom> = (1..=MAX_STATE_ATOMS).map(|a| a + 1000).collect();
        assert_eq!(update(&read(&limit), false), Ok(None));
    }

    #[test]
    fn propiedad_mal_formada() {
        for (type_, format) in [(6, 32), (ATOM_TYPE, 8), (31, 8)] {
            let bad = StateRead {
                type_,
                format,
                bytes_after: 0,
                values: vec![],
            };
            let error = StateError::Malformed { type_, format };
            assert_eq!(update(&bad, true), Err(error));
            assert_eq!(update(&bad, false), Err(error));
            assert_eq!(bad.requests_fullscreen(ATOM_TYPE, FULLSCREEN), Err(error));
        }
    }

    #[test]
    fn lectura_incompleta_sin_escritura_parcial() {
        // Con más átomos de los que se leen, nunca se escribe una parte.
        let mut truncated = read(&(1..=MAX_STATE_ATOMS).collect::<Vec<_>>());
        truncated.bytes_after = 4;
        let error = StateError::TooLarge { bytes_after: 4 };
        assert_eq!(update(&truncated, true), Err(error));
        assert_eq!(update(&truncated, false), Err(error));
        truncated.values.push(FULLSCREEN);
        assert_eq!(
            truncated.requests_fullscreen(ATOM_TYPE, FULLSCREEN),
            Err(error),
            "una lectura incompleta no cuenta como petición"
        );
    }

    #[test]
    fn orden_de_apilamiento() {
        let children = [5, 1, 9, 2, 7, 3];
        let managed = [1, 2, 3];
        assert_eq!(stacking(&children, |w| managed.contains(&w)), vec![1, 2, 3]);
        assert_eq!(stacking(&[], |_| true), Vec::<Window>::new());
        assert_eq!(stacking(&children, |_| false), Vec::<Window>::new());
    }

    // U22: _NET_WM_STRUT y _NET_WM_STRUT_PARTIAL (ADR 0012).

    const CARDINAL: Atom = 6;

    fn prop(values: &[u32]) -> StrutRead {
        StrutRead {
            type_: CARDINAL,
            format: 32,
            bytes_after: 0,
            values: values.to_vec(),
        }
    }

    fn absent() -> StrutRead {
        StrutRead::default()
    }

    fn parse(partial: &StrutRead, legacy: &StrutRead) -> Result<(StrutSource, Strut), StrutDefect> {
        struts(partial, legacy, CARDINAL, 1280, 720)
    }

    const TOP_40: [u32; 12] = [0, 0, 40, 0, 0, 0, 0, 0, 0, 1279, 0, 0];

    #[test]
    fn partial_valida() {
        let (source, strut) = parse(&prop(&TOP_40), &absent()).unwrap();
        assert_eq!(source, StrutSource::Partial);
        assert_eq!(
            strut.side(Side::Top),
            Reservation {
                thickness: 40,
                start: 0,
                end: 1279
            }
        );
        assert_eq!(strut.side(Side::Left), Reservation::default());
        assert_eq!(strut.effective(1280, 720), [0, 0, 40, 0]);
        // Los doce valores en su sitio.
        let all = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let (_, s) = parse(&prop(&all), &absent()).unwrap();
        let got: Vec<(u32, u32, u32)> = Side::ALL
            .iter()
            .map(|&side| {
                let r = s.side(side);
                (r.thickness, r.start, r.end)
            })
            .collect();
        assert_eq!(got, [(1, 5, 6), (2, 7, 8), (3, 9, 10), (4, 11, 12)]);
    }

    #[test]
    fn prioridad_y_tres_estados() {
        let legacy = prop(&[0, 0, 0, 30]);
        // PARTIAL válida: STRUT se ignora, sea como sea.
        let (source, strut) = parse(&prop(&TOP_40), &legacy).unwrap();
        assert_eq!(
            (source, strut.effective(1280, 720)),
            (StrutSource::Partial, [0, 0, 40, 0])
        );
        let bad_legacy = prop(&[1, 2]);
        assert_eq!(
            parse(&prop(&TOP_40), &bad_legacy).unwrap().0,
            StrutSource::Partial
        );
        // PARTIAL ausente: STRUT válida, con tramos completos.
        let (source, strut) = parse(&absent(), &legacy).unwrap();
        assert_eq!(source, StrutSource::Legacy);
        assert_eq!(
            strut.side(Side::Bottom),
            Reservation {
                thickness: 30,
                start: 0,
                end: 1279
            }
        );
        let (_, strut) = parse(&absent(), &prop(&[25, 0, 0, 0])).unwrap();
        assert_eq!(strut.side(Side::Left).end, 719);
        // Las dos ausentes: sin reserva.
        let (source, strut) = parse(&absent(), &absent()).unwrap();
        assert_eq!(source, StrutSource::None);
        assert!(strut.is_empty());
        // PARTIAL presente pero mal formada: defecto, sin recurrir a STRUT.
        let short = prop(&[0, 0, 40, 0]);
        assert_eq!(
            parse(&short, &legacy),
            Err(StrutDefect {
                partial: true,
                error: StrutError::Length {
                    values: 4,
                    bytes_after: 0
                }
            })
        );
        // STRUT mal formada sin PARTIAL: defecto de STRUT.
        assert!(!parse(&absent(), &bad_legacy).unwrap_err().partial);
    }

    #[test]
    fn propiedades_mal_formadas() {
        let mut long = prop(&TOP_40);
        long.bytes_after = 4;
        let mut wrong_type = prop(&TOP_40);
        wrong_type.type_ = 4;
        let mut wrong_format = prop(&[]);
        wrong_format.format = 8;
        let mut thirteen = TOP_40.to_vec();
        thirteen.push(0);
        for (read, error) in [
            (
                long,
                StrutError::Length {
                    values: 12,
                    bytes_after: 4,
                },
            ),
            (
                wrong_type,
                StrutError::Type {
                    type_: 4,
                    format: 32,
                },
            ),
            (
                wrong_format,
                StrutError::Type {
                    type_: CARDINAL,
                    format: 8,
                },
            ),
            (
                prop(&thirteen),
                StrutError::Length {
                    values: 13,
                    bytes_after: 0,
                },
            ),
            (
                prop(&[]),
                StrutError::Length {
                    values: 0,
                    bytes_after: 0,
                },
            ),
        ] {
            assert_eq!(
                parse(&read, &absent()),
                Err(StrutDefect {
                    partial: true,
                    error
                }),
                "{read:?}"
            );
        }
        // Tramo invertido en un borde con grosor: defecto; sin grosor, no
        // importa.
        let inverted = [0, 0, 40, 0, 0, 0, 0, 0, 900, 100, 0, 0];
        assert_eq!(
            parse(&prop(&inverted), &absent()).unwrap_err().error,
            StrutError::Range(Side::Top)
        );
        let ignored = [0, 0, 0, 0, 9, 1, 9, 1, 9, 1, 9, 1];
        assert!(parse(&prop(&ignored), &absent()).unwrap().1.is_empty());
    }

    #[test]
    fn reserva_efectiva_de_los_tramos() {
        // Tramo parcial: cuenta mientras toque la raíz.
        let partial = [0, 0, 40, 0, 0, 0, 0, 0, 0, 399, 0, 0];
        let (_, s) = parse(&prop(&partial), &absent()).unwrap();
        assert_eq!(s.effective(1280, 720), [0, 0, 40, 0]);
        // Tramo fuera de la raíz (otro monitor): no reserva.
        let outside = [0, 0, 40, 0, 0, 0, 0, 0, 1280, 2000, 0, 0];
        let (_, s) = parse(&prop(&outside), &absent()).unwrap();
        assert_eq!(s.effective(1280, 720), [0, 0, 0, 0]);
        assert!(s.outside(1280, 720));
        // Tramo que se sale: cuenta.
        let beyond = [0, 0, 0, 30, 0, 0, 0, 0, 0, 0, 1000, u32::MAX];
        let (_, s) = parse(&prop(&beyond), &absent()).unwrap();
        assert_eq!(s.effective(1280, 720), [0, 0, 0, 30]);
        assert!(!s.outside(1280, 720));
        // Grosores excesivos: se limitan a la dimensión perpendicular.
        let huge = [
            u32::MAX,
            5000,
            u32::MAX,
            900,
            0,
            719,
            0,
            719,
            0,
            1279,
            0,
            1279,
        ];
        let (_, s) = parse(&prop(&huge), &absent()).unwrap();
        assert_eq!(s.effective(1280, 720), [1280, 1280, 720, 720]);
        // Los valores declarados se conservan sin recortar.
        assert_eq!(s.side(Side::Left).thickness, u32::MAX);
    }
}
