//! Crimson WM: mosaico master-stack (ADR 0004), foco por teclado (ADR 0005),
//! lanzamiento de aplicaciones (ADR 0006), escritorios virtuales (ADR 0007),
//! compatibilidad ICCCM/EWMH (ADR 0008 a 0012), reglas de ventanas (ADR
//! 0013) y configuración TOML (ADR 0014). Solo arranca en un servidor X de
//! prueba que supere las salvaguardas del ADR 0003.

mod cli;
mod clients;
mod config;
mod connect;
mod display;
mod error;
mod ewmh;
mod icccm;
mod keynames;
mod keys;
mod layout;
mod rules;
mod safeguards;
mod spawn;
mod wm;
mod workspaces;

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use x11rb::connection::Connection as _;

use crate::error::Fatal;

fn main() -> ExitCode {
    let result = cli::parse(std::env::args_os().skip(1)).and_then(|command| match command {
        cli::Command::Help => {
            let _ = std::io::stdout().write_all(cli::USAGE.as_bytes());
            Ok(())
        }
        cli::Command::Version => {
            let _ = writeln!(
                std::io::stdout(),
                "crimson-wm {}",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        cli::Command::Check(path) => check(path.as_deref()),
        cli::Command::Run(path) => run(path.as_deref()),
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("crimson-wm: {error}");
            if let Fatal::Usage(_) = error {
                eprint!("{}", cli::USAGE);
            }
            ExitCode::from(error.exit_code())
        }
    }
}

fn config_error(error: config::Error) -> Fatal {
    Fatal::Config(format!("configuración rechazada: {error}"))
}

/// `--check-config`: no lee DISPLAY, XAUTHORITY ni el token del sandbox, no
/// conecta con X y no lanza nada.
fn check(path: Option<&Path>) -> Result<(), Fatal> {
    let lines = config::check(
        path,
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        std::env::var_os(rules::RULES_VAR),
    )
    .map_err(config_error)?;
    let mut out = std::io::stdout().lock();
    for line in lines {
        let _ = writeln!(out, "crimson-wm: {line}");
    }
    Ok(())
}

fn run(config_path: Option<&Path>) -> Result<(), Fatal> {
    // Capa 1, antes de abrir ninguna conexión. DISPLAY se lee una sola vez.
    let target = display::validate(std::env::var_os("DISPLAY"))?;
    let token = safeguards::sandbox_token(std::env::var_os(safeguards::TOKEN_VAR))?;
    // Configuración (ADR 0014): se lee y valida entera una vez, antes de
    // conectar. Si es inválida, el WM no arranca (código 15) y el servidor
    // no se toca.
    let loaded = config::load(
        config_path,
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
    .map_err(config_error)?;
    for note in &loaded.notes {
        eprintln!("crimson-wm: {note}");
    }
    match &loaded.source {
        config::Source::File(_) => eprintln!(
            "crimson-wm: configuración: {} ({})",
            loaded.source,
            loaded.config.summary()
        ),
        config::Source::Builtin(_) => eprintln!("crimson-wm: configuración: {}", loaded.source),
    }
    // Reglas de ventanas de CRIMSON_RULES (ADR 0013, obsoleta): si está
    // definida sustituye a las del archivo. Una fuente inválida se descarta
    // entera; no detiene el WM ni relaja ninguna salvaguarda.
    let mut wm_config = loaded.config;
    for note in config::apply_env_rules(&mut wm_config, std::env::var_os(rules::RULES_VAR)) {
        eprintln!("crimson-wm: {note}");
    }

    let conn = connect::connect(&target)?;
    let setup = conn.setup();
    let screen = setup
        .roots
        .get(usize::from(target.screen))
        .ok_or_else(|| Fatal::Startup(format!("la pantalla {} no existe", target.screen)))?;
    eprintln!(
        "crimson-wm: conectado a :{}.{} (fabricante {:?}, release {}, raíz 0x{:x}, {}x{})",
        target.display,
        target.screen,
        String::from_utf8_lossy(&setup.vendor),
        setup.release_number,
        screen.root,
        screen.width_in_pixels,
        screen.height_in_pixels,
    );

    safeguards::check_sandbox_marker(&conn, screen.root, &token)?;
    safeguards::check_no_other_wm(&conn, screen.root, target.screen)?;
    safeguards::take_substructure_redirect(&conn, screen.root)?;
    eprintln!(
        "crimson-wm: salvaguardas superadas; gestionando la raíz 0x{:x}",
        screen.root
    );

    wm::run(&conn, screen, &target, wm_config)
}
