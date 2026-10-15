//! Documento TOML (ADR 0014): del texto a un árbol de valores con datos
//! propios. `toml_parser` hace el análisis léxico y sintáctico (TOML 1.1)
//! y la decodificación de claves, cadenas y números; este módulo organiza
//! los eventos en tablas y aplica las reglas lógicas de la especificación,
//! que `toml_parser` deja al usuario:
//! - una clave no se define dos veces;
//! - una tabla `[x]` se define una sola vez y no puede ser una creada con
//!   claves con punto ni una tabla en línea;
//! - las claves con punto amplían tablas implícitas o creadas por claves
//!   con punto de la misma sección;
//! - una tabla en línea y un array estático no se amplían después;
//! - `[[x]]` solo añade elementos a un array de tablas.
//!
//! Cualquier defecto rechaza el documento entero. El árbol no guarda
//! referencias al texto: solo cadenas propias y posiciones (bytes).
//!
//! Las fechas y los decimales no se usan en el esquema: se reconocen como
//! tipos, sin interpretar su valor, y el esquema los rechaza siempre.

use toml_parser::decoder::ScalarKind;
use toml_parser::parser::{Event, EventKind, RecursionGuard, ValidateWhitespace, parse_document};
use toml_parser::{ParseError, Source};

use super::Problem;

/// Profundidad máxima de arrays y tablas en línea anidados. El esquema usa
/// como mucho dos niveles (`argv` dentro de un programa en línea).
const MAX_DEPTH: u32 = 8;

/// Valor con la posición (byte) donde empieza en el texto.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub offset: usize,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Boolean(bool),
    Float,
    Datetime,
    Array(Vec<Item>, ArrayKind),
    Table(Table),
}

/// Origen de un array: escrito entre corchetes o formado por `[[x]]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayKind {
    Static,
    Tables,
}

/// Cómo se creó una tabla; decide si se puede ampliar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Intermedia de una cabecera (`a` en `[a.b]`): una cabecera posterior
    /// puede definirla.
    Implicit,
    /// Definida por una cabecera, o la raíz.
    Header,
    /// Creada por claves con punto (`a.b = 1`).
    Dotted,
    /// Tabla en línea (`{ … }`): completa al cerrarse.
    Inline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// (clave, posición de la clave, valor), en el orden del texto.
    entries: Vec<(String, usize, Item)>,
    kind: Kind,
}

impl Table {
    fn new(kind: Kind) -> Self {
        Self {
            entries: Vec::new(),
            kind,
        }
    }

    /// Entradas en el orden del texto: (clave, posición de la clave,
    /// valor).
    pub fn entries(&self) -> impl Iterator<Item = (&str, usize, &Item)> {
        self.entries.iter().map(|(k, o, v)| (k.as_str(), *o, v))
    }

    pub fn get(&self, key: &str) -> Option<&Item> {
        self.entries
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, _, v)| v)
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Item> {
        self.entries
            .iter_mut()
            .find(|(k, _, _)| k == key)
            .map(|(_, _, v)| v)
    }

    /// Añade una entrada nueva.
    fn push(&mut self, key: &Key, item: Item) {
        self.entries.push((key.name.clone(), key.offset, item));
    }
}

/// Parte de una clave (`a` y `b` en `a.b`) con su posición.
struct Key {
    name: String,
    offset: usize,
}

/// Analiza el documento completo.
pub fn parse(text: &str) -> Result<Table, Problem> {
    let source = Source::new(text);
    let tokens = source.lex().into_vec();
    let mut events: Vec<Event> = Vec::with_capacity(tokens.len());
    let mut errors: Vec<ParseError> = Vec::new();
    {
        let mut whitespace = ValidateWhitespace::new(&mut events, source);
        let mut guard = RecursionGuard::new(&mut whitespace, MAX_DEPTH);
        parse_document(&tokens, &mut guard, &mut errors);
    }
    if let Some(error) = errors.first() {
        return Err(syntax(error));
    }
    let events: Vec<Event> = events
        .into_iter()
        .filter(|e| {
            !matches!(
                e.kind(),
                EventKind::Whitespace | EventKind::Comment | EventKind::Newline
            )
        })
        .collect();
    Builder {
        source,
        events,
        next: 0,
    }
    .document()
}

