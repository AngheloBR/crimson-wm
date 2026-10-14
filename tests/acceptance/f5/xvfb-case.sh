#!/bin/bash
# Una prueba de aceptación de F5 (ADR 0007) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh). Como en F4, el único directorio del PATH del WM contiene
# copias de f4/fake.sh: ninguna aplicación real puede ejecutarse.
#
# TEST_CLIENT (examples/test-client.rs) envía el UnmapNotify sintético con
# el que un cliente retira una ventana (ICCCM §4.1.4).
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
records() {
    local f
    for f in "$LOGS/$1".*; do
        [[ ${f##*.} =~ ^[0-9]+$ ]] && echo "$f"
    done
}
has_record() { [[ -n $(records "$1") ]]; }
field() { sed -n "s/^$2=//p" "$1"; }
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

# start5: arranca el WM con los programas falsos instalados y registra la
# evidencia de aislamiento, como start4 en F4 (solo nombres de variables).
start5() {
    local app found summary='' names expected verdict=FALLA
    install_fakes
    start
    WM_REAL=$(pgrep -P "$WM_PID" -x crimson-wm)
    precondition "pid del WM" test -n "$WM_REAL"
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
# restart id: arranca otra instancia del WM en el mismo Xvfb (A51).
restart() {
    start_wm "$1"
    require_ready
    WM_REAL=$(pgrep -P "$WM_PID" -x crimson-wm)
    precondition "pid del WM" test -n "$WM_REAL"
}

# Estado de las ventanas.
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
    *"not found"*) echo ausente ;;
    *"window id # "*) name_of "${out##*# }" ;;
    *) echo "?$out" ;;
    esac
}
# map_state nombre: v (IsViewable), o (IsUnMapped) u otro valor.
map_state() {
    case $(xwininfo -id "${WIDS[$1]}" 2>/dev/null | awk '/Map State:/ {print $3}') in
    IsViewable) echo v ;;
    IsUnMapped) echo o ;;
    "") echo destruida ;;
    *) echo "?" ;;
    esac
}
# Escritorio activo (de 1 a 8) según _NET_CURRENT_DESKTOP.
current_desk() {
    local v
    v=$(xprop -root _NET_CURRENT_DESKTOP | sed -n 's/^_NET_CURRENT_DESKTOP(CARDINAL) = \([0-9]*\)$/\1/p')
    if [[ -n $v ]]; then echo $((v + 1)); else echo ausente; fi
}
# Escritorio de una ventana (de 1 a 8) según _NET_WM_DESKTOP.
net_desk() {
    local v
    v=$(xprop -id "${WIDS[$1]}" _NET_WM_DESKTOP 2>/dev/null | sed -n 's/^_NET_WM_DESKTOP(CARDINAL) = \([0-9]*\)$/\1/p')
    if [[ -n $v ]]; then echo $((v + 1)); else echo ausente; fi
}
# WM_STATE de una ventana: Normal, Iconic o ausente.
wm_state() {
    local v
    v=$(xprop -id "${WIDS[$1]}" WM_STATE 2>/dev/null | sed -n 's/^[[:space:]]*window state: //p')
    echo "${v:-ausente}"
}
# state nombres...: «escritorio=N foco=X activa=Y A=v B=o ...».
state() {
    local n s
    s="escritorio=$(current_desk) foco=$(focus_name) activa=$(active_name)"
    for n in "$@"; do s+=" $n=$(map_state "$n")"; done
    echo "$s"
}
state_is() { [[ $(state "${@:2}") == "$1" ]]; }
# expect id «estado» nombres...: espera hasta 5 s al estado (con `state`),
# con el WM en marcha.
expect() {
    local id=$1 want=$2 verdict=FALLA
    shift 2
    wait_for state_is "$want" "$@" && kill -0 "$WM_PID" && verdict=OK
    record "$id" "$want" "$(state "$@")" "$verdict" "_NET_CURRENT_DESKTOP, getwindowfocus, _NET_ACTIVE_WINDOW y Map State (v visible, o oculta)"
}
# expect_stays: comprobación negativa, tras una espera fija de 0,5 s.
expect_stays() {
    local id=$1 want=$2 verdict=FALLA
    shift 2
    sleep 0.5
    [[ $(state "$@") == "$want" ]] && kill -0 "$WM_PID" && verdict=OK
    record "$id" "$want" "$(state "$@")" "$verdict" "sin cambios tras 0,5 s"
}
# props nombres...: «A=escritorio/WM_STATE ...».
props() {
    local n s=''
    for n in "$@"; do s+="$n=$(net_desk "$n")/$(wm_state "$n") "; done
    echo "${s% }"
}
props_are() { [[ $(props "${@:2}") == "$1" ]]; }
expect_props() {
    local id=$1 want=$2 verdict=FALLA
    shift 2
    wait_for props_are "$want" "$@" && verdict=OK
    record "$id" "$want" "$(props "$@")" "$verdict" "_NET_WM_DESKTOP (de 1 a 8) / WM_STATE de cada ventana"
}
desk_is() { [[ $(current_desk) == "$1" ]]; }
# go N: activa el escritorio N y espera a que se publique.
go() {
    key "super+$1"
    precondition "escritorio $1 activo" wait_for desk_is "$1"
}
focus_is() { [[ $(focus_name) == "$1" ]]; }
visible() { [[ $(map_state "$1") == v ]]; }
hidden() { [[ $(map_state "$1") == o ]]; }
# vis nombres...: «A=v B=o ...», sin foco (también sin WM).
vis() {
    local n s=''
    for n in "$@"; do s+="$n=$(map_state "$n") "; done
    echo "${s% }"
}
vis_is() { [[ $(vis "${@:2}") == "$1" ]]; }
# focus_on nombre: Mod4+j hasta enfocar la ventana.
focus_on() {
    local i
    for ((i = 0; i < 8; i++)); do
        wait_for focus_is "$1" && return 0
        key super+j
    done
    return 1
}
# above a b: a está por encima de b en la pila (xwininfo lista primero la
# ventana de arriba).
above() {
    local list
    list=$(xwininfo -root -children | awk '{print $1}')
    [[ $(grep -nx "${WIDS[$1]}" <<<"$list" | cut -d: -f1) -lt $(grep -nx "${WIDS[$2]}" <<<"$list" | cut -d: -f1) ]]
}
# set_dialog nombre: retira la ventana y le pone _NET_WM_WINDOW_TYPE_DIALOG.
set_dialog() {
    retire "$1"
    xprop -id "${WIDS[$1]}" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE _NET_WM_WINDOW_TYPE_DIALOG
}
retired() { grep -q "ventana ${WIDS[$1]} retirada" "$WM_LOG"; }
retirements() { grep -c 'retirada' "$WM_LOG"; }

