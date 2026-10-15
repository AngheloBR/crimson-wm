//! U24: configuración TOML (ADR 0014).

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};

use super::schema::{
    MAX_ARG_BYTES, MAX_ARGV, MAX_BINDINGS, MAX_MATCH_BYTES, MAX_NAME, MAX_PROGRAMS,
};
use super::*;
use crate::ewmh::WindowType;
use crate::icccm::WmClass;
use crate::keynames::{CONTROL, MOD1, MOD4, SHIFT};
use crate::keys::Action;
use crate::rules::{Actions, Matcher, Subject};

fn ok(text: &str) -> Config {
    from_text(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
}

fn err(text: &str) -> Problem {
    match from_text(text) {
        Ok(config) => panic!("{text:?} aceptado: {config:?}"),
        Err(problem) => problem,
    }
}

/// El mensaje de error contiene `fragment`.
fn rejects(text: &str, fragment: &str) {
    let problem = err(text);
    assert!(
        problem.message.contains(fragment),
        "{text:?}: «{}» no contiene «{fragment}»",
        problem.message
    );
}

/// Atajo mínimo de salida, obligatorio en una tabla explícita.
const EXIT: &str = "[[keybindings]]\nkeys = \"Mod4+Shift+Escape\"\naction = \"exit\"\n";

// U27: extensión de F12 sin cambiar sus valores ni sus treinta atajos.
#[test]
fn u27_layout_predeterminado_y_formas_toml() {
    assert_eq!(
        ok(""),
        ok("[layout]\ndefault = 'master-stack'\nmaster_percent = 50\n")
    );
    assert_eq!(
        ok("[layout]\nmaster_percent = 60\n"),
        ok("layout = { default = 'master-stack', master_percent = 60 }\n")
    );
    for text in [
        "[layout]\ndefault = 'columns'\n",
        "layout.default = 'columns'\n",
        "layout = { default = 'columns' }\n",
    ] {
        let config = ok(text);
        assert_eq!(config.default_layout, LayoutKind::Columns);
        assert_eq!(config.master_percent, 50);
        assert_eq!(config.bindings, Config::default().bindings);
        assert_eq!(config.bindings.len(), 30);
        assert!(config.summary().contains("layout columns"));
    }
}

#[test]
fn u27_layout_y_accion_invalidos() {
    for text in [
        "[layout]\ndefault = 'monocle'\n",
        "[layout]\ndefault = 'Columns'\n",
        "[layout]\ndefault = ''\n",
        "[layout]\ndefault = 1\n",
        "[layout]\ndefault = true\n",
        "[layout]\ndefault = []\n",
        "[layout]\ndefault = 'columns'\nmaster_percent = 9\n",
        "[layout]\ndefault = 'columns'\nmaster_percent = 91\n",
    ] {
        assert!(from_text(text).is_err(), "{text}");
    }
    let config = ok(&format!(
        "{EXIT}[[keybindings]]\nkeys='Mod4+space'\naction='cycle_layout'\n"
    ));
    assert_eq!(config.bindings[1].action, Action::CycleLayout);
    for extra in ["desktop=1", "program='terminal'", "layout='columns'"] {
        assert!(
            from_text(&format!(
                "{EXIT}[[keybindings]]\nkeys='Mod4+space'\naction='cycle_layout'\n{extra}\n"
            ))
            .is_err()
        );
    }
    assert!(
        !Config::default()
            .bindings
            .iter()
            .any(|b| b.action == Action::CycleLayout)
    );
    let problem = err("[layout]\ndefault = 'grid'\n");
    assert_eq!(problem.offset, Some(19));
}

// Valores predeterminados.

#[test]
fn la_predeterminada_es_la_de_f11() {
    let config = Config::default();
    assert_eq!(config.master_percent, 50);
    assert!(config.rules.is_empty());
    let programs: Vec<(&str, Vec<&str>)> = config
        .programs
        .iter()
        .map(|p| (p.name.as_str(), p.argv.iter().map(String::as_str).collect()))
        .collect();
    assert_eq!(
        programs,
        [
            ("terminal", vec!["kitty"]),
            ("lanzador", vec!["rofi", "-show", "drun"]),
            ("archivos", vec!["thunar"]),
            ("navegador", vec!["firefox"]),
            ("editor", vec!["kitty", "-e", "nvim"]),
            ("capturas", vec!["flameshot", "gui"]),
        ]
    );
    // Los 30 atajos de F11, en el orden de captura, con sus keysyms.
    let mut expected = vec![
        ("Mod4+Shift+Escape", MOD4 | SHIFT, 0xff1b, Action::Exit),
        ("Mod4+j", MOD4, 0x6a, Action::FocusNext),
        ("Mod4+k", MOD4, 0x6b, Action::FocusPrev),
        ("Mod4+Shift+Return", MOD4 | SHIFT, 0xff0d, Action::Zoom),
        ("Mod4+Return", MOD4, 0xff0d, Action::Launch(0)),
        ("Mod4+d", MOD4, 0x64, Action::Launch(1)),
        ("Mod4+e", MOD4, 0x65, Action::Launch(2)),
        ("Mod4+b", MOD4, 0x62, Action::Launch(3)),
        ("Mod4+n", MOD4, 0x6e, Action::Launch(4)),
        ("Print", 0, 0xff61, Action::Launch(5)),
    ];
    let digits = ["1", "2", "3", "4", "5", "6", "7", "8"];
    let labels: Vec<(String, String)> = digits
        .iter()
        .map(|d| (format!("Mod4+{d}"), format!("Mod4+Shift+{d}")))
        .collect();
    for (i, (view, _)) in labels.iter().enumerate() {
        expected.push((view.as_str(), MOD4, 0x31 + i as u32, Action::View(i)));
    }
    for (i, (_, move_to)) in labels.iter().enumerate() {
        expected.push((
            move_to.as_str(),
            MOD4 | SHIFT,
            0x31 + i as u32,
            Action::MoveTo(i),
        ));
    }
    expected.extend([
        ("Mod4+Tab", MOD4, 0xff09, Action::ViewPrevious),
        ("Mod4+w", MOD4, 0x77, Action::Close),
        ("Mod4+Shift+w", MOD4 | SHIFT, 0x77, Action::Kill),
        ("Mod4+f", MOD4, 0x66, Action::Fullscreen),
    ]);
    let got: Vec<_> = config
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.modifiers, b.keysym, b.action))
        .collect();
    assert_eq!(got, expected);
}

