#!/bin/bash
# Una prueba de aceptación de F10 (ADR 0012) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh), y las de estado, las mismas que en F9. Como en F4–F9, el
# único directorio del PATH del WM contiene copias de f4/fake.sh: ninguna
# aplicación real puede ejecutarse.
#
# TEST_CLIENT (examples/test-client.rs) crea paneles con struts
# (`--strut`, `--strut-partial`); los valores mal formados y los cambios
# dinámicos se escriben con xprop. A118 se ejecuta con el binario de F9
# (commit 2b2df85) y A119 con el de F10; los dos hacen capturas reales del
# Xvfb con `xwd`.
# Uso: xvfb-case.sh BINARIO DIRECTORIO_DE_RESULTADOS CASO CLIENTE_DE_PRUEBA
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
# open_tc nombre [opciones...]: ventana de test-client (WM_DELETE_WINDOW por
# defecto) que registra en $OUT/$CASE-nombre.out los mensajes recibidos.
open_tc() {
    local name=$1
    shift
    open_window "$name" "$TEST_CLIENT" window "$name" "$@"
}

# start10: arranca el WM con los programas falsos instalados y registra la
# evidencia de aislamiento, como start4 en F4 (solo nombres de variables).
start10() {
    local app found summary='' names expected verdict=FALLA
    install_fakes
    start
    real_pid
    for app in "${APPS[@]}"; do
        found=$(resolve "$app")
        if [[ $found == "$SANDBOX/bin/$app" ]] && cmp -s "$found" "$FAKE"; then
            summary+="$app=falso "
        else
            summary+="$app=¡${found:-ninguno}! "
        fi
    done
    names=$(tr '\0' '\n' <"/proc/$WM_REAL/environ" | cut -d= -f1 | sort | paste -sd' ')
    expected=$(printf '%s\n' "${WM_ENV[@]%%=*}" | sort | paste -sd' ')
    isolated && [[ $names == "$expected" ]] && verdict=OK
    record "$CASE-aislamiento" "solo programas falsos; entorno mínimo" "${summary% }" "$verdict" "variables del WM (pid $WM_REAL): $names"
}
# WM_REAL: el proceso crimson-wm hijo del `timeout` que lanzó el arnés.
real_pid() {
    WM_REAL=$(pgrep -P "$WM_PID" -x crimson-wm)
    precondition "pid del WM" test -n "$WM_REAL"
}
# restart id: otra instancia del WM en el mismo Xvfb.
restart() {
    start_wm "$1"
    require_ready
    real_pid
}

