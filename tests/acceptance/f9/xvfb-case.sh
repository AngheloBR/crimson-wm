#!/bin/bash
# Una prueba de aceptación de F9 (ADR 0011) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh), y las de estado, las mismas que en F8. Como en F4–F8, el
# único directorio del PATH del WM contiene copias de f4/fake.sh: ninguna
# aplicación real puede ejecutarse.
#
# TEST_CLIENT (examples/test-client.rs) crea ventanas con cualquier lista de
# tipos (`--type`), override-redirect, WM_TRANSIENT_FOR y colores de fondo.
# A101 se ejecuta con el binario de F8 (commit 195938d) y A102 con el de
# F9; los dos hacen capturas reales del Xvfb con `xwd`.
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

# start9: arranca el WM con los programas falsos instalados y registra la
# evidencia de aislamiento, como start4 en F4 (solo nombres de variables).
start9() {
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
STICKY=4294967295
FS=$(atom_id _NET_WM_STATE_FULLSCREEN)
precondition "átomo de FULLSCREEN" test -n "$FS"

# Capturas (A101 y A102): volcado real del Xvfb y rótulo con datos del
# servidor.
shot() {
    sleep 1
    xwd -root -silent -out "$OUT/$1.xwd" && magick "$OUT/$1.xwd" "$OUT/$1.png"
    printf 'foco: %s · pila: %s\n' "$(focus_name)" "$(order_of "${@:2}" | sed 's/ / < /g')" >"$OUT/$1.txt"
}
px() { magick "$OUT/$1.png" -format "%[hex:p{$2}]" info:; }
# Colores de la escena de la comparación (estética Crimson).
RED=8B0A1F BLACK=1F2328 GRAY=6E7781 WHITE=F6F8FA DARK=3D444D LIGHT=D0D7DE
# scene: A y B en mosaico, un DOCK (K) arriba y un DESKTOP (W) que ocupa la
# pantalla, abierto el último.
scene() {
    open_tc A --background "$RED"
    open_tc B --background "$BLACK"
    open_tc K --type DOCK --geometry 0 0 1280 48 --background "$WHITE"
    open_tc W --type DESKTOP --geometry 0 0 1280 720 --background "$GRAY"
    precondition "foco en B" wait_for focus_is B
}

case $CASE in
A88)
    # NORMAL, desconocidos, listas y propiedades mal formadas.
    start9
    open_tc N --type NORMAL
    open_tc U --type _CRIMSON_TIPO_DESCONOCIDO
    open_tc L --type _CRIMSON_TIPO_DESCONOCIDO --type UTILITY --type NORMAL --geometry 900 100 200 100
    open_tc P --type NORMAL --type DIALOG
    verdict=FALLA
    managed_as N mosaico && managed_as U mosaico && managed_as L flotante && managed_as P mosaico && verdict=OK
    record A88-clasificacion "N, U y P en mosaico; L flotante" "N: $(managed_line N); U: $(managed_line U); L: $(managed_line L); P: $(managed_line P)" "$verdict" "registro del WM: NORMAL; desconocido; [desconocido, UTILITY, NORMAL]; [NORMAL, DIALOG]"
    expect_layout A88-mosaico "N=$LEFT" "U=640 0 640 360" "P=640 360 640 360"
    expect_frame A88-flotante-sin-mover L "900 100 200 100 0"
    # Mal formadas: tipo CARDINAL y formato 8; sin WM_TRANSIENT_FOR, van al
    # mosaico como una ventana sin tipo.
    open_tc C --dialog --geometry 100 100 200 100
    open_tc E --dialog --geometry 300 100 200 100
    precondition "C flotante" managed_as C flotante
    retire_n C 1
    retire_n E 1
    xprop -id "${WIDS[C]}" -f _NET_WM_WINDOW_TYPE 32c -set _NET_WM_WINDOW_TYPE 5
    xprop -id "${WIDS[E]}" -f _NET_WM_WINDOW_TYPE 8s -set _NET_WM_WINDOW_TYPE _NET_WM_WINDOW_TYPE_DIALOG
    remap C
    remap E
    verdict=FALLA
    wait_for managed_as C mosaico && wait_for managed_as E mosaico && kill -0 "$WM_PID" && verdict=OK
    record A88-mal-formadas "C y E en mosaico; WM en marcha" "C: $(managed_line C); E: $(managed_line E); tipos: $(xprop -id "${WIDS[C]}" _NET_WM_WINDOW_TYPE) | $(xprop -id "${WIDS[E]}" _NET_WM_WINDOW_TYPE)" "$verdict" "_NET_WM_WINDOW_TYPE de tipo CARDINAL y de formato 8"
    stop
    ;;
