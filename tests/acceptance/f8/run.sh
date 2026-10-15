#!/bin/bash
# Pruebas de aceptación A76–A87 de F8 (ADR 0010): diálogos y ventanas
# transitorias, solo con Xvfb y, como en F4–F7, con programas falsos en el
# PATH del WM. A86 hace capturas reales del Xvfb (`xwd`) y compone la
# comparación con ImageMagick.
#
# Uso: tests/acceptance/f8/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide a Cargo (`cargo build --locked --offline`) el binario y el cliente
#   de prueba (examples/test-client.rs), y Cargo compila lo que haya quedado
#   obsoleto (f1/binaries.sh). El cliente se pide a Cargo en los dos casos.
#
# Requisitos (Debian 13): los de tests/acceptance/f4/run.sh, más
# x11-apps (xwd, xlogo) e imagemagick (magick).
#
# - Cada prueba se ejecuta en un Xvfb nuevo de `xvfb-run`, con su cookie
#   temporal y sin TCP, con las mismas guardas que F1 (f1/xvfb-env.sh).
# - El WM se ejecuta con `env -i` y un entorno mínimo (f1/xvfb-env.sh): su
#   PATH es un único directorio temporal con copias de fake.sh llamadas
#   kitty, rofi, thunar, firefox, flameshot y nvim. Antes de arrancarlo se
#   comprueba cómo se resuelve cada nombre; si alguno pudiera resolverse
#   fuera de ese directorio, la prueba no se ejecuta. Las aplicaciones
#   reales nunca se ejecutan.
# - HOME, XDG_* y D-Bus apuntan a directorios temporales de la prueba;
#   XAUTHORITY es la cookie temporal de xvfb-run.
# - test-client (examples/test-client.rs) solo se conecta al Xvfb de cada
#   prueba: rechaza cualquier DISPLAY que no sea :N con N distinto de 0.
# - F8 no envía señales al WM.
# - La sesión principal solo se observa con `pgrep` (A87, equivalente a A9).
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
for tool in xvfb-run Xvfb xprop xlsatoms xwininfo xev xdotool xmodmap mcookie pgrep ps cmp timeout column xwd xlogo magick sha256sum; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
for tool in /usr/bin/ls /usr/bin/mv /usr/bin/sleep; do
    [[ -x $tool ]] || missing+=("$tool")
done
if ((${#missing[@]})); then
    echo "run.sh: faltan herramientas: ${missing[*]}" >&2
    exit 2
fi

# Binario del WM y cliente de prueba: los decide Cargo, salvo un BINARIO
# explícito (P1-B.1). Antes de crear ningún directorio, servidor o cliente X.
# shellcheck source=../f1/binaries.sh
source "$F1/binaries.sh"
ensure_binaries "$ROOT" 1 "${1:-}" || exit 2

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN DBUS_SESSION_BUS_ADDRESS WINDOWID DESKTOP_STARTUP_ID CRIMSON_RULES

OUT=$(mktemp -d -t crimson-f8.XXXXXX) || exit 2
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
: >"$RESULTS"
: >"$CODES"
: >"$RUNS"

APPS=(kitty rofi thunar firefox flameshot nvim)
apps_snapshot() {
    local app
    for app in "${APPS[@]}"; do pgrep -x "$app" | sed "s/^/$app /"; done | sort >"$OUT/apps.$1"
}

snapshot antes
apps_snapshot antes

CASES=(A76 A77 A78 A79 A80 A81 A82 A83 A84 A85 A86)
for c in "${CASES[@]}"; do
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A87: estado posterior, pánicos y residuos (como A9 en F1), sin programas
# falsos en marcha ni procesos nuevos con el nombre de una aplicación.
snapshot despues
apps_snapshot despues
session_checks A87
# Cada programa falso tiene su propio grupo (pgid = pid): ningún proceso
# debe quedar en esos grupos, tampoco los sleep que lanzan.
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A87-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A87-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    $(extras A76) A76-flotante A76-foco-dialogo A76-encima A76-centrado A76-foco-al-propietario
    $(extras A77) A77-pila A77-cadena A77-foco-tras-d3 A77-foco-tras-d1 A77-pila-final
    $(extras A78) A78-sin-cambios A78-escritorio-de-d A78-al-mostrar A78-encima A78-centrado
    $(extras A79) A79-familia A79-origen A79-destino-pila A79-dialogo-solo A79-net-wm-desktop
    $(extras A80) A80-principal-destruida A80-principal-retirada A80-foco-historial
    $(extras A81) A81-sin-propietario A81-ciclo A81-pila-coherente
    $(extras A82) A82-pendiente A82-resuelto A82-sin-centrar
    $(extras A83) A83-foco-dialogo A83-familia-arriba A83-sigue-a-pantalla-completa A83-centrado
    A83-foco-a-la-principal A83-ajena
    $(extras A84) A84-cambio A84-sin-mover-ni-foco A84-foco-nuevo-propietario A84-otro-escritorio
    A84-escritorio-de-e A84-propia
    $(extras A85) A85-centrado A85-usposition A85-pposition A85-dentro-del-area A85-sin-recentrar
    $(extras A86) A86-antes A86-dialogo A86-dialogo-centrado A86-pantalla-completa A86-dimensiones
    A86-colores A86-composicion
    A87-sin-panico A87-xprop-:0 A87-sin-falsos-en-marcha A87-sin-aplicaciones-nuevas
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
