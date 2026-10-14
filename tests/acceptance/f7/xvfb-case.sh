#!/bin/bash
# Una prueba de aceptación de F7 (ADR 0009) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh), y las de estado, las mismas que en F6. Como en F4–F6, el
# único directorio del PATH del WM contiene copias de f4/fake.sh: ninguna
# aplicación real puede ejecutarse.
#
# TEST_CLIENT (examples/test-client.rs) crea ventanas que piden la pantalla
# completa antes de mapearse y diálogos con un marco conocido, envía
# mensajes EWMH y registra los ConfigureNotify recibidos. A74 hace capturas
# reales del Xvfb con `xwd`.
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

# start7: arranca el WM con los programas falsos instalados y registra la
# evidencia de aislamiento, como start4 en F4 (solo nombres de variables).
start7() {
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
# señales de A73 solo se envían a ese proceso.
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

case $CASE in
A63)
    start7
    open_xev A
    open_xev B
    open_xev C
    key super+k
    precondition "foco en B" wait_for focus_is B
    before=$(layout_now A B C)
    key super+f
    expect_layout A63-entra "B=$FULL" "A=$LEFT" "C=$RIGHT"
    got=$(net_state B)
    record A63-estado _NET_WM_STATE_FULLSCREEN "$got" "$(ok_if test "$got" = _NET_WM_STATE_FULLSCREEN)" "xprop _NET_WM_STATE de B"
    verdict=FALLA
    wait_for top_is B && focus_is B && verdict=OK
    record A63-encima "B arriba y enfocada" "pila: $(tree_names); foco: $(focus_name)" "$verdict" "xwininfo -root -children y getwindowfocus"
    key super+f
    verdict=FALLA
    wait_for layout_now_is "$before" A B C && verdict=OK
    record A63-sale-mosaico "$before" "$(layout_now A B C)" "$verdict" "mismo mosaico y orden que antes de Mod4+f"
    verdict=FALLA
    wait_for not_fullscreen B && focus_is B && verdict=OK
    record A63-sale-estado "sin FULLSCREEN; foco en B" "$(net_state B); foco: $(focus_name)" "$verdict" "xprop y getwindowfocus"
    # Tecla mantenida: un solo cambio.
    xdotool keydown super+f
    sleep 1.5
    xdotool keyup super+f
    sleep 0.5
    verdict=FALLA
    fullscreen_is B && [[ $(fs_count B) == 2 ]] && verdict=OK
    record A63-mantenido "un solo cambio" "$(net_state B); entradas registradas: $(fs_count B)" "$verdict" "Mod4+f mantenido 1,5 s"
    stop
    ;;