A89)
    # DIALOG, UTILITY y TOOLBAR: flotantes con foco; la transitoria se
    # centra; una transitoria sin tipo es un diálogo.
    start9
    open_tc A
    open_tc D --dialog --geometry 50 60 300 200
    expect A89-dialog "escritorio=1 foco=D activa=D A=v D=v" A D
    open_tc T --type TOOLBAR --geometry 900 60 200 80
    expect A89-toolbar "escritorio=1 foco=T activa=T A=v D=v T=v" A D T
    open_dialog V A --type UTILITY --geometry 0 0 300 200
    expect A89-utility "escritorio=1 foco=V activa=V A=v D=v T=v V=v" A D T V
    expect_frame A89-utility-centrada V "490 260 300 200 0"
    open_dialog S A --geometry 0 0 200 100
    verdict=FALLA
    managed_as D flotante && managed_as T flotante && managed_as V flotante && managed_as S flotante && verdict=OK
    record A89-flotantes "D, T, V y S flotantes" "D: $(managed_line D); T: $(managed_line T); V: $(managed_line V); S: $(managed_line S)" "$verdict" "registro del WM"
    expect_frame A89-transitoria-sin-tipo S "540 310 200 100 0"
    expect_layout A89-mosaico "A=$FULL"
    stop
    ;;
A90)
    # SPLASH, MENU y las emergentes gestionadas (sin override-redirect):
    # flotantes, sin foco y en la posición pedida, aunque sean transitorias.
    start9
    open_tc A
    open_tc B
    precondition "foco en B" wait_for focus_is B
    names=(S M DM PM TT CB DD)
    types=(SPLASH MENU DROPDOWN_MENU POPUP_MENU TOOLTIP COMBO DND)
    want_frames=''
    for i in "${!names[@]}"; do
        x=$((20 + i * 170))
        open_dialog "${names[i]}" A --type "${types[i]}" --geometry "$x" 200 150 100
        want_frames+="${names[i]}=$x 200 150 100 0; "
    done
    verdict=FALLA
    n=0
    for nm in "${names[@]}"; do managed_as "$nm" flotante && n=$((n + 1)); done
    ((n == 7)) && verdict=OK
    record A90-flotantes "7 flotantes" "$n flotantes: $(for nm in "${names[@]}"; do printf '%s ' "$(managed_line "$nm")"; done)" "$verdict" "SPLASH, MENU, DROPDOWN_MENU, POPUP_MENU, TOOLTIP, COMBO y DND transitorias de A"
    expect_stays A90-sin-foco "escritorio=1 foco=B activa=B A=v B=v S=v M=v DM=v PM=v TT=v CB=v DD=v" A B "${names[@]}"
    got=''
    for nm in "${names[@]}"; do got+="$nm=$(frame "$nm"); "; done
    record A90-posicion "${want_frames%; }" "${got%; }" "$(ok_if test "$got" = "$want_frames")" "sin centrar sobre A (xwininfo)"
    seq=''
    for _ in 1 2 3; do
        key super+j
        sleep 0.3
        seq+="$(focus_name) "
    done
    record A90-navegacion "A B A" "${seq% }" "$(ok_if test "${seq% }" = "A B A")" "Mod4+j tres veces: solo las de mosaico"
    expect_layout A90-mosaico "A=$LEFT" "B=$RIGHT"
    stop
    ;;
