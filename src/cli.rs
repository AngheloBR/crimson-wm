//! Argumentos de la línea de órdenes. En F1 solo existen `--help` y
//! `--version`; no hay opciones que relajen las salvaguardas (ADR 0003).

use std::ffi::OsString;

use crate::error::Fatal;

pub const USAGE: &str = "\
Uso: crimson-wm [--help | --version]

Gestor de ventanas X11 en desarrollo (fase F1). Solo arranca en un servidor
X de prueba que supere las salvaguardas de docs/adr/0003-salvaguardas-desarrollo.md.
";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run,
    Help,
    Version,
}

/// Analiza los argumentos, sin incluir el nombre del programa.
pub fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command, Fatal> {
    let Some(first) = args.next() else {
        return Ok(Command::Run);
    };
    if args.next().is_some() {
        return Err(Fatal::Usage("se admite como máximo un argumento".into()));
    }
    match first.to_str() {
        Some("--help") => Ok(Command::Help),
        Some("--version") => Ok(Command::Version),
        _ => Err(Fatal::Usage(format!("argumento no reconocido: {first:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, Fatal> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn argumentos_validos() {
        assert_eq!(parse_strs(&[]).unwrap(), Command::Run);
        assert_eq!(parse_strs(&["--help"]).unwrap(), Command::Help);
        assert_eq!(parse_strs(&["--version"]).unwrap(), Command::Version);
    }

    #[test]
    fn argumentos_invalidos_dan_codigo_2() {
        for args in [
            &["-h"][..],
            &["--display", ":1"],
            &["--help", "--version"],
            &[""],
            &["--HELP"],
        ] {
            let error = parse_strs(args).unwrap_err();
            assert_eq!(error.exit_code(), 2, "{args:?}");
        }
    }

    #[test]
    fn argumento_no_unicode_da_codigo_2() {
        use std::os::unix::ffi::OsStringExt;
        let args = [OsString::from_vec(vec![0xff, 0xfe])];
        assert_eq!(parse(args.into_iter()).unwrap_err().exit_code(), 2);
    }
}
