#!/bin/bash
# Una prueba de aceptación de F11 (ADR 0013) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh), y las de estado, las mismas que en F10. Como en F4–F10, el
# único directorio del PATH del WM contiene copias de f4/fake.sh: ninguna
# aplicación real puede ejecutarse.
#
# Las reglas llegan al WM en CRIMSON_RULES solo en este arnés (start11).
# TEST_CLIENT (examples/test-client.rs) pone WM_CLASS (`--class`); los
# valores defectuosos se escriben con xprop y el cambio dinámico, con
# `xdotool set_window --class`. A133 se ejecuta con el binario de F10
# (commit f90dfa2) y A134 con el de F11; los dos hacen capturas reales del
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

# start11 [REGLAS]: arranca el WM con los programas falsos instalados y
# registra la evidencia de aislamiento, como start4 en F4 (solo nombres de
# variables). Con REGLAS, el WM recibe CRIMSON_RULES; sin ellas, el entorno es
# el de F1–F10.
BASE_ENV=("${WM_ENV[@]}")
start11() {
    local app found summary='' names expected verdict=FALLA
    WM_ENV=("${BASE_ENV[@]}")
    (($#)) && WM_ENV+=("CRIMSON_RULES=$1")
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
    # Entradas separadas por NUL: el valor de CRIMSON_RULES puede tener
    # varias líneas.
    names=$(while IFS= read -r -d '' entry; do printf '%s\n' "${entry%%=*}"; done <"/proc/$WM_REAL/environ" | sort | paste -sd' ')
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

# Reglas (ADR 0013).
# open_hidden nombre [opciones...]: ventana de test-client que el WM deja
# oculta en otro escritorio (no se espera a que sea visible).
open_hidden() {
    local name=$1
    shift
    "$TEST_CLIENT" window "$name" "$@" >"$OUT/$CASE-$name.out" 2>&1 &
    PIDS[$name]=$!
    precondition "ventana $name creada" wait_for grep -q '^ventana 0x' "$OUT/$CASE-$name.out"
    WIDS[$name]=$(sed -n 's/^ventana //p' "$OUT/$CASE-$name.out" | head -n1)
}
# desk_of nombre: _NET_WM_DESKTOP de 1 a 8, o «ausente».
desk_of() {
    local v
    v=$(raw_desk "$1")
    [[ $v =~ ^[0-9]+$ && $v -lt 8 ]] && echo $((v + 1)) || echo "$v"
}
rules_line() { grep -F "ventana ${WIDS[$1]}: reglas de las líneas" "$WM_LOG" | tail -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]*: //'; }
has_rules_line() { grep -qF "ventana ${WIDS[$1]}: reglas de las líneas" "$WM_LOG"; }
hidden_on() { logged "ventana ${WIDS[$1]} al escritorio $2 regla de la línea $3 (oculta)"; }
class_error() { grep -F "WM_CLASS de ${WIDS[$1]} no se interpreta" "$WM_LOG" | tail -n1 | sed 's/.*(\(.*\)): sin clase/\1/'; }

# Capturas (A133 y A134): volcado real del Xvfb y rótulo con datos del
# servidor.
shot() {
    sleep 1
    xwd -root -silent -out "$OUT/$1.xwd" && magick "$OUT/$1.xwd" "$OUT/$1.png"
    printf 'escritorio %s · foco: %s · pila: %s\n' "$(current_desk)" "$(focus_name)" "$(order_of "${@:2}" | sed 's/ / < /g')" >"$OUT/$1.txt"
}
px() { magick "$OUT/$1.png" -format "%[hex:p{$2}]" info:; }
RED=8B0A1F BLACK=1F2328 LIGHT=D0D7DE WHITE=F6F8FA
SCENE_RULES=$'class=Viewer; floating\nclass=Chat; desktop=2'
# scene: A y B normales, V (class Viewer) y D (class Chat), en ese orden.
scene() {
    open_tc A --background "$RED"
    open_tc B --background "$BLACK"
    open_tc V --class viewer Viewer --geometry 700 120 460 300 --background "$LIGHT"
}

case $CASE in
A121)
    # Sin CRIMSON_RULES: el comportamiento de F10.
    start11
    open_tc A --class alpha Alpha
    open_tc B --class beta Beta
    expect_layout A121-mosaico "A=$LEFT" "B=$RIGHT"
    expect A121-foco "escritorio=1 foco=B activa=B A=v B=v" A B
    n=$(grep -cE "reglas|CRIMSON_RULES|WM_CLASS" "$WM_LOG")
    record A121-sin-reglas 0 "$n" "$(ok_if test "$n" = 0)" "líneas de reglas, CRIMSON_RULES o WM_CLASS en el registro del WM"
    stop
    ;;
A122)
    # Coincidencia exacta de class: escritorio 3, oculta, sin cambiar el
    # activo ni el foco.
    start11 "class=Chat; desktop=3"
    open_tc A
    open_hidden C --class chat Chat
    verdict=FALLA
    wait_for hidden_on C 3 1 && verdict=OK
    record A122-regla "escritorio 3 (línea 1)" "$(rules_line C)" "$verdict" "registro del WM"
    expect_stays A122-oculta "escritorio=1 foco=A activa=A A=v C=o" A C
    record A122-propiedades "Iconic/2" "$(wm_state_of C)/$(raw_desk C)" "$(ok_if test "$(wm_state_of C)/$(raw_desk C)" = Iconic/2)" "WM_STATE y _NET_WM_DESKTOP de C"
    go 3
    expect A122-al-mostrar "escritorio=3 foco=C activa=C A=o C=v" A C
    expect_layout A122-mosaico-en-3 "C=$FULL"
    stop
    ;;
A123)
    # Sin coincidencia por mayúsculas o por una condición del Y lógico.
    start11 $'class=firefox; desktop=2\ninstance=nav; class=Firefox; desktop=3'
    open_tc F --class Navigator Firefox
    sleep 0.5
    verdict=FALLA
    ! has_rules_line F && [[ $(desk_of F) == 1 ]] && verdict=OK
    record A123-sin-coincidencia "escritorio 1, sin reglas" "escritorio $(desk_of F); reglas: $(rules_line F)" "$verdict" "firefox ≠ Firefox; instance Navigator ≠ nav"
    open_hidden G --class nav Firefox
    verdict=FALLA
    wait_for hidden_on G 3 2 && verdict=OK
    record A123-y-logico "escritorio 3 (línea 2)" "$(rules_line G)" "$verdict" "instance y class a la vez"
    expect A123-estado "escritorio=1 foco=F activa=F F=v G=o" F G
    stop
    ;;