A91)
    # Override-redirect: nunca se gestiona. Excepción documentada: el WM no
    # la reordena, así que una flotante que sube después queda encima.
    start9
    open_tc A
    open_tc D --dialog --geometry 450 350 200 100
    open_tc O --override-redirect --type POPUP_MENU --geometry 400 300 200 150
    UNMANAGED=O
    sleep 0.5
    verdict=FALLA
    ! logged "ventana ${WIDS[O]} gestionada" && [[ $(wm_state_of O) == ausente && $(raw_desk O) == ausente ]] && verdict=OK
    record A91-sin-gestionar "sin gestionar; sin WM_STATE ni _NET_WM_DESKTOP" "gestionada: $(logged "ventana ${WIDS[O]} gestionada" && echo sí || echo no); $(props O)" "$verdict" "registro del WM y xprop"
    expect_list A91-listas _NET_CLIENT_LIST "A D"
    expect_frame A91-geometria O "400 300 200 150 0"
    record A91-foco-intacto "D" "$(focus_name)" "$(ok_if focus_is D)" "el WM no enfoca la override-redirect"
    record A91-orden-inicial "A D O" "$(order_of A D O)" "$(ok_if order_is "A D O" A D O)" "O, creada la última, encima"
    # Una ventana nueva hace que el WM vuelva a subir las flotantes: D pasa
    # por encima de O, que el WM no toca.
    open_tc E
    verdict=FALLA
    wait_for order_is "A O E D" A D E O && wait_for stacking_matches && wait_for frame_is O "400 300 200 150 0" && verdict=OK
    record A91-excepcion "A O E D (O sin tocar; D, subida por el WM, encima)" "$(order_of A D E O); O=$(frame O)" "$verdict" "excepción documentada en el ADR 0011"
    stop
    ;;
A92)
    # DOCK y NOTIFICATION: fijas de la capa superior en todos los
    # escritorios, sin foco, fuera de la navegación y de las peticiones.
    start9
    open_tc A
    open_tc B
    open_tc K --type DOCK --geometry 0 0 1280 40
    open_tc N --type NOTIFICATION --geometry 1000 50 260 80
    verdict=FALLA
    managed_as K "fija, capa superior" && managed_as N "fija, capa superior" && verdict=OK
    record A92-fijas "K y N fijas, capa superior" "K: $(managed_line K); N: $(managed_line N)" "$verdict" "registro del WM"
    expect_props A92-propiedades "K=Normal/$STICKY N=Normal/$STICKY A=Normal/0" K N A
    expect A92-sin-foco "escritorio=1 foco=B activa=B A=v B=v K=v N=v" A B K N
    seq=''
    for _ in 1 2 3; do
        key super+j
        sleep 0.3
        seq+="$(focus_name) "
    done
    record A92-navegacion "A B A" "${seq% }" "$(ok_if test "${seq% }" = "A B A")" "Mod4+j tres veces"
    expect_layout A92-sin-reservar "A=$LEFT" "B=$RIGHT" "K=0 0 1280 40" "N=1000 50 260 80"
    expect_order A92-capa-superior "A B K N" A B K N
    go 2
    expect A92-escritorio-2 "escritorio=2 foco=PointerRoot activa=ninguna A=o B=o K=v N=v" A B K N
    expect_props A92-siguen-fijas "K=Normal/$STICKY N=Normal/$STICKY A=Iconic/0" K N A
    go 1
    expect_list A92-client-list _NET_CLIENT_LIST "A B K N"
    message K _NET_ACTIVE_WINDOW 2 0 0 0 0
    message K _NET_WM_DESKTOP 3 2 0 0 0
    message K _NET_CLOSE_WINDOW 0 2 0 0 0
    wm_state N 1 2
    verdict=FALLA
    wait_for output_is 3 rejected_fixed K && wait_for output_is 1 rejected_fixed N && verdict=OK
    record A92-peticiones-rechazadas "3 en K, 1 en N" "K: $(rejected_fixed K); N: $(rejected_fixed N)" "$verdict" "_NET_ACTIVE_WINDOW, _NET_WM_DESKTOP, _NET_CLOSE_WINDOW y _NET_WM_STATE"
    expect_stays A92-tras-peticiones "escritorio=1 foco=A activa=A A=v B=v K=v N=v" A B K N
    record A92-sin-pantalla-completa "K=$STICKY ausente" "K=$(raw_desk K) $(net_state N)" "$(ok_if test "$(raw_desk K) $(net_state N)" = "$STICKY ausente")" "_NET_WM_DESKTOP de K y _NET_WM_STATE de N"
    stop
    ;;
