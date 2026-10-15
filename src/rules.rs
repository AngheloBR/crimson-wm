//! Reglas de ventanas (ADR 0013): análisis de `CRIMSON_RULES` y evaluación
//! determinista. Sin E/S: `main.rs` lee la variable una vez al arrancar y
//! `wm.rs` aplica la decisión al gestionar una ventana.
//!
//! Formato provisional e inestable, que sustituirá una configuración TOML:
//! una regla por línea y cláusulas separadas por `;`. Las líneas vacías y las
//! que empiezan por `#` se ignoran. Condiciones (se combinan con Y):
//! `instance=…`, `class=…` (exactas, distinguiendo mayúsculas),
//! `type=normal|dialog|utility|toolbar|splash|menu|dropdown_menu|popup_menu|tooltip|combo|dnd`
//! y `transient=yes|no`. Acciones: `desktop=1…8`, `floating`, `nofocus` y
//! `fullscreen`. Ejemplo:
//!
//! ```text
//! class=Firefox; desktop=2
//! instance=pavucontrol; floating; nofocus
//! ```

use std::ffi::OsString;
use std::fmt;

use crate::ewmh::WindowType;
use crate::icccm::WmClass;
use crate::workspaces;

/// Variable de entorno con las reglas.
pub const RULES_VAR: &str = "CRIMSON_RULES";
/// Tamaño máximo de la fuente, en bytes.
pub const MAX_SOURCE: usize = 16 * 1024;
/// Número máximo de reglas.
pub const MAX_RULES: usize = 64;

/// Condiciones de una regla. `None`: no se comprueba.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matcher {
    pub instance: Option<Vec<u8>>,
    pub class: Option<Vec<u8>>,
    pub window_type: Option<WindowType>,
    pub transient: Option<bool>,
}

/// Acciones de una regla.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Actions {
    /// Escritorio de 0 a 7 (en la fuente, de 1 a 8).
    pub desktop: Option<usize>,
    pub floating: bool,
    pub nofocus: bool,
    pub fullscreen: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Línea de la fuente (desde 1), para los registros.
    pub line: usize,
    pub matcher: Matcher,
    pub actions: Actions,
}

/// Lo que se sabe de una ventana al gestionarla.
#[derive(Clone, Copy, Debug)]
pub struct Subject<'a> {
    /// `WM_CLASS` válida; `None` si falta o es defectuosa.
    pub wm_class: Option<&'a WmClass>,
    /// Tipo efectivo: el primero reconocido o, sin ninguno, `Normal` (o
    /// `Dialog` si es transitoria), como define EWMH.
    pub window_type: WindowType,
    /// Tiene la propiedad `WM_TRANSIENT_FOR`, con cualquier valor.
    pub transient: bool,
}

/// Resultado de evaluar las reglas: cada acción, con la línea de la regla
/// que la fija. La fija la primera regla que coincide y la define.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Decision {
    pub desktop: Option<(usize, usize)>,
    pub floating: Option<usize>,
    pub nofocus: Option<usize>,
    pub fullscreen: Option<usize>,
    /// Escritorios de reglas posteriores que contradicen al elegido:
    /// (línea, escritorio).
    pub ignored: Vec<(usize, usize)>,
    /// Líneas de todas las reglas que coinciden, en orden.
    pub matched: Vec<usize>,
}

impl Decision {
    pub fn is_empty(&self) -> bool {
        self.matched.is_empty()
    }
}

/// Por qué se rechaza una fuente entera.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    TooLong(usize),
    TooMany(usize),
    NotUtf8,
    Line(usize, LineError),
}

