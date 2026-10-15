//! Configuración declarativa (ADR 0014): `crimson/wm.toml`, leído una sola
//! vez al arrancar, antes de conectar con X. Etapas, cada una sin efectos
//! sobre la siguiente si falla:
//! 1. resolución de la ruta (`search`, `load`);
//! 2. lectura acotada y segura (`read`);
//! 3. análisis TOML (`document`);
//! 4. validación semántica y construcción de `Config` (`schema`).
//!
//! La aplicación es de `main.rs` y `wm.rs`, que reciben la `Config` ya
//! completa e inmutable. Una configuración inválida nunca se aplica en
//! parte: el WM no arranca (código 15). Sin archivo, la configuración es la
//! predeterminada, idéntica al comportamiento de F11.
//!
//! La configuración no participa en las salvaguardas del ADR 0003 ni puede
//! cambiar el entorno de los hijos, sus privilegios ni el display.

mod document;
mod schema;

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags};

use crate::keys::{self, Binding};
use crate::layout::{DEFAULT_MASTER_PERCENT, LayoutKind};
use crate::rules::{self, Rule};
use crate::spawn::{self, Program};

/// Ruta relativa del archivo dentro del directorio de configuración.
pub const FILE: &str = "crimson/wm.toml";
/// Tamaño máximo del archivo, en bytes.
pub const MAX_BYTES: u64 = 64 * 1024;

/// Configuración completa y validada, con datos propios.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// `[layout] default`, aplicado al iniciar cada escritorio.
    pub default_layout: LayoutKind,
    /// `[layout] master_percent`, de 10 a 90.
    pub master_percent: u8,
    /// `[programs]`: los programas de los atajos de lanzamiento.
    pub programs: Vec<Program>,
    /// `[[keybindings]]`; `Action::Launch` indexa `programs`.
    pub bindings: Vec<Binding>,
    /// `[[rules]]`, en orden.
    pub rules: Vec<Rule>,
}

impl Default for Config {
    /// La configuración de F11: las tablas fijas de keys.rs y spawn.rs, la
    /// mitad del ancho para la maestra y ninguna regla.
    fn default() -> Self {
        Self {
            default_layout: LayoutKind::default(),
            master_percent: DEFAULT_MASTER_PERCENT,
            programs: spawn::default_programs(),
            bindings: keys::default_bindings(),
            rules: Vec::new(),
        }
    }
}

impl Config {
    /// Resumen para los registros, sin datos del archivo.
    pub fn summary(&self) -> String {
        let mut summary = format!(
            "{} atajos, {} programas, maestra al {} %, {} reglas",
            self.bindings.len(),
            self.programs.len(),
            self.master_percent,
            self.rules.len()
        );
        // Conserva exactamente el informe histórico para Master-stack.
        if self.default_layout != LayoutKind::MasterStack {
            summary.push_str(&format!(", layout {}", self.default_layout.name()));
        }
        summary
    }
}

/// Defecto del texto: posición (byte) y descripción.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub offset: Option<usize>,
    pub message: String,
}

impl Problem {
    fn new(offset: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

/// Recorta un nombre del archivo para los mensajes y escapa los caracteres
/// de control.
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

/// Línea y columna (desde 1, en caracteres) de un byte del texto.
fn line_at(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

/// Analiza y valida un texto completo. Sin E/S.
pub fn from_text(text: &str) -> Result<Config, Problem> {
    let root = document::parse(text)?;
    schema::build(&root, text)
}

/// Error de configuración: el WM no arranca (código 15).
#[derive(Debug, PartialEq, Eq)]
pub struct Error {
    pub path: Option<PathBuf>,
    /// Línea y columna, si el defecto está en el texto.
    pub position: Option<(usize, usize)>,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(f, "{}:", path.display())?;
            if let Some((line, column)) = self.position {
                write!(f, "{line}:{column}:")?;
            }
            f.write_str(" ")?;
        }
        f.write_str(&self.message)
    }
}

impl Error {
    fn at(path: &Path, message: impl Into<String>) -> Self {
        Self {
            path: Some(path.to_path_buf()),
            position: None,
            message: message.into(),
        }
    }
}

/// Dónde se busca el archivo cuando no se indica con `--config`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    /// Rutas candidatas, en orden de precedencia y sin repetir.
    pub candidates: Vec<PathBuf>,
    /// Avisos sobre variables ignoradas.
    pub notes: Vec<String>,
}