A124)
    # WM_CLASS defectuosa: sin clase y sin coincidencia; las demás ventanas
    # siguen con sus reglas.
    start11 "class=Bad; floating"
    for n in X1 X2 X3; do open_tc "$n"; done
    for n in X1 X2 X3; do retire "$n"; done
    xprop -id "${WIDS[X1]}" -f WM_CLASS 8s -set WM_CLASS "Bad"
    xprop -id "${WIDS[X2]}" -f WM_CLASS 32c -set WM_CLASS 7
    long=$(printf 'b%.0s' {1..600})
    xprop -id "${WIDS[X3]}" -f WM_CLASS 8s -set WM_CLASS "$long"
    for n in X1 X2 X3; do remap "$n"; done
    verdict=FALLA
    wait_for output_is "Structure { strings: 1 }" class_error X1 &&
        wait_for output_is "Type { type_: 6, format: 32 }" class_error X2 &&
        wait_for output_is "TooLong { bytes_after: 88 }" class_error X3 && verdict=OK
    record A124-defectos "Structure, Type, TooLong" "$(class_error X1) | $(class_error X2) | $(class_error X3)" "$verdict" "una cadena, formato 32 y 600 bytes"
    verdict=FALLA
    managed_as X1 mosaico && managed_as X2 mosaico && managed_as X3 mosaico && ! has_rules_line X1 && kill -0 "$WM_PID" && verdict=OK
    record A124-sin-regla "en mosaico; WM en marcha" "X1: $(managed_line X1); X2: $(managed_line X2); X3: $(managed_line X3)" "$verdict" "registro del WM"
    open_tc Y --class bad Bad --geometry 900 100 200 100
    verdict=FALLA
    wait_for managed_as Y flotante && verdict=OK
    record A124-otra-ventana "Y flotante" "$(managed_line Y); $(rules_line Y)" "$verdict" "WM_CLASS válida"
    stop
    ;;
