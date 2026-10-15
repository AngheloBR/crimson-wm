//! Nombres de teclas y modificadores de los atajos configurables (ADR
//! 0014). Sin E/S: traduce el texto de un atajo, como `Mod4+Shift+Return`,
//! a la máscara de modificadores y el keysym que captura `keys.rs`.
//!
//! La tabla es deliberadamente pequeña y se contrasta con `keysymdef.h` y
//! `XF86keysym.h` en las pruebas: un nombre mal traducido capturaría otra
//! tecla. Los nombres distinguen mayúsculas, como en X11 (`Return`, `a`).

use std::fmt;

use x11rb::protocol::xproto::Keysym;

// Máscaras de modificadores del protocolo X11 (`ModMask`).
pub const SHIFT: u16 = 1 << 0;
pub const CONTROL: u16 = 1 << 2;
pub const MOD1: u16 = 1 << 3;
pub const MOD4: u16 = 1 << 6;

/// Modificadores admitidos, en el orden de las etiquetas normalizadas, con
/// sus alias. `Alt` y `Super` suponen la asignación habitual de Debian
/// (Alt_L en Mod1 y Super_L en Mod4).
const MODIFIERS: [(u16, &str, &[&str]); 4] = [
    (MOD4, "Mod4", &["Mod4", "Super"]),
    (MOD1, "Mod1", &["Mod1", "Alt"]),
    (CONTROL, "Control", &["Control", "Ctrl"]),
    (SHIFT, "Shift", &["Shift"]),
];

/// Modificadores que se reconocen pero no se admiten: Bloq Mayús y Bloq Num
/// se ignoran siempre (ADR 0005) y Mod3 y Mod5 no tienen una asignación
/// estable.
const UNSUPPORTED_MODIFIERS: [&str; 7] =
    ["Lock", "Mod2", "Mod3", "Mod5", "Hyper", "Meta", "NumLock"];

/// Clase de una tecla: las de escritura y edición solo se admiten con
/// Control, Mod1 o Mod4, para que un atajo no robe la escritura normal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Letras, dígitos, puntuación, espacio, edición y movimiento.
    Typing,
    /// Teclas de función, Impr Pant y multimedia: se admiten solas.
    Free,
}

/// Teclas con nombre, además de `a`–`z` y `0`–`9`. Un keysym puede tener
/// varios nombres: el primero es el de las etiquetas normalizadas.
const NAMED: &[(&str, Keysym, Class)] = &[
    ("Return", 0xff0d, Class::Typing),
    ("Escape", 0xff1b, Class::Typing),
    ("Tab", 0xff09, Class::Typing),
    ("BackSpace", 0xff08, Class::Typing),
    ("Delete", 0xffff, Class::Typing),
    ("Insert", 0xff63, Class::Typing),
    ("Home", 0xff50, Class::Typing),
    ("End", 0xff57, Class::Typing),
    ("Prior", 0xff55, Class::Typing),
    ("Page_Up", 0xff55, Class::Typing),
    ("Next", 0xff56, Class::Typing),
    ("Page_Down", 0xff56, Class::Typing),
    ("Left", 0xff51, Class::Typing),
    ("Up", 0xff52, Class::Typing),
    ("Right", 0xff53, Class::Typing),
    ("Down", 0xff54, Class::Typing),
    ("space", 0x0020, Class::Typing),
    ("minus", 0x002d, Class::Typing),
    ("equal", 0x003d, Class::Typing),
    ("comma", 0x002c, Class::Typing),
    ("period", 0x002e, Class::Typing),
    ("slash", 0x002f, Class::Typing),
    ("semicolon", 0x003b, Class::Typing),
    ("apostrophe", 0x0027, Class::Typing),
    ("grave", 0x0060, Class::Typing),
    ("bracketleft", 0x005b, Class::Typing),
    ("bracketright", 0x005d, Class::Typing),
    ("backslash", 0x005c, Class::Typing),
    ("Print", 0xff61, Class::Free),
    ("F1", 0xffbe, Class::Free),
    ("F2", 0xffbf, Class::Free),
    ("F3", 0xffc0, Class::Free),
    ("F4", 0xffc1, Class::Free),
    ("F5", 0xffc2, Class::Free),
    ("F6", 0xffc3, Class::Free),
    ("F7", 0xffc4, Class::Free),
    ("F8", 0xffc5, Class::Free),
    ("F9", 0xffc6, Class::Free),
    ("F10", 0xffc7, Class::Free),
    ("F11", 0xffc8, Class::Free),
    ("F12", 0xffc9, Class::Free),
    ("XF86AudioLowerVolume", 0x1008_ff11, Class::Free),
    ("XF86AudioMute", 0x1008_ff12, Class::Free),
    ("XF86AudioRaiseVolume", 0x1008_ff13, Class::Free),
    ("XF86AudioPlay", 0x1008_ff14, Class::Free),
    ("XF86AudioStop", 0x1008_ff15, Class::Free),
    ("XF86AudioPrev", 0x1008_ff16, Class::Free),
    ("XF86AudioNext", 0x1008_ff17, Class::Free),
    ("XF86AudioMicMute", 0x1008_ffb2, Class::Free),
    ("XF86MonBrightnessUp", 0x1008_ff02, Class::Free),
    ("XF86MonBrightnessDown", 0x1008_ff03, Class::Free),
];

