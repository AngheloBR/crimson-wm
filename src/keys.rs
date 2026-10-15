//! Atajos de teclado (ADR 0005 a 0009 y 0014): la tabla de la configuración
//! (o la predeterminada) capturada en la raíz, también con Bloq Mayús y Bloq
//! Num activos. La predeterminada incluye el atajo provisional de salida
//! `Mod4+Shift+Escape` (ADR 0003), los de lanzamiento de aplicaciones, los
//! de escritorios, los de cierre de ventanas y el de pantalla completa.

use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{
    ConnectionExt as _, GrabMode, KeyButMask, KeyPressEvent, KeyReleaseEvent, Keycode, Keysym,
    ModMask, Timestamp, Window,
};

use crate::error::Fatal;
use crate::keynames::{self, MOD4, SHIFT};
use crate::spawn;

const XK_ESCAPE: Keysym = 0xff1b;
const XK_RETURN: Keysym = 0xff0d;
const XK_PRINT: Keysym = 0xff61;
const XK_B: Keysym = 0x0062;
const XK_D: Keysym = 0x0064;
const XK_E: Keysym = 0x0065;
const XK_F: Keysym = 0x0066;
const XK_J: Keysym = 0x006a;
const XK_K: Keysym = 0x006b;
const XK_N: Keysym = 0x006e;
const XK_W: Keysym = 0x0077;
const XK_NUM_LOCK: Keysym = 0xff7f;
const XK_TAB: Keysym = 0xff09;
/// Dígitos `1` a `8`, en orden.
const XK_DIGITS: [Keysym; 8] = [0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38];

// Máscara de Bloq Mayús del protocolo X11 (`ModMask`); las demás, en
// keynames.rs.
const LOCK: u16 = 1 << 1;
/// Shift, Lock, Control y Mod1 a Mod5.
const ALL_MODIFIERS: u16 = 0x00ff;

/// Modificadores que distinguen un atajo. `Lock` (Bloq Mayús) y el
/// modificador de Bloq Num se ignoran.
const RELEVANT_MODIFIERS: u16 = ALL_MODIFIERS & !LOCK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Termina el WM con el código 0.
    Exit,
    /// Enfoca la siguiente ventana del orden de navegación.
    FocusNext,
    /// Enfoca la anterior.
    FocusPrev,
    /// Convierte la ventana enfocada en la maestra.
    Zoom,
    /// Alterna el layout del escritorio activo, sin cambiar orden ni foco.
    CycleLayout,
    /// Lanza el programa de ese índice de la tabla de programas de la
    /// configuración.
    Launch(usize),
    /// Activa el escritorio de ese índice (0 a 7).
    View(usize),
    /// Lleva la ventana enfocada al escritorio de ese índice.
    MoveTo(usize),
    /// Vuelve al escritorio anterior.
    ViewPrevious,
    /// Pide a la ventana enfocada que se cierre (`WM_DELETE_WINDOW`).
    Close,
    /// Cierra a la fuerza la conexión del cliente de la ventana enfocada
    /// (`KillClient`): todas sus ventanas.
    Kill,
    /// Alterna la pantalla completa de la ventana enfocada.
    Fullscreen,
}

/// Atajo de la configuración, ya validado (config.rs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Modificadores exactos, sin Bloq Mayús ni Bloq Num.
    pub modifiers: u16,
    pub keysym: Keysym,
    /// Etiqueta normalizada (keynames.rs), para los registros.
    pub name: String,
    pub action: Action,
}

impl Binding {
    fn new(modifiers: u16, keysym: Keysym, action: Action) -> Self {
        Self {
            modifiers,
            keysym,
            name: keynames::label(modifiers, keysym),
            action,
        }
    }

    /// Los atajos de control del WM son obligatorios: si alguno no se puede
    /// capturar, el WM no arranca (código 14). Los de lanzamiento son
    /// opcionales: se avisa y el WM sigue sin ellos.
    fn required(&self) -> bool {
        !matches!(self.action, Action::Launch(_))
    }
}