A125)
    # Flotante forzada: geometría pedida, encima del mosaico, navegable.
    start11 "class=Viewer; floating"
    open_tc A
    open_tc V --class viewer Viewer --geometry 300 200 400 300
    record A125-flotante "gestionada (flotante)" "$(managed_line V); $(rules_line V)" "$(ok_if managed_as V flotante)" "registro del WM"
    expect_layout A125-geometria "A=$FULL" "V=300 200 400 300"
    expect_order A125-encima "A V" A V
    seq=''
    for _ in 1 2 3; do
        key super+j
        sleep 0.3
        seq+="$(focus_name) "
    done
    record A125-navegacion "A V A" "${seq% }" "$(ok_if test "${seq% }" = "A V A")" "Mod4+j tres veces desde V"
    stop
    ;;
A126)
    # nofocus: sin foco inicial; el usuario puede enfocarla después.
    start11 "class=Quiet; nofocus"
    open_tc A
    open_tc Q --class quiet Quiet
    expect_stays A126-sin-foco-inicial "escritorio=1 foco=A activa=A A=v Q=v" A Q
    expect_layout A126-mosaico "A=$LEFT" "Q=$RIGHT"
    key super+j
    expect A126-foco-posterior "escritorio=1 foco=Q activa=Q A=v Q=v" A Q
    stop
    ;;
A127)
    # Pantalla completa inicial: admitida en una normal, rechazada en un
    # tipo que no la admite (F9).
    start11 $'class=Movie; fullscreen\nclass=Splashy; fullscreen'
    open_tc A
    open_tc M --class movie Movie
    verdict=FALLA
    wait_for fullscreen_is M && wait_for layout_is "M=$FULL" && verdict=OK
    record A127-admitida "M a pantalla completa ($FULL)" "$(net_state M); M=$(geometry M)" "$verdict" "regla de la línea 1"
    expect A127-foco "escritorio=1 foco=M activa=M A=v M=v" A M
    open_tc S --type SPLASH --class splashy Splashy --geometry 100 100 300 200
    verdict=FALLA
    wait_for logged "ventana ${WIDS[S]} no admite pantalla completa (regla de la línea 2)" && net_state_is S ausente && verdict=OK
    record A127-rechazada "rechazada; sin _NET_WM_STATE" "$(net_state S); S=$(frame S)" "$verdict" "SPLASH con la regla de la línea 2"
    stop
    ;;
A128)
    # Conflicto: gana la primera regla que define cada acción.
    start11 $'class=Chat; desktop=2\nclass=Chat; desktop=4; floating\ninstance=chat; nofocus'
    open_tc A
    open_hidden C --class chat Chat --geometry 200 150 500 300
    verdict=FALLA
    wait_for hidden_on C 2 1 && verdict=OK
    record A128-primera-gana "escritorio 2 (línea 1)" "$(rules_line C)" "$verdict" "registro del WM"
    verdict=FALLA
    wait_for logged "ventana ${WIDS[C]}: escritorio 4 de la regla de la línea 2 ignorado: ya lo fija una regla anterior" && verdict=OK
    record A128-ignorada "escritorio 4 de la línea 2 ignorado" "$(grep -cF "ventana ${WIDS[C]}: escritorio 4 de la regla" "$WM_LOG") registros" "$verdict" "registro del WM"
    record A128-acciones "flotante (línea 2), sin foco inicial (línea 3)" "$(rules_line C)" "$(ok_if managed_as C flotante)" "acciones distintas de reglas distintas"
    expect_stays A128-sin-foco "escritorio=1 foco=A activa=A A=v C=o" A C
    record A128-escritorio "2" "$(desk_of C)" "$(ok_if test "$(desk_of C)" = 2)" "_NET_WM_DESKTOP de C (de 1 a 8)"
    go 2
    expect_frame A128-geometria C "200 150 500 300 0"
    stop
    ;;
