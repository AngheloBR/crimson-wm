//! Lanzamiento de aplicaciones (ADR 0006). Cada programa se ejecuta con su
//! argv explícito, sin shell ni expansión de ningún tipo, desde un hilo
//! propio que espera a que termine: el bucle X11 nunca se bloquea y no
//! quedan procesos zombis mientras el WM está en marcha.
//!
//! El entorno de los hijos es el del WM con tres cambios, aplicados solo al
//! `Command` (el del WM no se modifica): `DISPLAY` se fija al display
//! validado y se eliminan el token del sandbox (ADR 0003), `WINDOWID`,
//! `DESKTOP_STARTUP_ID` y las reglas del WM (`CRIMSON_RULES`, ADR 0013).
//!
//! La tabla de programas sale de la configuración (ADR 0014), ya validada;
//! sin archivo, es la de F4 (`default_programs`). La configuración solo
//! elige el argv: no hay shell, expansión, cambios de entorno, de directorio
//! ni de usuario.

use std::io;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use crate::display::Target;
use crate::rules::RULES_VAR;
use crate::safeguards::TOKEN_VAR;

/// Programa de un atajo de lanzamiento.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// Nombre para los registros. De argv solo se registra argv[0]: los
    /// argumentos podrían contener datos privados.
    pub name: String,
    /// argv[0] se busca en el PATH del WM, salvo que sea una ruta absoluta.
    pub argv: Vec<String>,
}

impl Program {
    fn new(name: &str, argv: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            argv: argv.iter().map(|a| (*a).to_string()).collect(),
        }
    }
}

// Índices de `default_programs` que usan los atajos predeterminados
// (keys.rs).
pub const TERMINAL: usize = 0;
pub const LAUNCHER: usize = 1;
pub const FILES: usize = 2;
pub const BROWSER: usize = 3;
pub const EDITOR: usize = 4;
pub const SCREENSHOT: usize = 5;

/// Tabla predeterminada, la de F4 a F11: la que se usa sin archivo de
/// configuración o si este no define `[programs]`.
pub fn default_programs() -> Vec<Program> {
    vec![
        Program::new("terminal", &["kitty"]),
        Program::new("lanzador", &["rofi", "-show", "drun"]),
        Program::new("archivos", &["thunar"]),
        Program::new("navegador", &["firefox"]),
        Program::new("editor", &["kitty", "-e", "nvim"]),
        Program::new("capturas", &["flameshot", "gui"]),
    ]
}

/// Variables que no pasan a los hijos: el token del sandbox, las que
/// describen desde dónde se lanzó el WM, no al hijo, y las reglas del WM.
const REMOVED_VARS: [&str; 4] = [TOKEN_VAR, "WINDOWID", "DESKTOP_STARTUP_ID", RULES_VAR];
/// Máximo de lanzamientos supervisados a la vez. Cuenta los hijos directos,
/// no sus descendientes.
const MAX_LIVE: usize = 64;
/// Pila de los hilos de supervisión, que solo lanzan y esperan al hijo.
const STACK_SIZE: usize = 128 * 1024;

pub struct Launcher {
    /// `:N.S` del display validado (capa 1 del ADR 0003).
    display: String,
    home: Option<PathBuf>,
    /// Programas de la configuración. Cada hilo de supervisión comparte el
    /// suyo: la tabla no cambia mientras el WM está en marcha.
    programs: Vec<Arc<Program>>,
    live: Arc<AtomicUsize>,
}

impl Launcher {
    pub fn new(target: &Target, programs: Vec<Program>) -> Self {
        Self {
            display: format!(":{}.{}", target.display, target.screen),
            home: std::env::var_os("HOME").map(PathBuf::from),
            programs: programs.into_iter().map(Arc::new).collect(),
            live: Arc::default(),
        }
    }

    /// Lanza el programa `index` de la tabla sin esperar. Nunca falla: los
    /// errores se registran y el WM sigue en marcha.
    pub fn launch(&self, index: usize) {
        match self.programs.get(index) {
            Some(program) => self.start(Arc::clone(program)),
            None => eprintln!("crimson-wm: no hay ningún programa {index}"),
        }
    }