/// Defecto de una línea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineError {
    EmptyClause,
    UnknownKey(String),
    Duplicate(String),
    MissingValue(String),
    UnexpectedValue(String),
    BadType(String),
    FixedType(String),
    BadTransient(String),
    BadDesktop(String),
    NoCondition,
    NoAction,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::TooLong(n) => write!(f, "{n} bytes (máximo {MAX_SOURCE})"),
            ParseError::TooMany(n) => write!(f, "{n} reglas (máximo {MAX_RULES})"),
            ParseError::NotUtf8 => write!(f, "no es UTF-8"),
            ParseError::Line(line, error) => write!(f, "línea {line}: {error}"),
        }
    }
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LineError::EmptyClause => write!(f, "cláusula vacía"),
            LineError::UnknownKey(k) => write!(f, "clave desconocida «{k}»"),
            LineError::Duplicate(k) => write!(f, "«{k}» repetida"),
            LineError::MissingValue(k) => write!(f, "«{k}» necesita un valor"),
            LineError::UnexpectedValue(k) => write!(f, "«{k}» no admite valor"),
            LineError::BadType(v) => write!(f, "tipo desconocido «{v}»"),
            LineError::FixedType(v) => {
                write!(f, "«{v}» es una ventana fija: las reglas no se le aplican")
            }
            LineError::BadTransient(v) => write!(f, "transient debe ser yes o no, no «{v}»"),
            LineError::BadDesktop(v) => write!(f, "escritorio «{v}» fuera de 1–8"),
            LineError::NoCondition => write!(f, "regla sin condiciones"),
            LineError::NoAction => write!(f, "regla sin acciones"),
        }
    }
}

/// Reglas de la variable de entorno: vacías si no existe. El tamaño se
/// comprueba antes que la codificación, sin copiar más de lo necesario.
pub fn load(var: Option<OsString>) -> Result<Vec<Rule>, ParseError> {
    let Some(var) = var else {
        return Ok(Vec::new());
    };
    if var.len() > MAX_SOURCE {
        return Err(ParseError::TooLong(var.len()));
    }
    let source = var.into_string().map_err(|_| ParseError::NotUtf8)?;
    parse(&source)
}

/// Analiza la fuente completa. Cualquier defecto la rechaza entera: nunca
/// se acepta una parte. Los límites de tamaño y de número de reglas se
/// comprueban antes que la sintaxis, con el número real de reglas.
pub fn parse(source: &str) -> Result<Vec<Rule>, ParseError> {
    if source.len() > MAX_SOURCE {
        return Err(ParseError::TooLong(source.len()));
    }
    let lines: Vec<(usize, &str)> = source
        .split('\n')
        .enumerate()
        .map(|(index, raw)| (index + 1, raw.trim()))
        .filter(|(_, text)| !text.is_empty() && !text.starts_with('#'))
        .collect();
    if lines.len() > MAX_RULES {
        return Err(ParseError::TooMany(lines.len()));
    }
    lines
        .into_iter()
        .map(|(line, text)| parse_line(line, text).map_err(|e| ParseError::Line(line, e)))
        .collect()
}

/// Tipos que una regla puede nombrar: los de las ventanas de escritorio.
const TYPE_NAMES: [(&str, WindowType); 11] = [
    ("normal", WindowType::Normal),
    ("dialog", WindowType::Dialog),
    ("utility", WindowType::Utility),
    ("toolbar", WindowType::Toolbar),
    ("splash", WindowType::Splash),
    ("menu", WindowType::Menu),
    ("dropdown_menu", WindowType::DropdownMenu),
    ("popup_menu", WindowType::PopupMenu),
    ("tooltip", WindowType::Tooltip),
    ("combo", WindowType::Combo),
    ("dnd", WindowType::Dnd),
];
/// Tipos de ventanas fijas (ADR 0011), fuera de las reglas.
const FIXED_NAMES: [&str; 3] = ["dock", "desktop", "notification"];

