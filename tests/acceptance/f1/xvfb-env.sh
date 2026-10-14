# shellcheck shell=bash
# Preparación común de las pruebas que se ejecutan dentro de un Xvfb nuevo
# (F1 a F4). La cargan xvfb-case.sh de cada fase, después de lib.sh, con
# BIN, OUT y CASE definidas, como orden de `xvfb-run` (Debian 13), que fija
# DISPLAY y XAUTHORITY.
#
# Define XVFB_PID, SIZE (pantalla, p. ej. 1280x720), N (número de display),
# CRIMSON_SANDBOX_TOKEN, el entorno aislado del WM (SANDBOX, WM_ENV) y las
# funciones precondition, set_marker, require_ready, isolated, etc. Mantiene
# un cliente pasivo (HOLD) durante toda la prueba y registra el código de
# salida del script en RUNS.

HOLD=
on_exit() {
    local rc=$?
    [[ -n $HOLD ]] && kill "$HOLD" 2>/dev/null
    echo "$CASE xvfb-case.sh exit: $rc" >>"$RUNS"
}
trap on_exit EXIT

# Recursos propios (ADR 0010, F8-B.1): el Xvfb que lanzó este xvfb-run, su
# display y la cookie, dentro del directorio temporal de xvfb-run. Con esta
# lista, session_checks (lib.sh) distingue los residuos del arnés de los de
# otros Xvfb del sistema. Solo lee /proc y se anota antes de las guardas,
# para que un caso abortado también quede atribuido; los valores se validan
# al usarlos.
printf '%s\t%s\t%s\t%s\t%s\n' "$CASE" "$PPID" "$(pgrep -P "$PPID" -x Xvfb | paste -sd,)" \
    "${DISPLAY:-}" "${XAUTHORITY:-}" >>"$OUT/xvfb-propios.tsv"

# Guardas: solo se continúa dentro del Xvfb de este xvfb-run, con su cookie
# temporal, sin TCP y sin -ac. Si algo no encaja, no se conecta nada.
if ! [[ ${DISPLAY:-} =~ ^:[1-9][0-9]*$ ]]; then
    echo "GUARDA: DISPLAY inesperado"
    exit 90
fi
if [[ ${XAUTHORITY:-} != */xvfb-run.*/Xauthority || ! -s $XAUTHORITY ]]; then
    echo "GUARDA: XAUTHORITY no es la cookie temporal de xvfb-run"
    exit 90
fi
XVFB_PID=$(pgrep -P "$PPID" -x Xvfb) || {
    echo "GUARDA: no hay un Xvfb hijo de xvfb-run"
    exit 90
}
mapfile -d '' XVFB_ARGS <"/proc/$XVFB_PID/cmdline"
if [[ ${XVFB_ARGS[0]} != Xvfb || ${XVFB_ARGS[1]} != "$DISPLAY" ]]; then
    echo "GUARDA: el Xvfb hijo no sirve $DISPLAY"
    exit 90
fi
auth_ok=0 nolisten_ok=0 SIZE=
for ((i = 2; i < ${#XVFB_ARGS[@]}; i++)); do
    case ${XVFB_ARGS[i]} in
    -ac | -listen)
        echo "GUARDA: Xvfb arrancado con ${XVFB_ARGS[i]}"
        exit 90
        ;;
    -auth) [[ ${XVFB_ARGS[i + 1]:-} == "$XAUTHORITY" ]] && auth_ok=1 ;;
    -nolisten) [[ ${XVFB_ARGS[i + 1]:-} == tcp ]] && nolisten_ok=1 ;;
    -screen) [[ ${XVFB_ARGS[i + 2]:-} =~ ^([0-9]+x[0-9]+)x[0-9]+$ ]] && SIZE=${BASH_REMATCH[1]} ;;
    esac
done
if ((!auth_ok || !nolisten_ok)) || [[ -z $SIZE ]]; then
    echo "GUARDA: Xvfb sin -auth con la cookie temporal, sin -nolisten tcp o sin -screen"
    exit 90
fi
echo "$CASE: DISPLAY=$DISPLAY, Xvfb pid $XVFB_PID, pantalla $SIZE, -auth <cookie temporal>, -nolisten tcp"