#[test]
fn el_ejemplo_documentado_es_la_predeterminada() {
    let example = include_str!("../../docs/examples/wm.toml");
    assert_eq!(ok(example), Config::default());
}

#[test]
fn tabla_implicita_valida_se_rechaza_por_el_esquema() {
    let problem = err("[layout.extra.child]\n[layout]\nextra.other = 2\n");
    assert!(
        problem.message.starts_with("clave desconocida"),
        "{problem:?}"
    );
    assert_eq!(problem.offset, Some(8));
}

#[test]
fn archivo_vacio_o_solo_comentarios() {
    assert_eq!(ok(""), Config::default());
    assert_eq!(ok("# nada\n\n"), Config::default());
    // Tablas presentes pero vacías: tampoco cambian nada.
    assert_eq!(ok("[layout]\n"), Config::default());
}

// Esquema válido.

#[test]
fn configuracion_completa() {
    let config = ok(concat!(
        "[layout]\n",
        "master_percent = 60\n",
        "\n",
        "[programs.term]\n",
        "argv = [\"/opt/bin/term\", \"--class\", \"$literal ~ *\"]\n",
        "[programs.menu]\n",
        "argv = ['menu']\n",
        "\n",
        "[[keybindings]]\n",
        "keys = \"Super+Shift+q\"\n",
        "action = \"exit\"\n",
        "[[keybindings]]\n",
        "keys = \"Alt+j\"\n",
        "action = \"focus_next\"\n",
        "[[keybindings]]\n",
        "keys = \"Ctrl+Alt+Return\"\n",
        "action = \"launch\"\n",
        "program = \"term\"\n",
        "[[keybindings]]\n",
        "keys = \"F1\"\n",
        "action = \"launch\"\n",
        "program = \"menu\"\n",
        "[[keybindings]]\n",
        "keys = \"Mod4+Shift+3\"\n",
        "action = \"move_to_desktop\"\n",
        "desktop = 3\n",
        "[[keybindings]]\n",
        "keys = \"Mod4+F8\"\n",
        "action = \"view_desktop\"\n",
        "desktop = 8\n",
        "\n",
        "[[rules]]\n",
        "class = \"Firefox\"\n",
        "desktop = 2\n",
        "[[rules]]\n",
        "instance = \"pavucontrol\"\n",
        "type = \"dialog\"\n",
        "transient = false\n",
        "floating = true\n",
        "nofocus = true\n",
        "fullscreen = false\n",
    ));
    assert_eq!(config.master_percent, 60);
    assert_eq!(
        config.programs,
        [
            Program {
                name: "term".into(),
                argv: vec![
                    "/opt/bin/term".into(),
                    "--class".into(),
                    "$literal ~ *".into()
                ],
            },
            Program {
                name: "menu".into(),
                argv: vec!["menu".into()],
            },
        ]
    );
    let got: Vec<_> = config
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.modifiers, b.keysym, b.action))
        .collect();
    assert_eq!(
        got,
        [
            ("Mod4+Shift+q", MOD4 | SHIFT, 0x71, Action::Exit),
            ("Mod1+j", MOD1, 0x6a, Action::FocusNext),
            (
                "Mod1+Control+Return",
                MOD1 | CONTROL,
                0xff0d,
                Action::Launch(0)
            ),
            ("F1", 0, 0xffbe, Action::Launch(1)),
            ("Mod4+Shift+3", MOD4 | SHIFT, 0x33, Action::MoveTo(2)),
            ("Mod4+F8", MOD4, 0xffc5, Action::View(7)),
        ]
    );
    assert_eq!(config.rules.len(), 2);
    assert_eq!(config.rules[0].line, 32, "línea de la tabla de la regla");
    assert_eq!(
        config.rules[0].matcher.class.as_deref(),
        Some(&b"Firefox"[..])
    );
    assert_eq!(config.rules[0].actions.desktop, Some(1));
    assert_eq!(
        config.rules[1].matcher,
        Matcher {
            instance: Some(b"pavucontrol".to_vec()),
            class: None,
            window_type: Some(WindowType::Dialog),
            transient: Some(false),
        }
    );
    assert_eq!(
        config.rules[1].actions,
        Actions {
            desktop: None,
            floating: true,
            nofocus: true,
            fullscreen: false,
        }
    );
}

#[test]
fn formas_toml_equivalentes() {
    // Tablas en línea, claves con punto y arrays de tablas en línea dan la
    // misma configuración que las cabeceras.
    let headers = ok(&format!(
        "[layout]\nmaster_percent = 70\n[programs.t]\nargv = [\"t\"]\n{EXIT}[[keybindings]]\nkeys = \"Mod4+t\"\naction = \"launch\"\nprogram = \"t\"\n"
    ));
    let inline = ok(concat!(
        "layout.master_percent = 70\n",
        "programs = { t = { argv = [\"t\"] } }\n",
        "keybindings = [\n",
        "  { keys = \"Mod4+Shift+Escape\", action = \"exit\" },\n",
        "  { keys = \"Mod4+t\", action = \"launch\", program = \"t\" },\n",
        "]\n",
    ));
    assert_eq!(headers, inline);
}

// Errores de sintaxis y del documento TOML.

#[test]
fn toml_invalido() {
    for text in [
        "[layout\nmaster_percent = 50",
        "master_percent = ",
        "[layout]\nmaster_percent = 50\nmaster_percent = 60",
        "[layout]\n[layout]",
        "[programs.a]\nargv = [\"a\"]\n[programs.a]\nargv = [\"b\"]",
        "keybindings = []\n[[keybindings]]",
    ] {
        let problem = err(text);
        assert!(
            problem.message.starts_with("TOML inválido"),
            "{text:?}: {problem:?}"
        );
    }
}

#[test]
fn claves_desconocidas() {
    rejects("[general]\n", "clave desconocida «general» en la raíz");
    rejects(
        "[focus]\nfollows_mouse = true\n",
        "clave desconocida «focus»",
    );
    rejects(
        "master_percent = 50\n",
        "clave desconocida «master_percent» en la raíz",
    );
    rejects(
        "[layout]\ngaps = 4\n",
        "clave desconocida «gaps» en [layout]",
    );
    rejects(
        "[programs.t]\nargv = [\"t\"]\nenv = { A = \"1\" }\n",
        "clave desconocida «env» en [programs.t]",
    );
    rejects(
        "[programs.t]\nargv = [\"t\"]\nshell = true\n",
        "clave desconocida «shell»",
    );
    rejects(
        "[programs.t]\nargv = [\"t\"]\nuser = \"root\"\n",
        "clave desconocida «user»",
    );
    rejects(
        &format!("{EXIT}repeat = true\n"),
        "clave desconocida «repeat» en [[keybindings]]",
    );
    rejects(
        "[[rules]]\nclass = \"a\"\ndesktop = 2\ntitle = \"b\"\n",
        "clave desconocida «title» en [[rules]]",
    );
    // El nombre se recorta y se escapa.
    let problem = err(&format!("\"{}\" = 1\n", "x".repeat(40)));
    assert!(
        problem.message.contains(&format!("«{}…»", "x".repeat(32))),
        "{problem:?}"
    );
    let problem = err("\"a\\u0007\" = 1\n");
    assert!(problem.message.contains("«a\\u{7}»"), "{problem:?}");
}

