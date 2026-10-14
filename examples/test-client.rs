//! Cliente X11 auxiliar de las pruebas de aceptación de F5 y F6 (ADR 0007 y
//! 0008). Genera lo que las herramientas instaladas no pueden: el
//! `UnmapNotify` sintético de ICCCM §4.1.4, mensajes EWMH arbitrarios, la
//! propiedad de una selección y una ventana que registra los mensajes
//! `WM_PROTOCOLS` que recibe. No forma parte del WM: solo lo ejecutan los
//! scripts de tests/acceptance dentro de un Xvfb aislado.
//!
//! Uso:
//! - `withdraw VENTANA`: UnmapWindow y UnmapNotify sintético a la raíz.
//! - `synthetic-unmap VENTANA`: solo el UnmapNotify sintético.
//! - `message FORMATO VENTANA|root TIPO D0 D1 D2 D3 D4`: ClientMessage a la
//!   raíz con SubstructureRedirect|SubstructureNotify, como piden EWMH.
//! - `owner SELECCIÓN`: escribe el propietario (`0x…` o `ninguno`).
//! - `own SELECCIÓN SEGUNDOS`: toma la selección y la conserva hasta
//!   perderla o agotar el plazo.
//! - `redirect SEGUNDOS`: selecciona SubstructureRedirect en la raíz, como
//!   un gestor sin EWMH ni `WM_Sn`, y lo conserva durante el plazo.
//! - `window NOMBRE [--no-delete] [--take-focus] [--pair] [--seconds N]`:
//!   crea y mapea una ventana normal con `WM_PROTOCOLS` (por defecto,
//!   `WM_DELETE_WINDOW`) y escribe cada mensaje `WM_PROTOCOLS` recibido con
//!   su marca de tiempo. Con `WM_DELETE_WINDOW` destruye sus ventanas y
//!   termina. `--pair` crea otra ventana, NOMBRE-2, en la misma conexión.
//!
//! Solo se conecta a un DISPLAY de la forma `:N` con N distinto de 0.

use std::error::Error;
use std::io::Write as _;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _, CreateWindowAux,
    EventMask, PropMode, UNMAP_NOTIFY_EVENT, UnmapNotifyEvent, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

type Res<T> = Result<T, Box<dyn Error>>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("test-client: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Res<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let display = std::env::var("DISPLAY").unwrap_or_default();
    if !test_display(&display) {
        return Err("DISPLAY no es un servidor de prueba (:N, con N distinto de 0)".into());
    }
    let (conn, screen) = x11rb::connect(Some(&display))?;
    let root = conn.setup().roots[screen].root;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["withdraw", window] => unmap(&conn, root, window, true),
        ["synthetic-unmap", window] => unmap(&conn, root, window, false),
        ["message", format, window, type_, data @ ..] if data.len() == 5 => {
            message(&conn, root, format, window, type_, data)
        }
        ["owner", selection] => owner(&conn, selection),
        ["own", selection, seconds] => own(&conn, root, selection, seconds),
        ["redirect", seconds] => redirect(&conn, root, seconds),
        ["window", name, options @ ..] => client_window(&conn, root, screen, name, options),
        _ => Err("uso: ver la cabecera de examples/test-client.rs".into()),
    }
}