A129)
    # Un diálogo con propietario sigue a su principal; las condiciones de
    # transitoriedad sí se aplican.
    start11 $'class=Dlg; desktop=5\ntransient=yes; class=Quiet; nofocus'
    open_tc A
    open_dialog D A --class dlg Dlg --geometry 0 0 300 200
    verdict=FALLA
    wait_for logged "ventana ${WIDS[D]}: escritorio 5 de la regla de la línea 1 ignorado: diálogo de ${WIDS[A]}" && verdict=OK
    record A129-escritorio-ignorado "ignorado: diálogo de A" "$(grep -F "ventana ${WIDS[D]}: escritorio 5" "$WM_LOG" | sed 's/^crimson-wm: ventana 0x[0-9a-f]*: //')" "$verdict" "registro del WM"
    expect A129-con-su-principal "escritorio=1 foco=D activa=D A=v D=v" A D
    record A129-escritorio "1" "$(desk_of D)" "$(ok_if test "$(desk_of D)" = 1)" "_NET_WM_DESKTOP de D"
    expect_frame A129-centrado D "490 260 300 200 0"
    open_dialog E A --class quiet Quiet --geometry 0 0 200 100
    expect_stays A129-transient-nofocus "escritorio=1 foco=D activa=D A=v D=v E=v" A D E
    stop
    ;;
A130)
    # DOCK, DESKTOP y NOTIFICATION quedan fuera de las reglas.
    start11 $'class=Panel; desktop=2; floating; nofocus\nclass=Note; desktop=3\nclass=Back; fullscreen'
    open_tc A
    open_tc K --type DOCK --class panel Panel --geometry 0 0 1280 40
    open_tc N --type NOTIFICATION --class note Note --geometry 1000 50 260 80
    open_tc W --type DESKTOP --class back Back --geometry 0 0 1280 720
    verdict=FALLA
    managed_as K "fija, capa superior" && managed_as N "fija, capa superior" && managed_as W "fija, capa inferior" &&
        ! has_rules_line K && ! has_rules_line N && ! has_rules_line W && verdict=OK
    record A130-sin-reglas "fijas, sin reglas" "K: $(managed_line K); N: $(managed_line N); W: $(managed_line W)" "$verdict" "registro del WM"
    expect_props A130-propiedades "K=Normal/4294967295 N=Normal/4294967295 W=Normal/4294967295" K N W
    record A130-sin-pantalla-completa ausente "$(net_state W)" "$(ok_if net_state_is W ausente)" "_NET_WM_STATE de W"
    go 2
    expect A130-escritorio-2 "escritorio=2 foco=PointerRoot activa=ninguna A=o K=v N=v W=v" A K N W
    stop
    ;;
A131)
    # Un cambio de WM_CLASS con la ventana gestionada no reevalúa las
    # reglas; al retirarla y volver a mapearla, sí.
    start11 "class=Late; desktop=4"
    open_tc L --class late Other
    xdotool set_window --class Late "${WIDS[L]}"
    precondition "WM_CLASS cambiada" wait_for eval "xprop -id ${WIDS[L]} WM_CLASS | grep -qF '\"late\", \"Late\"'"
    sleep 0.5
    verdict=FALLA
    ! has_rules_line L && [[ $(desk_of L) == 1 && $(map_state L) == v ]] && verdict=OK
    record A131-sin-reevaluar "escritorio 1, visible" "escritorio $(desk_of L), L=$(map_state L), reglas: $(rules_line L)" "$verdict" "xdotool set_window --class con L mapeada"
    retire L
    xdotool windowmap "${WIDS[L]}"
    verdict=FALLA
    wait_for hidden_on L 4 1 && verdict=OK
    record A131-al-volver-a-mapear "escritorio 4 (línea 1)" "$(rules_line L)" "$verdict" "retirada y vuelta a mapear"
    expect A131-oculta "escritorio=1 foco=PointerRoot activa=ninguna L=o" L
    stop
    ;;
