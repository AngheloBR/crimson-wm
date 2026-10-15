#!/bin/bash
# A132 (parte aislada): las reglas de ventanas no relajan las capas 1 y 2
# del ADR 0003. run.sh lo ejecuta con `unshare -rmn`, como el
# tests/acceptance/f1/isolated.sh: un tmpfs vacío tapa /tmp/.X11-unix y la red
# propia oculta los sockets abstractos y TCP del sistema, así que ninguna
# ejecución puede llegar a un servidor X, tampoco al de la sesión real.
# Uso: isolated.sh BINARIO DIRECTORIO_DE_RESULTADOS
set -u
BIN=$1
OUT=$2
F1=$(cd "$(dirname "$0")/../f1" && pwd)
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
trap 'echo "isolated.sh exit: $?" >>"$RUNS"' EXIT

# Guardas, como en F1: espacio de nombres de usuario sin privilegios reales,
# red con solo lo y /tmp/.X11-unix vacío.
read -r _ outside_uid _ </proc/self/uid_map
if [[ $outside_uid == 0 ]]; then
    echo "GUARDA: no se está en un espacio de nombres de usuario sin privilegios"
    exit 90
fi
interfaces=$(awk -F: 'NR > 2 {gsub(/ /, "", $1); print $1}' /proc/net/dev | paste -sd,)
if [[ $interfaces != lo ]]; then
    echo "GUARDA: la red no está aislada (interfaces: $interfaces)"
    exit 90
fi
if ! mount -t tmpfs -o mode=1777 tmpfs /tmp/.X11-unix; then
    echo "GUARDA: no se pudo aislar /tmp/.X11-unix"
    exit 90
fi
if [[ -n $(find /tmp/.X11-unix -mindepth 1) ]]; then
    echo "GUARDA: /tmp/.X11-unix no está vacío"
    exit 90
fi
record A132-aislado "vacío" "$(find /tmp/.X11-unix -mindepth 1 | wc -l) entradas" OK \
    "tmpfs sobre /tmp/.X11-unix, uid real $outside_uid, interfaces: $interfaces"

EMPTY=$OUT/xauth-vacio
: >"$EMPTY"
VALID="CRIMSON_RULES=class=Chat; desktop=2"
INVALID="CRIMSON_RULES=class=Chat; desktop=9"
common=(XAUTHORITY="$EMPTY" CRIMSON_SANDBOX_TOKEN=token-de-prueba)

# Capa 1: :0 se rechaza igual con reglas válidas o inválidas.
run_bin A132-display-0-con-reglas 10 'display 0 se rechaza siempre' DISPLAY=:0 "${common[@]}" "$VALID"
run_bin A132-display-0-reglas-invalidas 10 'display 0 se rechaza siempre' DISPLAY=:0 "${common[@]}" "$INVALID"
# Capa 2: sin token no arranca, aunque haya reglas.
run_bin A132-sin-token-con-reglas 10 'CRIMSON_SANDBOX_TOKEN no está definida' -u CRIMSON_SANDBOX_TOKEN DISPLAY=:57 XAUTHORITY="$EMPTY" "$VALID"

connected=$(grep -l 'conectado a' "$OUT"/A132-*.log | wc -l)
verdict=FALLA
[[ $connected == 0 ]] && verdict=OK
record A132-sin-conexion 0 "$connected" "$verdict" "registros de las ejecuciones aisladas con «conectado a»"