/// Tabla predeterminada, la de F4 a F11: la que se usa sin archivo de
/// configuración o si este no define `[[keybindings]]`. Los índices de
/// lanzamiento son los de `spawn::default_programs`.
///
/// `Mod4+n` activa el escritorio n y `Mod4+Shift+n` lleva allí la ventana
/// enfocada. Con Shift se captura el mismo keysym del dígito: los atajos se
/// reconocen por código de tecla y modificadores, no por el carácter que
/// produce la tecla con Shift (`exclam`, `at`…).
pub fn default_bindings() -> Vec<Binding> {
    let mut bindings = vec![
        Binding::new(MOD4 | SHIFT, XK_ESCAPE, Action::Exit),
        Binding::new(MOD4, XK_J, Action::FocusNext),
        Binding::new(MOD4, XK_K, Action::FocusPrev),
        Binding::new(MOD4 | SHIFT, XK_RETURN, Action::Zoom),
        Binding::new(MOD4, XK_RETURN, Action::Launch(spawn::TERMINAL)),
        Binding::new(MOD4, XK_D, Action::Launch(spawn::LAUNCHER)),
        Binding::new(MOD4, XK_E, Action::Launch(spawn::FILES)),
        Binding::new(MOD4, XK_B, Action::Launch(spawn::BROWSER)),
        Binding::new(MOD4, XK_N, Action::Launch(spawn::EDITOR)),
        Binding::new(0, XK_PRINT, Action::Launch(spawn::SCREENSHOT)),
    ];
    for (index, &digit) in XK_DIGITS.iter().enumerate() {
        bindings.push(Binding::new(MOD4, digit, Action::View(index)));
    }
    for (index, &digit) in XK_DIGITS.iter().enumerate() {
        bindings.push(Binding::new(MOD4 | SHIFT, digit, Action::MoveTo(index)));
    }
    bindings.extend([
        Binding::new(MOD4, XK_TAB, Action::ViewPrevious),
        Binding::new(MOD4, XK_W, Action::Close),
        Binding::new(MOD4 | SHIFT, XK_W, Action::Kill),
        Binding::new(MOD4, XK_F, Action::Fullscreen),
    ]);
    bindings
}

/// Combinaciones reservadas para fases posteriores (ADR 0005). No se
/// capturan: hasta entonces llegan a la ventana enfocada. Solo las usan las
/// pruebas, para comprobar que la tabla no las ocupa. `Mod4+w` y
/// `Mod4+Shift+w` se implementaron en F6 (ADR 0008).
#[cfg(test)]
const RESERVED: &[(u16, Keysym)] = &[
    (MOD4, 0x0068), // h: foco a la izquierda.
    (MOD4, 0x006c), // l: foco a la derecha.
];

/// Atajos capturados: código de tecla, modificadores exactos y acción.
pub struct Bindings {
    keys: Vec<(Keycode, u16, Action)>,
    num_lock: u16,
}

impl Bindings {
    /// Acción del atajo pulsado, si lo es.
    pub fn action(&self, event: &KeyPressEvent) -> Option<Action> {
        self.action_for(event.detail, event.state)
    }

    fn action_for(&self, keycode: Keycode, state: KeyButMask) -> Option<Action> {
        let modifiers = relevant_modifiers(state, self.num_lock);
        self.keys
            .iter()
            .find(|&&(k, m, _)| k == keycode && m == modifiers)
            .map(|&(_, _, action)| action)
    }
}