A65)
    start7
    open_xev A
    open_xev B
    MAXV=$(atom_id _NET_WM_STATE_MAXIMIZED_VERT)
    wm_state B 1 2
    verdict=FALLA
    wait_for fullscreen_is B && wait_for layout_is "B=$FULL" && verdict=OK
    record A65-anadir "B a pantalla completa" "$(net_state B); B=$(geometry B)" "$verdict" "_NET_WM_STATE ADD, fuente 2"
    wm_state B 1 1
    sleep 0.5
    record A65-anadir-otra-vez "1 entrada" "$(fs_count B) entradas" "$(ok_if test "$(fs_count B)" = 1)" "ADD repetido: idempotente"
    wm_state B 2 1
    verdict=FALLA
    wait_for not_fullscreen B && wait_for layout_is "A=$LEFT" "B=$RIGHT" && verdict=OK
    record A65-alternar-sale "B sale" "$(net_state B); $(layout_now A B)" "$verdict" "TOGGLE, fuente 1"
    wm_state B 0 1
    sleep 0.5
    verdict=FALLA
    not_fullscreen B && [[ $(grep -c "ventana ${WIDS[B]} sale de pantalla completa" "$WM_LOG") == 1 ]] && verdict=OK
    record A65-quitar-sin-estado "sin cambios" "$(net_state B); salidas registradas: $(grep -c "ventana ${WIDS[B]} sale de pantalla completa" "$WM_LOG")" "$verdict" "REMOVE sin estar a pantalla completa: idempotente"
    wm_state B 2 0
    verdict=FALLA
    wait_for fullscreen_is B && verdict=OK
    record A65-alternar-entra "B entra" "$(net_state B)" "$verdict" "TOGGLE, fuente 0"
    wm_state B 0 2
    verdict=FALLA
    wait_for not_fullscreen B && verdict=OK
    record A65-quitar "B sale" "$(net_state B)" "$verdict" "REMOVE, fuente 2"
    # Estados no gestionados: ni se aplican ni se añaden.
    wm_state A 1 2 "$MAXV"
    wm_state A 3 2
    "$TEST_CLIENT" message 32 root _NET_WM_STATE 1 "$FS" 0 2 0
    verdict=FALLA
    wait_for logged 'UnsupportedState' && wait_for logged 'Action(3)' &&
        wait_for logged "_NET_WM_STATE ignorada: $ROOT_ID no está gestionada" && [[ $(net_state A) == ausente ]] && verdict=OK
    record A65-invalidas "MAXIMIZED_VERT, acción 3 y raíz rechazados; A sin cambios" "A: $(net_state A); $(grep -cE 'UnsupportedState|Action\(3\)|_NET_WM_STATE ignorada' "$WM_LOG") rechazos" "$verdict" "registro del WM"
    # FULLSCREEN en data[2] junto a un estado no gestionado en data[1].
    wm_state A 1 2 "$MAXV" "$FS"
    verdict=FALLA
    wait_for fullscreen_is A && [[ $(net_state A) == _NET_WM_STATE_FULLSCREEN ]] && verdict=OK
    record A65-segundo-estado "solo FULLSCREEN" "$(net_state A)" "$verdict" "data[1] = MAXIMIZED_VERT (ignorado), data[2] = FULLSCREEN"
    stop
    ;;
A66)
    start7
    open_xev A
    open_tc D --dialog --geometry 100 80 300 200 --border 3
    precondition "D flotante" wait_for logged "ventana ${WIDS[D]} gestionada (flotante)"
    precondition "foco en D" wait_for focus_is D
    F0=$(frame D)
    key super+f
    expect_frame A66-entra D "0 0 1280 720 0"
    key super+f
    expect_frame A66-sale D "$F0"
    record A66-marco-inicial "100 80 300 200 3" "$F0" "$(ok_if test "$F0" = "100 80 300 200 3")" "el marco que pidió el cliente, con borde"
    expect_layout A66-mosaico "A=$FULL"
    stop
    ;;
A67)
    start7
    open_xev A
    open_xev B
    open_xev C
    open_tc D --dialog --geometry 40 50 320 240 --border 2
    precondition "D flotante" wait_for logged "ventana ${WIDS[D]} gestionada (flotante)"
    key super+j
    precondition "foco en A" wait_for focus_is A
    key super+j
    precondition "foco en B" wait_for focus_is B
    before=$(layout_now A B C)
    F0=$(frame D)
    for ((i = 0; i < 20; i++)); do key super+f; done
    verdict=FALLA
    wait_for fs_count_is B 10 && wait_for layout_now_is "$before" A B C && not_fullscreen B && verdict=OK
    record A67-mosaico-20 "$before" "$(layout_now A B C); entradas: $(fs_count B)" "$verdict" "20 Mod4+f sobre B (10 entradas y 10 salidas)"
    for ((i = 0; i < 20; i++)); do wm_state D 2 2; done
    verdict=FALLA
    wait_for fs_count_is D 10 && wait_for frame_is D "$F0" && not_fullscreen D && verdict=OK
    record A67-flotante-20 "D=$F0" "D=$(frame D); entradas: $(fs_count D)" "$verdict" "20 TOGGLE por _NET_WM_STATE sobre D"
    stop
    ;;
