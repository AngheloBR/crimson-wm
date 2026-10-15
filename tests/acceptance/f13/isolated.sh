#!/bin/bash
# F13: validación y salvaguardas sin servidor X accesible.
set -u
BIN=$1
OUT=$2
F1=$(cd "$(dirname "$0")/../f1" && pwd)
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
trap 'echo "isolated.sh exit: $?" >>"$RUNS"' EXIT

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
record A164-aislado "vacío" "$(find /tmp/.X11-unix -mindepth 1 | wc -l) entradas" OK \
    "tmpfs sobre /tmp/.X11-unix, uid real $outside_uid, interfaces: $interfaces"

EMPTY=$OUT/xauth-vacio
: >"$EMPTY"
common=(DISPLAY=:57 XAUTHORITY="$EMPTY" CRIMSON_SANDBOX_TOKEN=token-de-prueba)
file=$OUT/invalid.toml
# EXPECT: A164-aislado A164-sin-display A164-display-cero A164-token A164-invalido A164-check-no-x A164-check-valido
printf '[layout]\ndefault="rows"\n' >"$file"
chmod 600 "$file"
run_bin A164-sin-display 11 'DISPLAY no está definido' -u DISPLAY -- --config "$file"
run_bin A164-display-cero 10 'display 0 se rechaza siempre' DISPLAY=:0 -- --config "$file"
run_bin A164-token 10 'CRIMSON_SANDBOX_TOKEN no está definida' -u CRIMSON_SANDBOX_TOKEN DISPLAY=:57 -- --config "$file"
run_bin A164-invalido 15 'default debe ser' "${common[@]}" -- --config "$file"
run_bin A164-check-no-x 15 'default debe ser' -u DISPLAY -u XAUTHORITY -u CRIMSON_SANDBOX_TOKEN -- --check-config "$file"
printf '[layout]\ndefault="columns"\n' >"$file"
run_bin A164-check-valido 0 'layout columns' -u DISPLAY -u XAUTHORITY -u CRIMSON_SANDBOX_TOKEN -- --check-config "$file"