#[test]
fn tipos_incorrectos() {
    for (text, fragment) in [
        ("layout = 50\n", "[layout] debe ser una tabla, no un entero"),
        (
            "[layout]\nmaster_percent = \"60\"\n",
            "master_percent debe ser un entero, no una cadena",
        ),
        ("[layout]\nmaster_percent = 60.0\n", "no un decimal"),
        ("[layout]\nmaster_percent = 1979-05-27\n", "no una fecha"),
        ("[layout]\nmaster_percent = true\n", "no un booleano"),
        ("programs = []\n", "[programs] debe ser una tabla"),
        (
            "[programs]\nt = \"kitty\"\n",
            "[programs.t] debe ser una tabla",
        ),
        (
            "[programs.t]\nargv = \"kitty\"\n",
            "argv de «t» debe ser un array de cadenas",
        ),
        (
            "[programs.t]\nargv = [\"a\", 1]\n",
            "argv de «t»[1] debe ser una cadena",
        ),
        (
            "[keybindings]\nkeys = \"Mod4+j\"\n",
            "keybindings debe ser un array de tablas",
        ),
        (
            "keybindings = [\"Mod4+j\"]\n",
            "cada elemento de keybindings debe ser una tabla",
        ),
        (
            "[[keybindings]]\nkeys = 1\naction = \"exit\"\n",
            "keys debe ser una cadena",
        ),
        (
            "[[keybindings]]\nkeys = \"Mod4+q\"\naction = [\"exit\"]\n",
            "action debe ser una cadena",
        ),
        (
            "[[keybindings]]\nkeys = \"Mod4+1\"\naction = \"view_desktop\"\ndesktop = \"1\"\n",
            "desktop debe ser un entero",
        ),
        (
            "[rules]\nclass = \"a\"\n",
            "rules debe ser un array de tablas",
        ),
        (
            "[[rules]]\nclass = \"a\"\nfloating = \"yes\"\n",
            "floating debe ser un booleano",
        ),
        (
            "[[rules]]\nclass = \"a\"\ndesktop = 2\ntransient = \"no\"\n",
            "transient debe ser un booleano",
        ),
        (
            "[[rules]]\nclass = 1\ndesktop = 2\n",
            "class debe ser una cadena",
        ),
    ] {
        rejects(text, fragment);
    }
}

// Límites exactos.

#[test]
fn master_percent_en_sus_limites() {
    assert_eq!(ok("[layout]\nmaster_percent = 10\n").master_percent, 10);
    assert_eq!(ok("[layout]\nmaster_percent = 90\n").master_percent, 90);
    for bad in [9, 91, 0, -50, 100, 256, i64::MAX] {
        rejects(
            &format!("[layout]\nmaster_percent = {bad}\n"),
            &format!("master_percent = {bad} fuera de 10–90"),
        );
    }
}

fn programs_text(count: usize) -> String {
    let mut text = String::new();
    for i in 0..count {
        text.push_str(&format!("[programs.p{i}]\nargv = [\"p\"]\n"));
    }
    text
}

#[test]
fn limite_de_programas() {
    let text = format!("{}{EXIT}", programs_text(MAX_PROGRAMS));
    assert_eq!(ok(&text).programs.len(), MAX_PROGRAMS);
    rejects(
        &format!("{}{EXIT}", programs_text(MAX_PROGRAMS + 1)),
        "65 programas (máximo 64)",
    );
}

fn bindings_text(count: usize) -> String {
    // F1–F12 con todas las combinaciones de modificadores: 12 × 16.
    let modifiers = ["", "Control+", "Mod1+", "Control+Mod1+"];
    let mut text = String::from(EXIT);
    let mut n = 1;
    'outer: for m4 in ["", "Mod4+"] {
        for a in modifiers {
            for b in ["", "Shift+"] {
                for f in 1..=12 {
                    if n == count {
                        break 'outer;
                    }
                    text.push_str(&format!(
                        "[[keybindings]]\nkeys = \"{m4}{a}{b}F{f}\"\naction = \"zoom\"\n"
                    ));
                    n += 1;
                }
            }
        }
    }
    assert_eq!(n, count, "no hay bastantes combinaciones distintas");
    text
}

#[test]
fn limite_de_atajos() {
    assert_eq!(
        ok(&bindings_text(MAX_BINDINGS)).bindings.len(),
        MAX_BINDINGS
    );
    rejects(
        &bindings_text(MAX_BINDINGS + 1),
        "129 elementos (máximo 128)",
    );
}

#[test]
fn limites_de_argv() {
    let argv = |n: usize| {
        let items: Vec<String> = (0..n).map(|_| "\"a\"".to_string()).collect();
        format!("[programs.t]\nargv = [{}]\n{EXIT}", items.join(", "))
    };
    assert_eq!(ok(&argv(MAX_ARGV)).programs[0].argv.len(), MAX_ARGV);
    rejects(&argv(MAX_ARGV + 1), "33 elementos (de 1 a 32)");
    rejects(&argv(0), "0 elementos (de 1 a 32)");

    let arg = |n: usize| {
        format!(
            "[programs.t]\nargv = [\"t\", \"{}\"]\n{EXIT}",
            "x".repeat(n)
        )
    };
    assert_eq!(
        ok(&arg(MAX_ARG_BYTES)).programs[0].argv[1].len(),
        MAX_ARG_BYTES
    );
    rejects(&arg(MAX_ARG_BYTES + 1), "ocupa 4097 bytes (máximo 4096)");

    let name = |n: usize| format!("[programs.{}]\nargv = [\"t\"]\n{EXIT}", "p".repeat(n));
    assert_eq!(ok(&name(MAX_NAME)).programs[0].name.len(), MAX_NAME);
    rejects(&name(MAX_NAME + 1), "nombre de programa");
}

