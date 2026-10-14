#!/bin/bash
# Pruebas de aceptación A22–A32 de F3 (ADR 0005): foco y navegación por
# teclado, solo con Xvfb.
#
# Uso: tests/acceptance/f3/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide el binario a Cargo (`cargo build --locked --offline`), que lo
#   compila si ha quedado obsoleto (f1/binaries.sh).
#
# Requisitos (Debian 13): los de tests/acceptance/f2/run.sh, salvo xlogo,
# más x11-apps (xclock).
#
# - Cada prueba se ejecuta en un Xvfb nuevo de `xvfb-run`, con su cookie
#   temporal y sin TCP, con las mismas guardas que F1 (f1/xvfb-env.sh).
#   Nunca se usa -ac ni se conecta con ningún otro servidor X.
# - El foco real se lee con `xdotool getwindowfocus` y la ventana activa con
#   `xprop -root _NET_ACTIVE_WINDOW`; las ventanas de prueba son de `xev`,
#   que registra sus propios FocusIn y ClientMessage.
# - La sesión principal solo se observa con `pgrep` (A32, equivalente a A9);
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
for tool in xvfb-run Xvfb xprop xlsatoms xwininfo xmessage xclock xev xdotool mcookie pgrep timeout column; do
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

OUT=$(mktemp -d -t crimson-f3.XXXXXX) || exit 2
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
: >"$RESULTS"
: >"$CODES"
: >"$RUNS"

snapshot antes

CASES=(A22 A23 A24 A25 A26 A27 A28 A29 A30 A31)
for c in "${CASES[@]}"; do
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A32: estado posterior, pánicos y residuos (como A9 en F1).
snapshot despues
session_checks A32

# Cada comprobación esperada debe tener exactamente un resultado.
expected=(
    A22-primera A22-segunda A22-focusin-cliente A22 A22-salida-limpia A22-extra-sin-errores-x
    A23-siguiente-vuelta A23-siguiente A23-anterior A23-anterior-vuelta A23 A23-extra-sin-errores-x
    A24-super-return-capturado A24-maestra A24-foco A24-zoom-capturado
    A24-intercambio A24-foco-intercambio A24 A24-extra-sin-errores-x
    A25-historial A25-mosaico A25 A25-extra-sin-errores-x
    A26-desmapeo A26-sin-ventanas A26 A26-extra-sin-errores-x
    A27-foco-dialogo A27-mosaico A27-navegacion-mosaico A27-navegacion-flotante
    A27-cierre-dialogo A27 A27-extra-sin-errores-x
    A28-sin-entrada A28-navegacion A28 A28-extra-sin-errores-x
    A29-foco A29-wm-take-focus A29-solo-si-lo-anuncia A29 A29-extra-sin-errores-x
    A30-cliente A30-navegacion A30 A30-extra-sin-errores-x
    A31-bloq-mayus A31-bloq-num A31-sin-shift A31-zoom-bloq-mayus A31 A31-extra-sin-errores-x
    A32-sin-panico A32-xprop-:0
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