precondition "pantalla 1280x720" test "$SIZE" = 1280x720
FULL="0 0 1280 720"
LEFT="0 0 640 720"
RIGHT="640 0 640 720"

case $CASE in
A41)
    start5
    nd=$(xprop -root _NET_NUMBER_OF_DESKTOPS)
    verdict=FALLA
    [[ $nd == '_NET_NUMBER_OF_DESKTOPS(CARDINAL) = 8' && $(current_desk) == 1 ]] && verdict=OK
    record A41-numero "8 escritorios; activo 1" "$nd; activo $(current_desk)" "$verdict" "xprop -root"
    # Una ventana por escritorio.
    names=(W1 W2 W3 W4 W5 W6 W7 W8)
    open_xev W1
    for d in 2 3 4 5 6 7 8; do
        go "$d"
        open_xev "W$d"
    done
    # Recorrido: solo es visible la del activo, que tiene el foco.
    for d in 8 1 2 3 4 5 6 7 8 1; do
        key "super+$d"
        want="escritorio=$d foco=W$d activa=W$d"
        for n in "${names[@]}"; do
            if [[ $n == "W$d" ]]; then want+=" $n=v"; else want+=" $n=o"; fi
        done
        id="A41-escritorio-$d"
        [[ $(awk -F'\t' -v id="$id" '$1 == id' "$RESULTS") ]] && id+="-de-nuevo"
        expect "$id" "$want" "${names[@]}"
    done
    want=$(for d in 1 2 3 4 5 6 7 8; do
        if ((d == 1)); then printf 'W%s=%s/Normal ' "$d" "$d"; else printf 'W%s=%s/Iconic ' "$d" "$d"; fi
    done)
    expect_props A41-propiedades "${want% }" "${names[@]}"
    alive=0
    for n in "${names[@]}"; do kill -0 "${PIDS[$n]}" && alive=$((alive + 1)); done
    record A41-aplicaciones-vivas 8 "$alive" "$(ok_if test "$alive" = 8)" "kill -0 a los 8 xev tras recorrer los escritorios"
    # Desde F6 (ADR 0008) el soporte EWMH está declarado: la raíz apunta a la
    # ventana de comprobación y publica _NET_SUPPORTED.
    verdict=FALLA
    check=$(xprop -root _NET_SUPPORTING_WM_CHECK)
    xprop -root _NET_SUPPORTED | grep -q '_NET_WM_DESKTOP' && [[ $check == *"window id # 0x"* ]] && verdict=OK
    record A41-ewmh-declarado "_NET_SUPPORTING_WM_CHECK y _NET_SUPPORTED presentes" "$check" "$verdict" "xprop -root (ADR 0008)"
    record A41-sin-retiradas 0 "$(retirements)" "$(ok_if test "$(retirements)" = 0)" "líneas «retirada» en $WM_ID.log: los desmapeos del WM no son retiradas"
    stop
    ;;
