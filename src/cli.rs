//! Argumentos de la línea de órdenes. Ninguno relaja las salvaguardas (ADR
//! 0003): `--config` solo elige el archivo de configuración (ADR 0014), y
//! `--check-config` lo valida sin conectar con ningún servidor X.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::Fatal;

pub const USAGE: &str = "\
Uso: crimson-wm [--config RUTA]
     crimson-wm --check-config [RUTA]
     crimson-wm --help | --version

Gestor de ventanas X11 en desarrollo. Solo arranca en un servidor X de prueba
que supere las salvaguardas de docs/adr/0003-salvaguardas-desarrollo.md.

  --config RUTA          usa ese archivo de configuración, que debe existir,
                         en lugar de buscar crimson/wm.toml en
                         $XDG_CONFIG_HOME y en $HOME/.config
  --check-config [RUTA]  valida la configuración (RUTA o la que se buscaría)
                         sin conectar con X: código 0 si es válida, 15 si no

Una RUTA no puede empezar por «-»: escribe ./-archivo. Configuración:
docs/configuration.md.
";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Arranca el WM, con el archivo de `--config` si se indica.
    Run(Option<PathBuf>),
    /// Valida la configuración y termina, sin conectar con X.
    Check(Option<PathBuf>),
    Help,
    Version,
}

/// Ruta de `--config` o `--check-config`: ni vacía ni con aspecto de
/// opción, para que un error de escritura no se tome por un archivo.
fn path(option: &str, value: OsString) -> Result<PathBuf, Fatal> {
    if value.is_empty() {
        return Err(Fatal::Usage(format!("{option}: la ruta está vacía")));
    }
    if value.as_encoded_bytes().starts_with(b"-") {
        return Err(Fatal::Usage(format!(
            "{option}: la ruta {value:?} empieza por «-»; si es un archivo, escribe ./{}",
            value.to_string_lossy()
        )));
    }
    Ok(PathBuf::from(value))
}

/// Analiza los argumentos, sin incluir el nombre del programa.
pub fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command, Fatal> {
    let Some(first) = args.next() else {
        return Ok(Command::Run(None));
    };
    let command = match first.to_str() {
        Some("--help") => Command::Help,
        Some("--version") => Command::Version,
        Some("--config") => {
            let value = args
                .next()
                .ok_or_else(|| Fatal::Usage("--config necesita una ruta".into()))?;
            Command::Run(Some(path("--config", value)?))
        }
        Some("--check-config") => match args.next() {
            None => return Ok(Command::Check(None)),
            Some(value) => Command::Check(Some(path("--check-config", value)?)),
        },
        _ => return Err(Fatal::Usage(format!("argumento no reconocido: {first:?}"))),
    };
    match args.next() {
        None => Ok(command),
        Some(extra) => Err(Fatal::Usage(match command {
            Command::Help | Command::Version => format!(
                "{} no admite más argumentos: se admite como máximo un argumento",
                first.to_string_lossy()
            ),
            _ => format!("argumento de más: {extra:?}"),
        })),
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
        assert_eq!(parse_strs(&[]).unwrap(), Command::Run(None));
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

    // U24: opciones de configuración (ADR 0014).

    #[test]
    fn opciones_de_configuracion() {
        assert_eq!(
            parse_strs(&["--config", "/x/wm.toml"]).unwrap(),
            Command::Run(Some("/x/wm.toml".into()))
        );
        assert_eq!(
            parse_strs(&["--config", "relativa.toml"]).unwrap(),
            Command::Run(Some("relativa.toml".into()))
        );
        assert_eq!(
            parse_strs(&["--check-config"]).unwrap(),
            Command::Check(None)
        );
        assert_eq!(
            parse_strs(&["--check-config", "./-raro.toml"]).unwrap(),
            Command::Check(Some("./-raro.toml".into()))
        );
        // Rutas no UTF-8: se admiten tal cual.
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![b'/', 0xff]);
        let args = [OsString::from("--config"), raw.clone()];
        assert_eq!(
            parse(args.into_iter()).unwrap(),
            Command::Run(Some(PathBuf::from(raw)))
        );
    }

    #[test]
    fn opciones_de_configuracion_invalidas() {
        for (args, fragment) in [
            (&["--config"][..], "--config necesita una ruta"),
            (&["--config", ""], "la ruta está vacía"),
            (&["--config", "--help"], "empieza por «-»"),
            (&["--config", "-"], "empieza por «-»"),
            (&["--check-config", "--config"], "empieza por «-»"),
            (&["--check-config", ""], "la ruta está vacía"),
            (&["--config", "a", "b"], "argumento de más"),
            (&["--check-config", "a", "b"], "argumento de más"),
            (&["--config", "a", "--check-config"], "argumento de más"),
            (&["--config=a"], "argumento no reconocido"),
            (&["--check-config=a"], "argumento no reconocido"),
            (&["--version", "--config"], "como máximo un argumento"),
        ] {
            let error = parse_strs(args).unwrap_err();
            assert_eq!(error.exit_code(), 2, "{args:?}");
            assert!(error.to_string().contains(fragment), "{args:?}: {error}");
        }
    }
}
