#!/bin/bash
# Pruebas de aceptación A121–A135 de F11 (ADR 0013): reglas de ventanas
# (`CRIMSON_RULES`), con Xvfb y, como en F4–F10, con programas falsos en el
# PATH del WM. Parte de A132 se ejecuta con `unshare -rmn`, sin ningún
# servidor X visible, como A1–A3a de F1. A133 y A134 hacen capturas reales
# del Xvfb (`xwd`) y componen la comparación entre F10 y F11 con
# ImageMagick.
#
# Uso: tests/acceptance/f11/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide a Cargo (`cargo build --locked --offline`) el binario y el cliente
#   de prueba (examples/test-client.rs), y Cargo compila lo que haya quedado
#   obsoleto (f1/binaries.sh). El cliente se pide a Cargo en los dos casos.
#   CRIMSON_F10_BIN (opcional): binario de F10, compilado a partir de
#   `git archive f90dfa2` en un directorio temporal. Solo lo usa A133, en
#   su propio Xvfb y con las mismas guardas; sin él, A133 y la composición
#   de A134 quedan OMITIDAS.
#
# Requisitos (Debian 13): los de tests/acceptance/f4/run.sh, unshare y
# mount (util-linux, mount) con espacios de nombres de usuario sin
# privilegios, más
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
# - CRIMSON_RULES solo llega al WM en este arnés, a través de start11.
# - F11 no envía señales al WM.
# - La sesión principal solo se observa con `pgrep` (A135, equivalente a A9).
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
for tool in xvfb-run Xvfb unshare xprop xlsatoms xwininfo xev xdotool xmodmap mcookie pgrep ps cmp timeout column xwd xlogo magick sha256sum; do
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
F10_BIN=${CRIMSON_F10_BIN:-}
if [[ -n $F10_BIN ]]; then
    if [[ ! -x $F10_BIN || $(basename "$F10_BIN") != crimson-wm ]]; then
        echo "run.sh: CRIMSON_F10_BIN debe ser un binario crimson-wm ejecutable" >&2
        exit 2
    fi
    F10_BIN=$(cd "$(dirname "$F10_BIN")" && pwd)/crimson-wm
fi

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN DBUS_SESSION_BUS_ADDRESS WINDOWID DESKTOP_STARTUP_ID CRIMSON_F10_BIN CRIMSON_RULES

OUT=$(mktemp -d -t crimson-f11.XXXXXX) || exit 2
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

# A132, parte aislada: sin ningún servidor X accesible.
unshare -rmn bash "$F/isolated.sh" "$BIN" "$OUT" >"$OUT/isolated.txt" 2>&1
echo "unshare exit: $?" >>"$RUNS"

CASES=(A121 A122 A123 A124 A125 A126 A127 A128 A129 A130 A131 A132 A133 A134)
for c in "${CASES[@]}"; do
    bin=$BIN
    if [[ $c == A133 ]]; then
        if [[ -z $F10_BIN ]]; then
            for id in A133 A133-aislamiento A133-mosaico A133-version A133-colores A133-extra-solo-falsos A133-extra-sin-errores-x; do
                record "$id" "binario de F10" "sin CRIMSON_F10_BIN" OMITIDA "comparación sin el panel de F10"
            done
            continue
        fi
        bin=$F10_BIN
        echo "A133: binario de F10 $(sha256sum "$F10_BIN" | cut -c1-16)…" >>"$RUNS"
    fi
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$bin" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A135: estado posterior, pánicos y residuos (como A9 en F1), sin programas
# falsos en marcha ni procesos nuevos con el nombre de una aplicación.
snapshot despues
apps_snapshot despues
session_checks A135
# Cada programa falso tiene su propio grupo (pgid = pid): ningún proceso
# debe quedar en esos grupos, tampoco los sleep que lanzan.
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A135-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A135-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    $(extras A121) A121-mosaico A121-foco A121-sin-reglas
    $(extras A122) A122-regla A122-oculta A122-propiedades A122-al-mostrar A122-mosaico-en-3
    $(extras A123) A123-sin-coincidencia A123-y-logico A123-estado
    $(extras A124) A124-defectos A124-sin-regla A124-otra-ventana
    $(extras A125) A125-flotante A125-geometria A125-encima A125-navegacion
    $(extras A126) A126-sin-foco-inicial A126-mosaico A126-foco-posterior
    $(extras A127) A127-admitida A127-foco A127-rechazada
    $(extras A128) A128-primera-gana A128-ignorada A128-acciones A128-sin-foco A128-escritorio
    A128-geometria
    $(extras A129) A129-escritorio-ignorado A129-con-su-principal A129-escritorio A129-centrado
    A129-transient-nofocus
    $(extras A130) A130-sin-reglas A130-propiedades A130-sin-pantalla-completa A130-escritorio-2
    $(extras A131) A131-sin-reevaluar A131-al-volver-a-mapear A131-oculta
    $(extras A132) A132-sintaxis A132-sin-aplicar A132-2 A132-demasiadas A132-3 A132-demasiado-grande
    A132-4 A132-no-utf8 A132-sin-marca
    A132-aislado A132-display-0-con-reglas A132-display-0-reglas-invalidas A132-sin-token-con-reglas
    A132-sin-conexion
    $(extras A133) A133-mosaico A133-version A133-colores
    $(extras A134) A134-reglas A134-escritorio-1 A134-colores-1 A134-escritorio-2 A134-chat
    A134-colores-2 A134-dimensiones A134-composicion
    A135-sin-panico A135-xprop-:0 A135-sin-falsos-en-marcha A135-sin-aplicaciones-nuevas
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