A132)
    # Fuentes inválidas: el WM arranca sin reglas y lo registra; la marca
    # del sandbox sigue siendo obligatoria.
    start11 "class=Chat; desktop=9"
    verdict=FALLA
    logged "CRIMSON_RULES inválida (línea 1: escritorio «9» fuera de 1–8): se continúa sin reglas" && verdict=OK
    record A132-sintaxis "inválida; sin reglas" "$(grep -F 'CRIMSON_RULES' "$WM_LOG" | head -n1 | sed 's/^crimson-wm: //')" "$verdict" "desktop=9"
    open_tc C --class chat Chat
    record A132-sin-aplicar "escritorio 1, sin reglas" "escritorio $(desk_of C); reglas: $(rules_line C)" "$(ok_if test "$(desk_of C)" = 1)" "C sigue en el activo"
    stop
    # Las demás fuentes, cada una con su instancia del WM en el mismo Xvfb.
    restart_with() {
        WM_ENV=("${BASE_ENV[@]}" "CRIMSON_RULES=$2")
        restart "$1"
    }
    many=$(for _ in $(seq 65); do echo "class=Chat; floating"; done)
    restart_with "$CASE-2" "$many"
    record A132-demasiadas "65 reglas (máximo 64)" "$(grep -F 'CRIMSON_RULES' "$WM_LOG" | head -n1 | sed 's/^crimson-wm: //')" "$(ok_if logged 'CRIMSON_RULES inválida (65 reglas (máximo 64))')" "65 líneas"
    stop
    big="class=Chat; floating"$'\n#'"$(printf 'x%.0s' {1..16400})"
    restart_with "$CASE-3" "$big"
    record A132-demasiado-grande "más de 16384 bytes" "$(grep -F 'CRIMSON_RULES' "$WM_LOG" | head -n1 | sed 's/^crimson-wm: //')" "$(ok_if logged 'bytes (máximo 16384)')" "${#big} bytes"
    stop
    restart_with "$CASE-4" $'class=Ch\xffat; floating'
    record A132-no-utf8 "no es UTF-8" "$(grep -F 'CRIMSON_RULES' "$WM_LOG" | head -n1 | sed 's/^crimson-wm: //')" "$(ok_if logged 'CRIMSON_RULES inválida (no es UTF-8)')" "byte 0xff"
    stop
    # Con reglas válidas, sin la marca del sandbox no arranca (capa 2).
    xprop -root -remove _CRIMSON_SANDBOX
    WM_ENV=("${BASE_ENV[@]}" "CRIMSON_RULES=class=Chat; desktop=2")
    start_wm "$CASE-sin-marca"
    wait_wm 10 'marca de sandbox'
    ;;
A133)
    # Panel 1 de la comparación, con el binario de F10 (f90dfa2): sin
    # reglas, las cuatro ventanas en mosaico.
    start11 "$SCENE_RULES"
    scene
    open_tc D --class chat Chat --background "$WHITE"
    expect_layout A133-mosaico "A=$LEFT" "B=640 0 640 240" "V=640 240 640 240" "D=640 480 640 240"
    record A133-version "sin registros de reglas (F10)" "$(grep -c 'reglas de las líneas' "$WM_LOG") registros" "$(ok_if test "$(grep -c 'reglas de las líneas' "$WM_LOG")" = 0)" "el binario de F10 no lee CRIMSON_RULES"
    shot "$CASE-1" A B V D
    colors="$(px "$CASE-1" 100,300) $(px "$CASE-1" 900,100) $(px "$CASE-1" 900,350) $(px "$CASE-1" 900,600)"
    want="$RED $BLACK $LIGHT $WHITE"
    record A133-colores "$want" "$colors" "$(ok_if test "$colors" = "$want")" "A, B, V y D en mosaico"
    stop
    ;;