# Estado de las ventanas y de la raíz (como en F8).
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
# raw_desk nombre: _NET_WM_DESKTOP tal cual (4294967295 en las fijas) o
# «ausente».
raw_desk() {
    local v
    v=$(xprop -id "${WIDS[$1]}" _NET_WM_DESKTOP 2>/dev/null | sed -n 's/^_NET_WM_DESKTOP(CARDINAL) = \([0-9]*\)$/\1/p')
    echo "${v:-ausente}"
}
# wm_state_of nombre: Normal, Iconic o ausente.
wm_state_of() {
    local v
    v=$(xprop -id "${WIDS[$1]}" WM_STATE 2>/dev/null | sed -n 's/^[[:space:]]*window state: //p')
    echo "${v:-ausente}"
}
# props nombres...: «K=Normal/4294967295 …» (WM_STATE / _NET_WM_DESKTOP).
props() {
    local n s=''
    for n in "$@"; do s+="$n=$(wm_state_of "$n")/$(raw_desk "$n") "; done
    echo "${s% }"
}
props_are() { [[ $(props "${@:2}") == "$1" ]]; }
expect_props() {
    local id=$1 want=$2 verdict=FALLA
    shift 2
    wait_for props_are "$want" "$@" && verdict=OK
    record "$id" "$want" "$(props "$@")" "$verdict" "WM_STATE / _NET_WM_DESKTOP de cada ventana"
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
desk_is() { [[ $(current_desk) == "$1" ]]; }
go() {
    key "super+$1"
    precondition "escritorio $1 activo" wait_for desk_is "$1"
}
focus_is() { [[ $(focus_name) == "$1" ]]; }
gone() { [[ $(map_state "$1") == destruida ]]; }
logged() { grep -qF -- "$1" "$WM_LOG"; }
count_logged() { grep -cF -- "$1" "$WM_LOG"; }
count_is() { [[ $(count_logged "$1") == "$2" ]]; }
# managed_as nombre clase: el WM registró que gestiona la ventana así
# («mosaico», «flotante», «fija, capa superior»…), la última vez.
managed_line() { grep -F "ventana ${WIDS[$1]} gestionada" "$WM_LOG" | tail -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]* //'; }
managed_as() { [[ $(managed_line "$1") == "gestionada ($2)"* ]]; }
# Lista de ventanas de una propiedad de la raíz, por nombre.
list_names() {
    local out ids id names=''
    out=$(xprop -root "$1")
    [[ $out == *"window id # "* ]] && ids=${out##*# } || ids=
    for id in ${ids//,/ }; do names+="$(name_of "$id") "; done
    names=${names% }
    echo "${names:-vacía}"
}
list_is() { [[ $(list_names "$1") == "$2" ]]; }
expect_list() {
    local verdict=FALLA
    wait_for list_is "$2" "$3" && verdict=OK
    record "$1" "$3" "$(list_names "$2")" "$verdict" "xprop -root $2"
}
# Orden real de las ventanas de WIDS, de abajo arriba (xwininfo lista
# primero la de arriba).
tree_names() {
    local id n names=''
    while read -r id; do
        n=$(name_of "$id")
        [[ $n != "$id" ]] && names="$n $names"
    done < <(xwininfo -root -children | awk '$1 ~ /^0x/ {print $1}')
    echo "${names% }"
}
# _NET_CLIENT_LIST_STACKING coincide con QueryTree, sin las ventanas que el
# WM no gestiona (override-redirect).
stacking_matches() {
    local n tree=''
    for n in $(tree_names); do
        [[ " ${UNMANAGED:-} " == *" $n "* ]] || tree+="$n "
    done
    [[ $(list_names _NET_CLIENT_LIST_STACKING) == "${tree% }" ]]
}
expect_stacking() {
    local verdict=FALLA
    wait_for stacking_matches && [[ $(list_names _NET_CLIENT_LIST_STACKING) == "$2" ]] && verdict=OK
    record "$1" "$2 (igual que QueryTree)" "$(list_names _NET_CLIENT_LIST_STACKING); QueryTree: $(tree_names)" "$verdict" "xprop -root _NET_CLIENT_LIST_STACKING y xwininfo -root -children"
}
# order_of nombres...: orden real de apilamiento (de abajo arriba) de esas
# ventanas, según QueryTree.
order_of() {
    local n s=''
    for n in $(tree_names); do
        [[ " $* " == *" $n "* ]] && s+="$n "
    done
    echo "${s% }"
}
order_is() { [[ $(order_of "${@:2}") == "$1" ]]; }
expect_order() {
    local id=$1 want=$2 verdict=FALLA
    shift 2
    wait_for order_is "$want" "$@" && wait_for stacking_matches && verdict=OK
    record "$id" "$want (de abajo arriba)" "$(order_of "$@"); _NET_CLIENT_LIST_STACKING = QueryTree: $(stacking_matches && echo sí || echo no)" "$verdict" "xwininfo -root -children y xprop -root _NET_CLIENT_LIST_STACKING"
}
# frame nombre: «x y ancho alto borde».
frame() {
    xwininfo -id "${WIDS[$1]}" 2>/dev/null |
        awk '/Absolute upper-left X|Absolute upper-left Y|Width:|Height:|Border width:/ {printf "%s%s", sep, $NF; sep = " "}'
}
frame_is() { [[ $(frame "$1") == "$2" ]]; }
expect_frame() {
    local verdict=FALLA
    wait_for frame_is "$2" "$3" && verdict=OK
    record "$1" "$2=$3" "$2=$(frame "$2")" "$verdict" "xwininfo: x, y, ancho, alto y borde"
}
# net_state nombre: átomos de _NET_WM_STATE separados por comas, «vacía» o
# «ausente» (como en F7 y F8).
net_state() {
    local out
    out=$(xprop -id "${WIDS[$1]}" _NET_WM_STATE 2>/dev/null)
    case $out in
    *"not found"* | *"no such atom"* | "") echo ausente ;;
    *"= "*)
        out=${out#*= }
        out=${out// /}
        echo "${out:-vacía}"
        ;;
    *) echo vacía ;;
    esac
}
fullscreen_is() { [[ $(net_state "$1") == *_NET_WM_STATE_FULLSCREEN* ]]; }
net_state_is() { [[ $(net_state "$1") == "$2" ]]; }
# atom_id nombre: número del átomo en este servidor (lo crea si no existe).
atom_id() {
    "$TEST_CLIENT" message 32 root "$1" 0 0 0 0 0 >/dev/null
    xlsatoms -name "$1" | awk '{print $1}'
}
# message nombre tipo d0…d4: ClientMessage EWMH sobre la ventana.
message() { "$TEST_CLIENT" message 32 "${WIDS[$1]}" "${@:2}"; }
# wm_state nombre acción fuente: petición _NET_WM_STATE de FULLSCREEN.
wm_state() { message "$1" _NET_WM_STATE "$2" "$FS" 0 "$3" 0; }
# open_dialog nombre propietario [opciones...]: ventana de test-client
# transitoria de la ventana `propietario` (nombre de WIDS).
open_dialog() {
    local name=$1 owner=${WIDS[$2]}
    shift 2
    open_window "$name" "$TEST_CLIENT" window "$name" --transient-for "$owner" "$@"
}
# focus_on nombre: Mod4+j hasta enfocar la ventana (como en F5).
focus_on() {
    local i
    for ((i = 0; i < 8; i++)); do
        wait_for focus_is "$1" && return 0
        key super+j
    done
    return 1
}
# remap nombre: vuelve a mapear una ventana retirada (el WM la clasifica
# de nuevo) y espera a que sea visible.
remap() {
    xdotool windowmap "${WIDS[$1]}"
    precondition "ventana $1 visible de nuevo" wait_for viewable "$1"
}
# retire_n nombre n: desmapea la ventana y espera a su n-ésima retirada.
retire_n() {
    xdotool windowunmap "${WIDS[$1]}"
    precondition "ventana $1 retirada" wait_for count_is "ventana ${WIDS[$1]} retirada" "$2"
}
rejected_fixed() { count_logged "petición EWMH rechazada para ${WIDS[$1]}: ventana fija"; }

precondition "pantalla 1280x720" test "$SIZE" = 1280x720
FULL="0 0 1280 720"
LEFT="0 0 640 720"
RIGHT="640 0 640 720"
FS=$(atom_id _NET_WM_STATE_FULLSCREEN)
precondition "átomo de FULLSCREEN" test -n "$FS"

# Área de trabajo (ADR 0012).
# workarea: las 8 entradas de _NET_WORKAREA como «x y ancho alto», o «8
# distintas: …» si no son iguales, o «ausente».
workarea() {
    local out v first i
    local -a nums
    out=$(xprop -root _NET_WORKAREA)
    case $out in
    *"not found"* | *"no such atom"*) echo ausente; return ;;
    esac
    v=${out#*= }
    v=${v//,/}
    read -ra nums <<<"$v"
    if ((${#nums[@]} != 32)); then
        echo "${#nums[@]} valores: $v"
        return
    fi
    first="${nums[*]:0:4}"
    for ((i = 4; i < 32; i += 4)); do
        [[ "${nums[*]:i:4}" == "$first" ]] || { echo "8 distintas: $v"; return; }
    done
    echo "$first"
}
workarea_is() { [[ $(workarea) == "$1" ]]; }
# expect_work id «x y ancho alto» nombre=geometría...: el área publicada y la
# geometría real de las ventanas (el resultado efectivo, no solo la
# propiedad).
expect_work() {
    local id=$1 want=$2 verdict=FALLA got pair
    shift 2
    wait_for workarea_is "$want" && wait_for layout_is "$@" && kill -0 "$WM_PID" && verdict=OK
    got=$(for pair in "$@"; do printf '%s=%s; ' "${pair%%=*}" "$(geometry "${pair%%=*}")"; done)
    record "$id" "_NET_WORKAREA=$want ×8; $*" "_NET_WORKAREA=$(workarea); ${got%; }" "$verdict" "xprop -root _NET_WORKAREA (32 valores) y xwininfo"
}
reserve_log() { grep -F "ventana ${WIDS[$1]}: " "$WM_LOG" | grep -E "reserva|defectuosa|fuera de la pantalla" | tail -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]*: //'; }
# set_partial nombre v1…v12: _NET_WM_STRUT_PARTIAL con xprop (CARDINAL).
set_partial() {
    local name=$1 IFS=,
    shift
    xprop -id "${WIDS[$name]}" -f _NET_WM_STRUT_PARTIAL 32c -set _NET_WM_STRUT_PARTIAL "$*"
}
TOP40=(0 0 40 0 0 0 0 0 0 1279 0 0)

# Capturas (A118 y A119): volcado real del Xvfb y rótulo con datos del
# servidor.
shot() {
    sleep 1
    xwd -root -silent -out "$OUT/$1.xwd" && magick "$OUT/$1.xwd" "$OUT/$1.png"
    printf 'pila: %s · área: %s\n' "$(order_of "${@:2}" | sed 's/ / < /g')" "$(workarea)" >"$OUT/$1.txt"
}
px() { magick "$OUT/$1.png" -format "%[hex:p{$2}]" info:; }
# ROOT_BG: color del fondo de la raíz de Xvfb (negro, sin trama), distinto
# de los de la escena.
ROOT_BG=000000
RED=8B0A1F BLACK=1F2328 WHITE=F6F8FA
# scene: A y B en mosaico y un DOCK (K) de 800 × 40 arriba que reserva su
# franja con _NET_WM_STRUT_PARTIAL (tramo 0–799). A la derecha del panel se
# ve, según el WM, B (F9) o el fondo negro de la raíz de Xvfb (F10, franja
# reservada entera).
scene() {
    open_tc A --background "$RED"
    open_tc B --background "$BLACK"
    open_tc K --type DOCK --geometry 0 0 800 40 --background "$WHITE" --strut-partial 0 0 40 0 0 0 0 0 0 799 0 0
    precondition "foco en B" wait_for focus_is B
}

case $CASE in
A104)
    # DOCK superior: el mosaico empieza debajo.
    start10
    open_tc A
    expect_work A104-sin-panel "0 0 1280 720" "A=$FULL"
    open_tc B
    open_tc K --type DOCK --geometry 0 0 1280 40 --strut-partial "${TOP40[@]}"
    expect_work A104-superior "0 40 1280 680" "A=0 40 640 680" "B=640 40 640 680" "K=0 0 1280 40"
    record A104-registro "_NET_WM_STRUT_PARTIAL, arriba 40" "$(reserve_log K)" "$(ok_if logged "ventana ${WIDS[K]}: reserva (_NET_WM_STRUT_PARTIAL): izquierda 0, derecha 0, arriba 40, abajo 0")" "registro del WM"
    expect A104-sin-foco "escritorio=1 foco=B activa=B A=v B=v K=v" A B K
    want="_NET_SUPPORTED, _NET_SUPPORTING_WM_CHECK, _NET_CLIENT_LIST, _NET_CLIENT_LIST_STACKING, _NET_NUMBER_OF_DESKTOPS, _NET_CURRENT_DESKTOP, _NET_ACTIVE_WINDOW, _NET_CLOSE_WINDOW, _NET_WM_DESKTOP, _NET_WM_STATE, _NET_WM_STATE_FULLSCREEN, _NET_WM_WINDOW_TYPE, _NET_WM_WINDOW_TYPE_NORMAL, _NET_WM_WINDOW_TYPE_DIALOG, _NET_WM_WINDOW_TYPE_UTILITY, _NET_WM_WINDOW_TYPE_TOOLBAR, _NET_WM_WINDOW_TYPE_SPLASH, _NET_WM_WINDOW_TYPE_MENU, _NET_WM_WINDOW_TYPE_DOCK, _NET_WM_WINDOW_TYPE_DESKTOP, _NET_WM_WINDOW_TYPE_NOTIFICATION, _NET_WM_STRUT, _NET_WM_STRUT_PARTIAL, _NET_WORKAREA"
    got=$(xprop -root _NET_SUPPORTED | sed 's/^_NET_SUPPORTED(ATOM) = //')
    record A104-supported "$want" "$got" "$(ok_if test "$got" = "$want")" "xprop -root _NET_SUPPORTED (después de verificar F10)"
    stop
    ;;
A105)
    # DOCK inferior.
    start10
    open_tc A
    open_tc B
    open_tc K --type DOCK --geometry 0 690 1280 30 --strut-partial 0 0 0 30 0 0 0 0 0 0 0 1279
    expect_work A105-inferior "0 0 1280 690" "A=0 0 640 690" "B=640 0 640 690" "K=0 690 1280 30"
    stop
    ;;
A106)
    # Izquierda (con _NET_WM_STRUT) y derecha (con _NET_WM_STRUT_PARTIAL).
    start10
    open_tc A
    open_tc B
    open_tc L --type DOCK --geometry 0 0 200 720 --strut 200 0 0 0
    expect_work A106-izquierda "200 0 1080 720" "A=200 0 540 720" "B=740 0 540 720"
    record A106-registro-strut "_NET_WM_STRUT, izquierda 200" "$(reserve_log L)" "$(ok_if logged "ventana ${WIDS[L]}: reserva (_NET_WM_STRUT): izquierda 200, derecha 0, arriba 0, abajo 0")" "PARTIAL ausente: se usa STRUT"
    open_tc R --type DOCK --geometry 1130 0 150 720 --strut-partial 0 150 0 0 0 0 0 719 0 0 0 0
    expect_work A106-derecha "200 0 930 720" "A=200 0 465 720" "B=665 0 465 720"
    stop
    ;;
A107)
    # Varios paneles: máximo por borde, no suma; al irse el mayor queda el
    # otro.
    start10
    open_tc A
    open_tc K1 --type DOCK --geometry 0 0 1280 30 --strut-partial 0 0 30 0 0 0 0 0 0 1279 0 0
    open_tc K2 --type DOCK --geometry 0 30 1280 30 --strut-partial 0 0 60 0 0 0 0 0 0 1279 0 0
    expect_work A107-maximo "0 60 1280 660" "A=0 60 1280 660"
    open_tc K3 --type DOCK --geometry 0 695 1280 25 --strut-partial 0 0 0 25 0 0 0 0 0 0 0 1279
    expect_work A107-dos-bordes "0 60 1280 635" "A=0 60 1280 635"
    # Esquina: un panel izquierdo que se cruza con el superior.
    open_tc K4 --type DOCK --geometry 0 0 100 720 --strut-partial 100 0 0 0 0 719 0 0 0 0 0 0
    expect_work A107-esquina "100 60 1180 635" "A=100 60 1180 635"
    kill "${PIDS[K2]}"
    unset 'PIDS[K2]'
    expect_work A107-retirado-el-mayor "100 30 1180 665" "A=100 30 1180 665"
    stop
    ;;
A108)
    # Tramos parciales: un panel que ocupa parte del borde reserva toda la
    # franja (el área es rectangular); un tramo fuera de la pantalla no
    # reserva.
    start10
    open_tc A
    open_tc K --type DOCK --geometry 0 0 400 40 --strut-partial 0 0 40 0 0 0 0 0 0 399 0 0
    expect_work A108-tramo-parcial "0 40 1280 680" "A=0 40 1280 680"
    open_tc X --type DOCK --geometry 0 680 300 40 --strut-partial 0 0 0 40 0 0 0 0 0 0 1280 1600
    verdict=FALLA
    wait_for logged "ventana ${WIDS[X]}: tramo de strut fuera de la pantalla" && wait_for workarea_is "0 40 1280 680" && layout_is "A=0 40 1280 680" && verdict=OK
    record A108-fuera-de-la-pantalla "registrado; sin reserva abajo" "$(reserve_log X); _NET_WORKAREA=$(workarea); A=$(geometry A)" "$verdict" "tramo inferior 1280–1600"
    stop
    ;;
A109)
    # Preferencia y los tres estados: PARTIAL válida gana a STRUT; PARTIAL
    # ausente usa STRUT; PARTIAL mal formada no recurre a STRUT.
    start10
    open_tc A
    open_tc K1 --type DOCK --geometry 0 0 1280 40 --strut 0 0 80 0 --strut-partial "${TOP40[@]}"
    expect_work A109-partial-gana "0 40 1280 680" "A=0 40 1280 680"
    open_tc K2 --type DOCK --geometry 0 690 1280 30 --strut 0 0 0 30
    expect_work A109-strut-sin-partial "0 40 1280 650" "A=0 40 1280 650"
    open_tc K3 --type DOCK --geometry 0 0 1280 100 --strut 0 0 100 0
    expect_work A109-strut-k3 "0 100 1280 590" "A=0 100 1280 590"
    # PARTIAL presente pero con 4 valores: defecto; K3 deja de reservar.
    xprop -id "${WIDS[K3]}" -f _NET_WM_STRUT_PARTIAL 32c -set _NET_WM_STRUT_PARTIAL "0,0,100,0"
    verdict=FALLA
    wait_for logged "ventana ${WIDS[K3]}: _NET_WM_STRUT_PARTIAL defectuosa (Length" && verdict=OK
    record A109-partial-mal-formada "defecto registrado" "$(reserve_log K3)" "$verdict" "PARTIAL de 4 valores con STRUT válida"
    expect_work A109-sin-recurrir-a-strut "0 40 1280 650" "A=0 40 1280 650"
    stop
    ;;