/// Candidatas según la especificación XDG: `$XDG_CONFIG_HOME/crimson/wm.toml`
/// si la variable es una ruta absoluta, y `$HOME/.config/crimson/wm.toml`
/// si HOME lo es. Una variable ausente o vacía no da candidata; una ruta
/// relativa se ignora con un aviso.
pub fn search(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Search {
    let mut candidates = Vec::new();
    let mut notes = Vec::new();
    let mut absolute = |var: &str, value: Option<OsString>| {
        let value = value.filter(|v| !v.is_empty()).map(PathBuf::from)?;
        if value.is_absolute() {
            Some(value)
        } else {
            notes.push(format!(
                "aviso: {var} no es una ruta absoluta; se ignora (especificación XDG)"
            ));
            None
        }
    };
    if let Some(dir) = absolute("XDG_CONFIG_HOME", xdg_config_home) {
        candidates.push(dir.join(FILE));
    }
    if let Some(dir) = absolute("HOME", home) {
        let path = dir.join(".config").join(FILE);
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    Search { candidates, notes }
}

/// Origen de la configuración cargada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Ningún archivo: la configuración predeterminada. Lista las rutas
    /// buscadas.
    Builtin(Vec<PathBuf>),
    File(PathBuf),
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::File(path) => write!(f, "{}", path.display()),
            Source::Builtin(searched) if searched.is_empty() => f.write_str(
                "predeterminada (ni XDG_CONFIG_HOME ni HOME dan una ruta absoluta en la que buscar)",
            ),
            Source::Builtin(searched) => {
                let paths: Vec<String> =
                    searched.iter().map(|p| p.display().to_string()).collect();
                write!(f, "predeterminada (no existe {})", paths.join(" ni "))
            }
        }
    }
}

/// Configuración cargada y avisos para el registro.
#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    pub source: Source,
    pub notes: Vec<String>,
}

/// Carga la configuración. Con `explicit` (de `--config`) solo se usa esa
/// ruta y debe existir. Sin ella se prueba cada candidata: la primera que
/// existe se usa, y si es ilegible o inválida es un error, sin pasar a la
/// siguiente. Si no existe ninguna, la predeterminada.
pub fn load(
    explicit: Option<&Path>,
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<Loaded, Error> {
    if let Some(path) = explicit {
        return match read(path) {
            Ok(text) => parse_file(path, &text, Vec::new()),
            Err(ReadError::Missing) => Err(Error::at(path, "no existe")),
            Err(ReadError::Failed(message)) => Err(Error::at(path, message)),
        };
    }
    let Search { candidates, notes } = search(xdg_config_home, home);
    for path in &candidates {
        match read(path) {
            Ok(text) => return parse_file(path, &text, notes),
            Err(ReadError::Missing) => {}
            Err(ReadError::Failed(message)) => return Err(Error::at(path, message)),
        }
    }
    Ok(Loaded {
        config: Config::default(),
        source: Source::Builtin(candidates),
        notes,
    })
}

fn parse_file(path: &Path, text: &str, notes: Vec<String>) -> Result<Loaded, Error> {
    match from_text(text) {
        Ok(config) => Ok(Loaded {
            config,
            source: Source::File(path.to_path_buf()),
            notes,
        }),
        Err(problem) => Err(Error {
            path: Some(path.to_path_buf()),
            position: problem.offset.map(|o| line_at(text, o)),
            message: problem.message,
        }),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReadError {
    /// No hay ninguna entrada con ese nombre.
    Missing,
    Failed(String),
}

fn io_failure(context: &str, error: &io::Error) -> ReadError {
    ReadError::Failed(format!("{context}: {error}"))
}

/// Lee el archivo con las comprobaciones del ADR 0014. Los enlaces
/// simbólicos se siguen y se comprueba su destino:
/// - es un archivo regular (se comprueba antes de abrirlo, para no
///   bloquearse en una FIFO, y otra vez sobre el descriptor abierto, que
///   debe ser el mismo archivo);
/// - ni el grupo ni otros pueden escribirlo: decide qué programas se
///   ejecutan con los permisos del usuario;
/// - ocupa como mucho `MAX_BYTES`, también si crece durante la lectura;
/// - es UTF-8.
///
/// Los mensajes no incluyen el contenido.
fn read(path: &Path) -> Result<String, ReadError> {
    let before = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Un enlace roto existe: no es un archivo ausente.
            return match fs::symlink_metadata(path) {
                Ok(_) => Err(ReadError::Failed(
                    "es un enlace simbólico a un destino que no existe".into(),
                )),
                Err(_) => Err(ReadError::Missing),
            };
        }
        Err(e) => return Err(io_failure("no se puede consultar", &e)),
    };
    check_type(&before)?;
    read_verified(path, &before)
}

/// Separado para probar sustituciones exactas entre metadata y open.
fn read_verified(path: &Path, before: &fs::Metadata) -> Result<String, ReadError> {
    // NONBLOCK impide que una sustitución por FIFO bloquee open; NOCTTY
    // evita adquirir un terminal de control si se sustituye por un tty.
    let file = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| io_failure("no se puede abrir", &io::Error::from(e)))?;
    let meta = file
        .metadata()
        .map_err(|e| io_failure("no se puede consultar el archivo abierto", &e))?;
    check_type(&meta)?;
    if (meta.dev(), meta.ino()) != (before.dev(), before.ino()) {
        return Err(ReadError::Failed(
            "el archivo cambió mientras se abría".into(),
        ));
    }
    let mode = meta.permissions().mode();
    if mode & 0o022 != 0 {
        return Err(ReadError::Failed(format!(
            "lo pueden modificar el grupo u otros usuarios (permisos {:04o}); corrígelo con chmod go-w",
            mode & 0o7777
        )));
    }
    if meta.len() > MAX_BYTES {
        return Err(too_large(meta.len()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_failure("no se puede leer", &e))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_large(bytes.len() as u64));
    }
    String::from_utf8(bytes).map_err(|e| {
        let valid = &e.as_bytes()[..e.utf8_error().valid_up_to()];
        let line = valid.iter().filter(|&&b| b == b'\n').count() + 1;
        ReadError::Failed(format!("no es UTF-8 válido (línea {line})"))
    })
}

