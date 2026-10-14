#!/bin/bash
# Pruebas de aceptación A63 y A65–A75 de F7 (ADR 0009): pantalla completa,
# solo con Xvfb y, como en F4–F6, con programas falsos en el PATH del WM.
# A74 hace capturas reales del Xvfb (`xwd`) y compone la comparación con
# ImageMagick.
#
# Uso: tests/acceptance/f7/run.sh [BINARIO]
#   BINARIO: por defecto target/debug/crimson-wm. Este y el cliente de
#   prueba target/debug/examples/test-client deben estar compilados y ser
#   más recientes que src/, examples/, Cargo.toml y Cargo.lock
#   (`cargo build --examples`).
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
# - La señal de A73 (SIGTERM) solo se envía al proceso crimson-wm hijo del
#   `timeout` que lanzó el propio arnés. La propiedad
#   _NET_SUPPORTING_WM_CHECK obsoleta que deja solo la borra el arnés,
#   explícitamente, en el Xvfb de la prueba; el WM nunca lo hace.
# - La sesión principal solo se observa con `pgrep` (A75, equivalente a A9).
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

BIN=${1:-$ROOT/target/debug/crimson-wm}
TEST_CLIENT=$ROOT/target/debug/examples/test-client
for b in "$BIN" "$TEST_CLIENT"; do
    if [[ ! -x $b ]]; then
        echo "run.sh: no existe $b (ejecuta cargo build --examples)" >&2
        exit 2
    fi
done
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")
# Cada binario se compara con sus fuentes: el WM con src/ y el cliente de
# prueba con examples/ (no depende de src/, y Cargo no lo vuelve a enlazar
# cuando solo cambia src/).
for b in "$BIN" "$TEST_CLIENT"; do
    sources=("$ROOT/Cargo.toml" "$ROOT/Cargo.lock")
    if [[ $b == "$BIN" ]]; then
        (($#)) && continue
        sources+=("$ROOT/src")
    else
        sources+=("$ROOT/examples")
    fi
    if [[ -n $(find "${sources[@]}" -newer "$b" -print -quit) ]]; then
        echo "run.sh: $b es anterior a las fuentes (ejecuta cargo build --examples)" >&2
        exit 2
    fi
done

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN DBUS_SESSION_BUS_ADDRESS WINDOWID DESKTOP_STARTUP_ID

OUT=$(mktemp -d -t crimson-f7.XXXXXX) || exit 2
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

CASES=(A63 A65 A66 A67 A68 A69 A70 A71 A72 A73 A74)
for c in "${CASES[@]}"; do
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A75: estado posterior, pánicos y residuos (como A9 en F1), sin programas
# falsos en marcha ni procesos nuevos con el nombre de una aplicación.
snapshot despues
apps_snapshot despues
session_checks A75
# Cada programa falso tiene su propio grupo (pgid = pid): ningún proceso
# debe quedar en esos grupos, tampoco los sleep que lanzan.
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A75-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A75-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    $(extras A63) A63-entra A63-estado A63-encima A63-sale-mosaico A63-sale-estado A63-mantenido
    $(extras A65) A65-anadir A65-anadir-otra-vez A65-alternar-sale A65-quitar-sin-estado A65-alternar-entra
    A65-quitar A65-invalidas A65-segundo-estado
    $(extras A66) A66-entra A66-sale A66-marco-inicial A66-mosaico
    $(extras A67) A67-mosaico-20 A67-flotante-20
    $(extras A68) A68-escritorio-1 A68-escritorio-1-geometria A68-escritorio-2 A68-escritorio-2-geometria
    A68-ultima-gana A68-oculta-sin-cambios A68-oculta-ultima-gana A68-vuelta A68-vuelta-pila
    $(extras A69) A69-mover A69-origen A69-origen-mosaico A69-destino A69-destino-geometria
    A69-net-wm-desktop A69-vuelta
    $(extras A70) A70-cerrar A70-foco-tras-cerrar A70-destruir A70-estado-ajeno A70-retirar
    A70-cien-estados A70-cien-estados-sale A70-demasiado-grande A70-mal-formada
    $(extras A71) A71-antes-del-mapeo A71-configure-denegada A71-flotante-inicial A71-flotante-restaurada
    $(extras A72) A72-encima A72-al-fondo A72-al-fondo-geometria A72-vuelve-arriba A72-dialogo
    A72-tras-dialogo A72-tras-dialogo-pila
    $(extras A73) A73-salida-normal A73-term A73-sigterm A73-tras-sigterm A73-adopcion A73-adopcion-estado A73-respaldo
    $(extras A74) A74-antes A74-pantalla-completa A74-restaurado A74-dimensiones A74-restaurado-identico
    A74-cubierta A74-composicion
    A75-sin-panico A75-xprop-:0 A75-sin-falsos-en-marcha A75-sin-aplicaciones-nuevas
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