A42)
    start5
    open_xev A
    open_xev B
    open_xev C
    key super+k
    expect A42-inicial "escritorio=1 foco=B activa=B A=v B=v C=v" A B C
    go 2
    open_xev D
    expect A42-escritorio-2 "escritorio=2 foco=D activa=D A=o B=o C=o D=v" A B C D
    expect_layout A42-mosaico-2 "D=$FULL"
    go 1
    expect A42-vuelta "escritorio=1 foco=B activa=B A=v B=v C=v D=o" A B C D
    expect_layout A42-vuelta-mosaico "A=$LEFT" "B=640 0 640 360" "C=640 360 640 360"
    key super+j
    expect A42-vuelta-navegacion "escritorio=1 foco=C activa=C A=v B=v C=v D=o" A B C D
    stop
    ;;
A43)
    start5
    open_xev A
    open_xev B
    open_xev C
    key super+k
    precondition "foco en B" wait_for focus_is B
    key super+shift+3
    expect A43-origen "escritorio=1 foco=C activa=C A=v B=o C=v" A B C
    expect_layout A43-origen-mosaico "A=$LEFT" "C=$RIGHT"
    expect_props A43-propiedades "A=1/Normal B=3/Iconic C=1/Normal" A B C
    go 3
    expect A43-destino "escritorio=3 foco=B activa=B A=o B=v C=o" A B C
    expect_layout A43-destino-mosaico "B=$FULL"
    expect_props A43-propiedades-destino "A=1/Iconic B=3/Normal C=1/Iconic" A B C
    key super+shift+3
    expect_stays A43-mismo-escritorio "escritorio=3 foco=B activa=B A=o B=v C=o" A B C
    go 4
    key super+shift+1
    expect_stays A43-sin-ventana "escritorio=4 foco=PointerRoot activa=ninguna A=o B=o C=o" A B C
    go 1
    expect A43-vuelta "escritorio=1 foco=C activa=C A=v B=o C=v" A B C
    stop
    ;;
A44)
    start5
    open_xev A
    open_xev B
    open_xev C
    key super+j
    # Historial del escritorio 1: A, C, B.
    expect A44-inicial "escritorio=1 foco=A activa=A A=v B=v C=v" A B C
    go 2
    open_xev E
    go 1
    expect A44-vuelta "escritorio=1 foco=A activa=A A=v B=v C=v E=o" A B C E
    go 2
    # Se cierra la enfocada del escritorio 1 mientras está oculto.
    kill "${PIDS[A]}"
    unset 'PIDS[A]'
    precondition "A retirada" wait_for retired A
    log=$(grep "ventana ${WIDS[A]} retirada" "$WM_LOG" | sed 's/^crimson-wm: //')
    verdict=FALLA
    [[ $log == *"del escritorio 1: destruida" ]] && verdict=OK
    record A44-cierre-oculta "retirada del escritorio 1 (destruida)" "$log" "$verdict" "registro del WM"
    expect_stays A44-activo-intacto "escritorio=2 foco=E activa=E B=o C=o E=v" B C E
    go 1
    # Vuelve la siguiente del historial (C), no la maestra.
    expect A44-foco-tras-cierre "escritorio=1 foco=C activa=C B=v C=v E=o" B C E
    expect_layout A44-mosaico-tras-cierre "B=$LEFT" "C=$RIGHT"
    stop
    ;;
