//! Esquema de `wm.toml` (ADR 0014): del árbol TOML a una `Config` validada.
//! Solo existen `[layout]`, `[programs.NOMBRE]`, `[[keybindings]]` y
//! `[[rules]]`; cualquier otra clave es un error. Los límites de número se
//! comprueban antes de validar los elementos.
//!
//! Los mensajes nombran claves, acciones, teclas y números, pero nunca
//! reproducen cadenas de argv ni de las reglas: podrían ser privadas.

use super::document::{Item, Table, Value};
use super::{Config, Problem, line_at, shown};
use crate::keynames;
use crate::keys::{Action, Binding};
use crate::layout::{MAX_MASTER_PERCENT, MIN_MASTER_PERCENT};
use crate::rules::{self, Actions, Matcher, Rule};
use crate::spawn::Program;
use crate::workspaces;

/// Máximo de atajos de `[[keybindings]]`.
pub const MAX_BINDINGS: usize = 128;
/// Máximo de programas de `[programs]`.
pub const MAX_PROGRAMS: usize = 64;
/// Máximo de elementos de un argv.
pub const MAX_ARGV: usize = 32;
/// Tamaño máximo de cada elemento de argv, en bytes.
pub const MAX_ARG_BYTES: usize = 4096;
/// Longitud máxima del nombre de un programa.
pub const MAX_NAME: usize = 32;
/// Tamaño máximo de `instance` y `class` en una regla, en bytes.
pub const MAX_MATCH_BYTES: usize = 256;

const ROOT_KEYS: [&str; 4] = ["layout", "programs", "keybindings", "rules"];
const LAYOUT_KEYS: [&str; 1] = ["master_percent"];
const PROGRAM_KEYS: [&str; 1] = ["argv"];
const BINDING_KEYS: [&str; 4] = ["keys", "action", "program", "desktop"];
const RULE_KEYS: [&str; 8] = [
    "instance",
    "class",
    "type",
    "transient",
    "desktop",
    "floating",
    "nofocus",
    "fullscreen",
];

/// Construye la configuración. Lo que el archivo no define conserva el
/// valor predeterminado; `[programs]` y `[[keybindings]]` sustituyen cada
/// uno su tabla predeterminada completa, sin fusionarla.
pub fn build(root: &Table, text: &str) -> Result<Config, Problem> {
    known(root, &ROOT_KEYS, "en la raíz")?;
    let mut config = Config::default();
    if let Some(item) = root.get("layout") {
        config.master_percent = layout(item)?;
    }
    let programs_item = root.get("programs");
    if let Some(item) = programs_item {
        config.programs = programs(item)?;
    }
    match (root.get("keybindings"), programs_item) {
        (Some(item), _) => config.bindings = keybindings(item, &config.programs, text)?,
        (None, Some(programs_item)) => {
            config.bindings = remap_defaults(config.bindings, &config.programs, programs_item)?;
        }
        (None, None) => {}
    }
    if let Some(item) = root.get("rules") {
        config.rules = window_rules(item, text)?;
    }
    Ok(config)
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "una cadena",
        Value::Integer(_) => "un entero",
        Value::Boolean(_) => "un booleano",
        Value::Float => "un decimal",
        Value::Datetime => "una fecha",
        Value::Array(..) => "un array",
        Value::Table(_) => "una tabla",
    }
}

fn wrong(item: &Item, what: &str, want: &str) -> Problem {
    Problem::new(
        Some(item.offset),
        format!("{what} debe ser {want}, no {}", kind(&item.value)),
    )
}

/// Rechaza las claves que no están en `allowed`.
fn known(table: &Table, allowed: &[&str], place: &str) -> Result<(), Problem> {
    match table.entries().find(|(k, _, _)| !allowed.contains(k)) {
        Some((key, offset, _)) => Err(Problem::new(
            Some(offset),
            format!("clave desconocida «{}» {place}", shown(key)),
        )),
        None => Ok(()),
    }
}

fn table<'t>(item: &'t Item, what: &str) -> Result<&'t Table, Problem> {
    match &item.value {
        Value::Table(t) => Ok(t),
        _ => Err(wrong(item, what, "una tabla")),
    }
}

fn string<'t>(item: &'t Item, what: &str) -> Result<&'t str, Problem> {
    match &item.value {
        Value::String(s) => Ok(s),
        _ => Err(wrong(item, what, "una cadena")),
    }
}

