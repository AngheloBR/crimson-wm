#!/bin/bash
# Pruebas de aceptación A53–A62 y A64 de F6 (ADR 0008): compatibilidad
# ICCCM/EWMH, solo con Xvfb y, como en F4 y F5, con programas falsos en el
# PATH del WM. A63 (pantalla completa) queda para F7.
#
# Uso: tests/acceptance/f6/run.sh [BINARIO]
#   BINARIO: por defecto target/debug/crimson-wm. Este y el cliente de
#   prueba target/debug/examples/test-client deben estar compilados y ser
#   más recientes que src/, examples/, Cargo.toml y Cargo.lock
#   (`cargo build --examples`).
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
# - Las señales de A54 (SIGKILL) y A64 (SIGTERM) solo se envían al proceso
#   crimson-wm hijo del `timeout` que lanzó el propio arnés.
# - Tras una terminación abrupta, la propiedad _NET_SUPPORTING_WM_CHECK
#   obsoleta solo la borra el arnés, explícitamente, en el Xvfb de la
#   prueba (A54), como haría el operador del servidor; el WM nunca lo hace.
# - La sesión principal solo se observa con `pgrep` (A64, equivalente a A9).
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

BIN=${1:-$ROOT/target/debug/crimson-wm}
TEST_CLIENT=$ROOT/target/debug/examples/test-client
for b in "$BIN" "$TEST_CLIENT"; do
    if [[ ! -x $b ]]; then
        echo "run.sh: no existe $b (ejecuta cargo build --examples)" >&2
        exit 2
    fi
done
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")
# Cada binario se compara con sus fuentes: el WM no depende de examples/.
for b in "$BIN" "$TEST_CLIENT"; do
    sources=("$ROOT/src" "$ROOT/Cargo.toml" "$ROOT/Cargo.lock")
    if [[ $b == "$BIN" ]]; then
        (($#)) && continue
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

OUT=$(mktemp -d -t crimson-f6.XXXXXX) || exit 2
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

CASES=(A53 A54 A55 A56 A57 A58 A59 A60 A61 A62 A64)
for c in "${CASES[@]}"; do
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A64 (en la sesión): estado posterior, pánicos y residuos (como A9 en F1),
# sin programas falsos en marcha ni procesos nuevos con el nombre de una
# aplicación.
snapshot despues
apps_snapshot despues
session_checks A64
# Cada programa falso tiene su propio grupo (pgid = pid): ningún proceso
# debe quedar en esos grupos, tampoco los sleep que lanzan.
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A64-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A64-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    $(extras A53) A53-comprobacion A53-ventana A53-supported A53-wm-s0 A53-manager A53-propiedades-raiz
    A53-limpieza-raiz A53-ventana-y-seleccion-liberadas A53-reinicio A53-reinicio-limpio
    A54 A54-extra-solo-falsos A54-extra-sin-errores-x A54-check-viva A54-check-viva-intacta
    A54-check-obsoleta A54-check-obsoleta-intacta A54-wm-s0-ocupada A54-wm-s0-intacta A54-capa-4
    A54-sigkill A54-tras-sigkill A54-tras-sigkill-intacta A54-recuperado A54-recuperado-identificado
    A54-relevo
    $(extras A55) A55-cooperativo A55-tras-cierre A55-mosaico A55-xev A55-mantenido
    $(extras A56) A56-sin-protocolo A56-forzado A56-misma-conexion A56-queda-A
    $(extras A57) A57-activo A57-oculta A57-sin-protocolo A57-invalidas
    $(extras A58) A58-vacia A58-tres A58-cierre-oculta A58-movida A58-retirada A58-nueva
    $(extras A59) A59-flotante-arriba A59-subida-denegada A59-con-ocultas A59-tras-mover
    $(extras A60) A60-aplicacion-mismo-escritorio A60-aplicacion-otro-escritorio A60-sin-indicacion
    A60-paginador A60-xdotool A60-invalidas A60-sin-cambios
    $(extras A62) A62-ventana-nueva A62-pulsacion A62-protocolos-retirados A62-protocolos-anadidos
    A62-cierre-tras-cambio A62-sin-bucles
    $(extras A61) A61-get-desktop A61-set-desktop A61-oculta-a-oculto A61-al-activo A61-al-activo-mosaico
    A61-invalidas A61-sin-cambios A61-escritorio-de-A
    A64-aislamiento A64-extra-solo-falsos A64-extra-sin-errores-x A64-sigterm A64-tras-sigterm
    A64-tras-sigterm-intacta
    A64-sin-panico A64-xprop-:0 A64-sin-falsos-en-marcha A64-sin-aplicaciones-nuevas
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