A134)
    # Paneles 2 y 3 con F11 y composición.
    start11 "$SCENE_RULES"
    scene
    open_hidden D --class chat Chat --background "$WHITE"
    precondition "D en el escritorio 2" wait_for hidden_on D 2 2
    expect_layout A134-reglas "A=$LEFT" "B=$RIGHT" "V=700 120 460 300"
    expect A134-escritorio-1 "escritorio=1 foco=V activa=V A=v B=v D=o V=v" A B D V
    shot "$CASE-2" A B V D
    colors="$(px "$CASE-2" 100,300) $(px "$CASE-2" 900,600) $(px "$CASE-2" 900,200)"
    want="$RED $BLACK $LIGHT"
    record A134-colores-1 "$want" "$colors" "$(ok_if test "$colors" = "$want")" "A y B en mosaico; V flotante encima"
    go 2
    expect A134-escritorio-2 "escritorio=2 foco=D activa=D A=o B=o D=v V=o" A B D V
    expect_layout A134-chat "D=$FULL"
    shot "$CASE-3" D
    colors="$(px "$CASE-3" 640,360)"
    record A134-colores-2 "$WHITE" "$colors" "$(ok_if test "$colors" = "$WHITE")" "D sola en el escritorio 2"
    first=$OUT/A133-1.png
    if [[ ! -s $first ]]; then
        for id in A134-dimensiones A134-composicion; do
            record "$id" "tres capturas" "falta la de F10" OMITIDA "A133 no se ejecutó"
        done
    else
        cp "$first" "$OUT/$CASE-1.png"
        cp "$OUT/A133-1.txt" "$OUT/$CASE-1.txt"
        dims=$(for i in 1 2 3; do magick identify -format '%wx%h ' "$OUT/$CASE-$i.png"; done)
        record A134-dimensiones "1280x720 1280x720 1280x720" "${dims% }" "$(ok_if test "${dims% }" = "1280x720 1280x720 1280x720")" "magick identify de las tres capturas"
        labels=("1 · F10 (f90dfa2): sin reglas, todo en mosaico" "2 · F11: Viewer flotante; Chat al escritorio 2" "3 · F11, escritorio 2: Chat")
        for i in 1 2 3; do
            magick "$OUT/$CASE-$i.png" -bordercolor '#1F2328' -border 2 -background white \
                -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 34 -fill '#1F2328' \
                -annotate +0+14 "${labels[i - 1]}" -font DejaVu-Sans -pointsize 26 -fill '#57606A' \
                -annotate +0+62 "$(cat "$OUT/$CASE-$i.txt")" +repage "$OUT/$CASE-panel-$i.png"
        done
        magick "$OUT/$CASE-panel-1.png" "$OUT/$CASE-panel-2.png" "$OUT/$CASE-panel-3.png" \
            -background white -splice 40x0 +append +repage -gravity east -splice 40x0 +repage \
            -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 46 -fill '#8B0A1F' \
            -annotate +0+28 "Crimson WM · F10 frente a F11: reglas de ventanas (capturas reales en Xvfb)" \
            -gravity south -splice 0x80 -font DejaVu-Sans -pointsize 28 -fill '#57606A' \
            -annotate +0+24 "CRIMSON_RULES: class=Viewer; floating · class=Chat; desktop=2. A: rojo · B: negro · V (Viewer): gris claro · D (Chat): blanco. Volcados de xwd -root, sin retoques." \
            +repage "$OUT/$CASE-comparison.png"
        same=''
        for i in 1 2 3; do
            x=$((40 + (i - 1) * (1284 + 40) + 2))
            magick "$OUT/$CASE-comparison.png" -crop "1280x720+$x+222" +repage "$OUT/$CASE-check-$i.png"
            same+="$(magick compare -metric AE "$OUT/$CASE-$i.png" "$OUT/$CASE-check-$i.png" null: 2>&1) "
        done
        record A134-composicion "0 0 0" "${same% }" "$(ok_if test "${same% }" = "0 0 0")" "píxeles distintos entre cada captura y su panel ($(magick identify -format '%wx%h' "$OUT/$CASE-comparison.png")); rótulos: $(cat "$OUT/$CASE"-[123].txt | paste -sd'|')"
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