/// Captura en la raíz todos los atajos de la tabla, también con Bloq Mayús
/// y Bloq Num activos.
pub fn grab_bindings(
    conn: &impl Connection,
    root: Window,
    table: &[Binding],
) -> Result<Bindings, Fatal> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let count = setup
        .max_keycode
        .checked_sub(min)
        .and_then(|span| span.checked_add(1))
        .ok_or_else(|| Fatal::Startup("rango de códigos de tecla inválido".into()))?;
    let mapping = conn
        .get_keyboard_mapping(min, count)
        .map_err(Fatal::lost)?
        .reply()
        .map_err(|e| Fatal::startup("GetKeyboardMapping", e))?;
    let modifiers = conn
        .get_modifier_mapping()
        .map_err(Fatal::lost)?
        .reply()
        .map_err(|e| Fatal::startup("GetModifierMapping", e))?;

    let per_keycode = mapping.keysyms_per_keycode;
    let num_lock_keycodes = keycodes_for(XK_NUM_LOCK, min, per_keycode, &mapping.keysyms);
    let num_lock = modifier_mask_for(&num_lock_keycodes, &modifiers.keycodes);

    let mut keys: Vec<(Keycode, u16, Action)> = Vec::new();
    for binding in table {
        // action_for elimina Bloq Num del estado. Admitir aquí uno de sus
        // modificadores dejaría el grab activo pero la acción inaccesible.
        if binding.modifiers & num_lock != 0 {
            let message = format!(
                "{} usa un modificador de Bloq Num que se ignora al reconocer atajos",
                binding.name
            );
            if binding.required() {
                return Err(Fatal::Startup(format!(
                    "{message}; no se puede capturar el atajo"
                )));
            }
            eprintln!("crimson-wm: aviso: {message}; atajo desactivado");
            continue;
        }
        let keycodes = keycodes_for(binding.keysym, min, per_keycode, &mapping.keysyms);
        if keycodes.is_empty() {
            if binding.required() {
                return Err(Fatal::Startup(format!(
                    "ningún código de tecla produce la tecla de {}; no se puede capturar el atajo",
                    binding.name
                )));
            }
            eprintln!(
                "crimson-wm: aviso: ningún código de tecla produce la tecla de {}; atajo desactivado",
                binding.name
            );
            continue;
        }
        for &keycode in &keycodes {
            // Dos keysyms distintos de la tabla pueden estar en el mismo
            // código de tecla (en otro nivel de la distribución). El
            // servidor no lo impide, porque las dos capturas son del mismo
            // cliente, así que se comprueba aquí: gana el primer atajo.
            if let Some(&(_, _, first)) = keys
                .iter()
                .find(|&&(k, m, _)| k == keycode && m == binding.modifiers)
            {
                let message = format!(
                    "el código de tecla {keycode} de {} ya está asignado ({first:?})",
                    binding.name
                );
                if binding.required() {
                    return Err(Fatal::Startup(format!(
                        "{message}; no se puede capturar el atajo"
                    )));
                }
                eprintln!("crimson-wm: aviso: {message}; atajo desactivado en esa tecla");
                continue;
            }
            if grab_key(conn, root, binding, keycode, num_lock)? {
                keys.push((keycode, binding.modifiers, binding.action));
            }
        }
    }

    Ok(Bindings { keys, num_lock })
}