A45)
    start5
    open_xev A
    open_xev B
    key super+shift+Return
    expect_layout A45-zoom-1 "B=$LEFT" "A=$RIGHT"
    go 2
    open_xev C
    open_xev D
    key super+j
    precondition "foco en C" wait_for focus_is C
    key super+shift+Return
    expect_layout A45-zoom-2 "D=$LEFT" "C=$RIGHT"
    go 1
    expect A45-escritorio-1-intacto "escritorio=1 foco=B activa=B A=v B=v C=o D=o" A B C D
    expect_layout A45-mosaico-1-intacto "B=$LEFT" "A=$RIGHT"
    # La navegación solo recorre el escritorio activo.
    seen=''
    for k in k k k k; do
        key "super+$k"
        sleep 0.3
        seen+="$(focus_name) "
    done
    record A45-navegacion-aislada "A B A B" "${seen% }" "$(ok_if test "${seen% }" = "A B A B")" "cuatro Mod4+k en el escritorio 1 (C y D están en el 2)"
    go 2
    expect A45-escritorio-2-intacto "escritorio=2 foco=C activa=C A=o B=o C=v D=v" A B C D
    expect_layout A45-mosaico-2-intacto "D=$LEFT" "C=$RIGHT"
    stop
    ;;
A46)
    start5
    open_xev A
    open_xev D
    set_dialog D
    xdotool windowmap "${WIDS[D]}"
    precondition "D flotante" wait_for grep -q "ventana ${WIDS[D]} gestionada (flotante)" "$WM_LOG"
    expect A46-flotante "escritorio=1 foco=D activa=D A=v D=v" A D
    G=$(geometry D)
    go 2
    expect A46-oculta "escritorio=2 foco=PointerRoot activa=ninguna A=o D=o" A D
    go 1
    expect A46-restaurada "escritorio=1 foco=D activa=D A=v D=v" A D
    verdict=FALLA
    [[ $(geometry D) == "$G" ]] && above D A && verdict=OK
    record A46-geometria-y-pila "$G, encima de A" "$(geometry D), $(above D A && echo encima || echo debajo) de A" "$verdict" "xwininfo; el mosaico es A=$(geometry A)"
    key super+shift+2
    expect A46-mover-flotante "escritorio=1 foco=A activa=A A=v D=o" A D
    expect_layout A46-mosaico-sin-flotante "A=$FULL"
    go 2
    expect A46-flotante-en-destino "escritorio=2 foco=D activa=D A=o D=v" A D
    record A46-geometria-en-destino "$G" "$(geometry D)" "$(ok_if test "$(geometry D)" = "$G")" "xwininfo"
    stop
    ;;
A47)
    start5
    open_xev A
    open_xev B
    go 2
    open_xev C
    open_xev D
    # D (activo) y A (oculto) terminan.
    kill "${PIDS[D]}" "${PIDS[A]}"
    unset 'PIDS[D]' 'PIDS[A]'
    precondition "A retirada" wait_for retired A
    precondition "D retirada" wait_for retired D
    expect A47-cierre-activo "escritorio=2 foco=C activa=C B=o C=v" B C
    expect_layout A47-mosaico-activo "C=$FULL"
    # MapRequest de una ventana oculta: sigue oculta, sin cambiar de
    # escritorio ni de foco.
    xdotool windowmap "${WIDS[B]}"
    precondition "MapRequest de B" wait_for grep -q "ventana ${WIDS[B]} del escritorio 1 sigue oculta" "$WM_LOG"
    expect_stays A47-mapeo-de-oculta "escritorio=2 foco=C activa=C B=o C=v" B C
    go 1
    expect A47-cierre-inactivo "escritorio=1 foco=B activa=B B=v C=o" B C
    expect_layout A47-mosaico-inactivo "B=$FULL"
    stop
    ;;