fn rules_text(count: usize) -> String {
    let mut text = String::new();
    for i in 0..count {
        text.push_str(&format!("[[rules]]\nclass = \"c{i}\"\nfloating = true\n"));
    }
    text
}

#[test]
fn limites_de_reglas() {
    assert_eq!(
        ok(&rules_text(rules::MAX_RULES)).rules.len(),
        rules::MAX_RULES
    );
    rejects(
        &rules_text(rules::MAX_RULES + 1),
        "65 elementos (máximo 64)",
    );
    let class = |n: usize| {
        format!(
            "[[rules]]\nclass = \"{}\"\nfloating = true\n",
            "c".repeat(n)
        )
    };
    assert_eq!(
        ok(&class(MAX_MATCH_BYTES)).rules[0]
            .matcher
            .class
            .as_ref()
            .unwrap()
            .len(),
        MAX_MATCH_BYTES
    );
    rejects(
        &class(MAX_MATCH_BYTES + 1),
        "class debe tener de 1 a 256 bytes",
    );
    rejects(&class(0), "class debe tener de 1 a 256 bytes");
}

// Atajos.

#[test]
fn atajos_repetidos_tras_normalizar() {
    for (a, b) in [
        ("Mod4+Shift+j", "Shift+Mod4+j"),
        ("Super+j", "Mod4+j"),
        ("Alt+Tab", "Mod1+Tab"),
        ("Ctrl+F1", "Control+F1"),
        ("Mod4+Page_Up", "Mod4+Prior"),
    ] {
        let text = format!(
            "{EXIT}[[keybindings]]\nkeys = \"{a}\"\naction = \"zoom\"\n[[keybindings]]\nkeys = \"{b}\"\naction = \"close\"\n"
        );
        let problem = err(&text);
        assert!(
            problem
                .message
                .contains("está repetido (ya definido en la línea 4)"),
            "{a} / {b}: {problem:?}"
        );
        assert_eq!(line_at(&text, problem.offset.unwrap()).0, 7, "{a} / {b}");
    }
    // Con otros modificadores o con otra tecla no hay conflicto.
    ok(&format!(
        "{EXIT}[[keybindings]]\nkeys = \"Mod4+j\"\naction = \"zoom\"\n[[keybindings]]\nkeys = \"Mod4+Shift+j\"\naction = \"close\"\n"
    ));
}

#[test]
fn textos_de_atajo_invalidos() {
    for (keys, fragment) in [
        ("Mod4+J", "las letras se escriben en minúscula"),
        ("Mod4+foo", "tecla desconocida «foo»"),
        ("Hyper+j", "modificador «Hyper» no admitido"),
        ("Lock+Mod4+j", "modificador «Lock» no admitido"),
        ("Mod2+Mod4+j", "modificador «Mod2» no admitido"),
        ("Mod4+Mod4+j", "modificador «Mod4» repetido"),
        ("j", "«j» es una tecla de escritura"),
        ("Shift+Return", "«Return» es una tecla de escritura"),
        ("Mod4 + j", "no admite espacios"),
        ("", "atajo vacío"),
        ("Mod4+", "parte vacía"),
    ] {
        rejects(
            &format!("[[keybindings]]\nkeys = \"{keys}\"\naction = \"exit\"\n"),
            fragment,
        );
    }
}

#[test]
fn acciones_y_parametros() {
    let one = |body: &str| format!("{EXIT}[[keybindings]]\nkeys = \"Mod4+x\"\n{body}");
    for (body, fragment) in [
        (
            "action = \"launch\"\n",
            "la acción launch necesita «program»",
        ),
        (
            "action = \"launch\"\nprogram = \"nada\"\n",
            "el programa «nada» no está definido",
        ),
        (
            "action = \"view_desktop\"\n",
            "view_desktop necesita «desktop»",
        ),
        (
            "action = \"move_to_desktop\"\n",
            "move_to_desktop necesita «desktop»",
        ),
        (
            "action = \"view_desktop\"\ndesktop = 0\n",
            "desktop = 0 fuera de 1–8",
        ),
        (
            "action = \"view_desktop\"\ndesktop = 9\n",
            "desktop = 9 fuera de 1–8",
        ),
        (
            "action = \"focus_next\"\nprogram = \"terminal\"\n",
            "«program» no se usa con la acción focus_next",
        ),
        (
            "action = \"close\"\ndesktop = 2\n",
            "«desktop» no se usa con la acción close",
        ),
        (
            "action = \"launch\"\nprogram = \"terminal\"\ndesktop = 2\n",
            "«desktop» no se usa con la acción launch",
        ),
        (
            "action = \"view_desktop\"\ndesktop = 2\nprogram = \"terminal\"\n",
            "«program» no se usa",
        ),
        ("action = \"spawn\"\n", "acción desconocida «spawn»"),
        ("action = \"Exit\"\n", "acción desconocida «Exit»"),
        (
            "keys2 = 1\naction = \"exit\"\n",
            "clave desconocida «keys2»",
        ),
        ("", "falta «action»"),
    ] {
        rejects(&one(body), fragment);
    }
    rejects("[[keybindings]]\naction = \"exit\"\n", "falta «keys»");
    // Límites exactos del escritorio.
    for (desk, index) in [(1, 0), (8, 7)] {
        let config = ok(&one(&format!(
            "action = \"move_to_desktop\"\ndesktop = {desk}\n"
        )));
        assert_eq!(config.bindings[1].action, Action::MoveTo(index));
    }
    // Todas las acciones sin parámetros.
    for (name, action) in [
        ("exit", Action::Exit),
        ("focus_next", Action::FocusNext),
        ("focus_prev", Action::FocusPrev),
        ("zoom", Action::Zoom),
        ("view_previous", Action::ViewPrevious),
        ("close", Action::Close),
        ("kill", Action::Kill),
        ("fullscreen", Action::Fullscreen),
    ] {
        let config = ok(&one(&format!("action = \"{name}\"\n")));
        assert_eq!(config.bindings[1].action, action, "{name}");
    }
}

#[test]
fn falta_el_atajo_de_salida() {
    rejects(
        "[[keybindings]]\nkeys = \"Mod4+j\"\naction = \"focus_next\"\n",
        "no tiene ningún atajo con action = \"exit\"",
    );
    rejects(
        "keybindings = []\n",
        "no tiene ningún atajo con action = \"exit\"",
    );
    // Puede haber más de uno y con cualquier tecla.
    let config = ok(concat!(
        "[[keybindings]]\nkeys = \"Mod1+F4\"\naction = \"exit\"\n",
        "[[keybindings]]\nkeys = \"Control+Mod1+BackSpace\"\naction = \"exit\"\n",
    ));
    assert_eq!(config.bindings.len(), 2);
}