/// Error de sintaxis de `toml_parser`. Su descripción es un texto fijo de
/// la biblioteca (en inglés) y no reproduce el contenido del archivo.
fn syntax(error: &ParseError) -> Problem {
    let offset = error
        .unexpected()
        .or(error.context())
        .map(|span| span.start());
    Problem::new(offset, format!("TOML inválido: {}", error.description()))
}

struct Builder<'s> {
    source: Source<'s>,
    events: Vec<Event>,
    next: usize,
}

impl Builder<'_> {
    fn peek(&self) -> Option<&Event> {
        self.events.get(self.next)
    }

    fn take(&mut self) -> Option<Event> {
        let event = self.events.get(self.next).cloned();
        self.next += 1;
        event
    }

    /// Fin del texto, para los errores sin evento.
    fn end(&self) -> usize {
        self.source.input().len()
    }

    /// El evento siguiente debe ser de esa clase. Con el análisis de
    /// `toml_parser` sin errores no debería fallar; si falla, el documento
    /// se rechaza sin entrar en pánico.
    fn expect(&mut self, kind: EventKind) -> Result<Event, Problem> {
        match self.take() {
            Some(event) if event.kind() == kind => Ok(event),
            Some(event) => Err(unexpected(&event)),
            None => Err(Problem::new(
                Some(self.end()),
                "TOML inválido: final inesperado",
            )),
        }
    }

    fn document(mut self) -> Result<Table, Problem> {
        let mut root = Table::new(Kind::Header);
        // Ruta de la última cabecera; vacía para la raíz.
        let mut section: Vec<String> = Vec::new();
        while let Some(event) = self.take() {
            match event.kind() {
                EventKind::StdTableOpen | EventKind::ArrayTableOpen => {
                    let array = event.kind() == EventKind::ArrayTableOpen;
                    let keys = self.key()?;
                    self.expect(if array {
                        EventKind::ArrayTableClose
                    } else {
                        EventKind::StdTableClose
                    })?;
                    header(&mut root, &keys, array, event.span().start())?;
                    section = keys.into_iter().map(|k| k.name).collect();
                }
                EventKind::SimpleKey => {
                    self.next -= 1;
                    let (keys, item) = self.key_value()?;
                    dotted(section_mut(&mut root, &section)?, &keys, item)?;
                }
                _ => return Err(unexpected(&event)),
            }
        }
        Ok(root)
    }

    /// `clave = valor`, con la clave posiblemente con puntos.
    fn key_value(&mut self) -> Result<(Vec<Key>, Item), Problem> {
        let keys = self.key()?;
        self.expect(EventKind::KeyValSep)?;
        let item = self.value()?;
        Ok((keys, item))
    }

    /// Clave simple o con puntos.
    fn key(&mut self) -> Result<Vec<Key>, Problem> {
        let mut keys = vec![self.simple_key()?];
        while self.peek().is_some_and(|e| e.kind() == EventKind::KeySep) {
            self.next += 1;
            keys.push(self.simple_key()?);
        }
        Ok(keys)
    }

    fn simple_key(&mut self) -> Result<Key, Problem> {
        let event = self.expect(EventKind::SimpleKey)?;
        let raw = self.raw(&event)?;
        let mut name = std::borrow::Cow::Borrowed("");
        let mut errors: Vec<ParseError> = Vec::new();
        raw.decode_key(&mut name, &mut errors);
        if let Some(error) = errors.first() {
            return Err(syntax(error));
        }
        Ok(Key {
            name: name.into_owned(),
            offset: event.span().start(),
        })
    }

    fn raw(&self, event: &Event) -> Result<toml_parser::Raw<'_>, Problem> {
        self.source
            .get(event)
            .ok_or_else(|| Problem::new(Some(event.span().start()), "TOML inválido"))
    }

    fn value(&mut self) -> Result<Item, Problem> {
        let Some(event) = self.take() else {
            return Err(Problem::new(
                Some(self.end()),
                "TOML inválido: falta un valor",
            ));
        };
        let offset = event.span().start();
        let value = match event.kind() {
            EventKind::Scalar => self.scalar(&event)?,
            EventKind::ArrayOpen => {
                let mut items = Vec::new();
                loop {
                    match self.peek().map(Event::kind) {
                        Some(EventKind::ArrayClose) => {
                            self.next += 1;
                            break;
                        }
                        Some(EventKind::ValueSep) => self.next += 1,
                        _ => items.push(self.value()?),
                    }
                }
                Value::Array(items, ArrayKind::Static)
            }
            EventKind::InlineTableOpen => {
                let mut table = Table::new(Kind::Inline);
                loop {
                    match self.peek().map(Event::kind) {
                        Some(EventKind::InlineTableClose) => {
                            self.next += 1;
                            break;
                        }
                        Some(EventKind::ValueSep) => self.next += 1,
                        _ => {
                            let (keys, item) = self.key_value()?;
                            dotted(&mut table, &keys, item)?;
                        }
                    }
                }
                Value::Table(table)
            }
            _ => return Err(unexpected(&event)),
        };
        Ok(Item { offset, value })
    }

    fn scalar(&self, event: &Event) -> Result<Value, Problem> {
        let raw = self.raw(event)?;
        let mut decoded = std::borrow::Cow::Borrowed("");
        let mut errors: Vec<ParseError> = Vec::new();
        let kind = raw.decode_scalar(&mut decoded, &mut errors);
        if let Some(error) = errors.first() {
            return Err(syntax(error));
        }
        Ok(match kind {
            ScalarKind::String => Value::String(decoded.into_owned()),
            ScalarKind::Boolean(b) => Value::Boolean(b),
            ScalarKind::Integer(radix) => i64::from_str_radix(&decoded, radix.value())
                .map(Value::Integer)
                .map_err(|_| {
                    Problem::new(
                        Some(event.span().start()),
                        "TOML inválido: entero fuera del rango de 64 bits",
                    )
                })?,
            ScalarKind::Float => Value::Float,
            ScalarKind::DateTime => Value::Datetime,
        })
    }
}

