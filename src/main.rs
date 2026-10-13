//! Crimson WM, fase F4: mosaico master-stack (ADR 0004), foco por teclado
//! (ADR 0005) y lanzamiento de aplicaciones (ADR 0006). Solo arranca en un
//! servidor X de prueba que supere las salvaguardas del ADR 0003.

mod cli;
mod clients;
mod connect;
mod display;
mod error;
mod keys;
mod layout;
mod safeguards;
mod spawn;
mod wm;

use std::io::Write as _;
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
        cli::Command::Run => run(),
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

fn run() -> Result<(), Fatal> {
    // Capa 1, antes de abrir ninguna conexión. DISPLAY se lee una sola vez.
    let target = display::validate(std::env::var_os("DISPLAY"))?;
    let token = safeguards::sandbox_token(std::env::var_os(safeguards::TOKEN_VAR))?;

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
    safeguards::check_no_other_wm(&conn, screen.root)?;
    safeguards::take_substructure_redirect(&conn, screen.root)?;
    eprintln!(
        "crimson-wm: salvaguardas superadas; gestionando la raíz 0x{:x}",
        screen.root
    );

    wm::run(&conn, screen, &target)
}