fn parse_line(line: usize, text: &str) -> Result<Rule, LineError> {
    let mut matcher = Matcher::default();
    let mut actions = Actions::default();
    let mut seen: Vec<&str> = Vec::new();
    for clause in text.split(';') {
        let clause = clause.trim();
        if clause.is_empty() {
            return Err(LineError::EmptyClause);
        }
        let (key, value) = match clause.split_once('=') {
            Some((k, v)) => (k.trim(), Some(v.trim())),
            None => (clause, None),
        };
        if seen.contains(&key) {
            return Err(LineError::Duplicate(key.to_string()));
        }
        let needs_value = matches!(key, "instance" | "class" | "type" | "transient" | "desktop");
        let flag = matches!(key, "floating" | "nofocus" | "fullscreen");
        if !needs_value && !flag {
            return Err(LineError::UnknownKey(key.to_string()));
        }
        let value = match (needs_value, value) {
            (true, Some(v)) if !v.is_empty() => v,
            (true, _) => return Err(LineError::MissingValue(key.to_string())),
            (false, Some(_)) => return Err(LineError::UnexpectedValue(key.to_string())),
            (false, None) => "",
        };
        match key {
            "instance" => matcher.instance = Some(value.as_bytes().to_vec()),
            "class" => matcher.class = Some(value.as_bytes().to_vec()),
            "type" => {
                if FIXED_NAMES.contains(&value) {
                    return Err(LineError::FixedType(value.to_string()));
                }
                let found = TYPE_NAMES.iter().find(|(name, _)| *name == value);
                let Some(&(_, t)) = found else {
                    return Err(LineError::BadType(value.to_string()));
                };
                matcher.window_type = Some(t);
            }
            "transient" => {
                matcher.transient = Some(match value {
                    "yes" => true,
                    "no" => false,
                    other => return Err(LineError::BadTransient(other.to_string())),
                });
            }
            "desktop" => {
                let desk = value
                    .parse::<usize>()
                    .ok()
                    .filter(|d| (1..=workspaces::COUNT).contains(d))
                    .ok_or_else(|| LineError::BadDesktop(value.to_string()))?;
                actions.desktop = Some(desk - 1);
            }
            "floating" => actions.floating = true,
            "nofocus" => actions.nofocus = true,
            _ => actions.fullscreen = true,
        }
        seen.push(key);
    }
    if matcher == Matcher::default() {
        return Err(LineError::NoCondition);
    }
    if actions == Actions::default() {
        return Err(LineError::NoAction);
    }
    Ok(Rule {
        line,
        matcher,
        actions,
    })
}

impl Matcher {
    /// Todas las condiciones se cumplen. Una condición sobre `WM_CLASS`
    /// nunca se cumple si la propiedad falta o es defectuosa.
    pub fn matches(&self, subject: &Subject) -> bool {
        let field = |want: &Option<Vec<u8>>, got: Option<&[u8]>| match want {
            None => true,
            Some(w) => got.is_some_and(|g| g == w.as_slice()),
        };
        field(
            &self.instance,
            subject.wm_class.map(|c| c.instance.as_slice()),
        ) && field(&self.class, subject.wm_class.map(|c| c.class.as_slice()))
            && self.window_type.is_none_or(|t| t == subject.window_type)
            && self.transient.is_none_or(|t| t == subject.transient)
    }
}

