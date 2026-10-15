#!/bin/bash
# Pruebas de aceptación A1–A9 de F1 (ADR 0003), solo con Xvfb.
#
# Uso: tests/acceptance/f1/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide el binario a Cargo (`cargo build --locked --offline`), que lo
#   compila si ha quedado obsoleto (f1/binaries.sh).
#
# Requisitos (Debian 13): xvfb (xvfb-run, Xvfb), x11-utils (xprop,
# xlsatoms, xwininfo, xmessage), xauth, xdotool, util-linux (unshare,
# mcookie), mount (mount), bsdextrautils (column), procps (pgrep), bash,
# coreutils (timeout, mktemp, env, sleep, sort, paste, comm, wc, head,
# cat, dirname, basename), findutils (find), grep, mawk (awk) y espacios
# de nombres de usuario sin privilegios.
#
# - A1, A2 y A3a se ejecutan con `unshare -rmn`, sin ningún servidor X visible.
# - El resto, cada una en un Xvfb nuevo de `xvfb-run`, con su cookie temporal
#   y sin TCP. Nunca se usa -ac ni se conecta con ningún otro servidor X.
# - La sesión principal solo se observa con `pgrep`; la consulta de la raíz de
#   :0 que menciona el ADR 0003 se omite deliberadamente (A9 parcial).
#
# Los resultados se guardan en un directorio temporal nuevo, que se indica al
# final. Código de salida: 0 si no hay ningún FALLA ni falta ningún resultado,
# 1 si los hay, 2 si no se pudo preparar la ejecución.
set -u
F=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$F/../../.." && pwd)

if ((EUID == 0)); then
    echo "run.sh: no se ejecuta como root" >&2
    exit 2
fi
missing=()
for tool in xvfb-run Xvfb unshare xprop xlsatoms xwininfo xmessage xauth xdotool mcookie pgrep timeout column; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
if ((${#missing[@]})); then
    echo "run.sh: faltan herramientas: ${missing[*]}" >&2
    exit 2
fi

# Binario del WM: lo decide Cargo, salvo que se pase uno expresamente
# (P1-B.1). Antes de crear ningún directorio, servidor o cliente X.
# shellcheck source=binaries.sh
source "$F"/binaries.sh
ensure_binaries "$ROOT" 0 "${1:-}" || exit 2

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN CRIMSON_RULES

OUT=$(mktemp -d -t crimson-f1.XXXXXX) || exit 2
# shellcheck source=lib.sh
source "$F/lib.sh"
: >"$RESULTS"
: >"$CODES"
: >"$RUNS"

snapshot antes

unshare -rmn bash "$F/isolated.sh" "$BIN" "$OUT" >"$OUT/isolated.txt" 2>&1
echo "unshare exit: $?" >>"$RUNS"

CASES=(A3b A3c A3d A3e A4a A4b A4c A4d A5 A6 A7 A8)
for c in "${CASES[@]}"; do
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A9: estado posterior, pánicos y residuos.
snapshot despues
session_checks A9

# Cada comprobación esperada debe tener exactamente un resultado.
expected=(
    A0-aislamiento A1-help A1-version A1-desconocido A1-dos-args
    A2-sin-display A2-display-vacio A2-:0 A2-:0.0 A2-:0.1 A2-:00 A2-unix-barra-:0
    A2-unix:0 A2-ruta-X0 A2-localhost:1 A2-tcp-localhost:1 A2-sin-conexion
    A3a-sin-servidor A3b-xauthority-vacio A3c-cookie-otro-display A3d-cookie-erronea
    A3e-control-servidor A4a-sin-token A4b-sin-marca A4b-no-crea-atomos
    A4c-marca-distinta A4d-marca-tipo-utf8 A5-otro-wm-check
    A6-primera A6-segunda A6-extra-sin-errores-x
    A7-registro A7-mapeo A7-extra-configure A7-extra-sin-shift A7 A7-extra-sin-errores-x
    A8 A8-extra-sin-errores-x A9-sin-panico A9-xprop-:0
)
for id in "${expected[@]}"; do
    n=$(awk -F'\t' -v id="$id" '$1 == id' "$RESULTS" | wc -l)
    ((n == 1)) || record "$id" "1 resultado" "$n resultados" FALLA "comprobación sin resultado o duplicada"
done

column -t -s $'\t' "$RESULTS"
echo
cat "$RUNS"
echo
ok=$(awk -F'\t' '$4 == "OK"' "$RESULTS" | wc -l)
failed=$(awk -F'\t' '$4 == "FALLA"' "$RESULTS" | wc -l)
skipped=$(awk -F'\t' '$4 == "OMITIDA"' "$RESULTS" | wc -l)
echo "OK: $ok  FALLA: $failed  OMITIDA: $skipped"
echo "Resultados: $OUT"
((failed == 0))