A68)
    start7
    open_xev A
    open_xev B
    key super+f
    precondition "B a pantalla completa" wait_for fullscreen_is B
    go 2
    open_xev C
    open_xev D
    wm_state D 1 2
    precondition "D a pantalla completa" wait_for fullscreen_is D
    go 1
    expect A68-escritorio-1 "escritorio=1 foco=B activa=B A=v B=v C=o D=o" A B C D
    expect_layout A68-escritorio-1-geometria "B=$FULL"
    go 2
    expect A68-escritorio-2 "escritorio=2 foco=D activa=D A=o B=o C=v D=v" A B C D
    expect_layout A68-escritorio-2-geometria "D=$FULL"
    # Segunda petición en el mismo escritorio: la última gana.
    wm_state C 1 2
    verdict=FALLA
    wait_for fullscreen_is C && wait_for not_fullscreen D && wait_for layout_is "C=$FULL" "D=$FULL" && verdict=OK
    record A68-ultima-gana "C a pantalla completa; D sale" "C: $(net_state C); D: $(net_state D); $(layout_now C D)" "$verdict" "D vuelve a ocupar sola el mosaico"
    # Petición para una ventana de un escritorio oculto.
    wm_state A 1 1
    precondition "A a pantalla completa" wait_for fullscreen_is A
    expect_stays A68-oculta-sin-cambios "escritorio=2 foco=D activa=D A=o B=o C=v D=v" A B C D
    verdict=FALLA
    wait_for not_fullscreen B && verdict=OK
    record A68-oculta-ultima-gana "B sale en el escritorio 1" "B: $(net_state B)" "$verdict" "A, oculta, pasa a pantalla completa"
    go 1
    # El foco vuelve a B (historial); A, sin foco, queda al fondo.
    expect A68-vuelta "escritorio=1 foco=B activa=B A=v B=v C=o D=o" A B C D
    verdict=FALLA
    wait_for layout_is "A=$FULL" "B=$FULL" && wait_for bottom_is A && verdict=OK
    record A68-vuelta-pila "A al fondo, B visible" "$(layout_now A B); pila: $(tree_names)" "$verdict" "xwininfo -root -children"
    stop
    ;;
A69)
    start7
    open_xev A
    open_xev B
    go 3
    open_xev C
    open_xev E
    wm_state C 1 2
    precondition "C a pantalla completa" wait_for fullscreen_is C
    go 1
    key super+f
    precondition "B a pantalla completa" wait_for fullscreen_is B
    key super+shift+3
    verdict=FALLA
    wait_for fullscreen_is B && wait_for not_fullscreen C && verdict=OK
    record A69-mover "B conserva; C sale" "B: $(net_state B); C: $(net_state C)" "$verdict" "Mod4+Shift+3 con B a pantalla completa"
    expect A69-origen "escritorio=1 foco=A activa=A A=v B=o C=o" A B C
    expect_layout A69-origen-mosaico "A=$FULL"
    go 3
    expect A69-destino "escritorio=3 foco=B activa=B A=o B=v C=v" A B C
    expect_layout A69-destino-geometria "B=$FULL" "C=$LEFT" "E=$RIGHT"
    xdotool set_desktop_for_window "${WIDS[B]}" 0
    verdict=FALLA
    wait_for test "$(net_desk B)" = 1 && fullscreen_is B && desk_is 3 && verdict=OK
    record A69-net-wm-desktop "B al escritorio 1, a pantalla completa" "B en $(net_desk B): $(net_state B); activo $(current_desk)" "$verdict" "xdotool set_desktop_for_window"
    go 1
    expect_layout A69-vuelta "B=$FULL" "A=$FULL"
    stop
    ;;
