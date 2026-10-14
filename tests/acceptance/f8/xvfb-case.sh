#!/bin/bash
# Una prueba de aceptación de F8 (ADR 0010) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh), y las de estado, las mismas que en F7. Como en F4–F7, el
# único directorio del PATH del WM contiene copias de f4/fake.sh: ninguna
# aplicación real puede ejecutarse.
#
# TEST_CLIENT (examples/test-client.rs) crea principales y diálogos con
# WM_TRANSIENT_FOR, pistas de posición (WM_NORMAL_HINTS) y colores de fondo.
# A86 hace capturas reales del Xvfb con `xwd`.
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
open_xev() { open_window "$1" xev -name "$1"; }
# open_tc nombre [opciones...]: ventana de test-client (WM_DELETE_WINDOW por
# defecto) que registra en $OUT/$CASE-nombre.out los mensajes recibidos.
open_tc() {
    local name=$1
    shift
    open_window "$name" "$TEST_CLIENT" window "$name" "$@"
}

# start8: arranca el WM con los programas falsos instalados y registra la
# evidencia de aislamiento, como start4 en F4 (solo nombres de variables).
start8() {
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
# WM_REAL: el proceso crimson-wm hijo del `timeout` que lanzó el arnés. Las
# señales solo se envían a ese proceso.
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
identified() { grep -q 'identificación publicada' "$WM_LOG"; }

# Estado de las ventanas y de la raíz.
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
net_desk() {
    local v
    v=$(xprop -id "${WIDS[$1]}" _NET_WM_DESKTOP 2>/dev/null | sed -n 's/^_NET_WM_DESKTOP(CARDINAL) = \([0-9]*\)$/\1/p')
    if [[ -n $v ]]; then echo $((v + 1)); else echo ausente; fi
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
hidden() { [[ $(map_state "$1") == o ]]; }
gone() { [[ $(map_state "$1") == destruida ]]; }
vis() {
    local n s=''
    for n in "$@"; do s+="$n=$(map_state "$n") "; done
    echo "${s% }"
}
vis_is() { [[ $(vis "${@:2}") == "$1" ]]; }
retired() { grep -q "ventana ${WIDS[$1]} retirada" "$WM_LOG"; }
logged() { grep -qF -- "$1" "$WM_LOG"; }
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
# Orden real de las ventanas gestionadas, de abajo arriba (xwininfo lista
# primero la de arriba), limitado a las de WIDS.
tree_names() {
    local id n names=''
    while read -r id; do
        n=$(name_of "$id")
        [[ $n != "$id" ]] && names="$n $names"
    done < <(xwininfo -root -children | awk '$1 ~ /^0x/ {print $1}')
    echo "${names% }"
}
stacking_matches() { [[ $(list_names _NET_CLIENT_LIST_STACKING) == "$(tree_names)" ]]; }
expect_stacking() {
    local verdict=FALLA
    wait_for stacking_matches && [[ $(list_names _NET_CLIENT_LIST_STACKING) == "$2" ]] && verdict=OK
    record "$1" "$2 (igual que QueryTree)" "$(list_names _NET_CLIENT_LIST_STACKING); QueryTree: $(tree_names)" "$verdict" "xprop -root _NET_CLIENT_LIST_STACKING y xwininfo -root -children"
}
# received nombre protocolo: líneas «PROTOCOLO VENTANA HORA» del cliente.
received() { grep "^$2 " "$OUT/$CASE-$1.out"; }
# Marca de tiempo del último mensaje recibido (0 si no hay ninguno).
last_time() { received "$1" "$2" | tail -n1 | awk '{print $3+0}'; }
has_message() { [[ -n $(received "$1" "$2") ]]; }
messages_at_least() { (($(received "$1" "$2" | wc -l) >= $3)); }
dead() { ! kill -0 "$1" 2>/dev/null; }


# Pantalla completa.
# net_state nombre: átomos de _NET_WM_STATE separados por comas, «vacía» o
# «ausente».
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
not_fullscreen() { ! fullscreen_is "$1"; }
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
# La ventana gestionada más alta (o más baja) de la pila.
top_name() { tree_names | awk '{print $NF}'; }
bottom_name() { tree_names | awk '{print $1}'; }
top_is() { [[ $(top_name) == "$1" ]]; }
bottom_is() { [[ $(bottom_name) == "$1" ]]; }
# atom_id nombre: número del átomo en este servidor (lo crea si no existe).
atom_id() {
    "$TEST_CLIENT" message 32 root "$1" 0 0 0 0 0 >/dev/null
    xlsatoms -name "$1" | awk '{print $1}'
}
# wm_state nombre acción fuente [estado1 [estado2]]: petición _NET_WM_STATE
# (por defecto, FULLSCREEN en data[1]).
wm_state() {
    "$TEST_CLIENT" message 32 "${WIDS[$1]}" _NET_WM_STATE "$2" "${4:-$FS}" "${5:-0}" "$3" 0
}
layout_now() {
    local n s=''
    for n in "$@"; do s+="$n=$(geometry "$n"); "; done
    echo "${s%; }"
}
layout_now_is() { [[ $(layout_now "${@:2}") == "$1" ]]; }
fs_count() { grep -c "ventana ${WIDS[$1]} a pantalla completa" "$WM_LOG"; }
fs_count_is() { [[ $(fs_count "$1") == "$2" ]]; }
net_state_is() { [[ $(net_state "$1") == "$2" ]]; }
adopted() { [[ $(grep -c "gestionada" "$WM_LOG") == "$1" ]]; }

precondition "pantalla 1280x720" test "$SIZE" = 1280x720
FULL="0 0 1280 720"
LEFT="0 0 640 720"
RIGHT="640 0 640 720"
FS=$(atom_id _NET_WM_STATE_FULLSCREEN)
precondition "átomo de FULLSCREEN" test -n "$FS"


# Diálogos (ADR 0010).
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
# open_dialog nombre propietario [opciones...]: diálogo de test-client
# transitorio de la ventana `propietario` (nombre de WIDS, o root/self).
open_dialog() {
    local name=$1 owner=$2
    shift 2
    [[ $owner != root && $owner != self ]] && owner=${WIDS[$owner]}
    open_window "$name" "$TEST_CLIENT" window "$name" --transient-for "$owner" "$@"
}
# open_hidden_dialog: como open_dialog, para un diálogo que el WM deja
# oculto (no se espera a que sea visible).
open_hidden_dialog() {
    local name=$1 owner=${WIDS[$2]}
    shift 2
    "$TEST_CLIENT" window "$name" --transient-for "$owner" "$@" >"$OUT/$CASE-$name.out" 2>&1 &
    PIDS[$name]=$!
    precondition "ventana $name creada" wait_for grep -q '^ventana 0x' "$OUT/$CASE-$name.out"
    WIDS[$name]=$(sed -n 's/^ventana //p' "$OUT/$CASE-$name.out" | head -n1)
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
# desks_are «A=3 D=3 …»: _NET_WM_DESKTOP (de 1 a 8) de cada ventana.
desks_are() {
    local pair
    for pair in "$@"; do
        [[ $(net_desk "${pair%%=*}") == "${pair#*=}" ]] || return 1
    done
}
transient_log() { grep -F "ventana ${WIDS[$1]}: " "$WM_LOG" | grep -m1 -E "diálogo de|WM_TRANSIENT_FOR" | sed 's/^crimson-wm: //'; }

case $CASE in
A76)
    start8
    open_xev A
    open_xev B
    open_dialog D A --geometry 0 0 300 200
    verdict=FALLA
    logged "ventana ${WIDS[D]} gestionada (flotante)" && logged "ventana ${WIDS[D]}: diálogo de ${WIDS[A]}" && verdict=OK
    record A76-flotante "flotante, diálogo de A" "$(transient_log D)" "$verdict" "registro del WM"
    expect A76-foco-dialogo "escritorio=1 foco=D activa=D A=v B=v D=v" A B D
    expect_order A76-encima "A D" A D
    expect_frame A76-centrado D "170 260 300 200 0"
    # El historial tiene B antes que A; al cerrar D, el foco vuelve a A.
    key super+w
    precondition "D cerrada" wait_for gone D
    expect A76-foco-al-propietario "escritorio=1 foco=A activa=A A=v B=v" A B
    stop
    ;;
A77)
    start8
    open_xev A
    open_dialog D1 A --geometry 0 0 300 200
    open_dialog D2 A --geometry 0 0 260 160
    open_dialog D3 D1 --geometry 0 0 200 120
    # Familias de flotantes: D2 sola; D1 con su subdiálogo D3, la más
    # reciente, encima.
    expect_order A77-pila "A D2 D1 D3" A D1 D2 D3
    record A77-cadena "D3 diálogo de D1" "$(transient_log D3)" "$(ok_if logged "ventana ${WIDS[D3]}: diálogo de ${WIDS[D1]}")" "registro del WM"
    key super+w
    precondition "D3 cerrada" wait_for gone D3
    expect A77-foco-tras-d3 "escritorio=1 foco=D1 activa=D1 A=v D1=v D2=v" A D1 D2
    key super+w
    precondition "D1 cerrada" wait_for gone D1
    expect A77-foco-tras-d1 "escritorio=1 foco=A activa=A A=v D2=v" A D2
    expect_order A77-pila-final "A D2" A D2
    stop
    ;;
A78)
    start8
    go 2
    open_xev A
    go 1
    open_xev B
    open_hidden_dialog D A --geometry 0 0 300 200
    precondition "D gestionada" wait_for logged "ventana ${WIDS[D]} al escritorio 2 con su principal (oculta)"
    expect_stays A78-sin-cambios "escritorio=1 foco=B activa=B A=o B=v D=o" A B D
    got=$(net_desk D)
    record A78-escritorio-de-d 2 "$got" "$(ok_if test "$got" = 2)" "_NET_WM_DESKTOP de D (de 1 a 8)"
    go 2
    expect A78-al-mostrar "escritorio=2 foco=D activa=D A=v B=o D=v" A B D
    expect_order A78-encima "A D" A D
    expect_frame A78-centrado D "490 260 300 200 0"
    stop
    ;;