fn unexpected(event: &Event) -> Problem {
    Problem::new(
        Some(event.span().start()),
        format!("TOML inválido: {} inesperado", event.kind().description()),
    )
}

/// Ruta de claves para los mensajes: `a.b`.
fn path(keys: &[Key]) -> String {
    keys.iter()
        .map(|k| super::shown(&k.name))
        .collect::<Vec<_>>()
        .join(".")
}

/// Tabla de la sección actual. Las cabeceras ya se validaron al abrirse,
/// así que cada paso existe y es una tabla o un array de tablas.
fn section_mut<'t>(root: &'t mut Table, section: &[String]) -> Result<&'t mut Table, Problem> {
    let mut table = root;
    for name in section {
        table = match table.get_mut(name).map(|item| &mut item.value) {
            Some(Value::Table(sub)) => sub,
            Some(Value::Array(items, ArrayKind::Tables)) => match items.last_mut() {
                Some(Item {
                    value: Value::Table(sub),
                    ..
                }) => sub,
                _ => return Err(Problem::new(None, "TOML inválido: sección perdida")),
            },
            _ => return Err(Problem::new(None, "TOML inválido: sección perdida")),
        };
    }
    Ok(table)
}

/// Define `[a.b.c]` o añade un elemento a `[[a.b.c]]`.
fn header(root: &mut Table, keys: &[Key], array: bool, offset: usize) -> Result<(), Problem> {
    let shown = path(keys);
    let mut table = root;
    let Some((last, intermediate)) = keys.split_last() else {
        return Err(Problem::new(Some(offset), "TOML inválido: cabecera vacía"));
    };
    for key in intermediate {
        if table.get(&key.name).is_none() {
            table.push(
                key,
                Item {
                    offset: key.offset,
                    value: Value::Table(Table::new(Kind::Implicit)),
                },
            );
        }
        let item = table
            .get_mut(&key.name)
            .ok_or_else(|| Problem::new(Some(key.offset), "TOML inválido"))?;
        table = match &mut item.value {
            Value::Table(sub) if sub.kind != Kind::Inline => sub,
            Value::Array(items, ArrayKind::Tables) => match items.last_mut() {
                Some(Item {
                    value: Value::Table(sub),
                    ..
                }) => sub,
                _ => return Err(Problem::new(Some(key.offset), "TOML inválido")),
            },
            _ => {
                return Err(Problem::new(
                    Some(key.offset),
                    format!(
                        "TOML inválido: «{}» de [{shown}] ya tiene un valor que no se puede ampliar",
                        super::shown(&key.name)
                    ),
                ));
            }
        };
    }
    let fresh = |kind| Item {
        offset: last.offset,
        value: Value::Table(Table::new(kind)),
    };
    match table.get_mut(&last.name) {
        None if array => {
            table.push(
                last,
                Item {
                    offset: last.offset,
                    value: Value::Array(vec![fresh(Kind::Header)], ArrayKind::Tables),
                },
            );
        }
        None => {
            table.push(last, fresh(Kind::Header));
        }
        Some(Item {
            value: Value::Array(items, ArrayKind::Tables),
            ..
        }) if array => items.push(fresh(Kind::Header)),
        Some(Item {
            value: Value::Table(sub),
            offset: defined,
        }) if !array && sub.kind == Kind::Implicit => {
            sub.kind = Kind::Header;
            *defined = last.offset;
        }
        Some(_) if array => {
            return Err(Problem::new(
                Some(offset),
                format!("TOML inválido: [[{shown}]] ya está definida y no es un array de tablas"),
            ));
        }
        Some(_) => {
            return Err(Problem::new(
                Some(offset),
                format!("TOML inválido: la tabla [{shown}] ya está definida"),
            ));
        }
    }
    Ok(())
}