A110)
    # Valores excesivos o mal formados: sin pánicos, área nunca vacía.
    start10
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40 --strut-partial 0 0 4294967295 0 0 0 0 0 0 1279 0 0
    expect_work A110-toda-la-altura "0 719 1280 1" "A=0 719 1280 1"
    verdict=FALLA
    logged 'reservas limitadas' && logged "ventana ${WIDS[K]}: reserva declarada [0, 0, 4294967295, 0] limitada a la pantalla: [0, 0, 720, 0]" && verdict=OK
    record A110-limitada "declarada y efectiva registradas" "$(grep -F "ventana ${WIDS[K]}: reserva declarada" "$WM_LOG" | head -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]*: //')" "$verdict" "4294967295 arriba"
    set_partial K 0 0 400 400 0 0 0 0 0 1279 0 1279
    expect_work A110-opuestos "0 400 1280 1" "A=0 400 1280 1"
    # Tipo INTEGER, formato 8 y tramo invertido: defecto, sin reserva.
    xprop -id "${WIDS[K]}" -f _NET_WM_STRUT_PARTIAL 32i -set _NET_WM_STRUT_PARTIAL "0,0,40,0,0,0,0,0,0,1279,0,0"
    expect_work A110-tipo-integer "0 0 1280 720" "A=$FULL"
    set_partial K "${TOP40[@]}"
    precondition "reserva válida" wait_for workarea_is "0 40 1280 680"
    xprop -id "${WIDS[K]}" -f _NET_WM_STRUT_PARTIAL 8s -set _NET_WM_STRUT_PARTIAL "texto"
    expect_work A110-formato-8 "0 0 1280 720" "A=$FULL"
    set_partial K 0 0 40 0 0 0 0 0 900 100 0 0
    expect_work A110-tramo-invertido "0 0 1280 720" "A=$FULL"
    verdict=FALLA
    [[ $(grep -c "ventana ${WIDS[K]}: _NET_WM_STRUT_PARTIAL defectuosa" "$WM_LOG") == 3 ]] && kill -0 "$WM_PID" && verdict=OK
    record A110-defectos "3 defectos; WM en marcha" "$(grep -F "ventana ${WIDS[K]}: _NET_WM_STRUT_PARTIAL defectuosa" "$WM_LOG" | sed 's/.*defectuosa (//; s/).*//' | paste -sd'|')" "$verdict" "INTEGER, STRING de 8 bits y Range(Top)"
    stop
    ;;