# shellcheck disable=SC2034 # la usan los scripts que cargan xvfb-env.sh
N=${DISPLAY#:}
TOKEN=$(mcookie)
export CRIMSON_SANDBOX_TOKEN=$TOKEN

# Entorno del WM (ADR 0006). start_wm lo ejecuta con `env -i` y solo estas
# variables: nada de la sesión real llega al WM ni a los programas que
# lance. Su PATH contiene únicamente $SANDBOX/bin, vacío salvo en F4, donde
# xvfb-case.sh copia allí los programas falsos (f4/fake.sh). HOME, XDG_* y
# el bus de D-Bus apuntan a directorios temporales de esta prueba; el bus no
# existe.
SANDBOX=$OUT/$CASE-entorno
mkdir -p "$SANDBOX"/{bin,home,config,cache,data,state,lanzamientos}
mkdir -m 700 "$SANDBOX/runtime"
# shellcheck disable=SC2034 # la usa start_wm (lib.sh)
WM_ENV=(
    DISPLAY="$DISPLAY"
    XAUTHORITY="$XAUTHORITY"
    CRIMSON_SANDBOX_TOKEN="$TOKEN"
    PATH="$SANDBOX/bin"
    HOME="$SANDBOX/home"
    XDG_CONFIG_HOME="$SANDBOX/config"
    XDG_CACHE_HOME="$SANDBOX/cache"
    XDG_DATA_HOME="$SANDBOX/data"
    XDG_STATE_HOME="$SANDBOX/state"
    XDG_RUNTIME_DIR="$SANDBOX/runtime"
    DBUS_SESSION_BUS_ADDRESS="unix:path=$SANDBOX/runtime/sin-bus"
)
# Aplicaciones de los atajos de lanzamiento y las que lanzarían a su vez.
APPS=(kitty rofi thunar firefox flameshot nvim)
FAKE=$(cd "$(dirname "${BASH_SOURCE[0]}")/../f4" && pwd)/fake.sh
# resolve aplicación: ruta con la que la encontraría el WM (execvp en su
# PATH), vacía si no hay ninguna. Se resuelve en un bash nuevo, sin tabla
# de órdenes recordadas ni más entorno que ese PATH.
# shellcheck disable=SC2016 # $1 lo expande el bash nuevo
resolve() { env -i PATH="$SANDBOX/bin" "$BASH" -c 'command -v -- "$1"' _ "$1"; }
# isolated: el PATH del WM es un único directorio de esta prueba que solo
# contiene copias exactas de f4/fake.sh, y ninguna aplicación se resuelve
# fuera de él.
isolated() {
    local entry app found
    [[ -d $SANDBOX/bin && ! -L $SANDBOX/bin && -f $FAKE ]] || return 1
    for entry in "$SANDBOX"/bin/* "$SANDBOX"/bin/.[!.]*; do
        [[ -e $entry || -L $entry ]] || continue
        [[ -f $entry && ! -L $entry ]] && cmp -s "$entry" "$FAKE" || return 1
    done
    for app in "${APPS[@]}"; do
        found=$(resolve "$app")
        [[ -z $found || $found == "$SANDBOX/bin/$app" ]] || return 1
    done
}

# Xvfb se reinicia, y borra propiedades y átomos de la raíz, cuando se
# desconecta su último cliente (ADR 0003). Un cliente pasivo mantiene el
# servidor vivo durante toda la prueba; se confirma con una propiedad de
# control, que también sirve para detectar un reinicio inesperado.
xprop -root -spy >/dev/null 2>&1 &
HOLD=$!
holding() {
    kill -0 "$HOLD" &&
        xprop -root -f _CRIMSON_CONTROL 8s -set _CRIMSON_CONTROL 1 &&
        xprop -root _CRIMSON_CONTROL | grep -qx '_CRIMSON_CONTROL(STRING) = "1"'
}
if ! wait_for holding; then
    record "$CASE-precondicion" "servidor sin reinicios" "no confirmado" FALLA "_CRIMSON_CONTROL no persiste"
    exit 1
fi

# precondition descripción orden...: registra un fallo y aborta si no se cumple.
precondition() {
    local what=$1
    shift
    if ! "$@" >/dev/null 2>&1; then
        record "$CASE-precondicion" "$what" "no se cumple" FALLA "$*"
        exit 1
    fi
}
marker_is() { xprop -root _CRIMSON_SANDBOX | grep -qxF "_CRIMSON_SANDBOX($1) = \"$2\""; }
marker_differs() { xprop -root _CRIMSON_SANDBOX | grep -q '^_CRIMSON_SANDBOX(STRING) = ' && ! xprop -root _CRIMSON_SANDBOX | grep -qF -- "$TOKEN"; }
wm_check_present() { xprop -root _NET_SUPPORTING_WM_CHECK | grep -q '= 1$'; }
no_marker_atom() { [[ -z $(xlsatoms -name _CRIMSON_SANDBOX) ]]; }
set_marker() {
    xprop -root -f _CRIMSON_SANDBOX 8s -set _CRIMSON_SANDBOX "$1"
    precondition "marca presente" marker_is STRING "$1"
}
ready() { grep -q 'salvaguardas superadas' "$WM_LOG" && kill -0 "$WM_PID"; }
# require_ready: aborta la prueba si el WM no llega a gestionar la raíz.
require_ready() {
    if ! wait_for ready; then
        kill -0 "$WM_PID" 2>/dev/null && kill "$WM_PID"
        wait "$WM_PID"
        finish "$WM_ID" "en marcha" "$?" 'salvaguardas superadas' "$WM_LOG"
        record "$CASE-precondicion" "WM en marcha" "no arrancó" FALLA "ver $WM_ID.log"
        exit 1
    fi
}
