# Registro de dependencias

Las dependencias se resolvieron el 2026-10-11 (P3), se ampliaron el
2026-10-15 (F12-B, ADR 0014) y están fijadas en `Cargo.lock`. **No se ha hecho
una auditoría completa de seguridad de las dependencias:** el recuento
de `unsafe` de §4 es preliminar y se obtuvo con `grep`. Este documento
separa tres niveles de certeza:

1. **Declaradas:** lo que figura en `Cargo.toml`. Es un hecho del proyecto.
2. **Transitivas previstas:** lo que los metadatos públicos de crates.io
   indicaban antes de la resolución. Se conserva como registro histórico
   (§2); no es vinculante.
3. **Resueltas:** lo fijado en `Cargo.lock` y comprobado sobre las fuentes
   exactas descargadas en `~/.cargo/registry/src/` (§3).

Los campos marcados como _pendiente_ se completarán con datos verificados,
nunca con estimaciones.

## 1. Dependencias declaradas en `Cargo.toml`

| Crate | Requisito | Features | Fase | Fuente y fecha de verificación |
|---|---|---|---|---|
| `x11rb` | `0.14` (≥ 0.14.0, < 0.15.0) | `default-features = false` | F1 | `Cargo.toml`, 2026-10-11 |
| `toml_parser` | `1.1.5` (≥ 1.1.5, < 2.0.0) | `default-features = false`, `features = ["std"]` | F12 | `Cargo.toml`, 2026-10-15 |
| `rustix` | `1.1.5` (≥ 1.1.5, < 2.0.0) | `default-features = false`, `features = ["fs", "std"]` | F12-C (ya transitiva desde F1) | `Cargo.toml`, 2026-10-15 |

x11rb 0.14.0 no declara ninguna feature `default`, así que
`default-features = false` no cambia la selección: solo deja la intención
explícita (ver ADR 0002).

F12-C usa la API segura `rustix::fs::open` con `NONBLOCK`, `NOCTTY` y
`CLOEXEC` para la configuración. La versión y todas sus features ya estaban
en el árbol de x11rb: no añade paquetes, versiones, features ni requisitos
de compilador. `Cargo.lock` solo gana la arista directa desde Crimson WM.

`toml_parser` (F12, ADR 0014) analiza el archivo de configuración. Su
feature `default` es `std`; se declara explícitamente para dejar fuera, si
cambiara el valor por defecto, las features `unsafe` (bloques `unsafe` en
el analizador léxico), `simd` (añade `memchr`) y `debug` (añade `anstream` y
`anstyle`). Alternativas descartadas en el ADR 0014: `toml` con `serde`
(unos diez crates más, macros procedurales y sin copia local, así que
necesitaba red), `toml_edit` (más superficie para un archivo de solo
lectura) y un analizador propio de un subconjunto de TOML.

## 2. Transitivas previstas (registro previo a la resolución)

Dependencias no opcionales que declaraba x11rb 0.14.0 en el índice de
crates.io, y las de estas a su vez para Linux x86_64, consultadas antes de
generar `Cargo.lock`. Cada fila es un requisito declarado por un paquete
padre concreto, no una dependencia resuelta. Los datos vigentes están en §3.

| Crate | Paquete que lo declara | Requisito declarado | Condición | Fuente y fecha de verificación |
|---|---|---|---|---|
| `x11rb-protocol` | x11rb 0.14.0 | `^0.14.0` | — | Índice de crates.io, 2026-10-11 |
| `rustix` | x11rb 0.14.0 | `^1.0` | — | Índice de crates.io, 2026-10-11 |
| `gethostname` | x11rb 0.14.0 | `^1.0` | — | Índice de crates.io, 2026-10-11 |
| `rustix` | gethostname 1.1.0 | `^1.0.3` | Sistemas no Windows | Índice de crates.io, 2026-10-11 |
| `bitflags` | rustix 1.1.5 | `^2.4.0` | — | Índice de crates.io, 2026-10-11 |
| `linux-raw-sys` | rustix 1.1.5 | `^0.12` | Linux (declarada para ambos backends de rustix) | Índice de crates.io, 2026-10-11 |

## 3. Dependencias resueltas

**F12-B (2026-10-15):** `toml_parser` se añadió con
`cargo build --offline` (Cargo 1.85.1 de Debian), sin red: los dos crates
estaban en la caché local de Cargo. `Cargo.lock` solo ganó `toml_parser`
1.1.5+spec-1.1.0 y `winnow` 1.0.4, y sus sumas de comprobación coinciden con
el SHA-256 de los `.crate` de `~/.cargo/registry/cache/`:

| Crate | SHA-256 del `.crate` (= `checksum` de `Cargo.lock`) |
|---|---|
| `toml_parser` 1.1.5+spec-1.1.0 | `baa693a8032d7e1cada7d0041e96126df243179ff061456783ac7f12bda4744c` |
| `winnow` 1.0.4 | `23b97319f7b8343df12cc98938e5c3eb436064524c8d2b4e30a1d3a36eecdf81` |

El resto de esta sección conserva los datos de la resolución inicial,
ampliados con los dos crates nuevos.

`cargo generate-lockfile` (2026-10-11) respondió «Locking 10 packages to
latest Rust 1.85 compatible versions». `Cargo.lock` usa el formato 4 y todos
los paquetes proceden del registro de crates.io, con suma de comprobación.

Fuentes: `Cargo.lock`, `cargo tree --locked`,
`cargo tree --locked -e features` y
`cargo metadata --locked --format-version 1`, el 2026-10-11.

### 3.1. Compiladas en Linux (`x86_64-unknown-linux-gnu`)

| Crate | Versión fijada | Directa o transitiva (padre) | Licencia | Repositorio | `rust-version` | Features activas |
|---|---|---|---|---|---|---|
| `x11rb` | 0.14.0 | Directa | `MIT OR Apache-2.0` | github.com/psychon/x11rb | 1.68 | ninguna |
| `x11rb-protocol` | 0.14.0 | Transitiva (x11rb) | `MIT OR Apache-2.0` | github.com/psychon/x11rb | 1.68 | `std` |
| `gethostname` | 1.1.0 | Transitiva (x11rb) | **`Apache-2.0`** | codeberg.org/swsnr/gethostname.rs | 1.71 | ninguna |
| `rustix` | 1.1.5 | Directa (F12-C) y transitiva (x11rb, gethostname) | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | github.com/bytecodealliance/rustix | 1.65 | `alloc`, `event`, `fs`, `net`, `std`, `system` |
| `bitflags` | 2.13.2 | Transitiva (rustix) | `MIT OR Apache-2.0` | github.com/bitflags/bitflags | 1.56.0 | `std` |
| `linux-raw-sys` | 0.12.1 | Transitiva (rustix) | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | github.com/sunfishcode/linux-raw-sys | 1.63 | `auxvec`, `elf`, `errno`, `general`, `if_ether`, `ioctl`, `net`, `netlink`, `no_std`, `system`, `xdp` |
| `toml_parser` | 1.1.5+spec-1.1.0 | Directa (F12) | `MIT OR Apache-2.0` | github.com/toml-rs/toml | 1.85 | `alloc`, `std` |
| `winnow` | 1.0.4 | Transitiva (toml_parser) | **`MIT`** | github.com/winnow-rs/winnow | 1.65.0 | ninguna |

`winnow` solo tiene licencia MIT: es compatible con la doble licencia del
proyecto, pero una distribución binaria debe conservar su aviso de
copyright y de licencia (el de `LICENSE-MIT` del crate).

```
crimson-wm v0.1.0
├── rustix v1.1.5
│   ├── bitflags v2.13.2
│   └── linux-raw-sys v0.12.1
├── toml_parser v1.1.5+spec-1.1.0
│   └── winnow v1.0.4
└── x11rb v0.14.0
    ├── gethostname v1.1.0
    │   └── rustix v1.1.5 (*)
    ├── rustix v1.1.5 (*)
    └── x11rb-protocol v0.14.0
```

Todos los `rust-version` declarados son ≤ 1.85; el más alto de los crates
compilados en Linux es 1.85 (toml_parser, desde F12), igual al MSRV del
proyecto, y se compila con rustc 1.85.1 de Debian.

### 3.2. Solo en otras plataformas (figuran en `Cargo.lock`, no se compilan en Linux)

`Cargo.lock` es independiente de la plataforma, así que incluye las
dependencias condicionales de todos los objetivos. Estas no aparecen en
`cargo tree --locked` para Linux.

| Crate | Versión fijada | Padres en `Cargo.lock` | Licencia | Repositorio | `rust-version` |
|---|---|---|---|---|---|
| `errno` | 0.3.14 | rustix | `MIT OR Apache-2.0` | github.com/lambda-fairy/rust-errno | 1.56 |
| `libc` | 0.2.190 | rustix, errno | `MIT OR Apache-2.0` | github.com/rust-lang/libc | 1.65 |
| `windows-sys` | 0.61.2 | rustix, errno | `MIT OR Apache-2.0` | github.com/microsoft/windows-rs | 1.71 |
| `windows-link` | 0.2.1 | gethostname, windows-sys | `MIT OR Apache-2.0` | github.com/microsoft/windows-rs | 1.71 |

## 4. Recuento preliminar de `unsafe` (no es una auditoría)