A111)
    # Ciclo de vida: aparición, movimiento, retirada, vuelta y destrucción.
    start10
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40 --strut-partial "${TOP40[@]}"
    expect_work A111-aparece "0 40 1280 680" "A=0 40 1280 680"
    xdotool windowmove "${WIDS[K]}" 0 300
    precondition "K movida" wait_for frame_is K "0 300 1280 40 0"
    expect_work A111-mover-no-cambia "0 40 1280 680" "A=0 40 1280 680"
    "$TEST_CLIENT" withdraw "${WIDS[K]}" >/dev/null
    precondition "K retirada" wait_for logged "ventana ${WIDS[K]} retirada"
    expect_work A111-retirada "0 0 1280 720" "A=$FULL"
    remap K
    expect_work A111-vuelve "0 40 1280 680" "A=0 40 1280 680"
    kill "${PIDS[K]}"
    unset 'PIDS[K]'
    expect_work A111-destruida "0 0 1280 720" "A=$FULL"
    expect_list A111-client-list _NET_CLIENT_LIST "A"
    stop
    ;;
A112)
    # Cambios dinámicos con PropertyNotify.
    start10
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40
    expect_work A112-dock-sin-struts "0 0 1280 720" "A=$FULL"
    set_partial K 0 0 50 0 0 0 0 0 0 1279 0 0
    expect_work A112-aparece-la-reserva "0 50 1280 670" "A=0 50 1280 670"
    set_partial K 0 0 0 60 0 0 0 0 0 0 0 1279
    expect_work A112-cambia-de-borde "0 0 1280 660" "A=0 0 1280 660"
    xprop -id "${WIDS[K]}" -remove _NET_WM_STRUT_PARTIAL
    expect_work A112-se-borra "0 0 1280 720" "A=$FULL"
    xprop -id "${WIDS[K]}" -f _NET_WM_STRUT 32c -set _NET_WM_STRUT "0,0,20,0"
    expect_work A112-strut-dinamico "0 20 1280 700" "A=0 20 1280 700"
    stop
    ;;