fn integer(item: &Item, what: &str) -> Result<i64, Problem> {
    match item.value {
        Value::Integer(n) => Ok(n),
        _ => Err(wrong(item, what, "un entero")),
    }
}

fn boolean(item: &Item, what: &str) -> Result<bool, Problem> {
    match item.value {
        Value::Boolean(b) => Ok(b),
        _ => Err(wrong(item, what, "un booleano (true o false)")),
    }
}

/// Tablas de `[[x]]` o de un array de tablas en línea, con sus posiciones.
/// El límite de número se comprueba antes de mirar los elementos.
fn tables<'t>(item: &'t Item, what: &str, max: usize) -> Result<Vec<&'t Item>, Problem> {
    let Value::Array(items, _) = &item.value else {
        return Err(wrong(item, what, "un array de tablas ([[…]])"));
    };
    if items.len() > max {
        return Err(Problem::new(
            Some(item.offset),
            format!("{what}: {} elementos (máximo {max})", items.len()),
        ));
    }
    items
        .iter()
        .map(|element| match element.value {
            Value::Table(_) => Ok(element),
            _ => Err(wrong(
                element,
                &format!("cada elemento de {what}"),
                "una tabla",
            )),
        })
        .collect()
}

fn required<'t>(table: &'t Table, key: &str, at: &Item, place: &str) -> Result<&'t Item, Problem> {
    table
        .get(key)
        .ok_or_else(|| Problem::new(Some(at.offset), format!("falta «{key}» {place}")))
}

/// Escritorio de 1 a 8, como índice de 0 a 7.
fn desktop(item: &Item, what: &str) -> Result<usize, Problem> {
    let n = integer(item, what)?;
    usize::try_from(n)
        .ok()
        .filter(|d| (1..=workspaces::COUNT).contains(d))
        .map(|d| d - 1)
        .ok_or_else(|| {
            Problem::new(
                Some(item.offset),
                format!("{what} = {n} fuera de 1–{}", workspaces::COUNT),
            )
        })
}

// [layout]

fn layout(item: &Item) -> Result<u8, Problem> {
    let t = table(item, "[layout]")?;
    known(t, &LAYOUT_KEYS, "en [layout]")?;
    let Some(value) = t.get("master_percent") else {
        return Ok(crate::layout::DEFAULT_MASTER_PERCENT);
    };
    let n = integer(value, "master_percent")?;
    u8::try_from(n)
        .ok()
        .filter(|p| (MIN_MASTER_PERCENT..=MAX_MASTER_PERCENT).contains(p))
        .ok_or_else(|| {
            Problem::new(
                Some(value.offset),
                format!("master_percent = {n} fuera de {MIN_MASTER_PERCENT}–{MAX_MASTER_PERCENT}"),
            )
        })
}

// [programs.NOMBRE]

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_NAME
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn programs(item: &Item) -> Result<Vec<Program>, Problem> {
    let t = table(item, "[programs]")?;
    let count = t.entries().count();
    if count > MAX_PROGRAMS {
        return Err(Problem::new(
            Some(item.offset),
            format!("[programs]: {count} programas (máximo {MAX_PROGRAMS})"),
        ));
    }
    t.entries()
        .map(|(name, offset, entry)| program(name, offset, entry))
        .collect()
}

