//! Atajos de teclado (ADR 0005): una tabla fija capturada en la raíz,
//! también con Bloq Mayús y Bloq Num activos. Incluye el atajo provisional
//! de salida `Mod4+Shift+Escape` (ADR 0003).

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt as _, GrabMode, KeyButMask, KeyPressEvent, Keycode, Keysym, ModMask, Window,
};

use crate::error::Fatal;

const XK_ESCAPE: Keysym = 0xff1b;
const XK_RETURN: Keysym = 0xff0d;
const XK_J: Keysym = 0x006a;
const XK_K: Keysym = 0x006b;
const XK_NUM_LOCK: Keysym = 0xff7f;

// Máscaras de modificadores del protocolo X11 (`ModMask`).
const SHIFT: u16 = 1 << 0;
const LOCK: u16 = 1 << 1;
const MOD4: u16 = 1 << 6;
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
}

struct Binding {
    /// Modificadores exactos, sin Bloq Mayús ni Bloq Num.
    modifiers: u16,
    keysym: Keysym,
    name: &'static str,
    action: Action,
}

/// Todos los atajos son obligatorios: si alguno no se puede capturar, el WM
/// no arranca (código 14).
const BINDINGS: &[Binding] = &[
    Binding {
        modifiers: MOD4 | SHIFT,
        keysym: XK_ESCAPE,
        name: "Mod4+Shift+Escape",
        action: Action::Exit,
    },
    Binding {
        modifiers: MOD4,
        keysym: XK_J,
        name: "Mod4+j",
        action: Action::FocusNext,
    },
    Binding {
        modifiers: MOD4,
        keysym: XK_K,
        name: "Mod4+k",
        action: Action::FocusPrev,
    },
    Binding {
        modifiers: MOD4 | SHIFT,
        keysym: XK_RETURN,
        name: "Mod4+Shift+Return",
        action: Action::Zoom,
    },
];

/// Combinaciones reservadas para fases posteriores (ADR 0005). No se
/// capturan: hasta entonces llegan a la ventana enfocada. Solo las usan las
/// pruebas, para comprobar que la tabla no las ocupa.
#[cfg(test)]
const RESERVED: &[(u16, Keysym)] = &[
    (MOD4, XK_RETURN),      // Terminal (F4).
    (MOD4, 0x0064),         // d: lanzador (F4).
    (MOD4, 0x0065),         // e: archivos (F4).
    (MOD4, 0x0062),         // b: navegador (F4).
    (MOD4, 0x006e),         // n: editor (F4).
    (0, 0xff61),            // Print: capturas (F4).
    (MOD4, 0x0068),         // h: foco a la izquierda.
    (MOD4, 0x006c),         // l: foco a la derecha.
    (MOD4, 0x0077),         // w: cerrar la ventana.
    (MOD4 | SHIFT, 0x0077), // Shift+w: forzar el cierre.
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
pub fn grab_bindings(conn: &impl Connection, root: Window) -> Result<Bindings, Fatal> {
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

    let mut keys = Vec::new();
    for binding in BINDINGS {
        let keycodes = keycodes_for(binding.keysym, min, per_keycode, &mapping.keysyms);
        if keycodes.is_empty() {
            return Err(Fatal::Startup(format!(
                "ningún código de tecla produce la tecla de {}; no se puede capturar el atajo",
                binding.name
            )));
        }
        for &keycode in &keycodes {
            for extra in lock_variants(num_lock) {
                conn.grab_key(
                    false,
                    root,
                    ModMask::from(binding.modifiers | extra),
                    keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .map_err(Fatal::lost)?
                .check()
                .map_err(|e| Fatal::startup(&format!("GrabKey({})", binding.name), e))?;
            }
            keys.push((keycode, binding.modifiers, binding.action));
        }
    }

    Ok(Bindings { keys, num_lock })
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
    /// Xvfb: Escape = 9, Return = 36, j = 44, k = 45.
    fn bindings(num_lock: u16) -> Bindings {
        Bindings {
            keys: vec![
                (9, MOD4 | SHIFT, Action::Exit),
                (44, MOD4, Action::FocusNext),
                (45, MOD4, Action::FocusPrev),
                (36, MOD4 | SHIFT, Action::Zoom),
            ],
            num_lock,
        }
    }

    #[test]
    fn reconoce_cada_atajo() {
        let b = bindings(NUM_LOCK_MOD2);
        let m4 = KeyButMask::MOD4;
        assert_eq!(b.action_for(9, m4 | KeyButMask::SHIFT), Some(Action::Exit));
        assert_eq!(b.action_for(44, m4), Some(Action::FocusNext));
        assert_eq!(b.action_for(45, m4), Some(Action::FocusPrev));
        assert_eq!(b.action_for(36, m4 | KeyButMask::SHIFT), Some(Action::Zoom));
        // Mod4+Return queda libre (reservado para la terminal en F4).
        assert_eq!(b.action_for(36, m4), None);
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
        assert_eq!(b.action_for(10, m4), None);
    }

    #[test]
    fn la_tabla_no_tiene_atajos_repetidos() {
        for (i, a) in BINDINGS.iter().enumerate() {
            assert_eq!(a.modifiers & !RELEVANT_MODIFIERS, 0, "{}", a.name);
            for b in &BINDINGS[i + 1..] {
                assert!(
                    (a.modifiers, a.keysym) != (b.modifiers, b.keysym),
                    "{} y {}",
                    a.name,
                    b.name
                );
            }
        }
        // Ningún atajo ocupa una combinación reservada.
        for b in BINDINGS {
            assert!(
                !RESERVED.contains(&(b.modifiers, b.keysym)),
                "{} está reservado",
                b.name
            );
        }
        let exit: Vec<_> = BINDINGS
            .iter()
            .filter(|b| b.action == Action::Exit)
            .collect();
        assert_eq!(exit.len(), 1);
        assert_eq!(
            (exit[0].modifiers, exit[0].keysym),
            (MOD4 | SHIFT, XK_ESCAPE)
        );
    }
}
