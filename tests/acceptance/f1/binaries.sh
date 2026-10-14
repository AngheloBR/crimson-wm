# shellcheck shell=bash
# Binarios de las pruebas de aceptación (P1-B.1). Lo cargan los run.sh de
# F1 a F10 antes de crear ningún directorio, servidor o cliente X.
#
# Hasta P1-B, cada run.sh rechazaba un binario si `src/`, `examples/`,
# `Cargo.toml` o `Cargo.lock` eran más recientes que él. Cargo, en cambio,
# decide si un binario está al día con su propio registro (fingerprint): no
# vuelve a enlazar tras un cambio que no afecta a la compilación (por
# ejemplo, la descripción del paquete). Con las dos reglas a la vez, un
# binario correcto quedaba rechazado y `cargo build` no lo arreglaba.
#
# Ahora decide Cargo: sin BINARIO explícito, el arnés ejecuta
#   cargo build --locked --offline --bin crimson-wm [--example test-client]
# y usa los ejecutables de los que informa Cargo. Cargo recompila lo que
# haya quedado obsoleto (código, dependencias, Cargo.lock, perfiles,
# features, opciones del compilador o la toolchain) y nada más. Si falla
# (errores de compilación, Cargo.lock que no cuadra con --locked,
# dependencias ausentes con --offline), el arnés termina con 2 sin ejecutar
# ninguna prueba. Cargo se ejecuta sin DISPLAY, XAUTHORITY ni el token del
# sandbox; la compilación no conecta con ningún servidor X.
#
# CARGO elige el ejecutable de cargo (por defecto, `cargo` del PATH). Para
# usar la toolchain de referencia de Debian con rustup instalado, ejecuta el
# arnés con `env PATH=/usr/bin:/bin` (ADR 0001).

# ensure_binaries RAÍZ CLIENTE [BINARIO]
#   CLIENTE: 1 si el arnés necesita también examples/test-client (F5–F10).
#   BINARIO: crimson-wm suministrado expresamente (por ejemplo, uno
#   histórico): se usa tal cual, sin compilarlo ni compararlo con las
#   fuentes. test-client, si hace falta, se sigue pidiendo a Cargo.
# Fija BIN y, con CLIENTE=1, TEST_CLIENT, con rutas absolutas. Devuelve 2,
# con un mensaje, si algo falla.
# shellcheck disable=SC2034 # BIN y TEST_CLIENT los usan los run.sh
ensure_binaries() {
    local root=$1 client=$2 explicit=${3:-} json rc
    local -a targets=()
    BIN=
    TEST_CLIENT=
    if [[ -n $explicit ]]; then
        if [[ -d $explicit || ! -x $explicit ]]; then
            echo "run.sh: no existe el binario $explicit" >&2
            return 2
        fi
        BIN=$(cd "$(dirname "$explicit")" && pwd)/$(basename "$explicit")
        echo "run.sh: crimson-wm: $BIN (suministrado; no se compila)" >&2
    else
        targets+=(--bin crimson-wm)
    fi
    ((client)) && targets+=(--example test-client)
    ((${#targets[@]})) || return 0
    json=$(env -u DISPLAY -u XAUTHORITY -u CRIMSON_SANDBOX_TOKEN "${CARGO:-cargo}" build \
        --manifest-path "$root/Cargo.toml" --locked --offline --message-format=json "${targets[@]}")
    rc=$?
    if ((rc != 0)) || ! grep -qF '"reason":"build-finished","success":true' <<<"$json"; then
        echo "run.sh: cargo build falló (código $rc): no se ejecuta ninguna prueba" >&2
        return 2
    fi
    if [[ -z $explicit ]]; then
        BIN=$(cargo_artifact "$json" crimson-wm) || return 2
    fi
    if ((client)); then
        TEST_CLIENT=$(cargo_artifact "$json" test-client) || return 2
    fi
}

# cargo_artifact JSON NOMBRE: ruta del ejecutable del objetivo NOMBRE según
# la salida JSON de Cargo. Informa en stderr de si Cargo lo dio por bueno
# («al día») o tuvo que compilarlo.
cargo_artifact() {
    local line path state
    line=$(grep -F '"reason":"compiler-artifact"' <<<"$1" | grep -F "\"name\":\"$2\"" |
        grep -F '"executable":"' | tail -n1)
    path=$(grep -o '"executable":"[^"\\]*"' <<<"$line" | sed 's/^"executable":"//; s/"$//')
    if [[ -z $path || ! -x $path ]]; then
        echo "run.sh: Cargo no informó del ejecutable $2" >&2
        return 2
    fi
    if grep -qF '"fresh":true' <<<"$line"; then
        state="al día"
    else
        state="compilado ahora"
    fi
    echo "run.sh: $2: $path ($state según Cargo)" >&2
    echo "$path"
}