A93)
    # DESKTOP: capa inferior, siempre bajo el mosaico; geometría concedida,
    # apilamiento pedido ignorado.
    start9
    open_tc A
    open_tc B
    open_tc W --type DESKTOP --geometry 0 0 1280 720
    record A93-fija "fija, capa inferior" "$(managed_line W)" "$(ok_if managed_as W "fija, capa inferior")" "registro del WM"
    expect_order A93-al-fondo "W A B" A B W
    expect A93-sin-foco "escritorio=1 foco=B activa=B A=v B=v W=v" A B W
    open_tc D --dialog --geometry 100 100 300 200
    expect_order A93-flotante-encima "W A B D" A B D W
    go 2
    open_tc C
    expect_order A93-escritorio-2 "W C" C W
    expect A93-visible-en-2 "escritorio=2 foco=C activa=C A=o B=o C=v D=o W=v" A B C D W
    go 1
    xdotool windowraise "${WIDS[W]}"
    verdict=FALLA
    wait_for logged "ventana ${WIDS[W]} (fija, capa inferior): apilamiento pedido ignorado" && wait_for grep -q "^ConfigureNotify sintético ${WIDS[W]} 0 0 1280 720" "$OUT/$CASE-W.out" && verdict=OK
    record A93-subida-ignorada "registrada; ConfigureNotify sintético con la geometría real" "$(grep -F "ventana ${WIDS[W]} (fija" "$WM_LOG" | sed 's/^crimson-wm: //'); $(grep -m1 '^ConfigureNotify sintético' "$OUT/$CASE-W.out")" "$verdict" "xdotool windowraise de W"
    expect_order A93-sigue-al-fondo "W A B D" A B D W
    xdotool windowmove "${WIDS[W]}" 10 20
    expect_frame A93-geometria-concedida W "10 20 1280 720 0"
    expect_order A93-tras-mover "W A B D" A B D W
    stop
    ;;
A94)
    # Capas y pantalla completa: la enfocada, por encima del DOCK; sin foco,
    # justo encima del DESKTOP. La familia de F8 sube con ella.
    start9
    open_tc W --type DESKTOP --geometry 0 0 1280 720
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40
    precondition "foco en A" wait_for focus_is A
    key super+f
    precondition "A a pantalla completa" wait_for fullscreen_is A
    expect_order A94-enfocada-arriba "W K A" A K W
    expect_layout A94-area-completa "A=$FULL"
    open_tc B
    expect_order A94-sin-foco "W A B K" A B K W
    precondition "foco en A" focus_on A
    expect_order A94-vuelve-arriba "W B K A" A B K W
    xdotool windowraise "${WIDS[K]}"
    precondition "subida de K registrada" wait_for logged "ventana ${WIDS[K]} (fija, capa superior): apilamiento pedido ignorado"
    expect_order A94-dock-no-sube "W B K A" A B K W
    open_dialog D A --geometry 0 0 300 200
    expect A94-foco-dialogo "escritorio=1 foco=D activa=D A=v B=v D=v K=v W=v" A B D K W
    expect_order A94-familia "W B K A D" A B D K W
    stop
    ;;