A79)
    start8
    open_xev A
    open_xev B
    open_dialog D A --geometry 0 0 300 200
    open_dialog E D --geometry 0 0 200 120
    precondition "foco en A" focus_on A
    key super+shift+3
    verdict=FALLA
    wait_for desks_are A=3 D=3 E=3 && verdict=OK
    record A79-familia "A, D y E en el escritorio 3" "A=$(net_desk A) D=$(net_desk D) E=$(net_desk E) B=$(net_desk B)" "$verdict" "Mod4+Shift+3 sobre A"
    expect A79-origen "escritorio=1 foco=B activa=B A=o B=v D=o E=o" A B D E
    go 3
    expect_order A79-destino-pila "A D E" A D E
    # Solo un diálogo: se lleva su subdiálogo, no a su principal.
    precondition "foco en D" focus_on D
    key super+shift+4
    verdict=FALLA
    wait_for desks_are D=4 E=4 A=3 && verdict=OK
    record A79-dialogo-solo "D y E al 4; A en el 3" "A=$(net_desk A) D=$(net_desk D) E=$(net_desk E)" "$verdict" "Mod4+Shift+4 sobre D"
    # _NET_WM_DESKTOP sobre la principal (sin dependientes en su escritorio).
    xdotool set_desktop_for_window "${WIDS[A]}" 0
    verdict=FALLA
    wait_for output_is 1 net_desk A && [[ $(net_desk D) == 4 ]] && desk_is 3 && verdict=OK
    record A79-net-wm-desktop "A al 1; D sigue en el 4" "A=$(net_desk A) D=$(net_desk D); activo $(current_desk)" "$verdict" "xdotool set_desktop_for_window"
    stop
    ;;
