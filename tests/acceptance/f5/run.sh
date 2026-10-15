#!/bin/bash
# Pruebas de aceptación A41–A52 de F5 (ADR 0007): escritorios virtuales,
# solo con Xvfb y, como en F4, con programas falsos en el PATH del WM.
#
# Uso: tests/acceptance/f5/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide a Cargo (`cargo build --locked --offline`) el binario y el cliente
#   de prueba (examples/test-client.rs), y Cargo compila lo que haya quedado
#   obsoleto (f1/binaries.sh). El cliente se pide a Cargo en los dos casos.
#
# Requisitos (Debian 13): los de tests/acceptance/f4/run.sh.
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
# - Las señales de A51 (SIGTERM y SIGKILL) solo se envían al proceso
#   crimson-wm hijo del `timeout` que lanzó el propio arnés.
# - La sesión principal solo se observa con `pgrep` (A52, equivalente a A9).
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
for tool in xvfb-run Xvfb xprop xlsatoms xwininfo xev xdotool xmodmap mcookie pgrep ps cmp timeout column; do
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

OUT=$(mktemp -d -t crimson-f5.XXXXXX) || exit 2
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

CASES=(A41 A42 A43 A44 A45 A46 A47 A48 A49 A50 A51)
for c in "${CASES[@]}"; do
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A52: estado posterior, pánicos y residuos (como A9 en F1), sin programas
# falsos en marcha ni procesos nuevos con el nombre de una aplicación.
snapshot despues
apps_snapshot despues
session_checks A52
# Cada programa falso tiene su propio grupo (pgid = pid): ningún proceso
# debe quedar en esos grupos, tampoco los sleep que lanzan.
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A52-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A52-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    $(extras A41) A41-numero A41-escritorio-{1..8} A41-escritorio-8-de-nuevo A41-escritorio-1-de-nuevo
    A41-propiedades A41-aplicaciones-vivas A41-ewmh-declarado A41-sin-retiradas
    $(extras A42) A42-inicial A42-escritorio-2 A42-mosaico-2 A42-vuelta A42-vuelta-mosaico A42-vuelta-navegacion
    $(extras A43) A43-origen A43-origen-mosaico A43-propiedades A43-destino A43-destino-mosaico
    A43-propiedades-destino A43-mismo-escritorio A43-sin-ventana A43-vuelta
    $(extras A44) A44-inicial A44-vuelta A44-cierre-oculta A44-activo-intacto A44-foco-tras-cierre A44-mosaico-tras-cierre
    $(extras A45) A45-zoom-1 A45-zoom-2 A45-escritorio-1-intacto A45-mosaico-1-intacto A45-navegacion-aislada
    A45-escritorio-2-intacto A45-mosaico-2-intacto
    $(extras A46) A46-flotante A46-oculta A46-restaurada A46-geometria-y-pila A46-mover-flotante
    A46-mosaico-sin-flotante A46-flotante-en-destino A46-geometria-en-destino
    $(extras A47) A47-cierre-activo A47-mosaico-activo A47-mapeo-de-oculta A47-cierre-inactivo A47-mosaico-inactivo
    $(extras A48) A48-cambios-rapidos A48-tab-rapido A48-sin-retiradas-falsas A48-retirada-real
    A48-mosaico-tras-retirada A48-retirada-oculta A48-propiedades-retirada A48-retirada-no-vuelve
    A48-mapeo-de-oculta A48-carreras A48-control-no-retirada
    $(extras A49) A49-tab-sin-anterior A49-mismo-escritorio A49-sin-cambios A49-alterna A49-tab-mantenido
    A49-mover-mantenido A49-una-sola-movida
    $(extras A50) A50-mosaico A50-navegacion A50-zoom A50-lanzamiento A50-tras-cambios A50-hijo-sobrevive
    $(extras A51) A51-antes-de-salir A51-salida-normal A51-salida-propiedades A51-term A51-sigterm
    A51-tras-sigterm A51-kill A51-sigkill A51-aplicaciones-vivas
    A52-sin-panico A52-xprop-:0 A52-sin-falsos-en-marcha A52-sin-aplicaciones-nuevas
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