fn too_large(len: u64) -> ReadError {
    ReadError::Failed(format!("ocupa {len} bytes o más (máximo {MAX_BYTES})"))
}

fn check_type(meta: &fs::Metadata) -> Result<(), ReadError> {
    let file_type = meta.file_type();
    if file_type.is_file() {
        return Ok(());
    }
    let what = if file_type.is_dir() {
        "un directorio"
    } else if file_type.is_fifo() {
        "una FIFO"
    } else if file_type.is_socket() {
        "un socket"
    } else if file_type.is_char_device() || file_type.is_block_device() {
        "un dispositivo"
    } else {
        "un tipo de archivo no admitido"
    };
    Err(ReadError::Failed(format!(
        "no es un archivo regular: es {what}"
    )))
}

/// Aplica `CRIMSON_RULES` (ADR 0013), obsoleta desde F12: si está
/// definida, sus reglas sustituyen a las del archivo, sin fusionarlas. Si
/// es inválida se descarta entera, como en F11, y el WM sigue sin reglas:
/// tampoco se aplican las del archivo. Devuelve los mensajes para el
/// registro, en orden (los de F11 primero).
pub fn apply_env_rules(config: &mut Config, var: Option<OsString>) -> Vec<String> {
    let Some(var) = var else {
        return Vec::new();
    };
    let from_file = config.rules.len();
    let mut notes = Vec::new();
    match rules::load(Some(var)) {
        Ok(list) => {
            if !list.is_empty() {
                notes.push(format!("{}: {} reglas", rules::RULES_VAR, list.len()));
            }
            config.rules = list;
        }
        Err(error) => {
            notes.push(format!(
                "{} inválida ({error}): se continúa sin reglas",
                rules::RULES_VAR
            ));
            config.rules = Vec::new();
        }
    }
    notes.push(format!(
        "aviso: {} está obsoleta; usa [[rules]] en {FILE} (ADR 0014)",
        rules::RULES_VAR
    ));
    if from_file > 0 {
        notes.push(format!(
            "aviso: {} está definida y sustituye a las {from_file} reglas del archivo de configuración, que no se aplican",
            rules::RULES_VAR
        ));
    }
    notes
}

/// Modo `--check-config`: carga y valida como al arrancar, con la misma
/// precedencia y la misma lectura, pero sin consultar DISPLAY, el token del
/// sandbox ni ningún servidor X, y sin lanzar nada ni escribir ningún
/// archivo. Devuelve las líneas del informe.
pub fn check(
    explicit: Option<&Path>,
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
    rules_var: Option<OsString>,
) -> Result<Vec<String>, Error> {
    let mut loaded = load(explicit, xdg_config_home, home)?;
    let mut lines = loaded.notes;
    lines.push(match &loaded.source {
        Source::File(path) => format!(
            "{}: configuración válida ({})",
            path.display(),
            loaded.config.summary()
        ),
        Source::Builtin(_) => format!(
            "configuración {}: válida ({})",
            loaded.source,
            loaded.config.summary()
        ),
    });
    lines.extend(apply_env_rules(&mut loaded.config, rules_var));
    Ok(lines)
}

#[cfg(test)]
mod tests;