/// Inserta `a.b.c = valor` en `table`.
fn dotted(table: &mut Table, keys: &[Key], item: Item) -> Result<(), Problem> {
    let Some((last, intermediate)) = keys.split_last() else {
        return Err(Problem::new(
            Some(item.offset),
            "TOML inválido: clave vacía",
        ));
    };
    let mut table = table;
    for key in intermediate {
        if table.get(&key.name).is_none() {
            table.push(
                key,
                Item {
                    offset: key.offset,
                    value: Value::Table(Table::new(Kind::Dotted)),
                },
            );
        }
        table = match table.get_mut(&key.name).map(|i| &mut i.value) {
            Some(Value::Table(sub)) if matches!(sub.kind, Kind::Dotted | Kind::Implicit) => {
                sub.kind = Kind::Dotted;
                sub
            }
            _ => {
                return Err(Problem::new(
                    Some(key.offset),
                    format!(
                        "TOML inválido: «{}» ya está definida; no se puede ampliar con claves con punto",
                        path(keys)
                    ),
                ));
            }
        };
    }
    if table.get(&last.name).is_some() {
        return Err(Problem::new(
            Some(last.offset),
            format!("TOML inválido: la clave «{}» está repetida", path(keys)),
        ));
    }
    table.push(last, item);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Table {
        parse(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
    }

    fn err(text: &str) -> Problem {
        match parse(text) {
            Ok(table) => panic!("{text:?} aceptado: {table:?}"),
            Err(problem) => problem,
        }
    }

    fn value<'t>(table: &'t Table, path: &[&str]) -> &'t Value {
        let mut value = &table.get(path[0]).expect(path[0]).value;
        for key in &path[1..] {
            let Value::Table(t) = value else {
                panic!("{key}: no es una tabla");
            };
            value = &t.get(key).expect(key).value;
        }
        value
    }

    #[test]
    fn claves_con_punto_amplian_tablas_implicitas() {
        for header in ["[a.b.c]", "[[a.b.c]]"] {
            let text = format!("{header}\n[a]\nb.d = 2\nb.e = 3\n");
            let t = ok(&text);
            assert_eq!(value(&t, &["a", "b", "d"]), &Value::Integer(2));
            assert_eq!(value(&t, &["a", "b", "e"]), &Value::Integer(3));
            // Al usarse con puntos, la tabla implícita queda definida.
            err(&format!("{text}[a.b]\nf = 4\n"));
            // No se puede atravesar una tabla explícita ni un array.
            err(&format!("{text}b.c.x = 4\n"));
        }
    }

    #[test]
    fn documento_vacio_y_comentarios() {
        assert_eq!(ok("").entries().count(), 0);
        assert_eq!(ok("# solo un comentario\n\n   \n").entries().count(), 0);
    }

    #[test]
    fn escalares() {
        let t = ok(concat!(
            "a = \"cadena\\t\\u00e9\"\n",
            "b = 'literal \\n'\n",
            "c = 42\n",
            "d = -17\n",
            "e = 0o17\n",
            "f = 0b101\n",
            "g = 1_000\n",
            "h = true\n",
            "i = 1.5\n",
            "j = 1979-05-27\n",
            "k = \"\"\"\nvarias\nlíneas\"\"\"\n",
            "l = +7\n",
        ));
        assert_eq!(value(&t, &["a"]), &Value::String("cadena\té".into()));
        assert_eq!(value(&t, &["b"]), &Value::String("literal \\n".into()));
        assert_eq!(value(&t, &["c"]), &Value::Integer(42));
        assert_eq!(value(&t, &["d"]), &Value::Integer(-17));
        assert_eq!(value(&t, &["e"]), &Value::Integer(15));
        assert_eq!(value(&t, &["f"]), &Value::Integer(5));
        assert_eq!(value(&t, &["g"]), &Value::Integer(1000));
        assert_eq!(value(&t, &["h"]), &Value::Boolean(true));
        assert_eq!(value(&t, &["i"]), &Value::Float);
        assert_eq!(value(&t, &["j"]), &Value::Datetime);
        assert_eq!(value(&t, &["k"]), &Value::String("varias\nlíneas".into()));
        assert_eq!(value(&t, &["l"]), &Value::Integer(7));
    }

    #[test]
    fn tablas_y_claves_con_punto() {
        let t = ok(concat!(
            "x.y = 1\n",
            "[a]\n",
            "b.c = 2\n",
            "[a.b.d]\n",
            "e = 3\n",
            "[f.g]\n",
            "[f]\n",
            "h = 4\n",
            "[\"con espacio\".'literal']\n",
            "i = { j = 5, k.l = 6 }\n",
        ));
        assert_eq!(value(&t, &["x", "y"]), &Value::Integer(1));
        assert_eq!(value(&t, &["a", "b", "c"]), &Value::Integer(2));
        assert_eq!(value(&t, &["a", "b", "d", "e"]), &Value::Integer(3));
        assert_eq!(value(&t, &["f", "h"]), &Value::Integer(4));
        assert!(matches!(value(&t, &["f", "g"]), Value::Table(_)));
        assert_eq!(
            value(&t, &["con espacio", "literal", "i", "k", "l"]),
            &Value::Integer(6)
        );
    }

    #[test]
    fn arrays_de_tablas() {
        let t = ok(concat!(
            "[[r]]\n",
            "a = 1\n",
            "[r.sub]\n",
            "b = 2\n",
            "[[r]]\n",
            "a = 3\n",
            "[r.sub]\n",
            "b = 4\n",
        ));
        let Value::Array(items, ArrayKind::Tables) = value(&t, &["r"]) else {
            panic!("no es un array de tablas");
        };
        assert_eq!(items.len(), 2);
        let Value::Table(second) = &items[1].value else {
            panic!();
        };
        assert_eq!(second.get("a").unwrap().value, Value::Integer(3));
        // Posición: la cabecera del segundo elemento.
        assert_eq!(items[1].offset, "[[r]]\na = 1\n[r.sub]\nb = 2\n[[".len());
    }

    #[test]
    fn arrays_estaticos_e_inline() {
        let t = ok("a = [\"x\", 'y',\n  \"z\", ]\nb = [ { c = 1 }, { c = 2 } ]\nc = []\n");
        let Value::Array(items, ArrayKind::Static) = value(&t, &["a"]) else {
            panic!();
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].value, Value::String("z".into()));
        let Value::Array(tables, ArrayKind::Static) = value(&t, &["b"]) else {
            panic!();
        };
        assert_eq!(tables.len(), 2);
        assert_eq!(
            value(&t, &["c"]),
            &Value::Array(Vec::new(), ArrayKind::Static)
        );
    }

    /// Documentos inválidos según TOML 1.1: sintaxis (de `toml_parser`) y
    /// reglas lógicas (de este módulo).
    #[test]
    fn documentos_invalidos() {
        let cases = [
            // Sintaxis y decodificación.
            "a =",
            "= 1",
            "a = 1 b = 2",
            "a = \"sin cerrar",
            "a = \"\\q\"",
            "a = 'línea\nnueva'",
            "a = 01",
            "a = -0x10",
            "a = 0xG",
            "a = 1__0",
            "a = 9223372036854775808",
            "a = tru",
            "a = [1, 2",
            "a = { b = 1",
            "[a",
            "[[a]",
            "[]",
            "a = \"\\uD800\"",
            "a = 1 # comentario con \u{7f} control",
            "a.b. = 1",
            "\"a\nb\" = 1",
            // Claves y tablas repetidas.
            "a = 1\na = 2",
            "a = 1\n\"a\" = 2",
            "[a]\n[a]",
            "[a]\nb = 1\n[a]\nc = 2",
            "[a.b]\n[a.b]",
            "a = 1\n[a]",
            "a = {}\n[a]",
            "a = { b = 1 }\na.c = 2",
            "a = { b = 1 }\n[a.c]",
            "a = { b = 1, b = 2 }",
            "a = [ 1 ]\n[[a]]",
            "[[a]]\n[a]",
            "[a]\n[[a]]",
            "a.b = 1\n[a]",
            "a.b = 1\n[a.b]",
            "a.b = 1\na = 2",
            "a.b = 1\na.b = 2",
            "a.b = 1\na.b.c = 2",
            "[a.b.c]\nz = 9\n[a]\nb.c.t = 9",
            "[a.b]\n[a]\nb.c = 1",
            "a = 1\na.b = 2",
            "[a]\nb = 1\n[a.b]",
            "[a]\nb = 1\n[a.b.c]",
            "a = [ { b = 1 } ]\n[a.c]",
        ];
        for text in cases {
            let problem = err(text);
            assert!(
                problem.message.starts_with("TOML inválido"),
                "{text:?}: {problem:?}"
            );
            if let Some(offset) = problem.offset {
                assert!(offset <= text.len(), "{text:?}: {offset}");
            }
        }
    }

    #[test]
    fn ubicacion_de_los_errores() {
        assert_eq!(err("a = 1\na = 2").offset, Some(6));
        assert_eq!(err("[x]\n[x]\n").offset, Some(4));
        let syntax = err("a = 1\nb = \"\\q\"\n");
        assert!(
            syntax.offset.is_some_and(|o| (6..=14).contains(&o)),
            "{syntax:?}"
        );
    }

    #[test]
    fn profundidad_limitada() {
        let deep = format!("a = {}1{}", "[".repeat(9), "]".repeat(9));
        assert!(err(&deep).message.starts_with("TOML inválido"));
        let ok_depth = format!("a = {}1{}", "[".repeat(8), "]".repeat(8));
        ok(&ok_depth);
    }

    #[test]
    fn los_errores_no_reproducen_valores() {
        let secret = "valor-privado-123";
        for text in [
            format!("a = \"{secret}\nb = 1"),
            format!("a = \"{secret}\"\na = \"{secret}\""),
            format!("a = [\"{secret}\""),
        ] {
            let problem = err(&text);
            assert!(!problem.message.contains(secret), "{problem:?}");
        }
    }
}