fn program(name: &str, name_offset: usize, item: &Item) -> Result<Program, Problem> {
    if !valid_name(name) {
        return Err(Problem::new(
            Some(name_offset),
            format!(
                "nombre de programa «{}» inválido: debe empezar por una minúscula y tener solo minúsculas, dígitos, «_» o «-» (máximo {MAX_NAME})",
                shown(name)
            ),
        ));
    }
    let place = format!("[programs.{name}]");
    let t = table(item, &place)?;
    known(t, &PROGRAM_KEYS, &format!("en {place}"))?;
    let argv_item = required(t, "argv", item, &format!("en {place}"))?;
    let what = format!("argv de «{name}»");
    let Value::Array(elements, _) = &argv_item.value else {
        return Err(wrong(argv_item, &what, "un array de cadenas"));
    };
    if elements.is_empty() || elements.len() > MAX_ARGV {
        return Err(Problem::new(
            Some(argv_item.offset),
            format!("{what}: {} elementos (de 1 a {MAX_ARGV})", elements.len()),
        ));
    }
    let mut argv = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        let arg = string(element, &format!("{what}[{index}]"))?;
        let problem =
            |reason: &str| Problem::new(Some(element.offset), format!("{what}[{index}] {reason}"));
        if arg.len() > MAX_ARG_BYTES {
            return Err(problem(&format!(
                "ocupa {} bytes (máximo {MAX_ARG_BYTES})",
                arg.len()
            )));
        }
        if arg.contains('\0') {
            return Err(problem("contiene un carácter NUL"));
        }
        if index == 0 {
            if arg.is_empty() {
                return Err(problem("está vacío"));
            }
            if arg.starts_with('~') {
                return Err(problem(
                    "empieza por «~»: no se expande; usa una ruta absoluta o un nombre del PATH",
                ));
            }
            if arg.contains('$') {
                return Err(problem(
                    "contiene «$»: no se expanden variables; usa una ruta absoluta o un nombre del PATH",
                ));
            }
            if arg.contains('/') && !arg.starts_with('/') {
                return Err(problem(
                    "es una ruta relativa: usa una ruta absoluta o un nombre del PATH",
                ));
            }
        }
        argv.push(arg.to_string());
    }
    Ok(Program {
        name: name.to_string(),
        argv,
    })
}

// [[keybindings]]

fn keybindings(item: &Item, programs: &[Program], text: &str) -> Result<Vec<Binding>, Problem> {
    let list = tables(item, "keybindings", MAX_BINDINGS)?;
    let mut bindings: Vec<(Binding, usize)> = Vec::with_capacity(list.len());
    for element in list {
        let binding = keybinding(element, programs)?;
        if let Some((first, offset)) = bindings
            .iter()
            .find(|(b, _)| (b.modifiers, b.keysym) == (binding.modifiers, binding.keysym))
        {
            return Err(Problem::new(
                Some(element.offset),
                format!(
                    "el atajo {} está repetido (ya definido en la línea {})",
                    first.name,
                    line_at(text, *offset).0
                ),
            ));
        }
        bindings.push((binding, element.offset));
    }
    if !bindings.iter().any(|(b, _)| b.action == Action::Exit) {
        return Err(Problem::new(
            Some(item.offset),
            "[[keybindings]] sustituye a la tabla predeterminada completa y no tiene ningún atajo con action = \"exit\"",
        ));
    }
    Ok(bindings.into_iter().map(|(b, _)| b).collect())
}

fn keybinding(element: &Item, programs: &[Program]) -> Result<Binding, Problem> {
    const PLACE: &str = "en [[keybindings]]";
    let t = table(element, "[[keybindings]]")?;
    known(t, &BINDING_KEYS, PLACE)?;
    let keys_item = required(t, "keys", element, PLACE)?;
    let combo = keynames::parse(string(keys_item, "keys")?)
        .map_err(|e| Problem::new(Some(keys_item.offset), format!("keys: {e}")))?;
    let action_item = required(t, "action", element, PLACE)?;
    let action_name = string(action_item, "action")?;
    let program = t.get("program");
    let desk = t.get("desktop");
    let action = match action_name {
        "exit" => Action::Exit,
        "focus_next" => Action::FocusNext,
        "focus_prev" => Action::FocusPrev,
        "zoom" => Action::Zoom,
        "view_previous" => Action::ViewPrevious,
        "close" => Action::Close,
        "kill" => Action::Kill,
        "fullscreen" => Action::Fullscreen,
        "launch" => {
            let item = program.ok_or_else(|| {
                Problem::new(
                    Some(action_item.offset),
                    "la acción launch necesita «program»",
                )
            })?;
            let name = string(item, "program")?;
            let index = programs
                .iter()
                .position(|p| p.name == name)
                .ok_or_else(|| {
                    Problem::new(
                        Some(item.offset),
                        format!(
                            "el programa «{}» no está definido en [programs]",
                            shown(name)
                        ),
                    )
                })?;
            Action::Launch(index)
        }
        "view_desktop" | "move_to_desktop" => {
            let item = desk.ok_or_else(|| {
                Problem::new(
                    Some(action_item.offset),
                    format!("la acción {action_name} necesita «desktop»"),
                )
            })?;
            let index = desktop(item, "desktop")?;
            if action_name == "view_desktop" {
                Action::View(index)
            } else {
                Action::MoveTo(index)
            }
        }
        other => {
            return Err(Problem::new(
                Some(action_item.offset),
                format!(
                    "acción desconocida «{}» (exit, focus_next, focus_prev, zoom, launch, view_desktop, move_to_desktop, view_previous, close, kill o fullscreen)",
                    shown(other)
                ),
            ));
        }
    };
    // Parámetros que la acción no usa: sin semántica, se rechazan.
    if let (Some(item), false) = (program, matches!(action, Action::Launch(_))) {
        return Err(Problem::new(
            Some(item.offset),
            format!("«program» no se usa con la acción {action_name}"),
        ));
    }
    if let (Some(item), false) = (desk, matches!(action, Action::View(_) | Action::MoveTo(_))) {
        return Err(Problem::new(
            Some(item.offset),
            format!("«desktop» no se usa con la acción {action_name}"),
        ));
    }
    Ok(Binding {
        modifiers: combo.modifiers,
        keysym: combo.keysym,
        name: combo.label,
        action,
    })
}