A95)
    # Pantalla completa solo en los tipos que la admiten.
    start9
    open_tc S --type SPLASH --fullscreen --state _NET_WM_STATE_SKIP_TASKBAR --geometry 100 100 300 200
    verdict=FALLA
    wait_for logged "ventana ${WIDS[S]} no admite pantalla completa (estado inicial)" && wait_for net_state_is S _NET_WM_STATE_SKIP_TASKBAR && verdict=OK
    record A95-splash-inicial "rechazada; _NET_WM_STATE = SKIP_TASKBAR" "$(net_state S)" "$verdict" "SPLASH con FULLSCREEN antes de mapearse"
    expect_frame A95-splash-sin-cambios S "100 100 300 200 0"
    open_tc M --type MENU --geometry 500 100 200 300
    wm_state M 1 2
    verdict=FALLA
    wait_for logged "ventana ${WIDS[M]} no admite pantalla completa (_NET_WM_STATE Add (Pager))" && net_state_is M ausente && frame_is M "500 100 200 300 0" && verdict=OK
    record A95-menu-peticion "rechazada; sin _NET_WM_STATE; sin cambios" "$(net_state M); M=$(frame M)" "$verdict" "_NET_WM_STATE Add de un paginador"
    open_tc K --type DOCK --fullscreen --geometry 0 680 1280 40
    verdict=FALLA
    wait_for logged "ventana ${WIDS[K]} no admite pantalla completa (estado inicial)" && wait_for net_state_is K vacía && frame_is K "0 680 1280 40 0" && verdict=OK
    record A95-dock-inicial "rechazada; _NET_WM_STATE vacía" "$(net_state K); K=$(frame K)" "$verdict" "DOCK con FULLSCREEN antes de mapearse"
    open_tc F --fullscreen
    verdict=FALLA
    wait_for fullscreen_is F && wait_for layout_is "F=$FULL" && verdict=OK
    record A95-normal-admitida "F a pantalla completa ($FULL)" "$(net_state F); F=$(geometry F)" "$verdict" "control: una ventana normal sí la admite"
    stop
    ;;
A96)
    # Diálogos: uno de una fija no tiene propietario efectivo ni se centra;
    # uno de una ventana normal se comporta como en F8.
    start9
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40
    open_dialog D K --geometry 100 100 300 200
    verdict=FALLA
    logged "ventana ${WIDS[D]}: WM_TRANSIENT_FOR sin propietario efectivo (propietario pendiente o no gestionado)" && managed_as D flotante && verdict=OK
    record A96-dialogo-de-fija "flotante, sin propietario efectivo" "$(managed_line D); $(grep -F "ventana ${WIDS[D]}: " "$WM_LOG" | sed 's/^crimson-wm: //' | head -n1)" "$verdict" "D transitoria de K (DOCK)"
    expect_frame A96-sin-centrar D "100 100 300 200 0"
    expect A96-foco "escritorio=1 foco=D activa=D A=v D=v K=v" A D K
    open_dialog E A --geometry 0 0 300 200
    expect_frame A96-dialogo-normal E "490 260 300 200 0"
    expect_order A96-pila "A D E K" A D E K
    record A96-dialogo-de-a "diálogo de A" "$(grep -F "ventana ${WIDS[E]}: diálogo de" "$WM_LOG" | sed 's/^crimson-wm: //')" "$(ok_if logged "ventana ${WIDS[E]}: diálogo de ${WIDS[A]}")" "registro del WM (F8)"
    stop
    ;;