    fn start(&self, program: Arc<Program>) {
        let Some(slot) = Slot::reserve(&self.live) else {
            eprintln!(
                "crimson-wm: no se lanza «{}»: ya hay {MAX_LIVE} programas en supervisión",
                program.name
            );
            return;
        };
        let display = self.display.clone();
        let home = self.home.clone();
        let shared = Arc::clone(&program);
        let spawned = thread::Builder::new()
            .name("crimson-launch".into())
            .stack_size(STACK_SIZE)
            .spawn(move || supervise(&shared, &display, home.as_deref(), slot));
        // Si el hilo no se crea, la reserva se libera con el cierre y no se
        // lanza nada: nunca queda un hijo sin nadie que lo espere.
        if let Err(e) = spawned {
            eprintln!(
                "crimson-wm: no se lanza «{}»: no se pudo crear el hilo de supervisión: {e}",
                program.name
            );
        }
    }
}

/// Cuerpo del hilo de supervisión: lanza el programa y lo espera.
fn supervise(program: &Program, display: &str, home: Option<&Path>, _slot: Slot) {
    let spawned = command(program, display, &working_dir(home)).and_then(|mut c| c.spawn());
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            eprintln!(
                "crimson-wm: no se pudo lanzar «{}» ({}): {e}",
                program.name,
                program.argv.first().map_or("", String::as_str)
            );
            return;
        }
    };
    let pid = child.id();
    eprintln!("crimson-wm: «{}» lanzado (pid {pid})", program.name);
    match child.wait() {
        Ok(status) => eprintln!(
            "crimson-wm: «{}» (pid {pid}) terminó {}",
            program.name,
            describe(status)
        ),
        Err(e) => eprintln!(
            "crimson-wm: «{}» (pid {pid}): no se pudo esperar su fin: {e}",
            program.name
        ),
    }
}

/// Orden del programa, sin shell. El hijo:
/// - recibe el entorno del WM sin REMOVED_VARS y con `DISPLAY` fijado;
/// - lee la entrada de /dev/null y hereda la salida y los errores del WM;
/// - tiene su propio grupo de procesos, así que las señales dirigidas al
///   grupo del WM (Ctrl+C en su terminal, `timeout`) no le llegan.
///
/// Los descriptores del WM, incluido el socket X11, tienen `O_CLOEXEC` y no
/// pasan al hijo.
fn command(program: &Program, display: &str, dir: &Path) -> io::Result<Command> {
    let Some((exe, args)) = program.argv.split_first() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "argv vacío"));
    };
    let mut command = Command::new(exe);
    command.args(args);
    for var in REMOVED_VARS {
        command.env_remove(var);
    }
    command
        .env("DISPLAY", display)
        .current_dir(dir)
        .stdin(Stdio::null())
        .process_group(0);
    Ok(command)
}

/// Directorio de trabajo de los hijos: HOME si es una ruta absoluta a un
/// directorio; si no, `/`, que siempre existe y no retiene ningún punto de
/// montaje.
fn working_dir(home: Option<&Path>) -> PathBuf {
    home.filter(|h| h.is_absolute() && h.is_dir())
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
}

fn describe(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("con el código {code}"),
        (None, Some(signal)) => format!("por la señal {signal}"),
        _ => format!("({status})"),
    }
}

