//! Bucle de eventos de F1: mapea y configura las ventanas tal como lo piden,
//! y termina con el atajo provisional `Mod4+Shift+Escape`.

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    ConfigureWindowAux, ConnectionExt as _, GrabMode, KeyButMask, Keycode, Keysym, ModMask, Window,
};

use crate::error::Fatal;

const XK_ESCAPE: Keysym = 0xff1b;
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
const EXIT_MODIFIERS: u16 = MOD4 | SHIFT;

struct ExitKey {
    keycodes: Vec<Keycode>,
    num_lock: u16,
}

pub fn run(conn: &impl Connection, root: Window) -> Result<(), Fatal> {
    let exit_key = grab_exit_key(conn, root)?;

    loop {
        conn.flush().map_err(Fatal::lost)?;
        match conn.wait_for_event().map_err(Fatal::lost)? {
            Event::MapRequest(event) => {
                conn.map_window(event.window).map_err(Fatal::lost)?;
            }
            Event::ConfigureRequest(event) => {
                let aux = ConfigureWindowAux::from_configure_request(&event);
                conn.configure_window(event.window, &aux)
                    .map_err(Fatal::lost)?;
            }
            Event::KeyPress(event)
                if exit_key.keycodes.contains(&event.detail)
                    && is_exit_combination(event.state, exit_key.num_lock) =>
            {
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

/// Captura `Mod4+Shift+Escape` en la raíz, también con Bloq Mayús y Bloq Num
/// activos.
fn grab_exit_key(conn: &impl Connection, root: Window) -> Result<ExitKey, Fatal> {
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
    let keycodes = keycodes_for(XK_ESCAPE, min, per_keycode, &mapping.keysyms);
    if keycodes.is_empty() {
        return Err(Fatal::Startup(
            "ningún código de tecla produce Escape; no se puede capturar el atajo de salida".into(),
        ));
    }
    let num_lock_keycodes = keycodes_for(XK_NUM_LOCK, min, per_keycode, &mapping.keysyms);
    let num_lock = modifier_mask_for(&num_lock_keycodes, &modifiers.keycodes);

    for &keycode in &keycodes {
        for extra in lock_variants(num_lock) {
            conn.grab_key(
                false,
                root,
                ModMask::from(EXIT_MODIFIERS | extra),
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(Fatal::lost)?
            .check()
            .map_err(|e| Fatal::startup("GrabKey(Mod4+Shift+Escape)", e))?;
        }
    }

    Ok(ExitKey { keycodes, num_lock })
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

fn is_exit_combination(state: KeyButMask, num_lock: u16) -> bool {
    u16::from(state) & RELEVANT_MODIFIERS & !num_lock == EXIT_MODIFIERS
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

    #[test]
    fn reconoce_el_atajo_de_salida() {
        let m4_shift = KeyButMask::MOD4 | KeyButMask::SHIFT;
        assert!(is_exit_combination(m4_shift, NUM_LOCK_MOD2));
        assert!(is_exit_combination(
            m4_shift | KeyButMask::LOCK,
            NUM_LOCK_MOD2
        ));
        assert!(is_exit_combination(
            m4_shift | KeyButMask::MOD2,
            NUM_LOCK_MOD2
        ));
        assert!(is_exit_combination(
            m4_shift | KeyButMask::BUTTON1,
            NUM_LOCK_MOD2
        ));

        assert!(!is_exit_combination(KeyButMask::MOD4, NUM_LOCK_MOD2));
        assert!(!is_exit_combination(KeyButMask::SHIFT, NUM_LOCK_MOD2));
        assert!(!is_exit_combination(
            m4_shift | KeyButMask::CONTROL,
            NUM_LOCK_MOD2
        ));
        assert!(!is_exit_combination(
            m4_shift | KeyButMask::MOD1,
            NUM_LOCK_MOD2
        ));
        // Sin Bloq Num asignado a Mod2, Mod2 cuenta como modificador.
        assert!(!is_exit_combination(m4_shift | KeyButMask::MOD2, 0));
    }
}