A70)
    start7
    open_xev A
    open_tc B
    open_xev C
    key super+k
    precondition "foco en B" wait_for focus_is B
    key super+f
    precondition "B a pantalla completa" wait_for fullscreen_is B
    key super+w
    verdict=FALLA
    wait_for gone B && wait_for layout_is "A=$LEFT" "C=$RIGHT" && verdict=OK
    record A70-cerrar "B cerrada; mosaico de A y C" "B $(map_state B); $(layout_now A C)" "$verdict" "Mod4+w a pantalla completa"
    expect A70-foco-tras-cerrar "escritorio=1 foco=C activa=C A=v C=v" A C
    open_xev D
    key super+f
    precondition "D a pantalla completa" wait_for fullscreen_is D
    kill "${PIDS[D]}"
    unset 'PIDS[D]'
    verdict=FALLA
    wait_for gone D && wait_for layout_is "A=$LEFT" "C=$RIGHT" && verdict=OK
    record A70-destruir "D destruida; mosaico de A y C" "D $(map_state D); $(layout_now A C)" "$verdict" "kill del cliente a pantalla completa"
    open_tc E --state _NET_WM_STATE_SKIP_TASKBAR
    key super+f
    precondition "E a pantalla completa" wait_for fullscreen_is E
    got=$(net_state E)
    record A70-estado-ajeno "_NET_WM_STATE_SKIP_TASKBAR,_NET_WM_STATE_FULLSCREEN" "$got" "$(ok_if test "$got" = "_NET_WM_STATE_SKIP_TASKBAR,_NET_WM_STATE_FULLSCREEN")" "el estado ajeno se conserva"
    xdotool windowunmap "${WIDS[E]}"
    precondition "E retirada" wait_for retired E
    verdict=FALLA
    wait_for net_state_is E _NET_WM_STATE_SKIP_TASKBAR && wait_for layout_is "A=$LEFT" "C=$RIGHT" && verdict=OK
    record A70-retirar "solo SKIP_TASKBAR; mosaico de A y C" "$(net_state E); $(layout_now A C)" "$verdict" "al retirarse, solo se quita FULLSCREEN"
    # Más de 64 estados ajenos: todos se conservan, en su orden.
    args=() names=()
    for ((i = 1; i <= 100; i++)); do
        names+=("$(printf '_CRIMSON_PRUEBA_%03d' "$i")")
        args+=(--state "${names[-1]}")
    done
    want=$(IFS=,; echo "${names[*]}")
    open_tc M "${args[@]}"
    key super+f
    verdict=FALLA
    wait_for net_state_is M "$want,_NET_WM_STATE_FULLSCREEN" && layout_is "M=$FULL" && verdict=OK
    record A70-cien-estados "100 ajenos y FULLSCREEN al final" "$(net_state M | tr ',' '\n' | wc -l) átomos; primero $(net_state M | cut -d, -f1); último $(net_state M | awk -F, '{print $NF}')" "$verdict" "Mod4+f con 100 estados ajenos"
    key super+f
    verdict=FALLA
    wait_for net_state_is M "$want" && verdict=OK
    record A70-cien-estados-sale "los 100 ajenos, en su orden" "$(net_state M | tr ',' '\n' | wc -l) átomos; iguales: $([[ $(net_state M) == "$want" ]] && echo sí || echo no)" "$verdict" "Mod4+f de nuevo"
    # Más de MAX_STATE_ATOMS (1024): no se lee entera, así que ni se escribe
    # ni la ventana entra en pantalla completa.
    args=()
    for ((i = 1; i <= 1100; i++)); do args+=(--state "$(printf '_CRIMSON_GRANDE_%04d' "$i")"); done
    open_tc G "${args[@]}"
    before=$(xprop -id "${WIDS[G]}" _NET_WM_STATE | sha256sum)
    key super+f
    precondition "rechazo registrado" wait_for logged "_NET_WM_STATE de ${WIDS[G]} no se modifica (TooLarge"
    sleep 0.5
    verdict=FALLA
    [[ $(xprop -id "${WIDS[G]}" _NET_WM_STATE | sha256sum) == "$before" ]] && not_fullscreen G && ! layout_is "G=$FULL" && verdict=OK
    record A70-demasiado-grande "propiedad intacta; sin pantalla completa" "$(xprop -id "${WIDS[G]}" _NET_WM_STATE | tr ',' '\n' | wc -l) átomos; G=$(geometry G)" "$verdict" "$(grep -o "_NET_WM_STATE de ${WIDS[G]} no se modifica ([A-Za-z]*" "$WM_LOG" | head -n1)"
    # Mal formada (CARDINAL, puesta por el cliente tras el mapeo): intacta.
    open_xev X
    xprop -id "${WIDS[X]}" -f _NET_WM_STATE 32c -set _NET_WM_STATE 7
    key super+f
    precondition "rechazo registrado" wait_for logged "_NET_WM_STATE de ${WIDS[X]} no se modifica (Malformed"
    sleep 0.5
    verdict=FALLA
    [[ $(xprop -id "${WIDS[X]}" _NET_WM_STATE) == '_NET_WM_STATE(CARDINAL) = 7' ]] && ! layout_is "X=$FULL" && verdict=OK
    record A70-mal-formada "propiedad intacta; sin pantalla completa" "$(xprop -id "${WIDS[X]}" _NET_WM_STATE); X=$(geometry X)" "$verdict" "$(grep -o "_NET_WM_STATE de ${WIDS[X]} no se modifica ([A-Za-z]*" "$WM_LOG" | head -n1)"
    stop
    ;;
A71)
    start7
    open_xev A
    open_tc F --fullscreen --state _NET_WM_STATE_SKIP_TASKBAR
    verdict=FALLA
    wait_for layout_is "F=$FULL" "A=$FULL" && fullscreen_is F && logged "ventana ${WIDS[F]} a pantalla completa (estado inicial)" && verdict=OK
    record A71-antes-del-mapeo "F a pantalla completa al mapearse" "$(layout_now F A); $(net_state F)" "$verdict" "_NET_WM_STATE puesto antes de MapWindow"
    xdotool windowsize "${WIDS[F]}" 300 200
    xdotool windowmove "${WIDS[F]}" 50 60
    verdict=FALLA
    sleep 0.5
    wait_for grep -q "^ConfigureNotify sintético ${WIDS[F]} 0 0 1280 720$" "$OUT/$CASE-F.out" && layout_is "F=$FULL" && verdict=OK
    record A71-configure-denegada "F sigue con el área; ConfigureNotify sintético del área" "F=$(geometry F); $(grep -c '^ConfigureNotify sintético' "$OUT/$CASE-F.out") sintéticos" "$verdict" "xdotool windowsize y windowmove"
    open_tc G --dialog --fullscreen --geometry 50 60 320 240 --border 1
    precondition "G a pantalla completa" wait_for fullscreen_is G
    expect_frame A71-flotante-inicial G "0 0 1280 720 0"
    xdotool windowsize "${WIDS[G]}" 100 100
    sleep 0.5
    wm_state G 0 2
    expect_frame A71-flotante-restaurada G "50 60 320 240 1"
    stop
    ;;
A72)
    start7
    open_xev A
    open_xev B
    key super+f
    precondition "B a pantalla completa" wait_for fullscreen_is B
    expect_stacking A72-encima "A B"
    key super+j
    precondition "foco en A" wait_for focus_is A
    expect_stacking A72-al-fondo "B A"
    expect_layout A72-al-fondo-geometria "A=$FULL" "B=$FULL"
    key super+j
    precondition "foco en B" wait_for focus_is B
    expect_stacking A72-vuelve-arriba "A B"
    open_tc D --dialog --geometry 200 150 300 200
    precondition "foco en D" wait_for focus_is D
    expect_stacking A72-dialogo "B A D"
    key super+w
    precondition "D cerrada" wait_for gone D
    expect A72-tras-dialogo "escritorio=1 foco=B activa=B A=v B=v" A B
    expect_stacking A72-tras-dialogo-pila "A B"
    stop
    ;;
