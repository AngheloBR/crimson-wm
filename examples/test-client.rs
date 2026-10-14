//! Cliente X11 auxiliar de las pruebas de aceptación de F5 a F9 (ADR 0007 a
//! 0011). Genera lo que las herramientas instaladas no pueden: el
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
//! - `window NOMBRE [--no-delete] [--take-focus] [--pair] [--seconds N]
//!   [--fullscreen] [--state ÁTOMO] [--dialog] [--geometry X Y ANCHO ALTO]
//!   [--border N]`: crea y mapea una ventana con `WM_PROTOCOLS` (por
//!   defecto, `WM_DELETE_WINDOW`) y escribe cada mensaje `WM_PROTOCOLS`
//!   recibido con su marca de tiempo, y cada `ConfigureNotify` («sintético»
//!   o «real», con x, y, ancho y alto). Con `WM_DELETE_WINDOW` destruye sus
//!   ventanas y termina. `--pair` crea otra ventana, NOMBRE-2, en la misma
//!   conexión. Antes de mapearla: `--fullscreen` y `--state` ponen átomos en
//!   `_NET_WM_STATE`, y `--dialog`, el tipo diálogo. Desde F8:
//!   `--transient-for VENTANA|root|self` pone `WM_TRANSIENT_FOR`;
//!   `--position-hint us|p`, `WM_NORMAL_HINTS` con `USPosition` o
//!   `PPosition`; `--background RRGGBB`, el color de fondo (visual
//!   TrueColor de 24 bits, como el de Xvfb), y `--map-delay MS` espera
//!   antes de mapearla. Desde F9: `--type TIPO`, repetible, añade un tipo a
//!   `_NET_WM_WINDOW_TYPE`, en el orden dado (`DOCK` equivale a
//!   `_NET_WM_WINDOW_TYPE_DOCK`; un nombre que empieza por `_` se usa tal
//!   cual); `--dialog` equivale a `--type DIALOG`, y `--override-redirect`
//!   crea la ventana override-redirect (el WM no la gestiona).
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
    let mut states: Vec<String> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    let mut override_redirect = false;
    let mut geometry: (i16, i16, u16, u16) = (0, 0, 200, 100);
    let mut border: u16 = 0;
    let mut transient_for: Option<String> = None;
    let mut position_hint: Option<u32> = None;
    let mut background: Option<u32> = None;
    let mut map_delay = 0u64;
    let mut rest = options.iter();
    let mut next =
        |what: &str| -> Res<String> { Ok(rest.next().ok_or(format!("falta {what}"))?.to_string()) };
    loop {
        let Ok(option) = next("") else { break };
        match option.as_str() {
            "--no-delete" => delete = false,
            "--take-focus" => take_focus = true,
            "--pair" => pair = true,
            "--seconds" => seconds = next("N")?.parse()?,
            "--fullscreen" => states.push("_NET_WM_STATE_FULLSCREEN".into()),
            "--state" => states.push(next("ÁTOMO")?),
            "--dialog" => types.push("_NET_WM_WINDOW_TYPE_DIALOG".into()),
            "--type" => {
                let name = next("TIPO")?;
                types.push(if name.starts_with('_') {
                    name
                } else {
                    format!("_NET_WM_WINDOW_TYPE_{name}")
                });
            }
            "--override-redirect" => override_redirect = true,
            "--geometry" => {
                geometry = (
                    next("X")?.parse()?,
                    next("Y")?.parse()?,
                    next("ANCHO")?.parse()?,
                    next("ALTO")?.parse()?,
                )
            }
            "--border" => border = next("N")?.parse()?,
            "--transient-for" => transient_for = Some(next("VENTANA")?),
            "--position-hint" => {
                position_hint = Some(match next("us|p")?.as_str() {
                    "us" => 1,
                    "p" => 1 << 2,
                    other => return Err(format!("pista desconocida: {other}").into()),
                })
            }
            "--background" => background = Some(u32::from_str_radix(&next("RRGGBB")?, 16)?),
            "--map-delay" => map_delay = next("MS")?.parse()?,
            other => return Err(format!("opción desconocida: {other}").into()),
        }
    }
    let state_atoms = states
        .iter()
        .map(|name| atom(conn, name))
        .collect::<Res<Vec<_>>>()?;
    let net_wm_state = atom(conn, "_NET_WM_STATE")?;
    let window_type = atom(conn, "_NET_WM_WINDOW_TYPE")?;
    let type_atoms = types
        .iter()
        .map(|name| atom(conn, name))
        .collect::<Res<Vec<_>>>()?;
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
            geometry.0,
            geometry.1,
            geometry.2,
            geometry.3,
            border,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .background_pixel(background.unwrap_or(conn.setup().roots[screen].white_pixel))
                .override_redirect(u32::from(override_redirect))
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
        if !state_atoms.is_empty() {
            conn.change_property32(
                PropMode::REPLACE,
                window,
                net_wm_state,
                AtomEnum::ATOM,
                &state_atoms,
            )?;
        }
        if !type_atoms.is_empty() {
            conn.change_property32(
                PropMode::REPLACE,
                window,
                window_type,
                AtomEnum::ATOM,
                &type_atoms,
            )?;
        }
        if let Some(owner) = &transient_for {
            let owner = match owner.as_str() {
                "root" => root,
                "self" => window,
                other => parse_window(other).ok_or("VENTANA debe ser un identificador")?,
            };
            conn.change_property32(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_TRANSIENT_FOR,
                AtomEnum::WINDOW,
                &[owner],
            )?;
        }
        if let Some(flags) = position_hint {
            // WM_SIZE_HINTS: 18 valores; flags, x, y, ancho y alto.
            let mut hints = [0u32; 18];
            hints[0] = flags;
            hints[1] = u32::try_from(geometry.0).unwrap_or(0);
            hints[2] = u32::try_from(geometry.1).unwrap_or(0);
            hints[3] = u32::from(geometry.2);
            hints[4] = u32::from(geometry.3);
            conn.change_property32(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_NORMAL_HINTS,
                AtomEnum::WM_SIZE_HINTS,
                &hints,
            )?;
        }
        if map_delay > 0 {
            sync(conn)?;
            say(&format!("creada 0x{window:x}"));
            std::thread::sleep(Duration::from_millis(map_delay));
        }
        conn.map_window(window)?;
        windows.push(window);
    }
    sync(conn)?;
    say(&format!("ventana 0x{:x}", windows[0]));
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        while let Some(event) = conn.poll_for_event()? {
            if let Event::ConfigureNotify(c) = &event {
                let kind = if c.response_type & 0x80 != 0 {
                    "sintético"
                } else {
                    "real"
                };
                say(&format!(
                    "ConfigureNotify {kind} 0x{:x} {} {} {} {}",
                    c.window, c.x, c.y, c.width, c.height
                ));
                continue;
            }
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