A48)
    start5
    open_xev A
    open_xev B
    go 2
    open_xev C
    go 1
    # Cambios muy rápidos, sin pausa entre teclas.
    seq=()
    for ((i = 0; i < 20; i++)); do seq+=(super+2 super+1); done
    xdotool key --delay 0 "${seq[@]}"
    expect A48-cambios-rapidos "escritorio=1 foco=B activa=B A=v B=v C=o" A B C
    # Con la pausa por defecto (12 ms): dos pulsaciones de la misma tecla con
    # la misma marca de tiempo que la liberación anterior se tomarían por
    # repetición automática.
    seq=()
    for ((i = 0; i < 15; i++)); do seq+=(super+Tab); done
    xdotool key "${seq[@]}"
    expect A48-tab-rapido "escritorio=2 foco=C activa=C A=o B=o C=v" A B C
    n=$(retirements)
    record A48-sin-retiradas-falsas 0 "$n" "$(ok_if test "$n" = 0)" "líneas «retirada» en $WM_ID.log tras $(grep -c '^crimson-wm: escritorio ' "$WM_LOG") cambios"
    go 1
    # Retirada real en el escritorio activo (UnmapNotify del servidor).
    xdotool windowunmap "${WIDS[A]}"
    precondition "A retirada" wait_for retired A
    log=$(grep "ventana ${WIDS[A]} retirada" "$WM_LOG" | sed 's/^crimson-wm: //')
    verdict=FALLA
    [[ $log == *": desmapeada por el cliente" ]] && verdict=OK
    record A48-retirada-real "desmapeada por el cliente" "$log" "$verdict" "xdotool windowunmap"
    expect_layout A48-mosaico-tras-retirada "B=$FULL"
    # Retirada de una ventana oculta (ICCCM §4.1.4): UnmapNotify sintético.
    go 2
    "$TEST_CLIENT" withdraw "${WIDS[B]}"
    precondition "B retirada" wait_for retired B
    log=$(grep "ventana ${WIDS[B]} retirada" "$WM_LOG" | sed 's/^crimson-wm: //')
    verdict=FALLA
    [[ $log == *"del escritorio 1: UnmapNotify sintético" ]] && verdict=OK
    record A48-retirada-oculta "UnmapNotify sintético, escritorio 1" "$log" "$verdict" "test-client withdraw con el escritorio 2 activo"
    expect_props A48-propiedades-retirada "B=ausente/ausente" B
    go 1
    expect A48-retirada-no-vuelve "escritorio=1 foco=PointerRoot activa=ninguna A=o B=o C=o" A B C
    # Una ventana oculta mapeada por otro cliente no se muestra.
    go 2
    open_xev H
    go 1
    xdotool windowmap "${WIDS[H]}"
    precondition "MapRequest de H" wait_for grep -q "ventana ${WIDS[H]} del escritorio 2 sigue oculta" "$WM_LOG"
    expect_stays A48-mapeo-de-oculta "escritorio=1 foco=PointerRoot activa=ninguna H=o" H
    # Carreras: el cliente retira su ventana a la vez que el WM la oculta,
    # con un retardo de 0 a 19 ms (1 ms más en cada vuelta) para que se den
    # los dos órdenes en el servidor. Sea cual sea, la ventana queda
    # retirada y la de control (K) sigue gestionada.
    open_xev K
    bad='' ids=()
    for ((i = 1; i <= 20; i++)); do
        open_xev "R$i"
        ids+=("${WIDS[R$i]}")
        # Solo esos dos procesos: `wait` sin argumentos esperaría también a
        # los clientes que siguen en marcha (xev y el xprop de xvfb-env.sh).
        key super+2 &
        k=$!
        (
            sleep "$(printf '0.%03d' $((i - 1)))"
            "$TEST_CLIENT" withdraw "${WIDS[R$i]}"
        ) &
        t=$!
        wait "$k" "$t"
        precondition "R$i retirada" wait_for retired "R$i"
        precondition "escritorio 2 activo" wait_for desk_is 2
        go 1
        wait_for visible K
        [[ $(map_state "R$i") == o && $(map_state K) == v && $(props "R$i") == "R$i=ausente/ausente" ]] || bad+=" R$i"
    done
    pattern=$(printf '%s|' "${ids[@]}")
    reasons=$(grep -E "ventana (${pattern%|}) retirada" "$WM_LOG" | sed 's/.*: //' | sort | uniq -c | sed 's/^ *//' | paste -sd',')
    record A48-carreras "20 retiradas; K gestionada" "incoherentes:${bad:- ninguna}" "$([[ -z $bad ]] && echo OK || echo FALLA)" "motivos registrados (todas las retiradas del caso): $reasons"
    retired K && kok=FALLA || kok=OK
    record A48-control-no-retirada "K nunca retirada" "$(retired K && echo retirada || echo gestionada)" "$kok" "registro del WM"
    stop
    ;;