A113)
    # Ocho escritorios: 32 valores iguales; un escritorio oculto se recoloca
    # al mostrarse.
    start10
    open_tc A
    go 2
    open_tc B
    open_tc K --type DOCK --geometry 0 0 1280 40 --strut-partial "${TOP40[@]}"
    expect_work A113-activo "0 40 1280 680" "B=0 40 1280 680"
    raw=$(xprop -root _NET_WORKAREA | sed 's/^.*= //')
    n=$(tr ',' '\n' <<<"$raw" | grep -c .)
    record A113-32-valores 32 "$n" "$(ok_if test "$n" = 32)" "xprop -root _NET_WORKAREA: $raw"
    record A113-oculto-sin-mover "A=$FULL" "A=$(geometry A)" "$(ok_if layout_is "A=$FULL")" "A, oculta en el escritorio 1, aún no se ha recolocado"
    go 1
    expect_work A113-al-mostrar "0 40 1280 680" "A=0 40 1280 680"
    expect A113-panel-visible "escritorio=1 foco=A activa=A A=v B=o K=v" A B K
    stop
    ;;
A114)
    # Diálogos centrados dentro del área; las flotantes no se mueven.
    start10
    open_tc A
    open_tc F --dialog --geometry 100 10 300 200
    open_tc K --type DOCK --geometry 0 0 1280 40 --strut-partial "${TOP40[@]}"
    expect_work A114-mosaico "0 40 1280 680" "A=0 40 1280 680"
    expect_frame A114-flotante-sin-mover F "100 10 300 200 0"
    open_dialog D A --geometry 0 0 400 700
    expect_frame A114-dialogo-dentro D "440 40 400 700 0"
    open_dialog E A --geometry 0 0 300 200
    expect_frame A114-dialogo-centrado E "490 280 300 200 0"
    stop
    ;;