A73)
    start7
    open_xev A
    open_xev B
    key super+f
    precondition "B a pantalla completa" wait_for fullscreen_is B
    go 2
    open_tc C --dialog --geometry 120 90 400 300 --border 2
    precondition "foco en C" wait_for focus_is C
    key super+f
    precondition "C a pantalla completa" wait_for fullscreen_is C
    xdotool key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    verdict=FALLA
    wait_for layout_is "A=$LEFT" "B=$RIGHT" && wait_for frame_is C "120 90 400 300 2" && not_fullscreen B && not_fullscreen C && verdict=OK
    record A73-salida-normal "A y B en mosaico; C con su marco; sin FULLSCREEN" "$(layout_now A B); C=$(frame C); B: $(net_state B); C: $(net_state C)" "$verdict" "tras Mod4+Shift+Escape"
    # SIGTERM con B (escritorio 1) y C (escritorio 2) a pantalla completa.
    restart A73-term
    precondition "adopción" wait_for adopted 3
    wm_state B 1 2
    "$TEST_CLIENT" message 32 "${WIDS[C]}" _NET_WM_DESKTOP 1 2 0 0 0
    precondition "C en el escritorio 2" wait_for test "$(net_desk C)" = 2
    wm_state C 1 2
    precondition "B y C a pantalla completa" wait_for eval 'fullscreen_is B && fullscreen_is C'
    # C, oculta, recibe el área al mostrarse su escritorio.
    go 2
    precondition "C con el área" wait_for layout_is "C=$FULL"
    go 1
    kill -TERM "$WM_REAL"
    wait_wm 143 'identificación publicada'
    verdict=FALLA
    wait_for vis_is "A=v B=v C=v" A B C && fullscreen_is B && fullscreen_is C && layout_is "B=$FULL" "C=$FULL" && verdict=OK
    record A73-sigterm "visibles; B y C conservan el área y FULLSCREEN" "$(vis A B C); $(layout_now B C); B: $(net_state B); C: $(net_state C)" "$verdict" "sin manejadores de señales: no hay restauración"
    # Recuperación: el arnés borra la propiedad obsoleta (fail-closed) y el
    # WM adopta las ventanas respetando _NET_WM_STATE; gana la más alta.
    run_bin A73-tras-sigterm 12 '_NET_SUPPORTING_WM_CHECK .*inexistente: obsoleta'
    xprop -root -remove _NET_SUPPORTING_WM_CHECK
    winner=$(tree_names | tr ' ' '\n' | grep -E '^(B|C)$' | tail -n1)
    loser=B
    [[ $winner == B ]] && loser=C
    restart A73-adopcion
    precondition "adopción" wait_for adopted 3
    verdict=FALLA
    wait_for fullscreen_is "$winner" && wait_for not_fullscreen "$loser" && wait_for layout_is "$winner=$FULL" && verdict=OK
    record A73-adopcion-estado "$winner a pantalla completa; $loser sale" "B: $(net_state B); C: $(net_state C); $(layout_now B C)" "$verdict" "la más alta de la pila ($winner) gana"
    # C, flotante con el área como marco, sale con el marco de respaldo.
    wm_state C 0 2
    verdict=FALLA
    wait_for not_fullscreen C && wait_for frame_is C "213 120 853 480 0" && verdict=OK
    record A73-respaldo "C=213 120 853 480 0" "C=$(frame C)" "$verdict" "marco centrado de 2/3: el original no se recupera tras SIGTERM"
    stop
    ;;