/// Evalúa todas las reglas, en orden. Cada acción la fija la primera regla
/// que coincide y la define; un escritorio distinto de una regla posterior
/// se anota como ignorado.
pub fn evaluate(rules: &[Rule], subject: &Subject) -> Decision {
    let mut decision = Decision::default();
    for rule in rules.iter().filter(|r| r.matcher.matches(subject)) {
        decision.matched.push(rule.line);
        let a = rule.actions;
        if let Some(desk) = a.desktop {
            match decision.desktop {
                None => decision.desktop = Some((desk, rule.line)),
                Some((chosen, _)) if chosen != desk => decision.ignored.push((rule.line, desk)),
                Some(_) => {}
            }
        }
        let first = |slot: &mut Option<usize>, set: bool| {
            if set && slot.is_none() {
                *slot = Some(rule.line);
            }
        };
        first(&mut decision.floating, a.floating);
        first(&mut decision.nofocus, a.nofocus);
        first(&mut decision.fullscreen, a.fullscreen);
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    // U23: reglas de ventanas.

    fn class(instance: &str, class: &str) -> WmClass {
        WmClass {
            instance: instance.as_bytes().to_vec(),
            class: class.as_bytes().to_vec(),
        }
    }

    fn subject(c: Option<&WmClass>, t: WindowType, transient: bool) -> Subject<'_> {
        Subject {
            wm_class: c,
            window_type: t,
            transient,
        }
    }

    fn line_error(source: &str) -> LineError {
        match parse(source) {
            Err(ParseError::Line(_, e)) => e,
            other => panic!("se esperaba un error de línea: {other:?}"),
        }
    }

    #[test]
    fn analiza_reglas_validas() {
        let rules = parse(
            "# comentario\n\nclass=Firefox; desktop=2\n  instance = pavucontrol ; floating;nofocus  \ntype=dialog; transient=no; fullscreen\n",
        )
        .unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].line, 3);
        assert_eq!(rules[0].matcher.class.as_deref(), Some(&b"Firefox"[..]));
        assert_eq!(rules[0].actions.desktop, Some(1));
        assert_eq!(
            rules[1].matcher.instance.as_deref(),
            Some(&b"pavucontrol"[..])
        );
        assert!(rules[1].actions.floating && rules[1].actions.nofocus);
        assert_eq!(rules[2].matcher.window_type, Some(WindowType::Dialog));
        assert_eq!(rules[2].matcher.transient, Some(false));
        assert!(rules[2].actions.fullscreen);
        assert_eq!(parse(""), Ok(Vec::new()));
        assert_eq!(parse("\n# solo comentarios\n"), Ok(Vec::new()));
        // Todos los tipos de escritorio; los fijos no.
        for (name, t) in TYPE_NAMES {
            let r = parse(&format!("type={name}; floating")).unwrap();
            assert_eq!(r[0].matcher.window_type, Some(t));
        }
    }

    #[test]
    fn rechaza_lineas_mal_formadas() {
        let cases = [
            ("class=a; ", LineError::EmptyClause),
            ("class=a;; floating", LineError::EmptyClause),
            (
                "colour=red; floating",
                LineError::UnknownKey("colour".into()),
            ),
            ("Class=a; floating", LineError::UnknownKey("Class".into())),
            (
                "class=a; class=b; floating",
                LineError::Duplicate("class".into()),
            ),
            (
                "class=a; floating; floating",
                LineError::Duplicate("floating".into()),
            ),
            (
                "class=a; desktop=1; desktop=2",
                LineError::Duplicate("desktop".into()),
            ),
            ("class=; floating", LineError::MissingValue("class".into())),
            ("class; floating", LineError::MissingValue("class".into())),
            (
                "class=a; floating=yes",
                LineError::UnexpectedValue("floating".into()),
            ),
            ("type=window; floating", LineError::BadType("window".into())),
            ("type=Dialog; floating", LineError::BadType("Dialog".into())),
            ("type=dock; floating", LineError::FixedType("dock".into())),
            (
                "type=desktop; floating",
                LineError::FixedType("desktop".into()),
            ),
            (
                "type=notification; floating",
                LineError::FixedType("notification".into()),
            ),
            (
                "transient=maybe; floating",
                LineError::BadTransient("maybe".into()),
            ),
            ("class=a; desktop=0", LineError::BadDesktop("0".into())),
            ("class=a; desktop=9", LineError::BadDesktop("9".into())),
            ("class=a; desktop=-1", LineError::BadDesktop("-1".into())),
            ("class=a; desktop=x", LineError::BadDesktop("x".into())),
            ("floating; nofocus", LineError::NoCondition),
            ("class=a; transient=yes", LineError::NoAction),
        ];
        for (source, error) in cases {
            assert_eq!(line_error(source), error, "{source}");
        }
        // Una línea mala rechaza la fuente entera, con su número.
        assert_eq!(
            parse("class=a; floating\n\nclass=b; desktop=12\nclass=c; nofocus"),
            Err(ParseError::Line(3, LineError::BadDesktop("12".into())))
        );
    }

    #[test]
    fn limites_de_tamano_y_numero() {
        let one = "class=a; floating\n";
        assert_eq!(parse(&one.repeat(MAX_RULES)).unwrap().len(), MAX_RULES);
        assert_eq!(
            parse(&one.repeat(MAX_RULES + 1)),
            Err(ParseError::TooMany(MAX_RULES + 1))
        );
        // El número real, no el de la primera regla que sobra; y antes que
        // la sintaxis de cualquier línea.
        assert_eq!(parse(&one.repeat(100)), Err(ParseError::TooMany(100)));
        let with_bad = format!("class=a; desktop=99\n{}", one.repeat(MAX_RULES));
        assert_eq!(parse(&with_bad), Err(ParseError::TooMany(MAX_RULES + 1)));
        // Los comentarios y las líneas vacías no cuentan.
        let commented = format!("{}# x\n\n", one).repeat(MAX_RULES);
        assert_eq!(parse(&commented).unwrap().len(), MAX_RULES);
        let big = format!("class=a; floating\n#{}", "x".repeat(MAX_SOURCE));
        assert!(matches!(parse(&big), Err(ParseError::TooLong(_))));
        let exact = format!("class=a; floating\n#{}", "x".repeat(MAX_SOURCE - 19));
        assert_eq!(exact.len(), MAX_SOURCE);
        assert!(parse(&exact).is_ok());
        // Desde la variable de entorno: ausente, demasiado grande, no UTF-8.
        assert_eq!(load(None), Ok(Vec::new()));
        assert!(matches!(
            load(Some(OsString::from(big))),
            Err(ParseError::TooLong(_))
        ));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let bad = OsString::from_vec(b"class=\xff; floating".to_vec());
            assert_eq!(load(Some(bad)), Err(ParseError::NotUtf8));
        }
    }

    #[test]
    fn coincidencias_exactas_y_combinadas() {
        let firefox = class("Navigator", "Firefox");
        let rules = parse(
            "class=Firefox; desktop=2\ninstance=Navigator; class=firefox; floating\ntype=dialog; transient=yes; nofocus",
        )
        .unwrap();
        let s = subject(Some(&firefox), WindowType::Normal, false);
        let d = evaluate(&rules, &s);
        assert_eq!(d.matched, vec![1], "mayúsculas distintas no coinciden");
        assert_eq!(d.desktop, Some((1, 1)));
        assert_eq!(d.floating, None);
        // Y lógico: tipo y transient a la vez.
        let dialog = subject(None, WindowType::Dialog, true);
        assert_eq!(evaluate(&rules, &dialog).nofocus, Some(3));
        let not_transient = subject(None, WindowType::Dialog, false);
        assert!(evaluate(&rules, &not_transient).is_empty());
        // Sin WM_CLASS, las condiciones de clase nunca se cumplen.
        let anonymous = subject(None, WindowType::Normal, false);
        assert!(evaluate(&rules, &anonymous).is_empty());
        // Bytes que no son UTF-8 se comparan tal cual.
        let latin = WmClass {
            instance: b"caf\xe9".to_vec(),
            class: b"Caf\xe9".to_vec(),
        };
        assert!(evaluate(&rules, &subject(Some(&latin), WindowType::Normal, false)).is_empty());
        let empty = class("", "");
        assert!(evaluate(&rules, &subject(Some(&empty), WindowType::Normal, false)).is_empty());
    }

    #[test]
    fn prioridad_y_conflictos() {
        let c = class("chat", "Chat");
        let rules = parse(
            "class=Chat; desktop=2\nclass=Chat; desktop=4; floating\ninstance=chat; desktop=2; nofocus\nclass=Chat; floating; fullscreen",
        )
        .unwrap();
        let d = evaluate(&rules, &subject(Some(&c), WindowType::Normal, false));
        assert_eq!(d.matched, vec![1, 2, 3, 4]);
        assert_eq!(d.desktop, Some((1, 1)), "gana la primera");
        assert_eq!(d.ignored, vec![(2, 3)], "solo la que contradice");
        assert_eq!(d.floating, Some(2));
        assert_eq!(d.nofocus, Some(3));
        assert_eq!(d.fullscreen, Some(4));
        // Sin reglas, la decisión está vacía.
        assert_eq!(
            evaluate(&[], &subject(Some(&c), WindowType::Normal, false)),
            Decision::default()
        );
    }
}