/// Combinación normalizada: dos atajos son el mismo si coinciden la máscara
/// y el keysym, se escriban como se escriban.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: u16,
    pub keysym: Keysym,
    /// Etiqueta normalizada, p. ej. `Mod4+Shift+Return`.
    pub label: String,
}

/// Por qué se rechaza el texto de un atajo. Solo incluye nombres de teclas
/// o modificadores, recortados, nunca otros valores del archivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComboError {
    Empty,
    EmptyPart,
    Whitespace,
    UnknownKey(String),
    Uppercase(String),
    UnknownModifier(String),
    UnsupportedModifier(String),
    RepeatedModifier(String),
    /// Una tecla de escritura sin Control, Mod1 ni Mod4.
    NeedsModifier(String),
}

impl fmt::Display for ComboError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComboError::Empty => write!(f, "atajo vacío"),
            ComboError::EmptyPart => write!(f, "atajo con una parte vacía (¿«++» o «+» al final?)"),
            ComboError::Whitespace => write!(f, "el atajo no admite espacios"),
            ComboError::UnknownKey(k) => write!(f, "tecla desconocida «{k}»"),
            ComboError::Uppercase(k) => write!(
                f,
                "tecla «{k}»: las letras se escriben en minúscula; para la mayúscula, añade Shift"
            ),
            ComboError::UnknownModifier(m) => write!(
                f,
                "modificador desconocido «{m}» (se admiten Shift, Control, Mod1 y Mod4, o Ctrl, Alt y Super)"
            ),
            ComboError::UnsupportedModifier(m) => write!(f, "modificador «{m}» no admitido"),
            ComboError::RepeatedModifier(m) => write!(f, "modificador «{m}» repetido"),
            ComboError::NeedsModifier(k) => write!(
                f,
                "«{k}» es una tecla de escritura: necesita Control, Mod1 o Mod4"
            ),
        }
    }
}

/// Recorta un nombre para los mensajes de error.
fn shown(text: &str) -> String {
    const MAX: usize = 32;
    let mut out: String = text
        .chars()
        .take(MAX)
        .flat_map(char::escape_debug)
        .collect();
    if text.chars().nth(MAX).is_some() {
        out.push('…');
    }
    out
}

/// Keysym y clase de un nombre de tecla.
fn key(name: &str) -> Result<(Keysym, Class), ComboError> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            return Ok((Keysym::from(c), Class::Typing));
        }
        if c.is_ascii_uppercase() {
            return Err(ComboError::Uppercase(name.to_string()));
        }
    }
    NAMED
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|&(_, keysym, class)| (keysym, class))
        .ok_or_else(|| ComboError::UnknownKey(shown(name)))
}

/// Nombre normalizado de un keysym de la tabla.
fn key_label(keysym: Keysym) -> String {
    if let Some(c) = char::from_u32(keysym).filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return c.to_string();
    }
    NAMED
        .iter()
        .find(|(_, k, _)| *k == keysym)
        .map_or_else(|| format!("0x{keysym:x}"), |(n, _, _)| (*n).to_string())
}

/// Etiqueta normalizada de una combinación.
pub fn label(modifiers: u16, keysym: Keysym) -> String {
    let mut parts: Vec<String> = MODIFIERS
        .iter()
        .filter(|(mask, _, _)| modifiers & mask != 0)
        .map(|(_, name, _)| (*name).to_string())
        .collect();
    parts.push(key_label(keysym));
    parts.join("+")
}

