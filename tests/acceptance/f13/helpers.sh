# shellcheck shell=bash
# Utilidades de F13: mismas guardas, entorno y observadores de F12.
# Se carga desde xvfb-case.sh con sus cuatro argumentos.
set -u
BIN=$1
OUT=$2
CASE=$3
TEST_CLIENT=$4
F1=$(cd "$(dirname "$0")/../f1" && pwd)
F2=$(cd "$(dirname "$0")/../f2" && pwd)
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
# shellcheck source=../f1/xvfb-env.sh
source "$F1/xvfb-env.sh"
# shellcheck source=../f2/windows.sh
source "$F2/windows.sh"

LOGS=$SANDBOX/lanzamientos
trap 'rc=$?; stop_fakes; (exit $rc); on_exit' EXIT

ROOT_ID=$(xwininfo -root | awk '/Window id:/ {print $4}')
install_fakes() {
    local app
    for app in "${APPS[@]}"; do
        cp "$FAKE" "$SANDBOX/bin/$app" && chmod 755 "$SANDBOX/bin/$app"
    done
}
# stop_fakes: como en F4, termina los programas falsos que sigan en marcha.
stop_fakes() {
    local f pid
    for f in "$LOGS"/*.*; do
        pid=${f##*.}
        [[ $pid =~ ^[0-9]+$ ]] || continue
        if tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null | grep -qF "$SANDBOX/bin/"; then
            kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null
        fi
    done
}
key() { xdotool key "$@"; }
ok_if() { if "$@"; then echo OK; else echo FALLA; fi; }
open_tc() {
    local name=$1
    shift
    open_window "$name" "$TEST_CLIENT" window "$name" "$@"
}

# Configuración (ADR 0014).
# shellcheck disable=SC2034 # leídas por xvfb-case.sh
XDG_FILE=$SANDBOX/config/crimson/wm.toml
# shellcheck disable=SC2034 # leída por xvfb-case.sh
HOME_FILE=$SANDBOX/home/.config/crimson/wm.toml
# write_config ruta: escribe la entrada estándar en esa ruta con permisos
# 0600.
write_config() {
    mkdir -p "${1%/*}"
    cat >"$1"
    chmod 600 "$1"
}
# start13: arranca el WM con los programas
# falsos instalados y registra la evidencia de aislamiento, como start11 en
# F11. WM_ENV se fija antes de llamarla (BASE_ENV por defecto).
# shellcheck disable=SC2034 # entorno base para reinicios en xvfb-case.sh
BASE_ENV=("${WM_ENV[@]}")
start13() {
    local app found summary='' names expected verdict=FALLA
    install_fakes
    set_marker "$TOKEN"
    start_wm "$CASE"
    require_ready
    real_pid
    for app in "${APPS[@]}"; do
        found=$(resolve "$app")
        if [[ $found == "$SANDBOX/bin/$app" ]] && cmp -s "$found" "$FAKE"; then
            summary+="$app=falso "
        else
            summary+="$app=¡${found:-ninguno}! "
        fi
    done
    names=$(while IFS= read -r -d '' entry; do printf '%s\n' "${entry%%=*}"; done <"/proc/$WM_REAL/environ" | sort | paste -sd' ')
    expected=$(printf '%s\n' "${WM_ENV[@]%%=*}" | sort | paste -sd' ')
    isolated && [[ $names == "$expected" ]] && verdict=OK
    record "$CASE-aislamiento" "solo programas falsos; entorno mínimo" "${summary% }" "$verdict" "variables del WM (pid $WM_REAL): $names"
}
real_pid() {
    WM_REAL=$(pgrep -P "$WM_PID" -x crimson-wm)
    precondition "pid del WM" test -n "$WM_REAL"
}
# restart id [argumentos...]: otra instancia del WM en el mismo Xvfb.
restart() {
    start_wm "$@"
    require_ready
    real_pid
}
# stop_with teclas: sale con el atajo de salida de la configuración.
stop_with() {
    key "$1"
    wait_wm 0 'atajo de salida recibido'
}
# stop_keep: sale con el atajo predeterminado sin cerrar las ventanas de
# prueba; la instancia siguiente las adopta.
stop_keep() { stop_with super+shift+Escape; }
# config_line: la línea de la configuración cargada, sin el prefijo.
config_line() { grep -m1 '^crimson-wm: configuración: ' "$WM_LOG" | sed 's/^crimson-wm: //'; }
# check_config: --check-config con el entorno del WM, sin DISPLAY,
# XAUTHORITY ni token. Deja la salida en CHECK_OUT y el código en CHECK_RC.
check_config() {
    local -a env_no_x=()
    local v
    for v in "${WM_ENV[@]}"; do
        case $v in DISPLAY=* | XAUTHORITY=* | CRIMSON_SANDBOX_TOKEN=*) ;; *) env_no_x+=("$v") ;; esac
    done
    CHECK_OUT=$(timeout 10 env -i "${env_no_x[@]}" "$BIN" --check-config 2>&1)
    CHECK_RC=$?
    echo "$CASE check_config -> $CHECK_RC" >>"$RUNS"
    printf '%s\n' "$CHECK_OUT" >>"$OUT/$CASE-check.log"
}
# server_state: átomos y propiedades de la raíz, para comprobar que una
# ejecución no dejó nada en el servidor.
server_state() {
    xlsatoms | wc -l
    xprop -root | sort
}
# launches programa: registros del programa falso.
launches() { find "$LOGS" -maxdepth 1 -regex ".*/$1\.[0-9]+" | sort; }
launch_count() { launches "$1" | wc -l; }
launch_count_is() { [[ $(launch_count "$1") == "$2" ]]; }
field() { sed -n "s/^$2=//p" "$1"; }

# Estado de las ventanas y de la raíz (como en F11).
name_of() {
    local n
    [[ $1 == "$ROOT_ID" ]] && { echo raíz; return; }
    [[ $1 == 0x0 ]] && { echo ninguna; return; }
    for n in "${!WIDS[@]}"; do
        [[ ${WIDS[$n]} == "$1" ]] && { echo "$n"; return; }
    done
    echo "$1"
}
focus_name() {
    local out
    out=$(xdotool getwindowfocus 2>&1)
    if [[ $out == *"focused window of 1."* ]]; then
        echo PointerRoot
    elif [[ $out =~ ^[0-9]+$ ]]; then
        name_of "$(printf '0x%x' "$out")"
    else
        echo "error: ${out%%$'\n'*}"
    fi
}
active_name() {
    local out
    out=$(xprop -root _NET_ACTIVE_WINDOW)
    case $out in
    *"not found"* | *"no such atom"*) echo ausente ;;
    *"window id # "*) name_of "${out##*# }" ;;
    *) echo "?$out" ;;
    esac
}
map_state() {
    case $(xwininfo -id "${WIDS[$1]}" 2>/dev/null | awk '/Map State:/ {print $3}') in
    IsViewable) echo v ;;
    IsUnMapped) echo o ;;
    "") echo destruida ;;
    *) echo "?" ;;
    esac
}
current_desk() {
    local v
    v=$(xprop -root _NET_CURRENT_DESKTOP | sed -n 's/^_NET_CURRENT_DESKTOP(CARDINAL) = \([0-9]*\)$/\1/p')
    if [[ -n $v ]]; then echo $((v + 1)); else echo ausente; fi
}
raw_desk() {
    local v
    v=$(xprop -id "${WIDS[$1]}" _NET_WM_DESKTOP 2>/dev/null | sed -n 's/^_NET_WM_DESKTOP(CARDINAL) = \([0-9]*\)$/\1/p')
    echo "${v:-ausente}"
}
desk_of() {
    local v
    v=$(raw_desk "$1")
    [[ $v =~ ^[0-9]+$ && $v -lt 8 ]] && echo $((v + 1)) || echo "$v"
}
state() {
    local n s
    s="escritorio=$(current_desk) foco=$(focus_name) activa=$(active_name)"
    for n in "$@"; do s+=" $n=$(map_state "$n")"; done
    echo "$s"
}
state_is() { [[ $(state "${@:2}") == "$1" ]]; }
expect() {
    local id=$1 want=$2 verdict=FALLA
    shift 2
    wait_for state_is "$want" "$@" && kill -0 "$WM_PID" && verdict=OK
    record "$id" "$want" "$(state "$@")" "$verdict" "_NET_CURRENT_DESKTOP, getwindowfocus, _NET_ACTIVE_WINDOW y Map State (v visible, o oculta)"
}
expect_stays() {
    local id=$1 want=$2 verdict=FALLA
    shift 2
    sleep 0.5
    [[ $(state "$@") == "$want" ]] && kill -0 "$WM_PID" && verdict=OK
    record "$id" "$want" "$(state "$@")" "$verdict" "sin cambios tras 0,5 s"
}
focus_is() { [[ $(focus_name) == "$1" ]]; }
logged() { grep -qF -- "$1" "$WM_LOG"; }
startup_finished() { ! kill -0 "$WM_PID" 2>/dev/null || xprop -root _NET_SUPPORTING_WM_CHECK | grep -q 'window id #'; }
managed_line() { grep -F "ventana ${WIDS[$1]} gestionada" "$WM_LOG" | tail -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]* //'; }
managed_as() { [[ $(managed_line "$1") == "gestionada ($2)"* ]]; }
rules_line() { grep -F "ventana ${WIDS[$1]}: reglas de las líneas" "$WM_LOG" | tail -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]*: //'; }
has_rules_line() { grep -qF "ventana ${WIDS[$1]}: reglas de las líneas" "$WM_LOG"; }
hidden_on() { logged "ventana ${WIDS[$1]} al escritorio $2 regla de la línea $3 (oculta)"; }
open_hidden() {
    local name=$1
    shift
    "$TEST_CLIENT" window "$name" "$@" >"$OUT/$CASE-$name.out" 2>&1 &
    PIDS[$name]=$!
    precondition "ventana $name creada" wait_for grep -q '^ventana 0x' "$OUT/$CASE-$name.out"
    WIDS[$name]=$(sed -n 's/^ventana //p' "$OUT/$CASE-$name.out" | head -n1)
}
open_dialog() {
    local name=$1 owner=${WIDS[$2]}
    shift 2
    open_window "$name" "$TEST_CLIENT" window "$name" --transient-for "$owner" "$@"
}
tree_names() {
    local id n names=''
    while read -r id; do
        n=$(name_of "$id")
        [[ $n != "$id" ]] && names="$n $names"
    done < <(xwininfo -root -children | awk '$1 ~ /^0x/ {print $1}')
    echo "${names% }"
}
order_of() {
    local n s=''
    for n in $(tree_names); do
        [[ " $* " == *" $n "* ]] && s+="$n "
    done
    echo "${s% }"
}
desk_is() { [[ $(current_desk) == "$1" ]]; }
