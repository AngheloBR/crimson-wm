#!/bin/bash
# Pruebas de aceptación A104–A120 de F10 (ADR 0012): struts y área de
# trabajo (`_NET_WM_STRUT`, `_NET_WM_STRUT_PARTIAL` y `_NET_WORKAREA`), solo
# con Xvfb y, como en F4–F9, con programas falsos en el PATH del WM. A118 y
# A119 hacen capturas reales del Xvfb (`xwd`) y componen la comparación entre
# F9 y F10 con ImageMagick.
#
# Uso: tests/acceptance/f10/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide a Cargo (`cargo build --locked --offline`) el binario y el cliente
#   de prueba (examples/test-client.rs), y Cargo compila lo que haya quedado
#   obsoleto (f1/binaries.sh). El cliente se pide a Cargo en los dos casos.
#   CRIMSON_F9_BIN (opcional): binario de F9, compilado a partir de
#   `git archive 2b2df85` en un directorio temporal. Solo lo usa A118, en
#   su propio Xvfb y con las mismas guardas; sin él, A118 y la composición
#   de A119 quedan OMITIDAS.
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
# - F10 no envía señales al WM.
# - La sesión principal solo se observa con `pgrep` (A120, equivalente a A9).
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
F9_BIN=${CRIMSON_F9_BIN:-}
if [[ -n $F9_BIN ]]; then
    if [[ ! -x $F9_BIN || $(basename "$F9_BIN") != crimson-wm ]]; then
        echo "run.sh: CRIMSON_F9_BIN debe ser un binario crimson-wm ejecutable" >&2
        exit 2
    fi
    F9_BIN=$(cd "$(dirname "$F9_BIN")" && pwd)/crimson-wm
fi

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN DBUS_SESSION_BUS_ADDRESS WINDOWID DESKTOP_STARTUP_ID CRIMSON_F9_BIN CRIMSON_RULES

OUT=$(mktemp -d -t crimson-f10.XXXXXX) || exit 2
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

CASES=(A104 A105 A106 A107 A108 A109 A110 A111 A112 A113 A114 A115 A116 A117 A118 A119)
for c in "${CASES[@]}"; do
    bin=$BIN
    if [[ $c == A118 ]]; then
        if [[ -z $F9_BIN ]]; then
            for id in A118 A118-aislamiento A118-mosaico A118-version A118-colores A118-extra-solo-falsos A118-extra-sin-errores-x; do
                record "$id" "binario de F9" "sin CRIMSON_F9_BIN" OMITIDA "comparación sin el panel de F9"
            done
            continue
        fi
        bin=$F9_BIN
        echo "A118: binario de F9 $(sha256sum "$F9_BIN" | cut -c1-16)…" >>"$RUNS"
    fi
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$bin" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A120: estado posterior, pánicos y residuos (como A9 en F1), sin programas
# falsos en marcha ni procesos nuevos con el nombre de una aplicación.
snapshot despues
apps_snapshot despues
session_checks A120
# Cada programa falso tiene su propio grupo (pgid = pid): ningún proceso
# debe quedar en esos grupos, tampoco los sleep que lanzan.
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A120-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A120-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    $(extras A104) A104-sin-panel A104-superior A104-registro A104-sin-foco A104-supported
    $(extras A105) A105-inferior
    $(extras A106) A106-izquierda A106-registro-strut A106-derecha
    $(extras A107) A107-maximo A107-dos-bordes A107-esquina A107-retirado-el-mayor
    $(extras A108) A108-tramo-parcial A108-fuera-de-la-pantalla
    $(extras A109) A109-partial-gana A109-strut-sin-partial A109-strut-k3 A109-partial-mal-formada
    A109-sin-recurrir-a-strut
    $(extras A110) A110-toda-la-altura A110-limitada A110-opuestos A110-tipo-integer A110-formato-8
    A110-tramo-invertido A110-defectos
    $(extras A111) A111-aparece A111-mover-no-cambia A111-retirada A111-vuelve A111-destruida
    A111-client-list
    $(extras A112) A112-dock-sin-struts A112-aparece-la-reserva A112-cambia-de-borde A112-se-borra
    A112-strut-dinamico
    $(extras A113) A113-activo A113-32-valores A113-oculto-sin-mover A113-al-mostrar A113-panel-visible
    $(extras A114) A114-mosaico A114-flotante-sin-mover A114-dialogo-dentro A114-dialogo-centrado
    $(extras A115) A115-pantalla-fisica A115-sobre-el-panel A115-vuelve-al-area A115-panel-encima
    $(extras A116) A116-2 A116-adopcion A116-salida A116-reinicio
    $(extras A117) A117-sin-reserva A117-sin-leer A117-cambio-ignorado
    $(extras A118) A118-mosaico A118-version A118-colores
    $(extras A119) A119-reserva A119-colores-1 A119-reajuste A119-colores-2 A119-dimensiones
    A119-composicion
    A120-sin-panico A120-xprop-:0 A120-sin-falsos-en-marcha A120-sin-aplicaciones-nuevas
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
