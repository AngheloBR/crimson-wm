# Contributing to Crimson WM

Thank you for your interest. Crimson WM is a pre-alpha project developed in
small, documented phases. Small, focused and well-tested changes are much
easier to review than large ones.

## Golden rule: never use your real X session

Crimson WM refuses to start on display `:0`, but do not try to work around
that. Do not run it, or its test harnesses, against the X server of your
desktop session. Every manual or automated test must use an isolated test
server (Xvfb, through the harnesses in `tests/acceptance/`). The harnesses
already set up the sandbox token, the root marker, a temporary cookie and a
minimal environment for you.

## Toolchain

The reference toolchain is Debian 13's Rust packages (rustc and cargo
1.85.1); see [ADR 0001](docs/adr/0001-toolchain.md). The declared minimum
supported Rust version is 1.85.

```sh
sudo apt install --no-install-recommends rustc cargo rustfmt rust-clippy gcc libc6-dev
```

If you also use rustup, run the commands below with Debian's toolchain,
for example `env PATH=/usr/bin:/bin cargo …`, so that results match the
reference environment.

## Before you submit a change

All of these must pass:

```sh
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

If your change touches the window manager's behaviour, also run the
acceptance harnesses in Xvfb. You need the packages listed in the
[README](README.md#acceptance-tests-in-xvfb).

```sh
env PATH=/usr/bin:/bin tests/acceptance/f10/run.sh   # and the earlier phases it may affect
```

You do not need to build first. Before creating any X server or client,
each harness runs `cargo build --locked --offline --bin crimson-wm` (plus
`--example test-client` from F5 on) through
`tests/acceptance/f1/binaries.sh`, and uses the executables that Cargo
reports:

- Cargo decides freshness with its own fingerprints, not file dates. It
  rebuilds after changes to the code, dependencies, `Cargo.lock`, build
  profiles, compiler flags or toolchain, and does nothing after changes
  that do not affect the build, such as package metadata in `Cargo.toml`.
- If the build fails (a compile error, a `Cargo.lock` that does not match
  `--locked`, or dependencies missing from the local cache with
  `--offline`), the harness exits with code 2 and runs no test. On a fresh
  machine, fetch the dependencies once with `cargo fetch --locked` (the
  only step that needs network access).
- Cargo runs without `DISPLAY`, `XAUTHORITY` or the sandbox token; building
  never talks to an X server.
- Passing a binary as the first argument (for example a historical build)
  tests that binary exactly as it is: it is neither rebuilt nor checked
  against the sources. The test client is still requested from Cargo.
- Cargo is taken from `PATH` (or from `CARGO`). With rustup installed, use
  `env PATH=/usr/bin:/bin` as above to build with Debian's toolchain.

If you modify a harness script, check it with `shellcheck -x` from its own
directory.

A battery passes when it reports `FALLA: 0`. One check (`…-xprop-:0`) is
always reported as skipped on purpose, because it would query the real
display.

## Guidelines

- Keep `unsafe` out of the crate (`unsafe_code = "forbid"`).
- Do not add dependencies, features or new tools without discussing them
  first; `Cargo.lock` is versioned and every command uses `--locked`.
- Handle X11 errors explicitly: a misbehaving client must never crash or
  block the window manager.
- Prefer events over polling, and pure functions (no I/O) for logic that
  can be unit-tested.
- Significant design decisions are recorded as ADRs in `docs/adr/`
  (currently in Spanish). Describe new ones in English or Spanish.
- Do not present planned features as implemented, in code comments or
  documentation.
- Do not commit build artifacts, logs, test results, captures from your
  own session or anything containing credentials or personal data.

## Licensing of contributions

Crimson WM is dual-licensed under MIT or Apache-2.0. Unless you explicitly
state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.

## Security issues

Please do not report security problems in public issues. See
[SECURITY.md](SECURITY.md).