#[test]
fn keybindings_sustituye_la_tabla_completa() {
    let config = ok(EXIT);
    assert_eq!(config.bindings.len(), 1);
    // Los programas predeterminados siguen disponibles para launch.
    let config = ok(&format!(
        "{EXIT}[[keybindings]]\nkeys = \"Mod4+t\"\naction = \"launch\"\nprogram = \"capturas\"\n"
    ));
    assert_eq!(config.bindings[1].action, Action::Launch(5));
    assert_eq!(config.programs, spawn::default_programs());
}

// Programas.

#[test]
fn programs_sustituye_la_tabla_completa() {
    // Sin [[keybindings]], los atajos predeterminados buscan sus programas
    // por nombre en la tabla nueva.
    let config = ok(concat!(
        "[programs.capturas]\nargv = [\"shot\"]\n",
        "[programs.editor]\nargv = [\"ed\"]\n",
        "[programs.navegador]\nargv = [\"web\"]\n",
        "[programs.archivos]\nargv = [\"files\"]\n",
        "[programs.lanzador]\nargv = [\"menu\"]\n",
        "[programs.terminal]\nargv = [\"/usr/bin/xterm\"]\n",
    ));
    let launch: Vec<(&str, &str)> = config
        .bindings
        .iter()
        .filter_map(|b| match b.action {
            Action::Launch(i) => Some((b.name.as_str(), config.programs[i].argv[0].as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        launch,
        [
            ("Mod4+Return", "/usr/bin/xterm"),
            ("Mod4+d", "menu"),
            ("Mod4+e", "files"),
            ("Mod4+b", "web"),
            ("Mod4+n", "ed"),
            ("Print", "shot"),
        ]
    );
    assert_eq!(config.bindings.len(), 30);
    // Si falta alguno de los que usan los atajos predeterminados, no se
    // completa con el predeterminado: es un error.
    rejects(
        "[programs.terminal]\nargv = [\"xterm\"]\n",
        "[programs] sustituye a la tabla predeterminada y no define «lanzador», que usa el atajo predeterminado Mod4+d",
    );
}

#[test]
fn programas_invalidos() {
    for (text, fragment) in [
        ("[programs.t]\n", "falta «argv» en [programs.t]"),
        ("[programs.t]\nargv = []\n", "0 elementos"),
        ("[programs.t]\nargv = [\"\"]\n", "argv de «t»[0] está vacío"),
        ("[programs.t]\nargv = [\"~/bin/t\"]\n", "empieza por «~»"),
        (
            "[programs.t]\nargv = [\"$TERMINAL\"]\n",
            "no se expanden variables",
        ),
        ("[programs.t]\nargv = [\"bin/t\"]\n", "es una ruta relativa"),
        ("[programs.t]\nargv = [\"./t\"]\n", "es una ruta relativa"),
        (
            "[programs.t]\nargv = [\"t\", \"a\\u0000b\"]\n",
            "argv de «t»[1] contiene un carácter NUL",
        ),
        (
            "[programs.Term]\nargv = [\"t\"]\n",
            "nombre de programa «Term» inválido",
        ),
        (
            "[programs.1t]\nargv = [\"t\"]\n",
            "nombre de programa «1t» inválido",
        ),
        (
            "[programs.\"a b\"]\nargv = [\"t\"]\n",
            "nombre de programa «a b» inválido",
        ),
        (
            "[programs.\"\"]\nargv = [\"t\"]\n",
            "nombre de programa «» inválido",
        ),
    ] {
        rejects(&format!("{text}{EXIT}"), fragment);
    }
    // Argumentos vacíos o con metacaracteres: se pasan literalmente.
    let config = ok(&format!(
        "[programs.t]\nargv = [\"t\", \"\", \"$HOME\", \"~\", \"a;b|c\"]\n{EXIT}"
    ));
    assert_eq!(config.programs[0].argv, ["t", "", "$HOME", "~", "a;b|c"]);
}

#[test]
fn los_errores_no_reproducen_argv_ni_reglas() {
    let secret = "token-secreto-123";
    for text in [
        format!("[programs.t]\nargv = [\"~/{secret}\"]\n{EXIT}"),
        format!("[programs.t]\nargv = [\"{secret}/x\"]\n{EXIT}"),
        format!("[programs.t]\nargv = [\"t\", \"{secret}\\u0000\"]\n{EXIT}"),
        format!(
            "[programs.t]\nargv = [\"t\", \"{}{secret}\"]\n{EXIT}",
            "x".repeat(4096)
        ),
        format!("[[rules]]\nclass = \"{secret}\"\n"),
        format!(
            "[[rules]]\nclass = \"{secret}{}\"\nfloating = true\n",
            "x".repeat(300)
        ),
        format!("[[rules]]\ninstance = \"{secret}\"\ndesktop = 9\n"),
    ] {
        let problem = err(&text);
        assert!(!problem.message.contains(secret), "{problem:?}");
    }
}

// Reglas.

fn wm_class(instance: &str, class: &str) -> WmClass {
    WmClass {
        instance: instance.as_bytes().to_vec(),
        class: class.as_bytes().to_vec(),
    }
}

#[test]
fn reglas_equivalentes_a_crimson_rules() {
    let env = concat!(
        "class=Firefox; desktop=2\n",
        "instance=pavucontrol; floating; nofocus\n",
        "type=dialog; transient=yes; desktop=5\n",
        "class=Chat; desktop=3; fullscreen\n",
        "class=Firefox; desktop=4; floating\n",
    );
    let toml = concat!(
        "[[rules]]\nclass = \"Firefox\"\ndesktop = 2\n",
        "[[rules]]\ninstance = \"pavucontrol\"\nfloating = true\nnofocus = true\n",
        "[[rules]]\ntype = \"dialog\"\ntransient = true\ndesktop = 5\n",
        "[[rules]]\nclass = \"Chat\"\ndesktop = 3\nfullscreen = true\n",
        "[[rules]]\nclass = \"Firefox\"\ndesktop = 4\nfloating = true\n",
    );
    let from_env = rules::parse(env).unwrap();
    let from_toml = ok(toml).rules;
    assert_eq!(from_env.len(), from_toml.len());
    for (a, b) in from_env.iter().zip(&from_toml) {
        assert_eq!((&a.matcher, a.actions), (&b.matcher, b.actions));
    }
    // Las líneas son las de cada fuente.
    assert_eq!(
        from_toml.iter().map(|r| r.line).collect::<Vec<_>>(),
        [1, 4, 8, 12, 16]
    );
    // La misma decisión para las mismas ventanas (salvo las líneas).
    let classes = [
        wm_class("navigator", "Firefox"),
        wm_class("pavucontrol", "Pavucontrol"),
        wm_class("chat", "Chat"),
        wm_class("x", "firefox"),
    ];
    for class in &classes {
        for window_type in [WindowType::Normal, WindowType::Dialog] {
            for transient in [false, true] {
                let subject = Subject {
                    wm_class: Some(class),
                    window_type,
                    transient,
                };
                let a = rules::evaluate(&from_env, &subject);
                let b = rules::evaluate(&from_toml, &subject);
                let line = |l: usize| from_env.iter().position(|r| r.line == l);
                let line_b = |l: usize| from_toml.iter().position(|r| r.line == l);
                assert_eq!(
                    a.desktop.map(|(d, l)| (d, line(l))),
                    b.desktop.map(|(d, l)| (d, line_b(l)))
                );
                assert_eq!(a.floating.map(line), b.floating.map(line_b));
                assert_eq!(a.nofocus.map(line), b.nofocus.map(line_b));
                assert_eq!(a.fullscreen.map(line), b.fullscreen.map(line_b));
                assert_eq!(a.matched.len(), b.matched.len());
                assert_eq!(a.ignored.len(), b.ignored.len());
            }
        }
    }
}

#[test]
fn reglas_invalidas() {
    for (text, fragment) in [
        ("[[rules]]\ndesktop = 2\n", "regla sin condiciones"),
        ("[[rules]]\nclass = \"a\"\n", "regla sin acciones"),
        (
            "[[rules]]\nclass = \"a\"\nfloating = false\n",
            "regla sin acciones",
        ),
        (
            "[[rules]]\ntype = \"dock\"\nfloating = true\n",
            "type «dock» es una ventana fija",
        ),
        (
            "[[rules]]\ntype = \"desktop\"\nfloating = true\n",
            "es una ventana fija",
        ),
        (
            "[[rules]]\ntype = \"notification\"\nfloating = true\n",
            "es una ventana fija",
        ),
        (
            "[[rules]]\ntype = \"Dialog\"\nfloating = true\n",
            "type «Dialog» es un tipo desconocido",
        ),
        (
            "[[rules]]\nclass = \"a\"\ndesktop = 9\n",
            "desktop = 9 fuera de 1–8",
        ),
        (
            "[[rules]]\nclass = \"a\"\ndesktop = 0\n",
            "desktop = 0 fuera de 1–8",
        ),
        ("[[rules]]\nclass = \"a\\u0000\"\ndesktop = 1\n", "sin NUL"),
    ] {
        rejects(text, fragment);
    }
    // Una regla inválida rechaza el archivo entero.
    let text = format!("{}[[rules]]\nclass = \"a\"\n", rules_text(3));
    rejects(&text, "regla sin acciones");
}

#[test]
fn precedencia_de_crimson_rules() {
    let file_rules = ok(&rules_text(2));

    // Sin la variable: las del archivo.
    let mut config = file_rules.clone();
    assert!(apply_env_rules(&mut config, None).is_empty());
    assert_eq!(config.rules.len(), 2);

    // Válida: la sustituye entera, con aviso de obsolescencia.
    let mut config = file_rules.clone();
    let notes = apply_env_rules(&mut config, Some("class=X; desktop=4".into()));
    assert_eq!(config.rules.len(), 1);
    assert_eq!(config.rules[0].matcher.class.as_deref(), Some(&b"X"[..]));
    assert_eq!(
        notes,
        [
            "CRIMSON_RULES: 1 reglas",
            "aviso: CRIMSON_RULES está obsoleta; usa [[rules]] en crimson/wm.toml (ADR 0014)",
            "aviso: CRIMSON_RULES está definida y sustituye a las 2 reglas del archivo de configuración, que no se aplican",
        ]
    );

    // Vacía: también la sustituye (ninguna regla).
    let mut config = file_rules.clone();
    let notes = apply_env_rules(&mut config, Some("".into()));
    assert!(config.rules.is_empty());
    assert_eq!(notes.len(), 2);

    // Inválida: como en F11, sin reglas; tampoco las del archivo.
    let mut config = file_rules.clone();
    let notes = apply_env_rules(&mut config, Some("class=X; desktop=9".into()));
    assert!(config.rules.is_empty());
    assert_eq!(
        notes[0],
        "CRIMSON_RULES inválida (línea 1: escritorio «9» fuera de 1–8): se continúa sin reglas"
    );
    assert!(notes[2].contains("sustituye a las 2 reglas"));

    // Sin reglas en el archivo: solo los mensajes de F11 y la obsolescencia.
    let mut config = Config::default();
    let notes = apply_env_rules(&mut config, Some("class=X; floating".into()));
    assert_eq!(notes.len(), 2);
    // Una variable no UTF-8 se rechaza como en F11.
    use std::os::unix::ffi::OsStringExt as _;
    let mut config = Config::default();
    let notes = apply_env_rules(&mut config, Some(OsString::from_vec(vec![0xff])));
    assert_eq!(
        notes[0],
        "CRIMSON_RULES inválida (no es UTF-8): se continúa sin reglas"
    );
}

#[test]
fn las_reglas_del_archivo_se_validan_aunque_se_sustituyan() {
    // La validación es del archivo completo, antes de mirar CRIMSON_RULES:
    // un archivo con una regla inválida no arranca aunque la variable esté
    // definida (la carga falla antes de apply_env_rules).
    let dir = TempDir::new("reglas-sustituidas");
    let path = dir.write("wm.toml", "[[rules]]\nclass = \"a\"\n", 0o600);
    let error = check(Some(&path), None, None, Some("class=b; floating".into())).unwrap_err();
    assert!(error.message.contains("regla sin acciones"), "{error}");
}

// Posiciones y mensajes.

#[test]
fn linea_y_columna() {
    assert_eq!(line_at("", 0), (1, 1));
    assert_eq!(line_at("abc", 2), (1, 3));
    assert_eq!(line_at("a\nbc", 3), (2, 2));
    assert_eq!(line_at("a\nbc", 99), (2, 3));
    // Columnas en caracteres, también dentro de uno de varios bytes.
    assert_eq!(line_at("ñañ = 1", 4), (1, 3));
    assert_eq!(line_at("ñ", 1), (1, 1));
}

#[test]
fn errores_con_archivo_linea_y_columna() {
    let dir = TempDir::new("posicion");
    let path = dir.write("wm.toml", "[layout]\nmaster_percent = 95\n", 0o600);
    let error = load(Some(&path), None, None).unwrap_err();
    assert_eq!(error.position, Some((2, 18)));
    assert_eq!(
        error.to_string(),
        format!(
            "{}:2:18: master_percent = 95 fuera de 10–90",
            path.display()
        )
    );
    let path = dir.write("dup.toml", "a = 1\n\n  a = 2\n", 0o600);
    let error = load(Some(&path), None, None).unwrap_err();
    assert_eq!(error.position, Some((3, 3)));
    // Una clave desconocida se señala en la clave, no en su valor.
    let path = dir.write("clave.toml", "[layout]\n  gaps = 5\n", 0o600);
    let error = load(Some(&path), None, None).unwrap_err();
    assert_eq!(error.position, Some((2, 3)));
    let path = dir.write(
        "nombre.toml",
        "[programs]\nMal = { argv = [\"t\"] }\n",
        0o600,
    );
    let error = load(Some(&path), None, None).unwrap_err();
    assert_eq!(error.position, Some((2, 1)));
}

// Rutas.

fn os(s: &str) -> Option<OsString> {
    Some(OsString::from(s))
}

#[test]
fn rutas_xdg_y_home() {
    let s = search(os("/x"), os("/h"));
    assert_eq!(
        s.candidates,
        [
            PathBuf::from("/x/crimson/wm.toml"),
            PathBuf::from("/h/.config/crimson/wm.toml")
        ]
    );
    assert!(s.notes.is_empty());
    // XDG_CONFIG_HOME ausente o vacía: solo HOME, sin aviso.
    for xdg in [None, os("")] {
        let s = search(xdg, os("/h"));
        assert_eq!(s.candidates, [PathBuf::from("/h/.config/crimson/wm.toml")]);
        assert!(s.notes.is_empty());
    }
    // Relativa: se ignora con un aviso.
    let s = search(os("config"), os("/h"));
    assert_eq!(s.candidates, [PathBuf::from("/h/.config/crimson/wm.toml")]);
    assert_eq!(
        s.notes,
        ["aviso: XDG_CONFIG_HOME no es una ruta absoluta; se ignora (especificación XDG)"]
    );
    // HOME ausente, vacía o relativa.
    assert_eq!(
        search(os("/x"), None).candidates,
        [PathBuf::from("/x/crimson/wm.toml")]
    );
    assert_eq!(search(os("/x"), os("")).candidates.len(), 1);
    let s = search(None, os("h"));
    assert!(s.candidates.is_empty());
    assert_eq!(s.notes.len(), 1);
    assert!(search(None, None).candidates.is_empty());
    // Si coinciden, una sola candidata.
    assert_eq!(search(os("/h/.config"), os("/h")).candidates.len(), 1);
}

/// Directorio temporal propio de una prueba; se borra al terminar.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("crimson-wm-u24-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, text: &str, mode: u32) -> PathBuf {
        self.write_bytes(relative, text.as_bytes(), mode)
    }

    fn write_bytes(&self, relative: &str, bytes: &[u8], mode: u32) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Devuelve los permisos de lectura antes de borrar.
        if let Ok(entries) = fs::read_dir(&self.0) {
            for entry in entries.flatten() {
                let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o700));
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn precedencia_de_rutas() {
    let dir = TempDir::new("precedencia");
    let xdg = dir.path().join("xdg");
    let home = dir.path().join("home");
    let xdg_os = Some(xdg.clone().into_os_string());
    let home_os = Some(home.clone().into_os_string());

    // Ningún archivo: predeterminada, con las rutas buscadas.
    let loaded = load(None, xdg_os.clone(), home_os.clone()).unwrap();
    assert_eq!(loaded.config, Config::default());
    assert_eq!(
        loaded.source,
        Source::Builtin(vec![
            xdg.join("crimson/wm.toml"),
            home.join(".config/crimson/wm.toml")
        ])
    );
    assert!(!xdg.exists(), "no se crea nada");

    // Solo en HOME.
    let in_home = dir.write(
        "home/.config/crimson/wm.toml",
        "[layout]\nmaster_percent = 30\n",
        0o600,
    );
    let loaded = load(None, xdg_os.clone(), home_os.clone()).unwrap();
    assert_eq!(loaded.source, Source::File(in_home.clone()));
    assert_eq!(loaded.config.master_percent, 30);

    // XDG gana a HOME.
    let in_xdg = dir.write(
        "xdg/crimson/wm.toml",
        "[layout]\nmaster_percent = 40\n",
        0o600,
    );
    let loaded = load(None, xdg_os.clone(), home_os.clone()).unwrap();
    assert_eq!(loaded.source, Source::File(in_xdg.clone()));
    assert_eq!(loaded.config.master_percent, 40);

    // --config gana a todo.
    let explicit = dir.write("otro.toml", "[layout]\nmaster_percent = 20\n", 0o600);
    let loaded = load(Some(&explicit), xdg_os.clone(), home_os.clone()).unwrap();
    assert_eq!(loaded.source, Source::File(explicit.clone()));
    assert_eq!(loaded.config.master_percent, 20);

    // XDG existe pero es inválido: error, sin pasar a HOME.
    dir.write(
        "xdg/crimson/wm.toml",
        "[layout]\nmaster_percent = 5\n",
        0o600,
    );
    let error = load(None, xdg_os.clone(), home_os.clone()).unwrap_err();
    assert_eq!(error.path.as_deref(), Some(in_xdg.as_path()));

    // XDG existe pero es ilegible por sus permisos: tampoco pasa a HOME.
    dir.write("xdg/crimson/wm.toml", "", 0o622);
    let error = load(None, xdg_os.clone(), home_os.clone()).unwrap_err();
    assert!(error.message.contains("grupo u otros"), "{error}");

    // --config que no existe: error, aunque haya otros archivos.
    let missing = dir.path().join("no-existe.toml");
    let error = load(Some(&missing), xdg_os, home_os).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{}: no existe", missing.display())
    );
}

#[test]
fn lectura_segura() {
    let dir = TempDir::new("lectura");
    let read_error = |path: &Path| match read(path) {
        Err(ReadError::Failed(message)) => message,
        other => panic!("{}: {other:?}", path.display()),
    };

    // Permisos: el propietario puede escribir; el grupo y otros, no.
    for mode in [0o600, 0o644, 0o400, 0o640] {
        let path = dir.write(&format!("m{mode:o}.toml"), "", mode);
        assert_eq!(read(&path), Ok(String::new()), "{mode:o}");
    }
    for mode in [0o620, 0o602, 0o666, 0o664, 0o622] {
        let path = dir.write(&format!("m{mode:o}.toml"), "", mode);
        assert_eq!(
            read_error(&path),
            format!(
                "lo pueden modificar el grupo u otros usuarios (permisos {mode:04o}); corrígelo con chmod go-w"
            )
        );
    }

    // Tamaño: 65536 bytes se admiten; uno más, no.
    let exact = dir.write("exacto.toml", &format!("#{}", "x".repeat(65_535)), 0o600);
    assert_eq!(read(&exact).unwrap().len(), 65_536);
    let big = dir.write("grande.toml", &format!("#{}", "x".repeat(65_536)), 0o600);
    assert_eq!(read_error(&big), "ocupa 65537 bytes o más (máximo 65536)");

    // UTF-8 inválido, con la línea.
    let bad = dir.write_bytes("latin1.toml", b"a = 1\n# \xe9\n", 0o600);
    assert_eq!(read_error(&bad), "no es UTF-8 válido (línea 2)");

    // Tipos de archivo.
    assert_eq!(
        read_error(dir.path()),
        "no es un archivo regular: es un directorio"
    );
    let socket_path = dir.path().join("socket.toml");
    // La ruta de un socket tiene un máximo de 107 bytes.
    match std::os::unix::net::UnixListener::bind(&socket_path) {
        Ok(_listener) => assert_eq!(
            read_error(&socket_path),
            "no es un archivo regular: es un socket"
        ),
        Err(e) => eprintln!("OMITIDA: no se pudo crear el socket de prueba: {e}"),
    }
    assert_eq!(
        read_error(Path::new("/dev/null")),
        "no es un archivo regular: es un dispositivo"
    );

    // Enlaces: se comprueba el destino.
    let target = dir.write("destino.toml", "[layout]\n", 0o600);
    let link = dir.path().join("enlace.toml");
    symlink(&target, &link).unwrap();
    assert_eq!(read(&link), Ok("[layout]\n".into()));
    let unsafe_target = dir.write("destino-inseguro.toml", "", 0o666);
    let unsafe_link = dir.path().join("enlace-inseguro.toml");
    symlink(&unsafe_target, &unsafe_link).unwrap();
    assert!(read_error(&unsafe_link).contains("grupo u otros"));
    let broken = dir.path().join("roto.toml");
    symlink(dir.path().join("no-existe"), &broken).unwrap();
    assert_eq!(
        read_error(&broken),
        "es un enlace simbólico a un destino que no existe"
    );
    let to_dir = dir.path().join("a-directorio.toml");
    symlink(dir.path(), &to_dir).unwrap();
    assert!(read_error(&to_dir).contains("es un directorio"));

    // Ausente.
    assert_eq!(read(&dir.path().join("nada.toml")), Err(ReadError::Missing));
    // Un componente intermedio que no es un directorio no es «ausente».
    let file = dir.write("archivo", "", 0o600);
    assert!(matches!(
        read(&file.join("wm.toml")),
        Err(ReadError::Failed(_))
    ));
}

#[test]
fn archivo_sin_permiso_de_lectura() {
    let dir = TempDir::new("sin-lectura");
    let path = dir.write("wm.toml", "", 0o000);
    // Con privilegios (root o CAP_DAC_OVERRIDE) se podría leer igualmente.
    if fs::File::open(&path).is_ok() {
        eprintln!("OMITIDA: el proceso puede leer archivos con permisos 000");
        return;
    }
    let error = load(Some(&path), None, None).unwrap_err();
    assert!(error.message.starts_with("no se puede abrir:"), "{error}");
}

#[test]
fn fifo_rechazada_sin_bloquear() {
    let dir = TempDir::new("fifo");
    let fifo = dir.path().join("wm.toml");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        eprintln!("OMITIDA: mkfifo no está disponible");
        return;
    }
    // Se comprueba antes de abrir: abrir una FIFO sin escritor bloquearía.
    let error = load(Some(&fifo), None, None).unwrap_err();
    assert_eq!(error.message, "no es un archivo regular: es una FIFO");
}