A49)
    start5
    open_xev A
    open_xev B
    key super+Tab
    expect_stays A49-tab-sin-anterior "escritorio=1 foco=B activa=B A=v B=v" A B
    key super+1
    expect_stays A49-mismo-escritorio "escritorio=1 foco=B activa=B A=v B=v" A B
    n=$(grep -c '^crimson-wm: escritorio ' "$WM_LOG")
    record A49-sin-cambios 0 "$n" "$(ok_if test "$n" = 0)" "líneas «escritorio» en $WM_ID.log"
    go 3
    seen=''
    for k in Tab Tab Tab; do
        key "super+$k"
        sleep 0.3
        seen+="$(current_desk) "
    done
    go 5
    key super+Tab
    sleep 0.3
    seen+="$(current_desk)"
    record A49-alterna "1 3 1 1" "$seen" "$(ok_if test "$seen" = "1 3 1 1")" "Mod4+3, tres Mod4+Tab, Mod4+5 y Mod4+Tab (vuelve al 1, el anterior al 5)"
    # Tecla mantenida: la repetición automática no alterna ni mueve más.
    go 3
    go 1
    xdotool keydown super+Tab
    sleep 1.5
    xdotool keyup super+Tab
    expect_stays A49-tab-mantenido "escritorio=3 foco=PointerRoot activa=ninguna A=o B=o" A B
    go 1
    xdotool keydown super+shift+2
    sleep 1.5
    xdotool keyup super+shift+2
    expect_stays A49-mover-mantenido "escritorio=1 foco=A activa=A A=v B=o" A B
    expect_props A49-una-sola-movida "A=1/Normal B=2/Iconic" A B
    stop
    ;;
A50)
    start5
    echo 30 >"$LOGS/kitty.espera"
    go 4
    open_xev A
    open_xev B
    expect_layout A50-mosaico "A=$LEFT" "B=$RIGHT"
    key super+j
    expect A50-navegacion "escritorio=4 foco=A activa=A A=v B=v" A B
    key super+shift+Return
    expect_layout A50-zoom "B=$LEFT" "A=$RIGHT"
    key super+Return
    precondition "terminal lanzada" wait_for has_record kitty
    R=$(records kitty | head -n1)
    K=$(field "$R" pid)
    verdict=FALLA
    [[ $(field "$R" argv) == "0:[]" ]] && kill -0 "$K" && verdict=OK
    record A50-lanzamiento "programa falso kitty" "kitty $(field "$R" argv), pid $K" "$verdict" "Mod4+Return en el escritorio 4"
    for d in 1 2 4 8 4; do go "$d"; done
    key super+shift+1
    expect A50-tras-cambios "escritorio=4 foco=B activa=B A=o B=v" A B
    verdict=FALLA
    kill -0 "$K" && [[ ! -e $R.senales ]] && verdict=OK
    record A50-hijo-sobrevive "vivo, sin señales" "$(kill -0 "$K" && echo vivo || echo terminado); señales: $(paste -sd' ' "$R.senales" 2>/dev/null || echo ninguna)" "$verdict" "tras cinco cambios de escritorio y un movimiento"
    stop
    ;;