A115)
    # Pantalla completa sobre la pantalla física; al salir, el hueco del
    # área de trabajo.
    start10
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40 --strut-partial "${TOP40[@]}"
    precondition "área reducida" wait_for workarea_is "0 40 1280 680"
    key super+f
    precondition "A a pantalla completa" wait_for fullscreen_is A
    expect_work A115-pantalla-fisica "0 40 1280 680" "A=$FULL"
    expect_order A115-sobre-el-panel "K A" A K
    key super+f
    precondition "A sin pantalla completa" wait_for net_state_is A vacía
    expect_work A115-vuelve-al-area "0 40 1280 680" "A=0 40 1280 680"
    expect_order A115-panel-encima "A K" A K
    stop
    ;;
A116)
    # Adopción, salida limpia y reinicio.
    open_tc K --type DOCK --geometry 0 0 1280 40 --strut-partial "${TOP40[@]}"
    open_tc A
    start10
    expect_work A116-adopcion "0 40 1280 680" "A=0 40 1280 680"
    key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    record A116-salida "ausente" "$(workarea)" "$(ok_if workarea_is ausente)" "_NET_WORKAREA tras Mod4+Shift+Escape"
    restart "$CASE-2"
    expect_work A116-reinicio "0 40 1280 680" "A=0 40 1280 680"
    stop
    ;;