/// Reserva de un lanzamiento supervisado. Se libera al destruirse: cuando el
/// hijo ha terminado y se ha recogido, o si no llegó a lanzarse.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn reserve(live: &Arc<AtomicUsize>) -> Option<Self> {
        live.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < MAX_LIVE).then_some(n + 1)
        })
        .ok()
        .map(|_| Self(Arc::clone(live)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::time::{Duration, Instant};

    use super::*;

    fn envs(command: &Command) -> Vec<(&OsStr, Option<&OsStr>)> {
        command.get_envs().collect()
    }

    #[test]
    fn la_tabla_tiene_los_seis_programas() {
        let programs = default_programs();
        let expected: [(usize, &str, &[&str]); 6] = [
            (TERMINAL, "terminal", &["kitty"]),
            (LAUNCHER, "lanzador", &["rofi", "-show", "drun"]),
            (FILES, "archivos", &["thunar"]),
            (BROWSER, "navegador", &["firefox"]),
            (EDITOR, "editor", &["kitty", "-e", "nvim"]),
            (SCREENSHOT, "capturas", &["flameshot", "gui"]),
        ];
        assert_eq!(programs.len(), expected.len());
        for (index, name, argv) in expected {
            assert_eq!(programs[index].name, name);
            assert_eq!(programs[index].argv, argv, "{name}");
        }
        // Ningún argv vacío ni con cadenas vacías; argv[0] es un nombre
        // simple, que se busca en el PATH.
        for program in &programs {
            assert!(!program.argv.is_empty(), "{}", program.name);
            assert!(program.argv.iter().all(|a| !a.is_empty()));
            assert!(!program.argv[0].contains('/'), "{}", program.name);
        }
    }

    #[test]
    fn orden_con_argv_exacto_y_sin_shell() {
        let editor = command(&default_programs()[EDITOR], ":5.0", Path::new("/")).unwrap();
        assert_eq!(editor.get_program(), "kitty");
        let args: Vec<_> = editor.get_args().collect();
        assert_eq!(args, ["-e", "nvim"]);

        // Los metacaracteres llegan literalmente como un único argumento.
        let literal = Program::new("prueba", &["echo", "$HOME; ~ *"]);
        let echo = command(&literal, ":5.0", Path::new("/")).unwrap();
        assert_eq!(echo.get_program(), "echo");
        assert_eq!(echo.get_args().collect::<Vec<_>>(), ["$HOME; ~ *"]);
    }

    #[test]
    fn entorno_del_hijo() {
        let terminal = command(&default_programs()[TERMINAL], ":7.1", Path::new("/")).unwrap();
        let envs = envs(&terminal);
        for var in [TOKEN_VAR, "WINDOWID", "DESKTOP_STARTUP_ID", RULES_VAR] {
            assert!(
                envs.contains(&(OsStr::new(var), None)),
                "{var} no se elimina"
            );
        }
        assert!(envs.contains(&(OsStr::new("DISPLAY"), Some(OsStr::new(":7.1")))));
        // XAUTHORITY, PATH, HOME y el resto se heredan sin cambios.
        assert_eq!(envs.len(), 5);
        assert_eq!(terminal.get_current_dir(), Some(Path::new("/")));
    }

    #[test]
    fn display_del_target_validado() {
        let launcher = Launcher::new(
            &Target {
                display: 12,
                screen: 0,
            },
            default_programs(),
        );
        assert_eq!(launcher.display, ":12.0");
    }

    #[test]
    fn argv_vacio_es_un_error() {
        let empty = Program::new("vacío", &[]);
        let error = command(&empty, ":5.0", Path::new("/")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn ejecutable_inexistente_no_crea_ningun_proceso() {
        let missing = Program::new("prueba", &["/nonexistent/crimson-wm-prueba"]);
        let error = command(&missing, ":5.0", Path::new("/"))
            .unwrap()
            .spawn()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn directorio_de_trabajo() {
        let dir = std::env::temp_dir();
        assert_eq!(working_dir(Some(&dir)), dir);
        let root = PathBuf::from("/");
        assert_eq!(working_dir(None), root);
        assert_eq!(working_dir(Some(Path::new(""))), root);
        assert_eq!(working_dir(Some(Path::new("relativo"))), root);
        assert_eq!(
            working_dir(Some(Path::new("/nonexistent/crimson-wm-prueba"))),
            root
        );
        // Un archivo que no es un directorio.
        let exe = std::env::current_exe().unwrap();
        assert_eq!(working_dir(Some(&exe)), root);
    }

    #[test]
    fn limite_de_lanzamientos_supervisados() {
        let live = Arc::default();
        let mut slots: Vec<_> = (0..MAX_LIVE).map_while(|_| Slot::reserve(&live)).collect();
        assert_eq!(slots.len(), MAX_LIVE);
        assert!(Slot::reserve(&live).is_none());
        slots.pop();
        assert_eq!(live.load(Ordering::Relaxed), MAX_LIVE - 1);
        slots.extend(Slot::reserve(&live));
        assert_eq!(live.load(Ordering::Relaxed), MAX_LIVE);
        assert!(Slot::reserve(&live).is_none());
        drop(slots);
        assert_eq!(live.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn describe_el_estado_de_salida() {
        assert_eq!(describe(ExitStatus::from_raw(0)), "con el código 0");
        assert_eq!(describe(ExitStatus::from_raw(3 << 8)), "con el código 3");
        assert_eq!(describe(ExitStatus::from_raw(15)), "por la señal 15");
    }

    /// Lanza un programa que termina enseguida y espera a que el hilo de
    /// supervisión lo recoja y libere su reserva.
    #[test]
    fn recoge_el_hijo_al_terminar() {
        let launcher = Launcher {
            display: ":5.0".into(),
            home: None,
            programs: vec![Arc::new(Program::new("prueba", &["/bin/true"]))],
            live: Arc::default(),
        };
        launcher.launch(0);
        launcher.launch(0);
        let deadline = Instant::now() + Duration::from_secs(5);
        while launcher.live.load(Ordering::Relaxed) != 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(launcher.live.load(Ordering::Relaxed), 0);
    }
}