A51)
    # Tres instancias del WM en el mismo Xvfb: salida normal, SIGTERM y
    # SIGKILL. Las ventanas ocultas deben reaparecer; las retiradas, no.
    start5
    open_xev A
    open_xev W
    go 2
    open_xev B
    open_xev X
    go 3
    open_xev C
    # W (oculta) se retira con el UnmapNotify sintético; X (visible en su
    # momento), con un desmapeo real.
    "$TEST_CLIENT" withdraw "${WIDS[W]}"
    precondition "W retirada" wait_for retired W
    go 2
    xdotool windowunmap "${WIDS[X]}"
    precondition "X retirada" wait_for retired X
    go 3
    expect A51-antes-de-salir "escritorio=3 foco=C activa=C A=o B=o C=v W=o X=o" A B C W X
    xdotool key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    # Sin WM: el foco y la ventana activa no se comprueban.
    verdict=FALLA
    wait_for vis_is "A=v B=v C=v W=o X=o" A B C W X && verdict=OK
    record A51-salida-normal "A=v B=v C=v W=o X=o" "$(vis A B C W X)" "$verdict" "Map State tras Mod4+Shift+Escape"
    verdict=FALLA
    rootp=$(xprop -root _NET_CURRENT_DESKTOP _NET_NUMBER_OF_DESKTOPS _NET_ACTIVE_WINDOW | grep -c 'not found')
    [[ $rootp == 3 && $(props A B C) == "A=1/Normal B=2/Normal C=3/Normal" ]] && verdict=OK
    record A51-salida-propiedades "raíz sin propiedades; WM_STATE Normal" "$((3 - rootp)) en la raíz; $(props A B C)" "$verdict" "xprop tras la salida"

    # SIGTERM: el WM adopta A, B y C en el escritorio 1.
    adopted() { [[ $(grep -c "gestionada" "$WM_LOG") == 3 ]]; }
    restart A51-term
    precondition "A, B y C adoptadas" wait_for adopted
    precondition "foco en A" focus_on A
    key super+shift+2
    precondition "A oculta" wait_for hidden A
    open_xev Y
    xdotool windowunmap "${WIDS[Y]}"
    precondition "Y retirada" wait_for retired Y
    go 5
    term_before=$(vis A B C W X)
    kill -TERM "$WM_REAL"
    wait_wm 143 'salvaguardas superadas'
    verdict=FALLA
    wait_for vis_is "A=v B=v C=v W=o X=o Y=o" A B C W X Y && verdict=OK
    record A51-sigterm "A=v B=v C=v W=o X=o Y=o" "$(vis A B C W X Y)" "$verdict" "antes de la señal (escritorio 5): $term_before"
    # Desde F6 (ADR 0008), tras una terminación abrupta queda
    # _NET_SUPPORTING_WM_CHECK obsoleta y el siguiente arranque se rechaza
    # (fail-closed). El arnés la borra explícitamente en este Xvfb, como
    # haría el operador; el WM nunca lo hace.
    run_bin A51-tras-sigterm 12 '_NET_SUPPORTING_WM_CHECK .*inexistente: obsoleta'
    xprop -root -remove _NET_SUPPORTING_WM_CHECK

    # SIGKILL: sin ninguna oportunidad de limpiar; solo el save-set.
    restart A51-kill
    precondition "A, B y C adoptadas" wait_for adopted
    open_xev Z
    go 6
    "$TEST_CLIENT" withdraw "${WIDS[Z]}"
    precondition "Z retirada" wait_for retired Z
    kill_before=$(vis A B C Z)
    kill -KILL "$WM_REAL"
    wait_wm 137 'salvaguardas superadas'
    verdict=FALLA
    wait_for vis_is "A=v B=v C=v W=o X=o Y=o Z=o" A B C W X Y Z && verdict=OK
    record A51-sigkill "A=v B=v C=v W=o X=o Y=o Z=o" "$(vis A B C W X Y Z)" "$verdict" "antes de la señal (escritorio 6): $kill_before"
    alive=0
    for n in "${!PIDS[@]}"; do kill -0 "${PIDS[$n]}" && alive=$((alive + 1)); done
    record A51-aplicaciones-vivas "${#PIDS[@]}" "$alive" "$(ok_if test "$alive" = "${#PIDS[@]}")" "kill -0 a los xev tras las tres salidas"
    kill "${PIDS[@]}" 2>/dev/null
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
