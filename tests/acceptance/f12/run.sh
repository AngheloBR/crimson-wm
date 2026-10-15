#!/bin/bash
# Pruebas de aceptación A136–A149 de F12 (ADR 0014): configuración TOML
# (crimson/wm.toml), con Xvfb y, como en F4–F11, con programas falsos en el
# PATH del WM. Los casos de configuraciones inválidas se ejecutan además con
# `unshare -rmn`, sin ningún servidor X visible, como A1–A3a de F1: un
# código 15 (y no 11) demuestra que la configuración se rechaza antes de
# intentar conectar. A147 y A148 hacen capturas reales del Xvfb (`xwd`) y
# componen la comparación sin configuración / con configuración.
#
# Uso: tests/acceptance/f12/run.sh [BINARIO]
#   BINARIO: un crimson-wm concreto, que se usa tal cual. Sin él, el arnés
#   pide a Cargo (`cargo build --locked --offline`) el binario y el cliente
#   de prueba (examples/test-client.rs), como en F5–F11 (f1/binaries.sh).
#
# Requisitos (Debian 13): los de tests/acceptance/f11/run.sh, más mkfifo
# (coreutils).
#
# - Cada prueba se ejecuta en un Xvfb nuevo de `xvfb-run`, con su cookie
#   temporal y sin TCP, con las mismas guardas que F1 (f1/xvfb-env.sh).
# - El WM se ejecuta con `env -i` y un entorno mínimo (f1/xvfb-env.sh): su
#   PATH es un único directorio temporal con copias de fake.sh. Las
#   aplicaciones reales nunca se ejecutan; los programas de las
#   configuraciones de prueba solo nombran esos programas falsos.
# - HOME y XDG_CONFIG_HOME apuntan a directorios temporales de la prueba:
#   los archivos de configuración del usuario nunca se leen. Las
#   configuraciones de prueba se escriben ahí o se pasan con --config.
# - CRIMSON_RULES solo llega al WM en A145.
# - F12 no envía señales al WM.
# - La sesión principal solo se observa con `pgrep` (A149, equivalente a A9).
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
for tool in xvfb-run Xvfb unshare xprop xlsatoms xwininfo xev xdotool xmodmap mcookie pgrep ps cmp timeout column xwd magick sha256sum mkfifo; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
for tool in /usr/bin/ls /usr/bin/mv /usr/bin/sleep; do
    [[ -x $tool ]] || missing+=("$tool")