#[test]
fn no_se_crea_ni_modifica_ningun_archivo() {
    let dir = TempDir::new("sin-escrituras");
    let path = dir.write("wm.toml", "[layout]\nmaster_percent = 60\n", 0o600);
    let before = fs::metadata(&path).unwrap();
    check(Some(&path), None, None, None).unwrap();
    load(None, Some(dir.path().join("xdg").into()), None).unwrap();
    let after = fs::metadata(&path).unwrap();
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn sustitucion_entre_metadata_y_open() {
    let dir = TempDir::new("sustitucion");
    let path = dir.write("wm.toml", "", 0o600);
    let before = fs::metadata(&path).unwrap();
    // Conserva el original para impedir que se reutilice su inodo.
    fs::rename(&path, dir.path().join("original.toml")).unwrap();
    dir.write("wm.toml", "", 0o600);
    assert_eq!(
        read_verified(&path, &before),
        Err(ReadError::Failed(
            "el archivo cambió mientras se abría".into()
        ))
    );
    fs::remove_file(&path).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    // Sin escritor: una apertura bloqueante nunca llega a check_type.
    assert_eq!(
        read_verified(&path, &before),
        Err(ReadError::Failed(
            "no es un archivo regular: es una FIFO".into()
        ))
    );
}

// --check-config.

#[test]
fn informe_de_check() {
    let dir = TempDir::new("check");
    let path = dir.write("wm.toml", "[layout]\nmaster_percent = 60\n", 0o600);
    let lines = check(Some(&path), None, None, None).unwrap();
    assert_eq!(
        lines,
        [format!(
            "{}: configuración válida (30 atajos, 6 programas, maestra al 60 %, 0 reglas)",
            path.display()
        )]
    );
    // Sin archivo: la predeterminada es válida.
    let lines = check(None, None, os("relativa"), None).unwrap();
    assert_eq!(
        lines,
        [
            "aviso: HOME no es una ruta absoluta; se ignora (especificación XDG)".to_string(),
            "configuración predeterminada (ni XDG_CONFIG_HOME ni HOME dan una ruta absoluta en la que buscar): válida (30 atajos, 6 programas, maestra al 50 %, 0 reglas)".to_string(),
        ]
    );
    // Con CRIMSON_RULES, los avisos de precedencia.
    let lines = check(Some(&path), None, None, Some("class=a; floating".into())).unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1], "CRIMSON_RULES: 1 reglas");
}