A80)
    start8
    open_xev A
    open_dialog D A --geometry 0 0 300 200
    kill "${PIDS[A]}"
    unset 'PIDS[A]'
    precondition "A retirada" wait_for retired A
    sleep 0.5
    verdict=FALLA
    [[ $(map_state D) == v ]] && ! retired D && focus_is D && verdict=OK
    record A80-principal-destruida "D sigue gestionada, visible y enfocada" "D $(map_state D); retirada: $(retired D && echo sí || echo no); foco: $(focus_name)" "$verdict" "kill del cliente de A"
    open_xev A2
    open_dialog D2 A2 --geometry 0 0 300 200
    xdotool windowunmap "${WIDS[A2]}"
    precondition "A2 retirada" wait_for retired A2
    verdict=FALLA
    sleep 0.5
    [[ $(map_state D2) == v ]] && ! retired D2 && verdict=OK
    record A80-principal-retirada "D2 sigue gestionada" "D2 $(map_state D2); retirada: $(retired D2 && echo sí || echo no)" "$verdict" "xdotool windowunmap de A2"
    # Sin principal, al cerrar D2 el foco sigue el historial.
    precondition "foco en D2" focus_on D2
    key super+w
    precondition "D2 cerrada" wait_for gone D2
    expect A80-foco-historial "escritorio=1 foco=D activa=D D=v" D
    stop
    ;;