A97)
    # Cambios de _NET_WM_WINDOW_TYPE: ignorados mientras está gestionada;
    # se reclasifica al retirarse y volver a mapearse.
    start9
    open_tc T
    open_tc U
    xprop -id "${WIDS[T]}" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE _NET_WM_WINDOW_TYPE_DIALOG
    verdict=FALLA
    wait_for logged "ventana ${WIDS[T]}: cambio de _NET_WM_WINDOW_TYPE ignorado" && verdict=OK
    record A97-ignorado "registrado" "$(count_logged "ventana ${WIDS[T]}: cambio de _NET_WM_WINDOW_TYPE ignorado") registros" "$verdict" "xprop -set con T mapeada"
    sleep 0.5
    record A97-sigue-en-mosaico "T=$LEFT; U=$RIGHT; 1 gestión" "T=$(geometry T); U=$(geometry U); $(count_logged "ventana ${WIDS[T]} gestionada") gestión" "$(ok_if layout_is "T=$LEFT" "U=$RIGHT")" "sin reclasificar"
    retire_n T 1
    remap T
    verdict=FALLA
    wait_for managed_as T flotante && wait_for layout_is "U=$FULL" && verdict=OK
    record A97-reclasificada "T flotante; U ocupa el área" "$(managed_line T); U=$(geometry U)" "$verdict" "xdotool windowunmap y windowmap"
    open_tc K --type DOCK --geometry 0 0 1280 40
    xprop -id "${WIDS[K]}" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE _NET_WM_WINDOW_TYPE_NORMAL
    verdict=FALLA
    wait_for logged "ventana ${WIDS[K]}: cambio de _NET_WM_WINDOW_TYPE ignorado" && sleep 0.5 && managed_as K "fija, capa superior" && frame_is K "0 0 1280 40 0" && verdict=OK
    record A97-fija-ignorado "sigue fija" "$(managed_line K); K=$(frame K)" "$verdict" "xprop -set con K mapeada"
    retire_n K 1
    remap K
    verdict=FALLA
    wait_for managed_as K mosaico && wait_for layout_is "U=$LEFT" "K=$RIGHT" && verdict=OK
    record A97-fija-reclasificada "K en mosaico" "$(managed_line K); U=$(geometry U); K=$(geometry K)" "$verdict" "retirada y vuelta a mapear"
    stop
    ;;
A98)
    # Retirada y destrucción de fijas: sin residuos en las listas ni en las
    # propiedades.
    start9
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40
    open_tc N --type NOTIFICATION --geometry 1000 50 260 80
    open_tc W --type DESKTOP --geometry 0 0 1280 720
    expect_list A98-antes _NET_CLIENT_LIST "A K N W"
    kill "${PIDS[N]}"
    unset 'PIDS[N]'
    verdict=FALLA
    wait_for logged "ventana ${WIDS[N]} retirada (fija, capa superior)" && wait_for gone N && verdict=OK
    record A98-destruida "N retirada" "$(grep -F "ventana ${WIDS[N]} retirada" "$WM_LOG" | sed 's/^crimson-wm: //')" "$verdict" "kill del cliente de N"
    "$TEST_CLIENT" withdraw "${WIDS[K]}" >/dev/null
    verdict=FALLA
    wait_for logged "ventana ${WIDS[K]} retirada (fija, capa superior)" && wait_for output_is o map_state K && verdict=OK
    record A98-retirada-sintetica "K retirada y oculta" "$(grep -F "ventana ${WIDS[K]} retirada" "$WM_LOG" | sed 's/^crimson-wm: //'); K=$(map_state K)" "$verdict" "UnmapNotify sintético (ICCCM §4.1.4)"
    xdotool windowunmap "${WIDS[W]}"
    verdict=FALLA
    wait_for logged "ventana ${WIDS[W]} retirada (fija, capa inferior): desmapeada por el cliente" && verdict=OK
    record A98-desmapeada "W retirada" "$(grep -F "ventana ${WIDS[W]} retirada" "$WM_LOG" | sed 's/^crimson-wm: //')" "$verdict" "xdotool windowunmap"
    UNMANAGED="K N W"
    expect_props A98-propiedades "K=ausente/ausente W=ausente/ausente" K W
    expect_list A98-client-list _NET_CLIENT_LIST "A"
    expect_stacking A98-stacking "A"
    expect A98-foco "escritorio=1 foco=A activa=A A=v" A
    stop
    ;;