**Esto no es una auditoría de seguridad.** Es un recuento textual con
`grep` sobre `src/` de cada crate. Incluye código de otras plataformas y
arquitecturas, código condicionado a features no activas, macros y tests, y
no evalúa si cada uso es correcto.

Orden usada (2026-10-11), por crate, sobre
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/<crate>/src`:

```sh
grep -rE --include='*.rs' -o 'unsafe \{' <dir> | wc -l
grep -rE --include='*.rs' -o 'unsafe fn' <dir> | wc -l
grep -rE --include='*.rs' -o 'unsafe impl' <dir> | wc -l
```

El informe de P3 usaba otro patrón para `unsafe fn` y daba cifras algo
mayores (x11rb 30, rustix 519, linux-raw-sys 2387). Las de esta tabla son las
de la orden anterior.

| Crate | Versión | Lint en la raíz del crate | `unsafe {` | `unsafe fn` | `unsafe impl` | Observaciones |
|---|---|---|---|---|---|---|
| `x11rb` | 0.14.0 | `#![cfg_attr(not(feature = "allow-unsafe-code"), forbid(unsafe_code))]` (`src/lib.rs:147`) | 30 | 29 | 6 | Todos los usos están en código condicionado a `allow-unsafe-code`: `src/xcb_ffi/` (`src/lib.rs:161-162`) y `src/utils.rs:13-99`. Esa feature no está activa. |
| `x11rb-protocol` | 0.14.0 | `unsafe_code` en `#![forbid(...)]`; `#![no_std]` (`src/lib.rs:55-69`) | 0 | 0 | 0 | — |
| `gethostname` | 1.1.0 | ninguno | 2 | 0 | 0 | Ambos bloques están en una función `#[cfg(windows)]` (`src/lib.rs:60-100`); no se compilan en Linux. |
| `rustix` | 1.1.5 | `#![cfg_attr(linux_raw, deny(unsafe_code))]` (`src/lib.rs:101`) | 1049 | 511 | 22 | Llamadas al sistema. Sin revisar qué parte se compila con el backend y las features activas. |
| `bitflags` | 2.13.2 | `#![cfg_attr(not(test), forbid(unsafe_code))]` (`src/lib.rs:273`) | 0 | 0 | 2 | Los dos `unsafe impl` están en la macro de integración con `bytemuck` (`src/external.rs:236-243`), que no está activa. |
| `linux-raw-sys` | 0.12.1 | ninguno | 4541 | 2296 | 0 | Bindings generados para todas las arquitecturas. Sin revisar. |
| `toml_parser` | 1.1.5+spec-1.1.0 | `#![cfg_attr(not(feature = "unsafe"), forbid(unsafe_code))]` (`src/lib.rs:18`) | 42 | 9 | 0 | La feature `unsafe` no está activa: con `forbid(unsafe_code)`, el compilador rechazaría cualquier bloque `unsafe` compilado, así que ninguno lo está (recuento del 2026-10-15). |
| `winnow` | 1.0.4 | ninguno | 24 | 20 | 0 | Cortes sin comprobación de límites en `src/stream/` (implementaciones de `Stream`), que sí se compilan. Sin revisar. |

Pendiente: una revisión real del `unsafe` que se compila en rustix,
linux-raw-sys y winnow, si el proyecto llega a requerirla. En winnow, el
texto que recibe es el de `wm.toml`, que ya se ha leído con un máximo de
64 KiB, es UTF-8 válido y pertenece al usuario (ADR 0014).

## 5. Avisos de seguridad

Herramientas previstas cuando el árbol lo justifique:

- **`cargo-deny`:** las comprobaciones de licencias, crates prohibidos o
  duplicados y orígenes funcionan sin red. La comprobación de avisos
  necesita la base RustSec, que se descarga por red y puede quedar en caché.
- **`cargo-audit`:** comprueba avisos de la base RustSec y necesita esa
  base, descargada por red o en caché.
- Instalar cualquiera de las dos con `cargo install` requiere red.

| Fecha | Herramienta y versión | Resultado |
|---|---|---|
| _pendiente_ | | |

## 6. Historial de cambios

| Fecha | Cambio | Motivo |
|---|---|---|
| 2026-10-11 | Resolución inicial con `cargo generate-lockfile`: 10 paquetes, 6 compilados en Linux | P3: esqueleto compilable y `Cargo.lock` |
| 2026-10-15 | `toml_parser` 1.1.5 (directa) y `winnow` 1.0.4 (transitiva), resueltos sin red desde la caché local: 12 paquetes, 8 compilados en Linux | F12-B: configuración TOML (ADR 0014) |
| 2026-10-15 | `rustix` 1.1.5 también directa, sin cambiar paquetes ni features | F12-C: evitar bloqueo por FIFO entre metadata y open |
