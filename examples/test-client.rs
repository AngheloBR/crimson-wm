//! Cliente X11 auxiliar de las pruebas de aceptación de F5 (ADR 0007). Hace,
//! sobre una ventana de otro cliente, lo que haría su propietario para
//! retirarla (ICCCM §4.1.4); xdotool no puede enviar el UnmapNotify
//! sintético. No forma parte del WM: solo lo ejecutan los scripts de
//! tests/acceptance/f5 dentro de un Xvfb aislado.
//!
//! Uso: test-client withdraw|synthetic-unmap VENTANA
//! - withdraw: UnmapWindow y UnmapNotify sintético a la raíz;
//! - synthetic-unmap: solo el UnmapNotify sintético.
//!
//! Solo se conecta a un DISPLAY de la forma `:N` con N distinto de 0 y
//! termina cuando el servidor ha procesado sus peticiones.

use std::error::Error;
use std::process::ExitCode;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt as _, EventMask, UNMAP_NOTIFY_EVENT, UnmapNotifyEvent, Window,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("test-client: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mode, window] = args.as_slice() else {
        return Err("uso: test-client withdraw|synthetic-unmap VENTANA".into());
    };
    let unmap = match mode.as_str() {
        "withdraw" => true,
        "synthetic-unmap" => false,
        _ => return Err(format!("modo desconocido: {mode}").into()),
    };
    let window = parse_window(window).ok_or("VENTANA debe ser un identificador")?;
    let display = std::env::var("DISPLAY").unwrap_or_default();
    if !test_display(&display) {
        return Err("DISPLAY no es un servidor de prueba (:N, con N distinto de 0)".into());
    }

    let (conn, screen) = x11rb::connect(Some(&display))?;
    let root = conn.setup().roots[screen].root;
    if unmap {
        conn.unmap_window(window)?;
    }
    let event = UnmapNotifyEvent {
        response_type: UNMAP_NOTIFY_EVENT,
        sequence: 0,
        event: root,
        window,
        from_configure: false,
    };
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    // Una petición con respuesta: las anteriores ya se han procesado.
    conn.get_input_focus()?.reply()?;
    Ok(())
}

/// Identificador en hexadecimal (`0x…`, como xwininfo) o en decimal.
fn parse_window(text: &str) -> Option<Window> {
    match text.strip_prefix("0x") {
        Some(hex) => Window::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn test_display(display: &str) -> bool {
    display.strip_prefix(':').is_some_and(|n| {
        n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0') && !n.is_empty()
    })
}