A99)
    # Adopción al arrancar, salida y nueva adopción.
    open_tc K --type DOCK --geometry 0 0 1280 40
    open_tc A
    open_tc W --type DESKTOP --geometry 0 0 1280 720
    precondition "sin WM, W arriba" order_is "K A W" A K W
    start9
    precondition "adopción" wait_for output_is 3 count_logged gestionada
    verdict=FALLA
    managed_as K "fija, capa superior" && managed_as W "fija, capa inferior" && managed_as A mosaico && verdict=OK
    record A99-adopcion "K y W fijas; A en mosaico" "K: $(managed_line K); W: $(managed_line W); A: $(managed_line A)" "$verdict" "ventanas mapeadas antes de arrancar"
    expect_order A99-capas "W A K" A K W
    expect A99-foco "escritorio=1 foco=A activa=A A=v K=v W=v" A K W
    expect_props A99-propiedades "K=Normal/$STICKY W=Normal/$STICKY A=Normal/0" K W A
    key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    sleep 0.5
    record A99-salida "A=v K=v W=v; propiedades conservadas" "$(for n in A K W; do printf '%s=%s ' "$n" "$(map_state "$n")"; done)$(props K W)" "$(ok_if test "$(map_state A)$(map_state K)$(map_state W) $(props K W)" = "vvv K=Normal/$STICKY W=Normal/$STICKY")" "tras la salida del WM"
    restart "$CASE-2"
    verdict=FALLA
    wait_for output_is 3 count_logged gestionada && managed_as K "fija, capa superior" && managed_as W "fija, capa inferior" && wait_for order_is "W A K" A K W && verdict=OK
    record A99-readopcion "K y W fijas otra vez; W A K" "K: $(managed_line K); W: $(managed_line W); pila: $(order_of A K W)" "$verdict" "segunda instancia del WM"
    stop
    ;;
A100)
    # _NET_SUPPORTED exacta y listas con fijas y ventanas de dos
    # escritorios.
    start9
    want="_NET_SUPPORTED, _NET_SUPPORTING_WM_CHECK, _NET_CLIENT_LIST, _NET_CLIENT_LIST_STACKING, _NET_NUMBER_OF_DESKTOPS, _NET_CURRENT_DESKTOP, _NET_ACTIVE_WINDOW, _NET_CLOSE_WINDOW, _NET_WM_DESKTOP, _NET_WM_STATE, _NET_WM_STATE_FULLSCREEN, _NET_WM_WINDOW_TYPE, _NET_WM_WINDOW_TYPE_NORMAL, _NET_WM_WINDOW_TYPE_DIALOG, _NET_WM_WINDOW_TYPE_UTILITY, _NET_WM_WINDOW_TYPE_TOOLBAR, _NET_WM_WINDOW_TYPE_SPLASH, _NET_WM_WINDOW_TYPE_MENU, _NET_WM_WINDOW_TYPE_DOCK, _NET_WM_WINDOW_TYPE_DESKTOP, _NET_WM_WINDOW_TYPE_NOTIFICATION"
    got=$(xprop -root _NET_SUPPORTED | sed 's/^_NET_SUPPORTED(ATOM) = //')
    record A100-supported "$want" "$got" "$(ok_if test "$got" = "$want")" "xprop -root _NET_SUPPORTED"
    open_tc A
    open_tc K --type DOCK --geometry 0 0 1280 40
    go 2
    open_tc B
    open_tc W --type DESKTOP --geometry 0 0 1280 720
    expect_list A100-client-list _NET_CLIENT_LIST "A K B W"
    expect_stacking A100-stacking "W A B K"
    expect A100-estado "escritorio=2 foco=B activa=B A=o B=v K=v W=v" A B K W
    stop
    ;;
A101)
    # Panel 1 de la comparación, con el binario de F8 (195938d): el
    # DESKTOP, abierto el último, sube por encima de todo.
    start9
    scene
    expect_order A101-pila "A B K W" A B K W
    verdict=FALLA
    managed_as W flotante && managed_as K flotante && verdict=OK
    record A101-version "W y K flotantes (comportamiento de F8)" "W: $(managed_line W); K: $(managed_line K)" "$verdict" "registro del WM de F8"
    shot "$CASE-1" A B K W
    colors="$(px "$CASE-1" 100,300) $(px "$CASE-1" 900,300) $(px "$CASE-1" 640,20)"
    want="$GRAY $GRAY $GRAY"
    record A101-colores "$want" "$colors" "$(ok_if test "$colors" = "$want")" "W tapa a A, a B y al DOCK"
    stop
    ;;
