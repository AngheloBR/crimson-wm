#!/bin/bash
# A1, A2 y A3a sin ningún servidor X accesible. run.sh lo ejecuta con
# `unshare -rmn` (espacios de nombres de usuario, montajes y red, sin
# privilegios): un tmpfs vacío tapa /tmp/.X11-unix y la red propia oculta
# los sockets abstractos y TCP del sistema. El tmpfs desaparece con el
# espacio de nombres.
# Uso: isolated.sh BINARIO DIRECTORIO_DE_RESULTADOS
set -u
BIN=$1
OUT=$2
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
trap 'echo "isolated.sh exit: $?" >>"$RUNS"' EXIT

# Guardas: solo se monta nada dentro de un espacio de nombres de usuario
# sin privilegios reales y con una red que solo tiene lo.
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
record A0-aislamiento "vacío" "$(find /tmp/.X11-unix -mindepth 1 | wc -l) entradas" OK \
    "tmpfs sobre /tmp/.X11-unix, uid real $outside_uid, interfaces: $interfaces"

EMPTY=$OUT/xauth-vacio
: >"$EMPTY"

# A1: argumentos.
run_bin A1-help 0 '^Uso: crimson-wm' -u DISPLAY -- --help
run_bin A1-version 0 '^crimson-wm [0-9]+\.[0-9]+\.[0-9]+$' -u DISPLAY -- --version
run_bin A1-desconocido 2 'argumento no reconocido' -u DISPLAY -- --bogus
run_bin A1-dos-args 2 'como máximo un argumento' -u DISPLAY -- --help --version

# A2: DISPLAY ausente o vacío (11) y valores rechazados por la capa 1 (10).
# El token y XAUTHORITY se fijan para que el rechazo solo pueda venir de la capa 1.
common=(XAUTHORITY="$EMPTY" CRIMSON_SANDBOX_TOKEN=token-de-prueba)
run_bin A2-sin-display 11 'DISPLAY no está definido' -u DISPLAY "${common[@]}"
run_bin A2-display-vacio 11 'DISPLAY no está definido' DISPLAY= "${common[@]}"
run_bin A2-:0 10 'display 0 se rechaza siempre' DISPLAY=:0 "${common[@]}"
run_bin A2-:0.0 10 'display 0 se rechaza siempre' DISPLAY=:0.0 "${common[@]}"
run_bin A2-:0.1 10 'display 0 se rechaza siempre' DISPLAY=:0.1 "${common[@]}"
run_bin A2-:00 10 'display 0 se rechaza siempre' DISPLAY=:00 "${common[@]}"
run_bin A2-unix-barra-:0 10 'display 0 se rechaza siempre' DISPLAY=unix/:0 "${common[@]}"
# Las formas de ruta dan «malformado» si la ruta no existe (aquí, por el
# tmpfs) y «solo locales» si existe; ambas ramas terminan con 10.
run_bin A2-unix:0 10 'DISPLAY malformado|solo se admiten displays locales' DISPLAY=unix:0 "${common[@]}"
run_bin A2-ruta-X0 10 'DISPLAY malformado|solo se admiten displays locales' DISPLAY=/tmp/.X11-unix/X0 "${common[@]}"
run_bin A2-localhost:1 10 'solo se admiten displays locales' DISPLAY=localhost:1 "${common[@]}"
run_bin A2-tcp-localhost:1 10 'protocolo "tcp" no permitido' DISPLAY=tcp/localhost:1 "${common[@]}"

# A3a: display sin servidor.
run_bin A3a-sin-servidor 11 'no se pudo conectar al socket /tmp/.X11-unix/X57' DISPLAY=:57 "${common[@]}"

# Ninguna de estas ejecuciones debe haber llegado a conectar.
connected=$(grep -l 'conectado a' "$OUT"/A1-*.log "$OUT"/A2-*.log "$OUT"/A3a-*.log | wc -l)
verdict=FALLA
[[ $connected == 0 ]] && verdict=OK
record A2-sin-conexion 0 "$connected" "$verdict" "registros de A1–A3a con «conectado a»"