fn unmap(conn: &RustConnection, root: Window, window: &str, real: bool) -> Res<()> {
    let window = parse_window(window).ok_or("VENTANA debe ser un identificador")?;
    if real {
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
    sync(conn)
}

fn message(
    conn: &RustConnection,
    root: Window,
    format: &str,
    window: &str,
    type_: &str,
    data: &[&str],
) -> Res<()> {
    let format: u8 = format.parse()?;
    let window = if window == "root" {
        root
    } else {
        parse_window(window).ok_or("VENTANA debe ser un identificador o root")?
    };
    let type_ = atom(conn, type_)?;
    let mut values = [0u32; 5];
    for (value, text) in values.iter_mut().zip(data) {
        *value = parse_window(text).ok_or("los datos deben ser números")?;
    }
    let event = ClientMessageEvent::new(format, window, type_, values);
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    sync(conn)
}

fn owner(conn: &RustConnection, selection: &str) -> Res<()> {
    let owner = conn
        .get_selection_owner(atom(conn, selection)?)?
        .reply()?
        .owner;
    if owner == NONE {
        println!("ninguno");
    } else {
        println!("0x{owner:x}");
    }
    Ok(())
}

fn own(conn: &RustConnection, root: Window, selection: &str, seconds: &str) -> Res<()> {
    let deadline = Instant::now() + Duration::from_secs(seconds.parse()?);
    let selection = atom(conn, selection)?;
    let window = conn.generate_id()?;
    conn.create_window(
        0,
        window,
        root,
        -1,
        -1,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        0,
        &CreateWindowAux::new().override_redirect(1),
    )?;
    conn.set_selection_owner(window, selection, CURRENT_TIME)?;
    let owner = conn.get_selection_owner(selection)?.reply()?.owner;
    if owner != window {
        return Err(format!("no se pudo tomar la selección (propietario 0x{owner:x})").into());
    }
    say(&format!("propietario 0x{window:x}"));
    while Instant::now() < deadline {
        while let Some(event) = conn.poll_for_event()? {
            if let Event::SelectionClear(e) = event {
                if e.selection == selection {
                    say("selección perdida");
                    return Ok(());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn redirect(conn: &RustConnection, root: Window, seconds: &str) -> Res<()> {
    let seconds = seconds.parse()?;
    let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_REDIRECT);
    conn.change_window_attributes(root, &aux)?.check()?;
    say("SubstructureRedirect tomado");
    std::thread::sleep(Duration::from_secs(seconds));
    Ok(())
}

fn client_window(
    conn: &RustConnection,
    root: Window,
    screen: usize,
    name: &str,
    options: &[&str],
) -> Res<()> {
    let mut delete = true;
    let mut take_focus = false;
    let mut pair = false;
    let mut seconds = 60;
    let mut rest = options.iter();
    while let Some(option) = rest.next() {
        match *option {
            "--no-delete" => delete = false,
            "--take-focus" => take_focus = true,
            "--pair" => pair = true,
            "--seconds" => seconds = rest.next().ok_or("falta N")?.parse()?,
            other => return Err(format!("opción desconocida: {other}").into()),
        }
    }
    let wm_protocols = atom(conn, "WM_PROTOCOLS")?;
    let wm_delete = atom(conn, "WM_DELETE_WINDOW")?;
    let wm_take_focus = atom(conn, "WM_TAKE_FOCUS")?;
    let mut protocols = Vec::new();
    if delete {
        protocols.push(wm_delete);
    }
    if take_focus {
        protocols.push(wm_take_focus);
    }
    let visual = conn.setup().roots[screen].root_visual;
    let names = if pair {
        vec![name.to_string(), format!("{name}-2")]
    } else {
        vec![name.to_string()]
    };
    let mut windows = Vec::new();
    for title in &names {
        let window = conn.generate_id()?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            200,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .background_pixel(conn.setup().roots[screen].white_pixel)
                .event_mask(EventMask::STRUCTURE_NOTIFY),
        )?;
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        )?;
        conn.change_property32(
            PropMode::REPLACE,
            window,
            wm_protocols,
            AtomEnum::ATOM,
            &protocols,
        )?;
        conn.map_window(window)?;
        windows.push(window);
    }
    sync(conn)?;
    say(&format!("ventana 0x{:x}", windows[0]));
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        while let Some(event) = conn.poll_for_event()? {
            let Event::ClientMessage(e) = event else {
                continue;
            };
            if e.type_ != wm_protocols || e.format != 32 {
                continue;
            }
            let [protocol, time, ..] = e.data.as_data32();
            let label = if protocol == wm_delete {
                "WM_DELETE_WINDOW"
            } else if protocol == wm_take_focus {
                "WM_TAKE_FOCUS"
            } else {
                "otro"
            };
            say(&format!("{label} 0x{:x} {time}", e.window));
            if protocol == wm_delete {
                for window in &windows {
                    conn.destroy_window(*window)?;
                }
                return sync(conn);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn atom(conn: &RustConnection, name: &str) -> Res<u32> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

/// Una petición con respuesta: las anteriores ya se han procesado.
fn sync(conn: &RustConnection) -> Res<()> {
    conn.get_input_focus()?.reply()?;
    Ok(())
}

/// Una línea en la salida estándar, escrita en el acto.
fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
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