A81)
    start8
    open_xev A
    open_dialog R root --geometry 0 0 200 100
    open_dialog S self --geometry 0 0 200 100
    open_window X "$TEST_CLIENT" window X --transient-for 0x7ffff1 --geometry 0 0 200 100
    verdict=FALLA
    logged "ventana ${WIDS[R]}: WM_TRANSIENT_FOR sin propietario efectivo (valor 0, la raíz o la propia ventana)" &&
        logged "ventana ${WIDS[S]}: WM_TRANSIENT_FOR sin propietario efectivo (valor 0, la raíz o la propia ventana)" &&
        logged "ventana ${WIDS[X]}: WM_TRANSIENT_FOR sin propietario efectivo (propietario pendiente o no gestionado)" && verdict=OK
    record A81-sin-propietario "raíz, propia ventana e identificador ajeno: sin propietario" "$(transient_log R); $(transient_log S); $(transient_log X)" "$verdict" "registro del WM"
    # Ciclo de dos con PropertyNotify.
    open_tc C1 --geometry 0 0 200 100
    open_tc C2 --geometry 0 0 200 100
    xprop -id "${WIDS[C1]}" -f WM_TRANSIENT_FOR 32x -set WM_TRANSIENT_FOR "${WIDS[C2]}"
    precondition "C1 cambiada" wait_for logged "ventana ${WIDS[C1]}: diálogo de ${WIDS[C2]}"
    xprop -id "${WIDS[C2]}" -f WM_TRANSIENT_FOR 32x -set WM_TRANSIENT_FOR "${WIDS[C1]}"
    verdict=FALLA
    wait_for logged "ventana ${WIDS[C2]}: WM_TRANSIENT_FOR sin propietario efectivo (ciclo o cadena de más de 16 propietarios)" && kill -0 "$WM_PID" && verdict=OK
    record A81-ciclo "ciclo detectado; WM en marcha" "$(grep -c 'ciclo o cadena' "$WM_LOG") registros de ciclo" "$verdict" "C1 → C2 → C1 con xprop"
    expect_stacking A81-pila-coherente "$(tree_names)"
    stop
    ;;
A82)
    start8
    # La principal A (flotante) se crea, pero se mapea 3 s después.
    "$TEST_CLIENT" window A --dialog --geometry 600 100 400 300 --map-delay 3000 >"$OUT/$CASE-A.out" 2>&1 &
    PIDS[A]=$!
    precondition "A creada" wait_for grep -q '^creada 0x' "$OUT/$CASE-A.out"
    WIDS[A]=$(sed -n 's/^creada //p' "$OUT/$CASE-A.out")
    open_dialog D A --geometry 650 150 200 100
    verdict=FALLA
    logged "ventana ${WIDS[D]}: WM_TRANSIENT_FOR sin propietario efectivo (propietario pendiente o no gestionado)" && verdict=OK
    record A82-pendiente "propietario pendiente" "$(transient_log D)" "$verdict" "D se mapea antes que A"
    precondition "A visible" wait_for viewable A
    expect_order A82-resuelto "A D" A D
    expect_frame A82-sin-centrar D "650 150 200 100 0"
    stop
    ;;
A83)
    start8
    open_xev A
    key super+f
    precondition "A a pantalla completa" wait_for fullscreen_is A
    open_dialog D A --geometry 0 0 300 200
    expect A83-foco-dialogo "escritorio=1 foco=D activa=D A=v D=v" A D
    expect_order A83-familia-arriba "A D" A D
    verdict=FALLA
    wait_for top_is D && fullscreen_is A && layout_is "A=$FULL" && verdict=OK
    record A83-sigue-a-pantalla-completa "A con el área y FULLSCREEN; D arriba" "A=$(geometry A); $(net_state A); pila: $(tree_names)" "$verdict" "la familia enfocada sigue visible"
    expect_frame A83-centrado D "490 260 300 200 0"
    key super+w
    precondition "D cerrada" wait_for gone D
    expect A83-foco-a-la-principal "escritorio=1 foco=A activa=A A=v" A
    open_xev E
    verdict=FALLA
    wait_for bottom_is A && wait_for focus_is E && verdict=OK
    record A83-ajena "A al fondo con el foco en E (F7)" "pila: $(tree_names); foco: $(focus_name)" "$verdict" "una ventana ajena enfocada"
    stop
    ;;