/// Captura `keycode` con los modificadores del atajo y cada variante de
/// bloqueo. Si el servidor rechaza alguna (p. ej., otro cliente ya la
/// tiene), un atajo obligatorio impide arrancar; en uno opcional se avisa,
/// se deshacen las variantes ya capturadas, para que la tecla no quede
/// capturada sin acción, y se devuelve `false`.
fn grab_key(
    conn: &impl Connection,
    root: Window,
    binding: &Binding,
    keycode: Keycode,
    num_lock: u16,
) -> Result<bool, Fatal> {
    let mut grabbed = Vec::new();
    for extra in lock_variants(num_lock) {
        let modifiers = ModMask::from(binding.modifiers | extra);
        let result = conn
            .grab_key(
                false,
                root,
                modifiers,
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(Fatal::lost)?
            .check();
        match result {
            Ok(()) => grabbed.push(modifiers),
            Err(ReplyError::X11Error(e)) if !binding.required() => {
                eprintln!(
                    "crimson-wm: aviso: GrabKey({}) rechazado en el código de tecla {keycode}: {:?}; atajo desactivado en esa tecla",
                    binding.name, e.error_kind
                );
                for modifiers in grabbed {
                    conn.ungrab_key(keycode, root, modifiers)
                        .map_err(Fatal::lost)?;
                }
                return Ok(false);
            }
            Err(e) => return Err(Fatal::startup(&format!("GrabKey({})", binding.name), e)),
        }
    }
    Ok(true)
}

/// Intervalo mínimo entre dos lanzamientos del mismo atajo, en ms.
const REPEAT_INTERVAL: Timestamp = 300;

/// Filtro de repetición de los atajos. Para los de lanzamiento ignora:
/// - la repetición automática del servidor: sin la extensión XKB, cada
///   repetición llega como un KeyRelease y un KeyPress de la misma tecla con
///   la misma marca de tiempo;
/// - las pulsaciones de un atajo a menos de `REPEAT_INTERVAL` de la
///   anterior, aceptada o no, según las marcas de tiempo del servidor (que
///   dan la vuelta cada 2^32 ms).
///
/// Los atajos de escritorio que no son idempotentes (mover la ventana y
/// volver al anterior), los de cierre y el de pantalla completa solo
/// ignoran la repetición automática: mantener `Mod4+Shift+n`, `Mod4+w` o
/// `Mod4+f` no debe llevarse, cerrar o alternar una ventana tras otra.
#[derive(Default)]
pub struct Repeats {
    /// Última liberación de una tecla capturada: código y marca de tiempo.
    release: Option<(Keycode, Timestamp)>,
    /// Última pulsación de cada atajo de lanzamiento, por índice de
    /// programa.
    presses: Vec<(usize, Timestamp)>,
}

impl Repeats {
    pub fn released(&mut self, event: &KeyReleaseEvent) {
        self.release = Some((event.detail, event.time));
    }

    /// Indica si la pulsación del atajo del programa `program` debe
    /// lanzarlo.
    pub fn accept(&mut self, program: usize, event: &KeyPressEvent) -> bool {
        self.accept_at(program, event.detail, event.time)
    }

    /// La pulsación es una repetición automática del servidor.
    pub fn autorepeat(&self, event: &KeyPressEvent) -> bool {
        self.autorepeat_at(event.detail, event.time)
    }

    fn autorepeat_at(&self, keycode: Keycode, time: Timestamp) -> bool {
        self.release == Some((keycode, time))
    }

    fn accept_at(&mut self, program: usize, keycode: Keycode, time: Timestamp) -> bool {
        let autorepeat = self.autorepeat_at(keycode, time);
        let previous = match self.presses.iter_mut().find(|(p, _)| *p == program) {
            Some((_, last)) => Some(std::mem::replace(last, time)),
            None => {
                self.presses.push((program, time));
                None
            }
        };
        let recent = previous.is_some_and(|last| time.wrapping_sub(last) < REPEAT_INTERVAL);
        !autorepeat && !recent
    }
}

/// Códigos de tecla cuya lista de keysyms contiene `keysym`.
fn keycodes_for(
    keysym: Keysym,
    min_keycode: Keycode,
    per_keycode: u8,
    keysyms: &[Keysym],
) -> Vec<Keycode> {
    if per_keycode == 0 {
        return Vec::new();
    }
    keysyms
        .chunks(usize::from(per_keycode))
        .zip(min_keycode..=Keycode::MAX)
        .filter(|(syms, _)| syms.contains(&keysym))
        .map(|(_, keycode)| keycode)
        .collect()
}

/// Máscara de los modificadores (de los ocho de X11) a los que está
/// asignado alguno de `keycodes`, según la respuesta de GetModifierMapping.
fn modifier_mask_for(keycodes: &[Keycode], modifier_map: &[Keycode]) -> u16 {
    let per_modifier = modifier_map.len() / 8;
    if per_modifier == 0 {
        return 0;
    }
    modifier_map
        .chunks(per_modifier)
        .enumerate()
        .filter(|(_, mapped)| mapped.iter().any(|k| *k != 0 && keycodes.contains(k)))
        .fold(0, |mask, (index, _)| mask | (1 << index))
}

/// Combinaciones de Bloq Mayús y Bloq Num con las que se repite la captura.
fn lock_variants(num_lock: u16) -> Vec<u16> {
    let mut variants = vec![0, LOCK];
    if num_lock != 0 && num_lock != LOCK {
        variants.extend([num_lock, num_lock | LOCK]);
    }
    variants
}

/// Modificadores de una pulsación sin Bloq Mayús, Bloq Num ni botones.
fn relevant_modifiers(state: KeyButMask, num_lock: u16) -> u16 {
    u16::from(state) & RELEVANT_MODIFIERS & !num_lock
}

#[cfg(test)]
mod tests {
    use super::*;

    const NUM_LOCK_MOD2: u16 = 1 << 4;

    #[test]
    fn busca_los_codigos_de_tecla_de_un_keysym() {
        // Tres keysyms por código de tecla, empezando en el código 8.
        let keysyms = [0, 0, 0, XK_ESCAPE, 0, 0, 0x61, 0x41, 0, 0, 0, XK_ESCAPE];
        assert_eq!(keycodes_for(XK_ESCAPE, 8, 3, &keysyms), vec![9, 11]);
        assert_eq!(
            keycodes_for(XK_NUM_LOCK, 8, 3, &keysyms),
            Vec::<Keycode>::new()
        );
        assert_eq!(
            keycodes_for(XK_ESCAPE, 8, 0, &keysyms),
            Vec::<Keycode>::new()
        );
    }

    #[test]
    fn calcula_la_mascara_de_bloq_num() {
        // Dos códigos por modificador: Shift, Lock, Control, Mod1..Mod5.
        let map = [
            50, 62, 66, 0, 37, 105, 64, 108, 77, 0, 0, 0, 133, 134, 92, 0,
        ];
        assert_eq!(modifier_mask_for(&[77], &map), NUM_LOCK_MOD2);
        assert_eq!(modifier_mask_for(&[200], &map), 0);
        assert_eq!(modifier_mask_for(&[0], &map), 0);
        assert_eq!(modifier_mask_for(&[77], &[]), 0);
    }

    #[test]
    fn variantes_de_bloqueo() {
        assert_eq!(lock_variants(0), vec![0, LOCK]);
        assert_eq!(
            lock_variants(NUM_LOCK_MOD2),
            vec![0, LOCK, NUM_LOCK_MOD2, NUM_LOCK_MOD2 | LOCK]
        );
    }

    /// Tabla como la que produce `grab_bindings` con el mapa por defecto de
    /// Xvfb: Escape = 9, Return = 36, j = 44, k = 45, d = 40, e = 26,
    /// b = 56, n = 57, Print = 107 y 218, 1 a 8 = 10 a 17, Tab = 23,
    /// w = 25 y f = 41.
    fn bindings(num_lock: u16) -> Bindings {
        let mut keys = vec![
            (9, MOD4 | SHIFT, Action::Exit),
            (44, MOD4, Action::FocusNext),
            (45, MOD4, Action::FocusPrev),
            (36, MOD4 | SHIFT, Action::Zoom),
            (36, MOD4, Action::Launch(spawn::TERMINAL)),
            (40, MOD4, Action::Launch(spawn::LAUNCHER)),
            (26, MOD4, Action::Launch(spawn::FILES)),
            (56, MOD4, Action::Launch(spawn::BROWSER)),
            (57, MOD4, Action::Launch(spawn::EDITOR)),
            (107, 0, Action::Launch(spawn::SCREENSHOT)),
            (218, 0, Action::Launch(spawn::SCREENSHOT)),
            (23, MOD4, Action::ViewPrevious),
            (25, MOD4, Action::Close),
            (25, MOD4 | SHIFT, Action::Kill),
            (41, MOD4, Action::Fullscreen),
        ];
        for (index, keycode) in (10..=17).enumerate() {
            keys.push((keycode, MOD4, Action::View(index)));
            keys.push((keycode, MOD4 | SHIFT, Action::MoveTo(index)));
        }
        Bindings { keys, num_lock }
    }

    #[test]
    fn reconoce_cada_atajo() {
        let b = bindings(NUM_LOCK_MOD2);
        let m4 = KeyButMask::MOD4;
        assert_eq!(b.action_for(9, m4 | KeyButMask::SHIFT), Some(Action::Exit));
        assert_eq!(b.action_for(44, m4), Some(Action::FocusNext));
        assert_eq!(b.action_for(45, m4), Some(Action::FocusPrev));
        assert_eq!(b.action_for(36, m4 | KeyButMask::SHIFT), Some(Action::Zoom));
        assert_eq!(b.action_for(36, m4), Some(Action::Launch(spawn::TERMINAL)));
        assert_eq!(b.action_for(40, m4), Some(Action::Launch(spawn::LAUNCHER)));
        assert_eq!(b.action_for(26, m4), Some(Action::Launch(spawn::FILES)));
        assert_eq!(b.action_for(56, m4), Some(Action::Launch(spawn::BROWSER)));
        assert_eq!(b.action_for(57, m4), Some(Action::Launch(spawn::EDITOR)));
        for print in [107, 218] {
            assert_eq!(
                b.action_for(print, KeyButMask::from(0u16)),
                Some(Action::Launch(spawn::SCREENSHOT))
            );
        }
    }

    #[test]
    fn ignora_bloqueos_y_botones() {
        let b = bindings(NUM_LOCK_MOD2);
        let m4 = KeyButMask::MOD4;
        for extra in [
            KeyButMask::LOCK,
            KeyButMask::MOD2,
            KeyButMask::LOCK | KeyButMask::MOD2,
            KeyButMask::BUTTON1,
        ] {
            assert_eq!(b.action_for(44, m4 | extra), Some(Action::FocusNext));
            assert_eq!(
                b.action_for(36, m4 | KeyButMask::SHIFT | extra),
                Some(Action::Zoom)
            );
            assert_eq!(
                b.action_for(9, m4 | KeyButMask::SHIFT | extra),
                Some(Action::Exit)
            );
        }
        for extra in [KeyButMask::LOCK, KeyButMask::MOD2] {
            assert_eq!(
                b.action_for(107, extra),
                Some(Action::Launch(spawn::SCREENSHOT))
            );
        }
        // Sin Bloq Num asignado a Mod2, Mod2 cuenta como modificador.
        assert_eq!(bindings(0).action_for(44, m4 | KeyButMask::MOD2), None);
    }

    #[test]
    fn exige_los_modificadores_exactos() {
        let b = bindings(NUM_LOCK_MOD2);
        let m4 = KeyButMask::MOD4;
        assert_eq!(b.action_for(44, KeyButMask::from(0u16)), None);
        assert_eq!(b.action_for(44, m4 | KeyButMask::SHIFT), None);
        assert_eq!(b.action_for(44, m4 | KeyButMask::CONTROL), None);
        assert_eq!(
            b.action_for(36, m4 | KeyButMask::SHIFT | KeyButMask::MOD1),
            None
        );
        assert_eq!(b.action_for(36, KeyButMask::SHIFT), None);
        assert_eq!(b.action_for(9, m4), None);
        assert_eq!(b.action_for(9, KeyButMask::SHIFT), None);
        assert_eq!(
            b.action_for(9, m4 | KeyButMask::SHIFT | KeyButMask::CONTROL),
            None
        );
        assert_eq!(b.action_for(18, m4), None);
        // Los atajos de lanzamiento tampoco admiten modificadores de más.
        assert_eq!(b.action_for(40, KeyButMask::from(0u16)), None);
        assert_eq!(b.action_for(40, m4 | KeyButMask::SHIFT), None);
        assert_eq!(b.action_for(57, m4 | KeyButMask::CONTROL), None);
        assert_eq!(b.action_for(107, m4), None);
        assert_eq!(b.action_for(107, KeyButMask::SHIFT), None);
    }

    #[test]
    fn la_tabla_no_tiene_atajos_repetidos() {
        let table = default_bindings();
        for (i, a) in table.iter().enumerate() {
            assert_eq!(a.modifiers & !RELEVANT_MODIFIERS, 0, "{}", a.name);
            for b in &table[i + 1..] {
                assert!(
                    (a.modifiers, a.keysym) != (b.modifiers, b.keysym),
                    "{} y {}",
                    a.name,
                    b.name
                );
            }
        }
        // Ningún atajo ocupa una combinación reservada.
        for b in &table {
            assert!(
                !RESERVED.contains(&(b.modifiers, b.keysym)),
                "{} está reservado",
                b.name
            );
        }
        let exit: Vec<_> = table.iter().filter(|b| b.action == Action::Exit).collect();
        assert_eq!(exit.len(), 1);
        assert_eq!(
            (exit[0].modifiers, exit[0].keysym),
            (MOD4 | SHIFT, XK_ESCAPE)
        );
    }

    #[test]
    fn atajos_de_lanzamiento() {
        let table = default_bindings();
        let programs = spawn::default_programs();
        let expected: [(u16, Keysym, &str, &[&str]); 6] = [
            (MOD4, XK_RETURN, "Mod4+Return", &["kitty"]),
            (MOD4, XK_D, "Mod4+d", &["rofi", "-show", "drun"]),
            (MOD4, XK_E, "Mod4+e", &["thunar"]),
            (MOD4, XK_B, "Mod4+b", &["firefox"]),
            (MOD4, XK_N, "Mod4+n", &["kitty", "-e", "nvim"]),
            (0, XK_PRINT, "Print", &["flameshot", "gui"]),
        ];
        let launchers: Vec<_> = table
            .iter()
            .filter_map(|b| match b.action {
                Action::Launch(program) => Some((b, program)),
                _ => None,
            })
            .collect();
        assert_eq!(launchers.len(), expected.len());
        for (binding, program) in &launchers {
            let (modifiers, keysym, _, argv) = expected
                .iter()
                .find(|e| e.2 == binding.name)
                .unwrap_or_else(|| panic!("atajo inesperado {}", binding.name));
            assert_eq!((binding.modifiers, binding.keysym), (*modifiers, *keysym));
            assert_eq!(programs[*program].argv, *argv, "{}", binding.name);
            assert!(!binding.required(), "{} es opcional", binding.name);
        }
        // Cada programa tiene exactamente un atajo.
        for program in 0..programs.len() {
            assert_eq!(launchers.iter().filter(|(_, p)| *p == program).count(), 1);
        }
        // Los de control siguen siendo obligatorios.
        for binding in table
            .iter()
            .filter(|b| !matches!(b.action, Action::Launch(_)))
        {
            assert!(binding.required(), "{}", binding.name);
        }
    }

    // U13: escritorios.

    #[test]
    fn reconoce_los_atajos_de_escritorio() {
        let b = bindings(NUM_LOCK_MOD2);
        let m4 = KeyButMask::MOD4;
        for (index, keycode) in (10..=17).enumerate() {
            assert_eq!(b.action_for(keycode, m4), Some(Action::View(index)));
            // Con Shift, la misma tecla (que produce `exclam`, `at`…).
            assert_eq!(
                b.action_for(keycode, m4 | KeyButMask::SHIFT),
                Some(Action::MoveTo(index))
            );
            for extra in [KeyButMask::LOCK, KeyButMask::MOD2] {
                assert_eq!(b.action_for(keycode, m4 | extra), Some(Action::View(index)));
                assert_eq!(
                    b.action_for(keycode, m4 | KeyButMask::SHIFT | extra),
                    Some(Action::MoveTo(index))
                );
            }
            // Sin Mod4, o con otro modificador, la tecla llega a la ventana.
            assert_eq!(b.action_for(keycode, KeyButMask::from(0u16)), None);
            assert_eq!(b.action_for(keycode, KeyButMask::SHIFT), None);
            assert_eq!(b.action_for(keycode, m4 | KeyButMask::CONTROL), None);
        }
        assert_eq!(b.action_for(23, m4), Some(Action::ViewPrevious));
        assert_eq!(
            b.action_for(23, m4 | KeyButMask::LOCK),
            Some(Action::ViewPrevious)
        );
        // Ni Alt+Tab ni Mod4+Shift+Tab.
        assert_eq!(b.action_for(23, KeyButMask::MOD1), None);
        assert_eq!(b.action_for(23, m4 | KeyButMask::SHIFT), None);
        assert_eq!(b.action_for(18, m4), None, "9 no es un escritorio");
    }

    #[test]
    fn atajos_de_escritorio() {
        let table = default_bindings();
        let desks: Vec<_> = table
            .iter()
            .filter(|b| {
                matches!(
                    b.action,
                    Action::View(_) | Action::MoveTo(_) | Action::ViewPrevious
                )
            })
            .collect();
        assert_eq!(desks.len(), 17);
        for (index, keysym) in XK_DIGITS.iter().enumerate() {
            let digit = index + 1;
            let find = |action| desks.iter().find(|b| b.action == action).unwrap();
            let view = find(Action::View(index));
            assert_eq!((view.modifiers, view.keysym), (MOD4, *keysym));
            assert_eq!(view.name, format!("Mod4+{digit}"));
            let move_to = find(Action::MoveTo(index));
            assert_eq!((move_to.modifiers, move_to.keysym), (MOD4 | SHIFT, *keysym));
            assert_eq!(move_to.name, format!("Mod4+Shift+{digit}"));
            assert_eq!(char::from_u32(*keysym), char::from_digit(digit as u32, 10));
        }
        let previous: Vec<_> = desks
            .iter()
            .filter(|b| b.action == Action::ViewPrevious)
            .collect();
        assert_eq!(previous.len(), 1);
        assert_eq!((previous[0].modifiers, previous[0].keysym), (MOD4, XK_TAB));
        // Todos son obligatorios (código 14 si no se pueden capturar).
        assert!(desks.iter().all(|b| b.required()));
    }

    // U15: cierre.

    #[test]
    fn atajos_de_cierre() {
        let table = default_bindings();
        let b = bindings(NUM_LOCK_MOD2);
        let m4 = KeyButMask::MOD4;
        assert_eq!(b.action_for(25, m4), Some(Action::Close));
        assert_eq!(b.action_for(25, m4 | KeyButMask::SHIFT), Some(Action::Kill));
        for extra in [KeyButMask::LOCK, KeyButMask::MOD2] {
            assert_eq!(b.action_for(25, m4 | extra), Some(Action::Close));
            assert_eq!(
                b.action_for(25, m4 | KeyButMask::SHIFT | extra),
                Some(Action::Kill)
            );
        }
        assert_eq!(b.action_for(25, KeyButMask::from(0u16)), None);
        assert_eq!(b.action_for(25, m4 | KeyButMask::CONTROL), None);
        let find = |action| table.iter().find(|b| b.action == action).unwrap();
        let close = find(Action::Close);
        assert_eq!(
            (close.modifiers, close.keysym, close.name.as_str()),
            (MOD4, XK_W, "Mod4+w")
        );
        let kill = find(Action::Kill);
        assert_eq!(
            (kill.modifiers, kill.keysym, kill.name.as_str()),
            (MOD4 | SHIFT, XK_W, "Mod4+Shift+w")
        );
        // Atajos de control: obligatorios.
        assert!(close.required() && kill.required());
    }

    // U18: pantalla completa.

    #[test]
    fn atajo_de_pantalla_completa() {
        let table = default_bindings();
        let b = bindings(NUM_LOCK_MOD2);
        let m4 = KeyButMask::MOD4;
        assert_eq!(b.action_for(41, m4), Some(Action::Fullscreen));
        for extra in [KeyButMask::LOCK, KeyButMask::MOD2] {
            assert_eq!(b.action_for(41, m4 | extra), Some(Action::Fullscreen));
        }
        assert_eq!(b.action_for(41, KeyButMask::from(0u16)), None);
        assert_eq!(b.action_for(41, m4 | KeyButMask::SHIFT), None);
        let binding = table
            .iter()
            .find(|b| b.action == Action::Fullscreen)
            .unwrap();
        assert_eq!(
            (binding.modifiers, binding.keysym, binding.name.as_str()),
            (MOD4, XK_F, "Mod4+f")
        );
        assert!(binding.required(), "atajo de control: obligatorio");
        assert_eq!(
            table.iter().filter(|b| b.keysym == XK_F).count(),
            1,
            "sin conflictos con otra combinación de f"
        );
    }

    #[test]
    fn repeticion_automatica_de_los_atajos_de_escritorio() {
        let mut r = Repeats::default();
        assert!(!r.autorepeat_at(12, 1_000));
        r.release = Some((12, 1_660));
        assert!(r.autorepeat_at(12, 1_660));
        assert!(!r.autorepeat_at(12, 1_700), "otra pulsación");
        assert!(!r.autorepeat_at(23, 1_660), "otra tecla");
    }

    #[test]
    fn repeticion_por_intervalo() {
        let mut r = Repeats::default();
        assert!(r.accept_at(0, 36, 1_000));
        assert!(!r.accept_at(0, 36, 1_100));
        // Se mide desde la última pulsación, aunque se ignorara.
        assert!(!r.accept_at(0, 36, 1_350));
        assert!(r.accept_at(0, 36, 1_650));
        assert!(!r.accept_at(0, 36, 1_949));
        assert!(r.accept_at(0, 36, 2_249));
        // El intervalo es por atajo.
        assert!(r.accept_at(1, 40, 2_250));
        assert!(r.accept_at(2, 26, 2_260));
    }

    #[test]
    fn repeticion_automatica_del_servidor() {
        let mut r = Repeats::default();
        assert!(r.accept_at(0, 36, 1_000));
        // Tecla mantenida: tras el retardo, liberación y pulsación con la
        // misma marca de tiempo.
        for time in [1_660, 1_700, 1_740] {
            r.release = Some((36, time));
            assert!(!r.accept_at(0, 36, time), "{time}");
        }
        // Liberación real y nueva pulsación más tarde.
        r.release = Some((36, 1_760));
        assert!(r.accept_at(0, 36, 2_100));
        // Una liberación de otra tecla no cuenta.
        r.release = Some((40, 3_000));
        assert!(r.accept_at(0, 36, 3_000));
    }

    #[test]
    fn repeticion_con_desbordamiento_del_reloj() {
        let mut r = Repeats::default();
        assert!(r.accept_at(0, 36, u32::MAX - 100));
        assert!(!r.accept_at(0, 36, 50));
        assert!(r.accept_at(0, 36, 400));
    }
}
