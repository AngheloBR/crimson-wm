#!/bin/bash
# Pruebas de aceptación A88–A103 de F9 (ADR 0011): tipos de ventana EWMH,
# ventanas fijas y capas de apilamiento, solo con Xvfb y, como en F4–F8, con
# programas falsos en el PATH del WM. A101 y A102 hacen capturas reales del
# Xvfb (`xwd`) y componen la comparación entre F8 y F9 con ImageMagick.
#
# Uso: tests/acceptance/f9/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide a Cargo (`cargo build --locked --offline`) el binario y el cliente
#   de prueba (examples/test-client.rs), y Cargo compila lo que haya quedado
#   obsoleto (f1/binaries.sh). El cliente se pide a Cargo en los dos casos.
#   CRIMSON_F8_BIN (opcional): binario de F8, compilado a partir de
#   `git archive 195938d` en un directorio temporal. Solo lo usa A101, en
#   su propio Xvfb y con las mismas guardas; sin él, A101 y la composición
#   de A102 quedan OMITIDAS.
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
# - F9 no envía señales al WM.
# - La sesión principal solo se observa con `pgrep` (A103, equivalente a A9).
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
F8_BIN=${CRIMSON_F8_BIN:-}
if [[ -n $F8_BIN ]]; then
    if [[ ! -x $F8_BIN || $(basename "$F8_BIN") != crimson-wm ]]; then
        echo "run.sh: CRIMSON_F8_BIN debe ser un binario crimson-wm ejecutable" >&2
        exit 2
    fi
    F8_BIN=$(cd "$(dirname "$F8_BIN")" && pwd)/crimson-wm
fi

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN DBUS_SESSION_BUS_ADDRESS WINDOWID DESKTOP_STARTUP_ID CRIMSON_F8_BIN

OUT=$(mktemp -d -t crimson-f9.XXXXXX) || exit 2
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

CASES=(A88 A89 A90 A91 A92 A93 A94 A95 A96 A97 A98 A99 A100 A101 A102)
for c in "${CASES[@]}"; do
    bin=$BIN
    if [[ $c == A101 ]]; then
        if [[ -z $F8_BIN ]]; then
            for id in A101 A101-aislamiento A101-pila A101-version A101-colores A101-extra-solo-falsos A101-extra-sin-errores-x; do
                record "$id" "binario de F8" "sin CRIMSON_F8_BIN" OMITIDA "comparación sin el panel de F8"
            done
            continue
        fi
        bin=$F8_BIN
        echo "A101: binario de F8 $(sha256sum "$F8_BIN" | cut -c1-16)…" >>"$RUNS"
    fi
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$bin" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A103: estado posterior, pánicos y residuos (como A9 en F1), sin programas
# falsos en marcha ni procesos nuevos con el nombre de una aplicación.
snapshot despues
apps_snapshot despues
session_checks A103
# Cada programa falso tiene su propio grupo (pgid = pid): ningún proceso
# debe quedar en esos grupos, tampoco los sleep que lanzan.
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A103-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A103-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    $(extras A88) A88-clasificacion A88-mosaico A88-flotante-sin-mover A88-mal-formadas
    $(extras A89) A89-dialog A89-toolbar A89-utility A89-utility-centrada A89-flotantes
    A89-transitoria-sin-tipo A89-mosaico
    $(extras A90) A90-flotantes A90-sin-foco A90-posicion A90-navegacion A90-mosaico
    $(extras A91) A91-sin-gestionar A91-listas A91-geometria A91-foco-intacto A91-orden-inicial
    A91-excepcion
    $(extras A92) A92-fijas A92-propiedades A92-sin-foco A92-navegacion A92-sin-reservar
    A92-capa-superior A92-escritorio-2 A92-siguen-fijas A92-client-list A92-peticiones-rechazadas
    A92-tras-peticiones A92-sin-pantalla-completa
    $(extras A93) A93-fija A93-al-fondo A93-sin-foco A93-flotante-encima A93-escritorio-2
    A93-visible-en-2 A93-subida-ignorada A93-sigue-al-fondo A93-geometria-concedida A93-tras-mover
    $(extras A94) A94-enfocada-arriba A94-area-completa A94-sin-foco A94-vuelve-arriba
    A94-dock-no-sube A94-foco-dialogo A94-familia
    $(extras A95) A95-splash-inicial A95-splash-sin-cambios A95-menu-peticion A95-dock-inicial
    A95-normal-admitida
    $(extras A96) A96-dialogo-de-fija A96-sin-centrar A96-foco A96-dialogo-normal A96-pila
    A96-dialogo-de-a
    $(extras A97) A97-ignorado A97-sigue-en-mosaico A97-reclasificada A97-fija-ignorado
    A97-fija-reclasificada
    $(extras A98) A98-antes A98-destruida A98-retirada-sintetica A98-desmapeada A98-propiedades
    A98-client-list A98-stacking A98-foco
    $(extras A99) A99-2 A99-adopcion A99-capas A99-foco A99-propiedades A99-salida A99-readopcion
    $(extras A100) A100-supported A100-client-list A100-stacking A100-estado
    $(extras A101) A101-pila A101-version A101-colores
    $(extras A102) A102-pila A102-colores-1 A102-escritorio-2 A102-menu-en-su-sitio A102-pila-2
    A102-colores-2 A102-dimensiones A102-composicion
    A103-sin-panico A103-xprop-:0 A103-sin-falsos-en-marcha A103-sin-aplicaciones-nuevas
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