A84)
    start8
    open_xev A
    open_xev B
    # D, diálogo de B; el historial queda D, B, A.
    open_dialog D B --geometry 0 0 300 200
    xprop -id "${WIDS[D]}" -f WM_TRANSIENT_FOR 32x -set WM_TRANSIENT_FOR "${WIDS[A]}"
    precondition "cambio registrado" wait_for logged "ventana ${WIDS[D]}: WM_TRANSIENT_FOR cambiado a Window"
    verdict=FALLA
    logged "ventana ${WIDS[D]}: diálogo de ${WIDS[A]}" && [[ $(grep -c "ventana ${WIDS[D]} gestionada" "$WM_LOG") == 1 ]] && verdict=OK
    record A84-cambio "D pasa a ser diálogo de A; sin reclasificar" "$(grep -F "ventana ${WIDS[D]}: diálogo de" "$WM_LOG" | tail -n1 | sed 's/^crimson-wm: //')" "$verdict" "xprop -set WM_TRANSIENT_FOR"
    expect_stays A84-sin-mover-ni-foco "escritorio=1 foco=D activa=D A=v B=v D=v" A B D
    # Al cerrarlo, el foco va al nuevo propietario (A), no a B, la más
    # reciente del historial.
    key super+w
    precondition "D cerrada" wait_for gone D
    expect A84-foco-nuevo-propietario "escritorio=1 foco=A activa=A A=v B=v" A B
    # Un cambio hacia una ventana de otro escritorio no mueve el diálogo.
    open_dialog E A --geometry 0 0 200 100
    go 2
    open_xev C
    go 1
    xprop -id "${WIDS[E]}" -f WM_TRANSIENT_FOR 32x -set WM_TRANSIENT_FOR "${WIDS[C]}"
    precondition "cambio registrado" wait_for logged "ventana ${WIDS[E]}: diálogo de ${WIDS[C]}"
    expect_stays A84-otro-escritorio "escritorio=1 foco=E activa=E C=o E=v" C E
    record A84-escritorio-de-e 1 "$(net_desk E)" "$(ok_if test "$(net_desk E)" = 1)" "_NET_WM_DESKTOP de E"
    # Apuntarse a sí mismo deja la ventana sin propietario.
    xprop -id "${WIDS[E]}" -f WM_TRANSIENT_FOR 32x -set WM_TRANSIENT_FOR "${WIDS[E]}"
    verdict=FALLA
    wait_for logged "ventana ${WIDS[E]}: WM_TRANSIENT_FOR cambiado a Itself" && verdict=OK
    record A84-propia "Itself: sin propietario" "$(grep -c "ventana ${WIDS[E]}: WM_TRANSIENT_FOR cambiado a Itself" "$WM_LOG") registros" "$verdict" "registro del WM"
    stop
    ;;
A85)
    start8
    open_xev A
    open_xev B
    open_dialog D1 A --geometry 0 0 300 200
    expect_frame A85-centrado D1 "170 260 300 200 0"
    open_dialog D2 A --geometry 900 50 300 200 --position-hint us
    expect_frame A85-usposition D2 "900 50 300 200 0"
    open_dialog D3 A --geometry 20 30 300 200 --position-hint p
    expect_frame A85-pposition D3 "20 30 300 200 0"
    open_dialog D4 B --geometry 0 0 1000 700
    expect_frame A85-dentro-del-area D4 "280 10 1000 700 0"
    # Una petición posterior se aplica (flotante) y no se vuelve a centrar.
    xdotool windowmove "${WIDS[D1]}" 10 10
    precondition "D1 movida" wait_for frame_is D1 "10 10 300 200 0"
    open_xev C
    sleep 0.5
    expect_frame A85-sin-recentrar D1 "10 10 300 200 0"
    stop
    ;;