/// Analiza `Mod4+Shift+Return`: modificadores separados por `+` y una tecla
/// al final. El orden de los modificadores no importa.
pub fn parse(text: &str) -> Result<Combo, ComboError> {
    if text.is_empty() {
        return Err(ComboError::Empty);
    }
    if text.chars().any(char::is_whitespace) {
        return Err(ComboError::Whitespace);
    }
    let parts: Vec<&str> = text.split('+').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(ComboError::EmptyPart);
    }
    let (key_name, modifier_names) = parts.split_last().ok_or(ComboError::Empty)?;
    let mut modifiers = 0;
    for name in modifier_names {
        let Some((mask, canonical, _)) = MODIFIERS
            .iter()
            .find(|(_, _, aliases)| aliases.contains(name))
        else {
            if UNSUPPORTED_MODIFIERS.contains(name) {
                return Err(ComboError::UnsupportedModifier((*name).to_string()));
            }
            return Err(ComboError::UnknownModifier(shown(name)));
        };
        if modifiers & mask != 0 {
            return Err(ComboError::RepeatedModifier((*canonical).to_string()));
        }
        modifiers |= mask;
    }
    let (keysym, class) = key(key_name)?;
    if class == Class::Typing && modifiers & (CONTROL | MOD1 | MOD4) == 0 {
        return Err(ComboError::NeedsModifier(key_label(keysym)));
    }
    Ok(Combo {
        modifiers,
        keysym,
        label: label(modifiers, keysym),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(text: &str) -> (u16, Keysym, String) {
        let c = parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        (c.modifiers, c.keysym, c.label)
    }

    #[test]
    fn errores_escapan_caracteres_de_control() {
        for text in ["Mod4+\u{1b}[31m", "\0+j", "Mod4+\u{7f}"] {
            let message = parse(text).unwrap_err().to_string();
            assert!(!message.chars().any(char::is_control), "{message:?}");
            assert!(message.contains('\\'), "{message:?}");
        }
    }

    #[test]
    fn combinaciones_validas() {
        assert_eq!(
            combo("Mod4+Shift+Escape"),
            (MOD4 | SHIFT, 0xff1b, "Mod4+Shift+Escape".into())
        );
        assert_eq!(combo("Mod4+j"), (MOD4, 0x6a, "Mod4+j".into()));
        assert_eq!(combo("Mod4+1"), (MOD4, 0x31, "Mod4+1".into()));
        assert_eq!(combo("Print"), (0, 0xff61, "Print".into()));
        assert_eq!(combo("F12"), (0, 0xffc9, "F12".into()));
        assert_eq!(combo("Shift+F1"), (SHIFT, 0xffbe, "Shift+F1".into()));
        assert_eq!(
            combo("XF86AudioRaiseVolume"),
            (0, 0x1008_ff13, "XF86AudioRaiseVolume".into())
        );
        assert_eq!(
            combo("Control+Mod1+Delete"),
            (CONTROL | MOD1, 0xffff, "Mod1+Control+Delete".into())
        );
    }

    #[test]
    fn normaliza_alias_y_orden() {
        let canonical = combo("Mod4+Shift+Return");
        for text in [
            "Shift+Mod4+Return",
            "Super+Shift+Return",
            "Shift+Super+Return",
        ] {
            assert_eq!(combo(text), canonical, "{text}");
        }
        assert_eq!(combo("Alt+Tab"), combo("Mod1+Tab"));
        assert_eq!(combo("Ctrl+Alt+Delete"), combo("Mod1+Control+Delete"));
        // Alias de teclas: la etiqueta usa el primer nombre.
        assert_eq!(combo("Mod4+Page_Up"), (MOD4, 0xff55, "Mod4+Prior".into()));
        assert_eq!(combo("Mod4+Page_Down"), combo("Mod4+Next"));
    }

    #[test]
    fn rechaza_textos_invalidos() {
        let cases: [(&str, ComboError); 15] = [
            ("", ComboError::Empty),
            ("Mod4+", ComboError::EmptyPart),
            ("+j", ComboError::EmptyPart),
            ("Mod4++j", ComboError::EmptyPart),
            ("Mod4 + j", ComboError::Whitespace),
            ("Mod4+\tj", ComboError::Whitespace),
            ("Mod4+J", ComboError::Uppercase("J".into())),
            ("Mod4+return", ComboError::UnknownKey("return".into())),
            ("Mod4+exclam", ComboError::UnknownKey("exclam".into())),
            ("Mod4+KP_1", ComboError::UnknownKey("KP_1".into())),
            ("Hyperx+j", ComboError::UnknownModifier("Hyperx".into())),
            (
                "Lock+Mod4+j",
                ComboError::UnsupportedModifier("Lock".into()),
            ),
            (
                "Mod2+Mod4+j",
                ComboError::UnsupportedModifier("Mod2".into()),
            ),
            ("Mod4+Super+j", ComboError::RepeatedModifier("Mod4".into())),
            ("Shift+j", ComboError::NeedsModifier("j".into())),
        ];
        for (text, want) in cases {
            assert_eq!(parse(text), Err(want), "{text:?}");
        }
        // Teclas de escritura solas o solo con Shift.
        for text in ["j", "1", "Return", "space", "Tab", "Shift+Return", "Left"] {
            assert!(
                matches!(parse(text), Err(ComboError::NeedsModifier(_))),
                "{text}"
            );
        }
        // Un modificador no puede ser la tecla, ni la tecla ir delante.
        assert!(matches!(parse("Mod4"), Err(ComboError::UnknownKey(_))));
        assert!(matches!(
            parse("j+Mod4"),
            Err(ComboError::UnknownModifier(_))
        ));
        // Los nombres largos se recortan en el mensaje.
        let long = format!("Mod4+{}", "x".repeat(100));
        let Err(ComboError::UnknownKey(shown)) = parse(&long) else {
            panic!("tecla larga aceptada");
        };
        assert_eq!(shown.chars().count(), 33);
    }

    #[test]
    fn todas_las_letras_y_digitos() {
        for c in ('a'..='z').chain('0'..='9') {
            let (m, k, l) = combo(&format!("Mod4+{c}"));
            assert_eq!((m, k), (MOD4, Keysym::from(c)));
            assert_eq!(l, format!("Mod4+{c}"));
        }
    }

    #[test]
    fn las_etiquetas_se_vuelven_a_analizar_igual() {
        for &(name, keysym, class) in NAMED {
            let text = if class == Class::Typing {
                format!("Mod4+{name}")
            } else {
                name.to_string()
            };
            let parsed = parse(&text).unwrap();
            assert_eq!(parsed.keysym, keysym, "{name}");
            assert_eq!(parse(&parsed.label).unwrap(), parsed, "{name}");
        }
    }

    #[test]
    fn la_tabla_no_tiene_nombres_repetidos() {
        for (i, a) in NAMED.iter().enumerate() {
            assert!(
                a.0.len() > 1,
                "{}: las teclas de un carácter van aparte",
                a.0
            );
            for b in &NAMED[i + 1..] {
                assert_ne!(a.0, b.0);
                if a.1 == b.1 {
                    assert_eq!(a.2, b.2, "alias de {} con otra clase", a.0);
                }
            }
        }
    }

    /// Contrasta la tabla con las cabeceras de X11 del sistema
    /// (`x11proto-core-dev`). Si no están instaladas, la comprobación se
    /// omite y lo dice; la prueba de aceptación A137 comprueba además la
    /// traducción con el servidor real.
    #[test]
    fn la_tabla_coincide_con_las_cabeceras_de_x11() {
        let headers = [
            ("/usr/include/X11/keysymdef.h", "XK_"),
            ("/usr/include/X11/XF86keysym.h", "XF86XK_"),
        ];
        let mut defined = Vec::new();
        for (path, prefix) in headers {
            let Ok(text) = std::fs::read_to_string(path) else {
                eprintln!("OMITIDA: no se puede leer {path}");
                return;
            };
            for line in text.lines() {
                let mut words = line.split_whitespace();
                if words.next() != Some("#define") {
                    continue;
                }
                let (Some(name), Some(value)) = (words.next(), words.next()) else {
                    continue;
                };
                let (Some(name), Some(hex)) = (name.strip_prefix(prefix), value.strip_prefix("0x"))
                else {
                    continue;
                };
                if let Ok(value) = Keysym::from_str_radix(hex, 16) {
                    let name = if prefix == "XF86XK_" {
                        format!("XF86{name}")
                    } else {
                        name.to_string()
                    };
                    defined.push((name, value));
                }
            }
        }
        let lookup = |name: &str| {
            defined
                .iter()
                .find(|(n, _)| n == name)
                .map(|&(_, v)| v)
                .unwrap_or_else(|| panic!("{name} no está en las cabeceras"))
        };
        for &(name, keysym, _) in NAMED {
            assert_eq!(lookup(name), keysym, "{name}");
        }
        for c in ('a'..='z').chain('0'..='9') {
            assert_eq!(lookup(&c.to_string()), Keysym::from(c), "{c}");
        }
    }
}