/// Con `[programs]` y sin `[[keybindings]]`, los atajos predeterminados de
/// lanzamiento buscan su programa por nombre en la tabla nueva.
fn remap_defaults(
    bindings: Vec<Binding>,
    programs: &[Program],
    programs_item: &Item,
) -> Result<Vec<Binding>, Problem> {
    let defaults = crate::spawn::default_programs();
    bindings
        .into_iter()
        .map(|mut binding| {
            if let Action::Launch(old) = binding.action {
                let name = &defaults[old].name;
                let index = programs.iter().position(|p| &p.name == name).ok_or_else(|| {
                    Problem::new(
                        Some(programs_item.offset),
                        format!(
                            "[programs] sustituye a la tabla predeterminada y no define «{name}», que usa el atajo predeterminado {}; añádelo o define [[keybindings]]",
                            binding.name
                        ),
                    )
                })?;
                binding.action = Action::Launch(index);
            }
            Ok(binding)
        })
        .collect()
}

// [[rules]]

fn window_rules(item: &Item, text: &str) -> Result<Vec<Rule>, Problem> {
    tables(item, "rules", rules::MAX_RULES)?
        .into_iter()
        .map(|element| window_rule(element, text))
        .collect()
}

fn match_string(t: &Table, key: &str) -> Result<Option<Vec<u8>>, Problem> {
    let Some(item) = t.get(key) else {
        return Ok(None);
    };
    let s = string(item, key)?;
    if s.is_empty() || s.len() > MAX_MATCH_BYTES || s.contains('\0') {
        return Err(Problem::new(
            Some(item.offset),
            format!("{key} debe tener de 1 a {MAX_MATCH_BYTES} bytes, sin NUL"),
        ));
    }
    Ok(Some(s.as_bytes().to_vec()))
}

fn flag(t: &Table, key: &str) -> Result<bool, Problem> {
    t.get(key).map_or(Ok(false), |item| boolean(item, key))
}

fn window_rule(element: &Item, text: &str) -> Result<Rule, Problem> {
    let t = table(element, "[[rules]]")?;
    known(t, &RULE_KEYS, "en [[rules]]")?;
    let window_type = match t.get("type") {
        None => None,
        Some(item) => {
            let name = string(item, "type")?;
            Some(rules::window_type(name).map_err(|_| {
                let reason = if ["dock", "desktop", "notification"].contains(&name) {
                    "es una ventana fija: las reglas no se le aplican"
                } else {
                    "es un tipo desconocido"
                };
                Problem::new(
                    Some(item.offset),
                    format!("type «{}» {reason}", shown(name)),
                )
            })?)
        }
    };
    let matcher = Matcher {
        instance: match_string(t, "instance")?,
        class: match_string(t, "class")?,
        window_type,
        transient: t
            .get("transient")
            .map(|item| boolean(item, "transient"))
            .transpose()?,
    };
    let actions = Actions {
        desktop: t
            .get("desktop")
            .map(|item| desktop(item, "desktop"))
            .transpose()?,
        floating: flag(t, "floating")?,
        nofocus: flag(t, "nofocus")?,
        fullscreen: flag(t, "fullscreen")?,
    };
    rules::rule(line_at(text, element.offset).0, matcher, actions)
        .map_err(|e| Problem::new(Some(element.offset), e.to_string()))
}