A86)
    # Captura real: A (rojo) y B (negro) en mosaico; el diálogo D (gris) de
    # A, centrado y enfocado; A a pantalla completa con D encima.
    start8
    open_tc A --background 8B0A1F
    open_tc B --background 1F2328
    precondition "foco en B" wait_for focus_is B
    shot() {
        sleep 1
        xwd -root -silent -out "$OUT/$CASE-$1.xwd" && magick "$OUT/$CASE-$1.xwd" "$OUT/$CASE-$1.png"
        # Rótulo con datos reales del servidor.
        printf 'foco: %s · pila: %s\n' "$(focus_name)" "$(order_of A B D | sed 's/ / < /g')" >"$OUT/$CASE-$1.txt"
    }
    expect_layout A86-antes "A=$LEFT" "B=$RIGHT"
    shot 1
    open_dialog D A --geometry 0 0 400 260 --background D0D7DE
    expect A86-dialogo "escritorio=1 foco=D activa=D A=v B=v D=v" A B D
    expect_frame A86-dialogo-centrado D "120 230 400 260 0"
    shot 2
    wm_state A 1 2
    precondition "A a pantalla completa" wait_for fullscreen_is A
    expect_order A86-pantalla-completa "B A D" A B D
    shot 3
    dims=$(for i in 1 2 3; do magick identify -format '%wx%h ' "$OUT/$CASE-$i.png"; done)
    record A86-dimensiones "1280x720 1280x720 1280x720" "${dims% }" "$(ok_if test "${dims% }" = "1280x720 1280x720 1280x720")" "magick identify de las tres capturas"
    px() { magick "$OUT/$CASE-$1.png" -format "%[hex:p{$2}]" info:; }
    colors="$(px 1 100,100) $(px 1 900,100) $(px 2 320,360) $(px 2 100,100) $(px 3 1200,700) $(px 3 320,360)"
    want="8B0A1F 1F2328 D0D7DE 8B0A1F 8B0A1F D0D7DE"
    record A86-colores "$want" "$colors" "$(ok_if test "$colors" = "$want")" "A y B; D sobre A; A a pantalla completa con D encima"
    labels=("1 · A y B en mosaico" "2 · Diálogo D de A, centrado" "3 · A a pantalla completa, D encima")
    for i in 1 2 3; do
        magick "$OUT/$CASE-$i.png" -bordercolor '#1F2328' -border 2 -background white \
            -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 34 -fill '#1F2328' \
            -annotate +0+14 "${labels[i - 1]}" -font DejaVu-Sans -pointsize 26 -fill '#57606A' \
            -annotate +0+62 "$(cat "$OUT/$CASE-$i.txt")" +repage "$OUT/$CASE-panel-$i.png"
    done
    magick "$OUT/$CASE-panel-1.png" "$OUT/$CASE-panel-2.png" "$OUT/$CASE-panel-3.png" \
        -background white -splice 40x0 +append +repage -gravity east -splice 40x0 +repage \
        -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 46 -fill '#8B0A1F' \
        -annotate +0+28 "Crimson WM · F8: diálogos (captura real en Xvfb)" \
        -gravity south -splice 0x80 -font DejaVu-Sans -pointsize 28 -fill '#57606A' \
        -annotate +0+24 "A: rojo · B: negro · D (diálogo de A): gris. Volcados de xwd -root de un Xvfb aislado de 1280×720, sin retoques; foco y pila consultados al servidor." \
        +repage "$OUT/$CASE-comparison.png"
    same=''
    for i in 1 2 3; do
        x=$((40 + (i - 1) * (1284 + 40) + 2))
        magick "$OUT/$CASE-comparison.png" -crop "1280x720+$x+222" +repage "$OUT/$CASE-check-$i.png"
        same+="$(magick compare -metric AE "$OUT/$CASE-$i.png" "$OUT/$CASE-check-$i.png" null: 2>&1) "
    done
    record A86-composicion "0 0 0" "${same% }" "$(ok_if test "${same% }" = "0 0 0")" "píxeles distintos entre cada captura y su panel ($(magick identify -format '%wx%h' "$OUT/$CASE-comparison.png")); rótulos: $(cat "$OUT/$CASE"-[123].txt | paste -sd'|')"
    sha256sum "$OUT/$CASE"-[123].png "$OUT/$CASE-comparison.png" | sed "s#$OUT/##" >"$OUT/$CASE-sha256.txt"
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