done
if ((${#missing[@]})); then
    echo "run.sh: faltan herramientas: ${missing[*]}" >&2
    exit 2
fi

# shellcheck source=../f1/binaries.sh
source "$F1/binaries.sh"
ensure_binaries "$ROOT" 1 "${1:-}" || exit 2

# Nada de lo que se ejecute aquí debe heredar el acceso a la sesión real ni
# la configuración del usuario.
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN DBUS_SESSION_BUS_ADDRESS WINDOWID DESKTOP_STARTUP_ID CRIMSON_RULES XDG_CONFIG_HOME

OUT=$(mktemp -d -t crimson-f12.XXXXXX) || exit 2
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

# A139–A143 y A146, parte aislada: sin ningún servidor X accesible.
unshare -rmn bash "$F/isolated.sh" "$BIN" "$OUT" >"$OUT/isolated.txt" 2>&1
echo "unshare exit: $?" >>"$RUNS"

CASES=(A136 A137 A138 A139 A144 A145 A146 A147 A148)
for c in "${CASES[@]}"; do
    xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    echo "$c xvfb-run exit: $?" >>"$RUNS"
done

# A149: estado posterior, pánicos y residuos (como A9 en F1), sin programas
# falsos en marcha ni procesos nuevos con el nombre de una aplicación.
snapshot despues
apps_snapshot despues
session_checks A149
groups=$(find "$OUT" -path '*-entorno/lanzamientos/*' -regex '.*\.[0-9]+' -printf '%f\n' | sed 's/.*\.//' | sort -un | paste -sd,)
left=$({
    pgrep -f -- "$OUT/"
    [[ -n $groups ]] && pgrep -g "$groups"
} | sort -un | paste -sd' ')
record A149-sin-falsos-en-marcha ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" "pgrep -f $OUT/ y pgrep -g en los $(tr ',' '\n' <<<"$groups" | grep -c .) grupos de los programas falsos"
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A149-sin-aplicaciones-nuevas ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" "pgrep -x ${APPS[*]} antes y después"

# Cada comprobación esperada debe tener exactamente un resultado.
extras() { printf '%s ' "$1" "$1-aislamiento" "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# failed: casos en los que el WM no llega a arrancar (sin aislamiento que
# comprobar en un proceso en marcha).
failed() { printf '%s ' "$1-extra-solo-falsos" "$1-extra-sin-errores-x"; }
# bad id: arranque normal y --check-config de un caso inválido aislado.
bad() { printf '%s %s-check ' "$1" "$1"; }
# shellcheck disable=SC2207 # los identificadores no contienen espacios
expected=(
    A139-aislado
    $(bad A139-sintaxis) $(bad A139-repetida) $(bad A139-tabla-repetida)
    $(bad A140-tabla) $(bad A140-clave) $(bad A140-tipo) $(bad A140-rango)
    $(bad A141-repetido) $(bad A141-sin-exit) $(bad A141-tecla) $(bad A141-modificador)
    $(bad A141-escritura) $(bad A141-escritorio) $(bad A141-accion) $(bad A141-parametro)
    $(bad A142-argv-vacio) $(bad A142-nul) $(bad A142-sin-definir) $(bad A142-tilde)
    $(bad A142-variable) $(bad A142-relativa) $(bad A142-reemplazo) A142-sin-valores
    A143-tamano-exacto $(bad A143-tamano-excedido) A143-atajos-128 $(bad A143-atajos-129)
    A143-programas-64 $(bad A143-programas-65) A143-reglas-64 $(bad A143-reglas-65)
    A143-argv-32 $(bad A143-argv-33)
    $(bad A144-directorio) $(bad A144-enlace-roto) $(bad A144-no-utf8) $(bad A144-escribible)
    $(bad A144-fifo) A144-config-ausente A144-check-ausente $(bad A144-sin-respaldo)
    A146-display-0-valida A146-display-0-invalida A146-sin-token-invalida
    A146-check-display-0 A146-check-display-basura A146-sin-conexion
    $(extras A136) A136-fuente A136-mosaico A136-foco A136-lanzamiento A136-sin-reglas A136-check
    $(extras A137) A137-fuente A137-mosaico-2 A137-mosaico-3 A137-foco-alt-j A137-sin-super-j
    A137-zoom-prior A137-argv-literal A137-tecla-funcion A137-tecla-multimedia A137-sin-predeterminados
    A137-mover A137-ver A137-colision A137-numlock
    $(extras A138) A138-fuente A138-chat A138-primera-gana A138-flotante A138-viewer A138-nofocus
    A138-dialogo A138-check
    $(failed A139) A139 A139-xvfb-sin-conexion A139-xvfb-servidor-intacto
    $(failed A144) A144-xvfb-sin-lectura A144-xvfb-escribible A144-xvfb-fifo A144-xvfb-sin-conexion
    A144-xvfb-servidor-intacto
    $(extras A145) A145-config A145-mosaico-config A145-xdg A145-mosaico-xdg A145-home A145-mosaico-home
    A145-xdg-relativa A145-mosaico-xdg-relativa A145-predeterminada A145-mosaico-predeterminada
    A145-crimson-rules A145-sustituye A145-obsoleta A145-crimson-rules-invalida A145-invalida-sin-reglas
    $(failed A146) A146-marca-invalida A146-orden A146-check-en-xvfb A146-check-sin-conexion
    $(extras A147) A147-mosaico A147-colores
    $(extras A148) A148-mosaico A148-colores A148-dimensiones A148-composicion
    A149-sin-panico A149-xprop-:0 A149-sin-falsos-en-marcha A149-sin-aplicaciones-nuevas
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