A102)
    # Paneles 2 y 3 con F9 y composición de la comparación.
    start9
    scene
    expect_order A102-pila "W A B K" A B K W
    shot "$CASE-2" A B K W
    colors="$(px "$CASE-2" 100,300) $(px "$CASE-2" 900,300) $(px "$CASE-2" 640,20)"
    want="$RED $BLACK $WHITE"
    record A102-colores-1 "$want" "$colors" "$(ok_if test "$colors" = "$want")" "A, B y el DOCK visibles; W al fondo"
    go 2
    open_tc C --background "$DARK"
    open_dialog M C --type POPUP_MENU --geometry 880 300 260 160 --background "$LIGHT"
    expect A102-escritorio-2 "escritorio=2 foco=C activa=C A=o B=o C=v K=v M=v W=v" A B C K M W
    expect_frame A102-menu-en-su-sitio M "880 300 260 160 0"
    expect_order A102-pila-2 "W C M K" C K M W
    shot "$CASE-3" C K M W
    colors="$(px "$CASE-3" 100,300) $(px "$CASE-3" 1000,380) $(px "$CASE-3" 640,20)"
    want="$DARK $LIGHT $WHITE"
    record A102-colores-2 "$want" "$colors" "$(ok_if test "$colors" = "$want")" "C, el menú y el DOCK"
    first=$OUT/A101-1.png
    if [[ ! -s $first ]]; then
        for id in A102-dimensiones A102-composicion; do
            record "$id" "tres capturas" "falta la de F8" OMITIDA "A101 no se ejecutó"
        done
    else
        cp "$first" "$OUT/$CASE-1.png"
        cp "$OUT/A101-1.txt" "$OUT/$CASE-1.txt"
        dims=$(for i in 1 2 3; do magick identify -format '%wx%h ' "$OUT/$CASE-$i.png"; done)
        record A102-dimensiones "1280x720 1280x720 1280x720" "${dims% }" "$(ok_if test "${dims% }" = "1280x720 1280x720 1280x720")" "magick identify de las tres capturas"
        labels=("1 · F8 (195938d): DESKTOP tapa el mosaico" "2 · F9: DESKTOP al fondo, DOCK encima" "3 · F9, escritorio 2: DOCK y menú en su sitio")
        for i in 1 2 3; do
            magick "$OUT/$CASE-$i.png" -bordercolor '#1F2328' -border 2 -background white \
                -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 34 -fill '#1F2328' \
                -annotate +0+14 "${labels[i - 1]}" -font DejaVu-Sans -pointsize 26 -fill '#57606A' \
                -annotate +0+62 "$(cat "$OUT/$CASE-$i.txt")" +repage "$OUT/$CASE-panel-$i.png"
        done
        magick "$OUT/$CASE-panel-1.png" "$OUT/$CASE-panel-2.png" "$OUT/$CASE-panel-3.png" \
            -background white -splice 40x0 +append +repage -gravity east -splice 40x0 +repage \
            -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 46 -fill '#8B0A1F' \
            -annotate +0+28 "Crimson WM · F8 frente a F9: tipos de ventana (capturas reales en Xvfb)" \
            -gravity south -splice 0x80 -font DejaVu-Sans -pointsize 28 -fill '#57606A' \
            -annotate +0+24 "A: rojo · B: negro · DESKTOP (W): gris · DOCK (K): blanco · C: gris oscuro · menú (M): gris claro. Volcados de xwd -root de Xvfb aislados de 1280×720, sin retoques." \
            +repage "$OUT/$CASE-comparison.png"
        same=''
        for i in 1 2 3; do
            x=$((40 + (i - 1) * (1284 + 40) + 2))
            magick "$OUT/$CASE-comparison.png" -crop "1280x720+$x+222" +repage "$OUT/$CASE-check-$i.png"
            same+="$(magick compare -metric AE "$OUT/$CASE-$i.png" "$OUT/$CASE-check-$i.png" null: 2>&1) "
        done
        record A102-composicion "0 0 0" "${same% }" "$(ok_if test "${same% }" = "0 0 0")" "píxeles distintos entre cada captura y su panel ($(magick identify -format '%wx%h' "$OUT/$CASE-comparison.png")); rótulos: $(cat "$OUT/$CASE"-[123].txt | paste -sd'|')"
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