A74)
    # Captura real: tres xlogo estáticas (colores de Crimson) en un Xvfb
    # aislado, antes, a pantalla completa y restauradas. Las imágenes son
    # volcados de `xwd -root` sin retocar.
    start7
    open_window L1 xlogo -title L1 -bg white -fg '#8B0A1F'
    open_window L2 xlogo -title L2 -bg '#1F2328' -fg white
    open_window L3 xlogo -title L3 -bg '#D0D7DE' -fg '#1F2328'
    key super+k
    precondition "foco en L2" wait_for focus_is L2
    shot() {
        sleep 1
        xwd -root -silent -out "$OUT/$CASE-$1.xwd" && magick "$OUT/$CASE-$1.xwd" "$OUT/$CASE-$1.png"
    }
    expect_layout A74-antes "L1=$LEFT" "L2=640 0 640 360" "L3=640 360 640 360"
    shot 1
    key super+f
    expect_layout A74-pantalla-completa "L2=$FULL"
    shot 2
    key super+f
    expect_layout A74-restaurado "L1=$LEFT" "L2=640 0 640 360" "L3=640 360 640 360"
    shot 3
    dims=$(for i in 1 2 3; do magick identify -format '%wx%h ' "$OUT/$CASE-$i.png"; done)
    record A74-dimensiones "1280x720 1280x720 1280x720" "${dims% }" "$(ok_if test "${dims% }" = "1280x720 1280x720 1280x720")" "magick identify de las tres capturas"
    diff13=$(magick compare -metric AE "$OUT/$CASE-1.png" "$OUT/$CASE-3.png" null: 2>&1)
    record A74-restaurado-identico 0 "$diff13" "$(ok_if test "$diff13" = 0)" "píxeles distintos entre «antes» y «restaurado»"
    corners=$(for p in 5,5 1274,5 5,714 1274,714; do magick "$OUT/$CASE-2.png" -format "%[hex:p{$p}] " info:; done)
    record A74-cubierta "1F2328 ×4" "${corners% }" "$(ok_if test "${corners% }" = "1F2328 1F2328 1F2328 1F2328")" "esquinas de la captura a pantalla completa: fondo de L2"
    # Composición: rótulos, márgenes y bordes; el contenido no se toca.
    labels=("1 · Antes: master-stack" "2 · Mod4+f: pantalla completa" "3 · Mod4+f: restaurado")
    for i in 1 2 3; do
        magick "$OUT/$CASE-$i.png" -bordercolor '#1F2328' -border 2 -background white \
            -gravity north -splice 0x70 -font DejaVu-Sans-Bold -pointsize 34 -fill '#1F2328' \
            -annotate +0+16 "${labels[i - 1]}" +repage "$OUT/$CASE-panel-$i.png"
    done
    magick "$OUT/$CASE-panel-1.png" "$OUT/$CASE-panel-2.png" "$OUT/$CASE-panel-3.png" \
        -background white -splice 40x0 +append +repage -gravity east -splice 40x0 +repage \
        -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 46 -fill '#8B0A1F' \
        -annotate +0+28 "Crimson WM · F7: pantalla completa (captura real en Xvfb)" \
        -gravity south -splice 0x80 -font DejaVu-Sans -pointsize 28 -fill '#57606A' \
        -annotate +0+24 "Volcados de xwd -root de un Xvfb aislado de 1280×720, sin retoques. Los paneles 1 y 3 son idénticos píxel a píxel." \
        +repage "$OUT/$CASE-comparison.png"
    # Cada panel de la composición es idéntico a su captura.
    same=''
    for i in 1 2 3; do
        x=$((40 + (i - 1) * (1284 + 40) + 2))
        magick "$OUT/$CASE-comparison.png" -crop "1280x720+$x+182" +repage "$OUT/$CASE-check-$i.png"
        same+="$(magick compare -metric AE "$OUT/$CASE-$i.png" "$OUT/$CASE-check-$i.png" null: 2>&1) "
    done
    record A74-composicion "0 0 0" "${same% }" "$(ok_if test "${same% }" = "0 0 0")" "píxeles distintos entre cada captura y su panel en la composición ($(magick identify -format '%wx%h' "$OUT/$CASE-comparison.png"))"
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