A117)
    # Solo los DOCK reservan: NOTIFICATION, DESKTOP y una ventana normal con
    # struts no cambian el área.
    start10
    open_tc N --type NOTIFICATION --geometry 1000 50 260 80 --strut-partial "${TOP40[@]}"
    open_tc W --type DESKTOP --geometry 0 0 1280 720 --strut 300 0 0 0
    open_tc A --strut 0 0 0 50
    sleep 0.5
    expect_work A117-sin-reserva "0 0 1280 720" "A=$FULL"
    verdict=FALLA
    n=$(grep -cE "ventana (${WIDS[N]}|${WIDS[W]}|${WIDS[A]}): (reserva|_NET_WM_STRUT)" "$WM_LOG")
    ((n == 0)) && verdict=OK
    record A117-sin-leer 0 "$n" "$verdict" "líneas de reserva de N, W y A en el registro del WM"
    set_partial N 0 0 90 0 0 0 0 0 0 1279 0 0
    sleep 0.5
    expect_work A117-cambio-ignorado "0 0 1280 720" "A=$FULL"
    stop
    ;;
A118)
    # Panel 1 de la comparación, con el binario de F9 (2b2df85): el DOCK
    # tapa la parte superior del mosaico.
    start10
    scene
    expect_layout A118-mosaico "A=$LEFT" "B=$RIGHT" "K=0 0 800 40"
    record A118-version "sin _NET_WORKAREA (F9)" "$(workarea)" "$(ok_if workarea_is ausente)" "el binario de F9 no la publica"
    shot "$CASE-1" A B K
    colors="$(px "$CASE-1" 100,20) $(px "$CASE-1" 1000,20) $(px "$CASE-1" 100,300)"
    want="$WHITE $BLACK $RED"
    record A118-colores "$want" "$colors" "$(ok_if test "$colors" = "$want")" "el DOCK tapa A en y < 40; B llega arriba a la derecha del panel"
    stop
    ;;
