# ADR 0001: Toolchain de Rust, MSRV y `Cargo.lock`

- **Estado:** aceptado. MSRV confirmado para el árbol de dependencias fijado
  el 2026-10-11 (P3).
- **Fecha:** 2026-10-11

## Contexto

Crimson WM se dirige a Debian 13 (Trixie). El sistema de desarrollo no
tenía Rust instalado. Debian 13 ofrece `rustc`, `cargo`, `rustfmt` y
`rust-clippy` en la versión 1.85.1; rustup también está disponible, pero
descarga toolchains fuera del sistema de paquetes.

## Decisión

1. **Toolchain:** Rust 1.85.1 de los paquetes oficiales de Debian,
   instalado con:

   ```sh
   sudo apt install --no-install-recommends rustc cargo rustfmt rust-clippy
   ```

   Paquetes instalados (2026-10-11): `libstd-rust-1.85`, `libstd-rust-dev`,
   `rustc`, `cargo`, `rust-clippy` y `rustfmt`, todos en la versión
   `1.85.1+dfsg1-1+deb13u1`.

   Sin `--no-install-recommends`, APT añadiría `rust-llvm` y la cadena
   LLVM/clang 19. No hacen falta para compilar: la primera compilación (P3,
   2026-10-11) se completó sin ellos.

   **Enlazador:** `rustc` depende en Debian de `gcc`, `binutils` y
   `libc-dev`. Están instalados (gcc 14.2, binutils 2.44, libc6-dev 2.41).
   `cc` es una alternativa del sistema (`update-alternatives`) que apunta a
   `/usr/bin/gcc`. Comprobado el 2026-10-11: `rustc --print link-args`
   muestra que rustc invoca `cc` en `x86_64-unknown-linux-gnu`, y
   `cargo build --locked` enlazó el binario sin configurar ningún
   enlazador.
2. **Sin rustup.** No se mezclan toolchains de APT y de rustup.
3. **Edición 2024.** Según el historial de versiones de Rust, está
   disponible desde Rust 1.85.0. Comprobado el 2026-10-11:
   `rustc --edition 2024 --print cfg` termina sin error y el crate compila
   con `edition = "2024"`.
4. **MSRV `rust-version = "1.85"`** (1.85.0 o superior).
   - No se deriva de la edición, sino de la toolchain de Debian y del
     `rust-version` declarado por cada dependencia.
   - El `rust-version` de cada dependencia lo declara su autor, y crates.io
     no lo verifica: el MSRV solo se considerará confirmado cuando
     `cargo build --locked` y `cargo test --locked` pasen con rustc 1.85.1.
   - **Confirmado el 2026-10-11 para el árbol fijado:** ambas órdenes
     pasan con rustc 1.85.1, y el `rust-version` más alto de los crates
     compilados en Linux es 1.71 (ver `docs/dependencies.md`). Cada
     actualización de dependencias exige repetir la comprobación.
   - Solo se probará con 1.85.1; 1.85.0 no se probará.
5. **Origen de las dependencias:** crates.io, con la caché en `~/.cargo`.
6. **`Cargo.lock`:**
   - Se generó una sola vez, en la resolución inicial
     (`cargo generate-lockfile`, 2026-10-11, formato 4). Cargo necesita al
     menos un objetivo de compilación (por ejemplo, `src/main.rs`) para
     aceptar el manifiesto, así que la resolución no puede hacerse con el
     `Cargo.toml` solo.
   - **Resolución y MSRV:** el resolvedor asociado a la edición 2024
     (resolvedor 3, con `incompatible-rust-versions = "fallback"` por
     defecto) *prefiere* versiones compatibles con el `rust-version`
     declarado. Si ninguna versión compatible cumple un requisito, elige una
     incompatible **sin dar error**, y el fallo aparecería al compilar. Por
     eso, tras la resolución se comprobó el `rust-version` de cada crate
     resuelto con `cargo metadata --locked --format-version 1`: todos son
     ≤ 1.85.
   - Se versionará, porque el proyecto produce un binario.
   - Todas las verificaciones posteriores usarán `--locked`, o `--frozen`
     (que además impide el acceso a la red) cuando las dependencias ya
     estén descargadas.
   - Las actualizaciones serán explícitas (`cargo update -p <crate>`) y se
     revisarán antes de aceptarse.

## Alternativas consideradas

- **rustup:** versiones más recientes, pero descarga binarios fuera de APT
  y desacopla el proyecto de la toolchain de Debian.
- **Crates empaquetados por Debian (`librust-*-dev`):** funcionan sin red
  y Debian los mantiene, pero tienen versiones más antiguas (x11rb 0.13.0),
  cada dependencia requiere una instalación con privilegios y el
  `Cargo.lock` no sería reproducible fuera de Debian.

## Consecuencias

- Las dependencias que exijan una versión de Rust superior a 1.85 no se
  pueden usar sin revisar esta decisión.
- Cambiar de toolchain o de MSRV requiere un nuevo ADR.
- Reversión: ver la sección "Volver al estado anterior" de
  `docs/README-technical-es.md`.
