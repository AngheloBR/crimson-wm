#!/bin/bash
# Pruebas de aceptación A10–A21 de F2 (ADR 0004): mosaico master-stack,
# solo con Xvfb.
#
# Uso: tests/acceptance/f2/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide el binario a Cargo (`cargo build --locked --offline`), que lo
#   compila si ha quedado obsoleto (f1/binaries.sh).
#
# Requisitos (Debian 13): los de tests/acceptance/f1/run.sh, salvo unshare,
# más x11-apps (xlogo) y x11-utils (xev).
#
# - Cada prueba se ejecuta en un Xvfb nuevo de `xvfb-run`, con su cookie
#   temporal y sin TCP, con las mismas guardas que F1 (f1/xvfb-env.sh).
#   Nunca se usa -ac ni se conecta con ningún otro servidor X.
# - La sesión principal solo se observa con `pgrep` (A21, equivalente a A9);
#   la consulta de la raíz de :0 se omite deliberadamente.
#
# Los resultados se guardan en un directorio temporal nuevo, que se indica al
# final. Código de salida: 0 si no hay ningún FALLA ni falta ningún resultado,
# 1 si los hay, 2 si no se pudo preparar la ejecución.
set -u
F=$(cd "$(dirname "$0")" && pwd)
F1=$(cd "$F/../f1" && pwd)
ROOT=$(cd "$F/../../.." && pwd)

if ((EUID == 0)); then
    echo "run.sh: no se ejecuta como root" >&2
    exit 2
fi
missing=()
for tool in xvfb-run Xvfb xprop xlsatoms xwininfo xmessage xlogo xev xdotool mcookie pgrep timeout column; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
if ((${#missing[@]})); then
    echo "run.sh: faltan herramientas: ${missing[*]}" >&2
    exit 2
fi

# Binario del WM: lo decide Cargo, salvo que se pase uno expresamente
# (P1-B.1). Antes de crear ningún directorio, servidor o cliente X.
# shellcheck source=../f1/binaries.sh
source "$F1"/binaries.sh
ensure_binaries "$ROOT" 0 "${1:-}" || exit 2

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN

OUT=$(mktemp -d -t crimson-f2.XXXXXX) || exit 2
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
: >"$RESULTS"
: >"$CODES"
: >"$RUNS"

snapshot antes

CASES=(A10 A11 A12 A13 A14 A15 A16 A17 A18 A19 A20)
for c in "${CASES[@]}"; do
    size=1280x720
    [[ $c == A20 ]] && size=1279x719
    xvfb-run -a -s "-screen 0 ${size}x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A21: estado posterior, pánicos y residuos (como A9 en F1).
snapshot despues
session_checks A21

# Cada comprobación esperada debe tener exactamente un resultado.
expected=(
    A10-una A10-borde A10 A10-extra-sin-errores-x
    A11-dos A11 A11-extra-sin-errores-x
    A12-tres A12 A12-extra-sin-errores-x
    A13-cierre-maestra A13 A13-extra-sin-errores-x
    A14-retirada A14-al-final A14 A14-extra-sin-errores-x
    A15-sinteticos A15-geometria A15 A15-extra-sin-errores-x
    A16-flotante A16-configure-aplicado A16-encima A16 A16-extra-sin-errores-x
    A17-flotante A17 A17-extra-sin-errores-x
    A18-adopcion A18-tras-adopcion A18 A18-extra-sin-errores-x
    A19-sigue-en-marcha A19-estado-coherente A19-registro-errores A19
    A20-impar A20 A20-extra-sin-errores-x
    A21-sin-panico A21-xprop-:0
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