A119)
    # Paneles 2 y 3 con F10: reserva arriba; después el panel pasa abajo y
    # cambia su reserva, y el mosaico se reajusta.
    start10
    scene
    expect_work A119-reserva "0 40 1280 680" "A=0 40 640 680" "B=640 40 640 680"
    shot "$CASE-2" A B K
    colors="$(px "$CASE-2" 100,20) $(px "$CASE-2" 1000,20) $(px "$CASE-2" 100,45) $(px "$CASE-2" 900,45)"
    want="$WHITE $ROOT_BG $RED $BLACK"
    record A119-colores-1 "$want" "$colors" "$(ok_if test "$colors" = "$want")" "el DOCK arriba; la franja entera reservada (fondo de la raíz a su derecha); A y B desde y = 40"
    xdotool windowmove "${WIDS[K]}" 0 680
    precondition "K abajo" wait_for frame_is K "0 680 800 40 0"
    set_partial K 0 0 0 40 0 0 0 0 0 0 0 799
    expect_work A119-reajuste "0 0 1280 680" "A=0 0 640 680" "B=640 0 640 680"
    shot "$CASE-3" A B K
    colors="$(px "$CASE-3" 100,20) $(px "$CASE-3" 100,700) $(px "$CASE-3" 1000,700) $(px "$CASE-3" 900,675)"
    want="$RED $WHITE $ROOT_BG $BLACK"
    record A119-colores-2 "$want" "$colors" "$(ok_if test "$colors" = "$want")" "A y B desde y = 0 hasta 680; el DOCK abajo con su franja reservada"
    first=$OUT/A118-1.png
    if [[ ! -s $first ]]; then
        for id in A119-dimensiones A119-composicion; do
            record "$id" "tres capturas" "falta la de F9" OMITIDA "A118 no se ejecutó"
        done
    else
        cp "$first" "$OUT/$CASE-1.png"
        cp "$OUT/A118-1.txt" "$OUT/$CASE-1.txt"
        dims=$(for i in 1 2 3; do magick identify -format '%wx%h ' "$OUT/$CASE-$i.png"; done)
        record A119-dimensiones "1280x720 1280x720 1280x720" "${dims% }" "$(ok_if test "${dims% }" = "1280x720 1280x720 1280x720")" "magick identify de las tres capturas"
        labels=("1 · F9 (2b2df85): el DOCK tapa el mosaico" "2 · F10: el DOCK reserva la franja de 40 px" "3 · F10: el DOCK pasa abajo y el mosaico se reajusta")
        for i in 1 2 3; do
            magick "$OUT/$CASE-$i.png" -bordercolor '#1F2328' -border 2 -background white \
                -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 34 -fill '#1F2328' \
                -annotate +0+14 "${labels[i - 1]}" -font DejaVu-Sans -pointsize 26 -fill '#57606A' \
                -annotate +0+62 "$(cat "$OUT/$CASE-$i.txt")" +repage "$OUT/$CASE-panel-$i.png"
        done
        magick "$OUT/$CASE-panel-1.png" "$OUT/$CASE-panel-2.png" "$OUT/$CASE-panel-3.png" \
            -background white -splice 40x0 +append +repage -gravity east -splice 40x0 +repage \
            -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 46 -fill '#8B0A1F' \
            -annotate +0+28 "Crimson WM · F9 frente a F10: struts y área de trabajo (capturas reales en Xvfb)" \
            -gravity south -splice 0x80 -font DejaVu-Sans -pointsize 28 -fill '#57606A' \
            -annotate +0+24 "A: rojo · B: negro · DOCK (K, 800×40, tramo 0–799): blanco · negro puro: fondo de la raíz de Xvfb. Volcados de xwd -root de 1280×720, sin retoques." \
            +repage "$OUT/$CASE-comparison.png"
        same=''
        for i in 1 2 3; do
            x=$((40 + (i - 1) * (1284 + 40) + 2))
            magick "$OUT/$CASE-comparison.png" -crop "1280x720+$x+222" +repage "$OUT/$CASE-check-$i.png"
            same+="$(magick compare -metric AE "$OUT/$CASE-$i.png" "$OUT/$CASE-check-$i.png" null: 2>&1) "
        done
        record A119-composicion "0 0 0" "${same% }" "$(ok_if test "${same% }" = "0 0 0")" "píxeles distintos entre cada captura y su panel ($(magick identify -format '%wx%h' "$OUT/$CASE-comparison.png")); rótulos: $(cat "$OUT/$CASE"-[123].txt | paste -sd'|')"
        sha256sum "$OUT/$CASE"-[123].png "$OUT/$CASE-comparison.png" | sed "s#$OUT/##" >"$OUT/$CASE-sha256.txt"
    fi
    stop
    ;;
*)
    echo "caso desconocido: $CASE"
    exit 2
    ;;
esac

if [[ -n ${WM_LOG:-} ]]; then
    n=0 unknown=
    while read -r pid; do
        n=$((n + 1))
        compgen -G "$LOGS/*.$pid" >/dev/null || unknown+=" $pid"
    done < <(cat "$OUT/$CASE"*.log | grep -o '» lanzado (pid [0-9]*)' | grep -o '[0-9]*')
    record "$CASE-extra-solo-falsos" "todos con registro de un programa falso" "$n lanzados; sin registro:${unknown:- ninguno}" "$([[ -z $unknown ]] && echo OK || echo FALLA)" "líneas «lanzado» de los registros del WM y $LOGS"
    xerrors=$(cat "$OUT/$CASE"*.log | grep -c 'error X ignorado')
    record "$CASE-extra-sin-errores-x" 0 "$xerrors" "$([[ $xerrors == 0 ]] && echo OK || echo FALLA)" "líneas «error X ignorado» en los registros del WM"
fi
