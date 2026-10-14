#!/bin/bash
# Una prueba de aceptación de F6 (ADR 0008) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh). Como en F4 y F5, el único directorio del PATH del WM
# contiene copias de f4/fake.sh: ninguna aplicación real puede ejecutarse.
#
# TEST_CLIENT (examples/test-client.rs) genera mensajes EWMH, toma
# selecciones y crea ventanas que registran los mensajes WM_PROTOCOLS que
# reciben, con su marca de tiempo.
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
# Lo que el WM debe declarar, exactamente y en este orden (ewmh::SUPPORTED).
SUPPORTED="_NET_SUPPORTED, _NET_SUPPORTING_WM_CHECK, _NET_CLIENT_LIST, _NET_CLIENT_LIST_STACKING, _NET_NUMBER_OF_DESKTOPS, _NET_CURRENT_DESKTOP, _NET_ACTIVE_WINDOW, _NET_CLOSE_WINDOW, _NET_WM_DESKTOP, _NET_WM_WINDOW_TYPE, _NET_WM_WINDOW_TYPE_NORMAL, _NET_WM_WINDOW_TYPE_DIALOG, _NET_WM_WINDOW_TYPE_UTILITY, _NET_WM_WINDOW_TYPE_TOOLBAR"
ROOT_PROPS=(_NET_SUPPORTING_WM_CHECK _NET_SUPPORTED _NET_CLIENT_LIST _NET_CLIENT_LIST_STACKING _NET_ACTIVE_WINDOW _NET_CURRENT_DESKTOP _NET_NUMBER_OF_DESKTOPS)

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

# start6: arranca el WM con los programas falsos instalados y registra la
# evidencia de aislamiento, como start4 en F4 (solo nombres de variables).
start6() {
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
# señales de A54 y A64 solo se envían a ese proceso.
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
# Ventana a la que apunta _NET_SUPPORTING_WM_CHECK de una ventana (o de la
# raíz con «-root»): 0x… o «ausente». Tipo WINDOW (el WM) o CARDINAL en
# decimal (xprop -f 32x, como las propiedades ajenas de A54).
check_of() {
    local out
    out=$(xprop "$@" _NET_SUPPORTING_WM_CHECK 2>/dev/null)
    if [[ $out == *"window id # "* ]]; then
        echo "${out##*# }"
    elif [[ $out =~ \(CARDINAL\)\ =\ ([0-9]+)$ ]]; then
        printf '0x%x\n' "${BASH_REMATCH[1]}"
    else
        echo ausente
    fi
}
owner() { "$TEST_CLIENT" owner WM_S0; }
# root_props: las propiedades del WM presentes en la raíz.
root_props() {
    local p present=''
    for p in "${ROOT_PROPS[@]}"; do
        xprop -root "$p" | grep -qE 'not found|no such atom' || present+="$p "
    done
    present=${present% }
    echo "${present:-ninguna}"
}
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

precondition "pantalla 1280x720" test "$SIZE" = 1280x720
FULL="0 0 1280 720"

case $CASE in
A53)
    # Un cliente pasivo de la raíz registra el anuncio MANAGER.
    xev -root -event structure >"$OUT/$CASE-raiz.out" 2>&1 &
    ROOT_XEV=$!
    sleep 0.5
    start6
    precondition "identificación publicada" wait_for identified
    W=$(check_of -root)
    verdict=FALLA
    [[ $W =~ ^0x[0-9a-f]+$ && $(check_of -id "$W") == "$W" ]] && verdict=OK
    record A53-comprobacion "raíz y ventana apuntan a la ventana" "raíz -> $W; ventana -> $(check_of -id "$W")" "$verdict" "xprop _NET_SUPPORTING_WM_CHECK"
    info=$(xwininfo -id "$W")
    name=$(xprop -id "$W" _NET_WM_NAME)
    verdict=FALLA
    [[ $name == '_NET_WM_NAME(UTF8_STRING) = "Crimson WM"' ]] && grep -q 'Class: InputOnly' <<<"$info" &&
        grep -q 'Map State: IsUnMapped' <<<"$info" && grep -q 'Override Redirect State: yes' <<<"$info" && verdict=OK
    record A53-ventana "InputOnly, sin mapear, override-redirect, «Crimson WM»" "$name; $(grep -oE 'Class: [A-Za-z]+|Map State: [A-Za-z]+|Override Redirect State: [a-z]+' <<<"$info" | paste -sd',')" "$verdict" "xwininfo -id $W y xprop _NET_WM_NAME"
    got=$(xprop -root _NET_SUPPORTED | sed 's/^_NET_SUPPORTED(ATOM) = //')
    record A53-supported "$SUPPORTED" "$got" "$(ok_if test "$got" = "$SUPPORTED")" "xprop -root _NET_SUPPORTED: lista exacta, sin funciones aplazadas"
    got=$(owner)
    record A53-wm-s0 "$W" "$got" "$(ok_if test "$got" = "$W")" "test-client owner WM_S0"
    verdict=FALLA
    grep -q 'message_type 0x[0-9a-f]* (MANAGER), format 32' "$OUT/$CASE-raiz.out" && verdict=OK
    record A53-manager "ClientMessage MANAGER en la raíz" "$(grep -c '(MANAGER)' "$OUT/$CASE-raiz.out") mensajes" "$verdict" "xev -root -event structure"
    open_xev A
    precondition "lista de clientes" wait_for list_is _NET_CLIENT_LIST A
    got=$(root_props)
    want="${ROOT_PROPS[*]}"
    record A53-propiedades-raiz "$want" "$got" "$(ok_if test "$got" = "$want")" "xprop -root, con una ventana gestionada"
    stop
    got=$(root_props)
    record A53-limpieza-raiz ninguna "$got" "$(ok_if test "$got" = ninguna)" "xprop -root tras Mod4+Shift+Escape"
    verdict=FALLA
    ! xwininfo -id "$W" >/dev/null 2>&1 && [[ $(owner) == ninguno ]] && verdict=OK
    record A53-ventana-y-seleccion-liberadas "ventana destruida; WM_S0 sin propietario" "ventana: $(xwininfo -id "$W" >/dev/null 2>&1 && echo existe || echo destruida); WM_S0: $(owner)" "$verdict" "xwininfo y test-client owner"
    # Tras una salida limpia, el WM puede volver a arrancar en el mismo
    # servidor.
    restart A53-reinicio
    precondition "segunda identificación" wait_for identified
    got=$(check_of -root)
    verdict=FALLA
    # El servidor puede reutilizar el mismo identificador en la conexión nueva.
    [[ $got =~ ^0x && $(owner) == "$got" && $(check_of -id "$got") == "$got" ]] && verdict=OK
    record A53-reinicio-limpio "ventana de comprobación nueva y propietaria de WM_S0" "$got (antes $W); WM_S0: $(owner)" "$verdict" "segunda instancia tras la salida limpia"
    stop
    kill "$ROOT_XEV" 2>/dev/null
    ;;
A54)
    set_marker "$TOKEN"
    open_xev V
    # a) Otro gestor vivo: la raíz apunta a una ventana existente.
    xprop -root -f _NET_SUPPORTING_WM_CHECK 32x -set _NET_SUPPORTING_WM_CHECK "${WIDS[V]}"
    precondition "comprobación ajena presente" test "$(check_of -root)" = "${WIDS[V]}"
    run_bin A54-check-viva 12 "_NET_SUPPORTING_WM_CHECK \\(${WIDS[V]}, existente\\)"
    verdict=FALLA
    [[ $(check_of -root) == "${WIDS[V]}" && $(owner) == ninguno ]] && ! xprop -root _NET_SUPPORTED | grep -q '=' && verdict=OK
    record A54-check-viva-intacta "propiedad intacta; sin WM_S0 ni _NET_SUPPORTED" "raíz -> $(check_of -root); WM_S0: $(owner); $(xprop -root _NET_SUPPORTED)" "$verdict" "xprop y test-client owner tras el rechazo"
    # b) Obsoleta: apunta a una ventana que no existe.
    xprop -root -f _NET_SUPPORTING_WM_CHECK 32x -set _NET_SUPPORTING_WM_CHECK 0x7ffff1
    run_bin A54-check-obsoleta 12 '_NET_SUPPORTING_WM_CHECK \(0x7ffff1, inexistente: obsoleta\)'
    got=$(check_of -root)
    record A54-check-obsoleta-intacta 0x7ffff1 "$got" "$(ok_if test "$got" = 0x7ffff1)" "el WM no borra ni sustituye la propiedad"
    # c) WM_S0 con propietario (sin _NET_SUPPORTING_WM_CHECK). La propiedad
    # la borra el arnés, como haría el operador del servidor de prueba.
    xprop -root -remove _NET_SUPPORTING_WM_CHECK
    "$TEST_CLIENT" own WM_S0 20 >"$OUT/$CASE-own.out" 2>&1 &
    OWNER_PID=$!
    precondition "WM_S0 tomada por otro cliente" wait_for grep -q '^propietario' "$OUT/$CASE-own.out"
    taken=$(sed -n 's/^propietario //p' "$OUT/$CASE-own.out")
    run_bin A54-wm-s0-ocupada 12 'la selección WM_S0 tiene propietario'
    verdict=FALLA
    [[ $(owner) == "$taken" && $(check_of -root) == ausente ]] && verdict=OK
    record A54-wm-s0-intacta "propietario $taken; sin comprobación en la raíz" "WM_S0: $(owner); raíz -> $(check_of -root)" "$verdict" "test-client owner y xprop tras el rechazo"
    kill "$OWNER_PID" 2>/dev/null
    wait "$OWNER_PID" 2>/dev/null
    precondition "WM_S0 libre" wait_for test "$(owner)" = ninguno
    # d) La capa 4 sigue rechazando a quien tenga SubstructureRedirect sin
    # anunciarse.
    "$TEST_CLIENT" redirect 20 >"$OUT/$CASE-redirect.out" 2>&1 &
    REDIRECT_PID=$!
    precondition "SubstructureRedirect tomado" wait_for grep -q 'tomado' "$OUT/$CASE-redirect.out"
    run_bin A54-capa-4 12 'BadAccess'
    kill "$REDIRECT_PID" 2>/dev/null
    wait "$REDIRECT_PID" 2>/dev/null
    # e) SIGKILL: las ocultas vuelven (save-set), la selección se libera con
    # la ventana, pero la propiedad de la raíz queda obsoleta y el siguiente
    # arranque se rechaza (fail-closed).
    start_wm A54
    require_ready
    real_pid
    precondition "identificación publicada" wait_for identified
    W=$(check_of -root)
    open_xev A
    go 2
    open_xev B
    go 3
    kill -KILL "$WM_REAL"
    wait_wm 137 'identificación publicada'
    verdict=FALLA
    wait_for vis_is "A=v B=v V=v" A B V && [[ $(check_of -root) == "$W" && $(owner) == ninguno ]] && verdict=OK
    record A54-sigkill "ocultas visibles; propiedad obsoleta; WM_S0 libre" "$(vis A B V); raíz -> $(check_of -root) ($(xwininfo -id "$W" >/dev/null 2>&1 && echo existe || echo destruida)); WM_S0: $(owner)" "$verdict" "tras SIGKILL al proceso del WM ($WM_REAL)"
    run_bin A54-tras-sigkill 12 "_NET_SUPPORTING_WM_CHECK \\($W, inexistente: obsoleta\\)"
    got=$(check_of -root)
    record A54-tras-sigkill-intacta "$W" "$got" "$(ok_if test "$got" = "$W")" "el WM no elimina la propiedad obsoleta"
    # Recuperación manual documentada: borrar la propiedad en el servidor de
    # prueba.
    xprop -root -remove _NET_SUPPORTING_WM_CHECK
    restart A54-recuperado
    precondition "identificación publicada" wait_for identified
    record A54-recuperado-identificado "arranca tras borrar la propiedad" "$(check_of -root)" "$([[ $(check_of -root) =~ ^0x && $(owner) == "$(check_of -root)" ]] && echo OK || echo FALLA)" "xprop -root -remove por el arnés y nuevo arranque"
    # f) Otro cliente toma WM_S0: el WM termina sin recuperarla.
    go 2
    "$TEST_CLIENT" own WM_S0 20 >"$OUT/$CASE-relevo.out" 2>&1 &
    OWNER_PID=$!
    wait_wm 0 'otro cliente ha tomado la selección del gestor'
    taken=$(sed -n 's/^propietario //p' "$OUT/$CASE-relevo.out")
    verdict=FALLA
    sleep 0.5
    [[ $(owner) == "$taken" && $(root_props) == ninguna ]] && vis_is "A=v B=v V=v" A B V && verdict=OK
    record A54-relevo "sin recuperar WM_S0; propiedades propias borradas; ventanas visibles" "WM_S0: $(owner) (test-client $taken); raíz: $(root_props); $(vis A B V)" "$verdict" "SelectionClear: el WM registra, limpia lo suyo y termina"
    kill "$OWNER_PID" "${PIDS[@]}" 2>/dev/null
    ;;
A55)
    start6
    open_xev A
    open_tc D
    precondition "foco en D" wait_for focus_is D
    key super+w
    verdict=FALLA
    wait_for gone D && has_message D WM_DELETE_WINDOW && (($(last_time D WM_DELETE_WINDOW) > 0)) && verdict=OK
    record A55-cooperativo "WM_DELETE_WINDOW con marca de tiempo; D cerrada" "$(received D WM_DELETE_WINDOW | head -n1); D $(map_state D)" "$verdict" "registro de test-client y xwininfo"
    expect A55-tras-cierre "escritorio=1 foco=A activa=A A=v" A
    expect_layout A55-mosaico "A=$FULL"
    # Un cliente habitual (xev) también se cierra.
    key super+w
    verdict=FALLA
    wait_for gone A && verdict=OK
    record A55-xev "xev se cierra" "A $(map_state A); proceso $(kill -0 "${PIDS[A]}" 2>/dev/null && echo vivo || echo terminado)" "$verdict" "Mod4+w sobre xev (anuncia WM_DELETE_WINDOW)"
    # Tecla mantenida: una sola ventana cerrada.
    open_tc E1
    open_tc E2
    open_tc E3
    precondition "foco en E3" wait_for focus_is E3
    xdotool keydown super+w
    sleep 1.5
    xdotool keyup super+w
    sleep 0.5
    got=$(vis E1 E2 E3)
    record A55-mantenido "E1=v E2=v E3=destruida" "$got" "$(ok_if test "$got" = "E1=v E2=v E3=destruida")" "Mod4+w mantenido 1,5 s: la repetición automática no cierra más ventanas"
    stop
    ;;
A56)
    start6
    open_xev A
    open_tc N --no-delete
    precondition "foco en N" wait_for focus_is N
    key super+w
    precondition "Mod4+w registrado" wait_for logged "Mod4+w: ${WIDS[N]} no anuncia WM_DELETE_WINDOW; no se cierra"
    verdict=FALLA
    sleep 0.5
    [[ $(map_state N) == v ]] && kill -0 "${PIDS[N]}" && ! has_message N WM_DELETE_WINDOW && verdict=OK
    record A56-sin-protocolo "N sigue abierta y sin mensaje" "N $(map_state N); proceso $(kill -0 "${PIDS[N]}" && echo vivo || echo terminado); mensajes: $(grep -c . "$OUT/$CASE-N.out")" "$verdict" "$(grep -o 'Mod4+w: 0x[0-9a-f]* no anuncia WM_DELETE_WINDOW; no se cierra' "$WM_LOG")"
    key super+shift+w
    verdict=FALLA
    wait_for gone N && wait_for dead "${PIDS[N]}" && logged "cierre forzado de ${WIDS[N]}" && verdict=OK
    record A56-forzado "N cerrada a la fuerza" "N $(map_state N); proceso $(kill -0 "${PIDS[N]}" 2>/dev/null && echo vivo || echo terminado)" "$verdict" "Mod4+Shift+w: KillClient"
    # KillClient cierra todas las ventanas de la conexión.
    open_tc P --no-delete --pair
    WIDS[P-2]=$(xwininfo -name P-2 | awk '/Window id:/ {print $4}')
    precondition "foco en P-2" wait_for focus_is P-2
    key super+shift+w
    verdict=FALLA
    wait_for gone P && wait_for gone P-2 && verdict=OK
    record A56-misma-conexion "P y P-2 cerradas" "$(vis P P-2)" "$verdict" "Mod4+Shift+w sobre P-2: KillClient cierra la conexión, también P"
    # Al retirar P, el WM reorganiza P-2, que el servidor ya ha destruido: un
    # BadWindow benigno (ADR 0003). Solo se admiten errores sobre P y P-2.
    XERR_ALLOWED="Window \(petición [0-9.]+, valor (${WIDS[P]}|${WIDS[P-2]})\)"
    expect A56-queda-A "escritorio=1 foco=A activa=A A=v" A
    stop
    ;;
A57)
    start6
    open_tc C3 --no-delete
    open_tc C1
    go 2
    open_tc C2
    go 1
    precondition "foco en C1" wait_for focus_is C1
    # Escritorio activo, fuente 2, sin marca de tiempo: el WM pone la suya.
    "$TEST_CLIENT" message 32 "${WIDS[C1]}" _NET_CLOSE_WINDOW 0 2 0 0 0
    verdict=FALLA
    wait_for gone C1 && (($(last_time C1 WM_DELETE_WINDOW) > 0)) && verdict=OK
    record A57-activo "C1 cerrada; marca de tiempo del servidor" "C1 $(map_state C1); $(received C1 WM_DELETE_WINDOW | head -n1)" "$verdict" "_NET_CLOSE_WINDOW, fuente 2, data[0] = 0"
    # Escritorio oculto, fuente 1, con marca de tiempo: se conserva.
    "$TEST_CLIENT" message 32 "${WIDS[C2]}" _NET_CLOSE_WINDOW 12345 1 0 0 0
    verdict=FALLA
    wait_for gone C2 && [[ $(last_time C2 WM_DELETE_WINDOW) == 12345 ]] && desk_is 1 && verdict=OK
    record A57-oculta "C2 cerrada con la marca 12345; escritorio 1" "C2 $(map_state C2); $(received C2 WM_DELETE_WINDOW | head -n1); escritorio $(current_desk)" "$verdict" "_NET_CLOSE_WINDOW, fuente 1, ventana del escritorio 2"
    # Sin WM_DELETE_WINDOW: nunca se termina por una petición externa.
    "$TEST_CLIENT" message 32 "${WIDS[C3]}" _NET_CLOSE_WINDOW 0 2 0 0 0
    precondition "petición registrada" wait_for logged "_NET_CLOSE_WINDOW (Pager): ${WIDS[C3]} no anuncia WM_DELETE_WINDOW"
    sleep 0.5
    verdict=FALLA
    [[ $(map_state C3) == v ]] && kill -0 "${PIDS[C3]}" && verdict=OK
    record A57-sin-protocolo "C3 abierta y viva" "C3 $(map_state C3); proceso $(kill -0 "${PIDS[C3]}" && echo vivo || echo terminado)" "$verdict" "_NET_CLOSE_WINDOW sobre un cliente sin WM_DELETE_WINDOW: ningún KillClient"
    # Mensajes mal formados.
    "$TEST_CLIENT" message 8 "${WIDS[C3]}" _NET_CLOSE_WINDOW 0 2 0 0 0
    "$TEST_CLIENT" message 32 "${WIDS[C3]}" _NET_CLOSE_WINDOW 0 9 0 0 0
    verdict=FALLA
    wait_for logged 'Format(8)' && wait_for logged 'Source(9)' && [[ $(map_state C3) == v ]] && verdict=OK
    record A57-invalidas "formato 8 y fuente 9 rechazados" "$(grep -oE 'petición EWMH rechazada para 0x[0-9a-f]+: [A-Za-z]+\([0-9]+\)' "$WM_LOG" | paste -sd';')" "$verdict" "registro del WM; C3 sigue abierta"
    stop
    ;;
A58)
    start6
    expect_list A58-vacia _NET_CLIENT_LIST vacía
    open_xev A
    open_xev B
    go 2
    open_xev C
    expect_list A58-tres _NET_CLIENT_LIST "A B C"
    kill "${PIDS[B]}"
    unset 'PIDS[B]'
    expect_list A58-cierre-oculta _NET_CLIENT_LIST "A C"
    xdotool set_desktop_for_window "${WIDS[A]}" 3
    precondition "A en el escritorio 4" wait_for test "$(net_desk A)" = 4
    expect_list A58-movida _NET_CLIENT_LIST "A C"
    xdotool windowunmap "${WIDS[C]}"
    expect_list A58-retirada _NET_CLIENT_LIST A
    open_xev D
    expect_list A58-nueva _NET_CLIENT_LIST "A D"
    stop
    ;;
A59)
    start6
    open_xev A
    open_xev B
    open_xev D
    retire D
    xprop -id "${WIDS[D]}" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE _NET_WM_WINDOW_TYPE_DIALOG
    xdotool windowmap "${WIDS[D]}"
    precondition "D flotante" wait_for logged "ventana ${WIDS[D]} gestionada (flotante)"
    expect_stacking A59-flotante-arriba "A B D"
    # Una ventana en mosaico no puede subir sobre la flotante.
    xdotool windowraise "${WIDS[A]}"
    sleep 0.3
    expect_stacking A59-subida-denegada "A B D"
    go 2
    open_xev C
    expect_stacking A59-con-ocultas "A B D C"
    key super+shift+1
    go 1
    expect_stacking A59-tras-mover "A B C D"
    stop
    ;;
A60)
    start6
    open_xev A
    open_xev B
    go 2
    open_xev C
    go 1
    precondition "foco en B" wait_for focus_is B
    "$TEST_CLIENT" message 32 "${WIDS[A]}" _NET_ACTIVE_WINDOW 1 0 0 0 0
    expect A60-aplicacion-mismo-escritorio "escritorio=1 foco=A activa=A A=v B=v C=o" A B C
    "$TEST_CLIENT" message 32 "${WIDS[C]}" _NET_ACTIVE_WINDOW 1 0 0 0 0
    precondition "petición registrada" wait_for logged "_NET_ACTIVE_WINDOW (Application) ignorada: ${WIDS[C]} está en el escritorio 2"
    expect_stays A60-aplicacion-otro-escritorio "escritorio=1 foco=A activa=A A=v B=v C=o" A B C
    "$TEST_CLIENT" message 32 "${WIDS[C]}" _NET_ACTIVE_WINDOW 0 0 0 0 0
    precondition "petición registrada" wait_for logged "_NET_ACTIVE_WINDOW (Legacy) ignorada: ${WIDS[C]}"
    expect_stays A60-sin-indicacion "escritorio=1 foco=A activa=A A=v B=v C=o" A B C
    "$TEST_CLIENT" message 32 "${WIDS[C]}" _NET_ACTIVE_WINDOW 2 0 0 0 0
    expect A60-paginador "escritorio=2 foco=C activa=C A=o B=o C=v" A B C
    # xdotool windowactivate (fuente 2, tras cambiar de escritorio si hace
    # falta): solo funciona porque _NET_SUPPORTED lo declara.
    xdotool windowactivate "${WIDS[B]}"
    expect A60-xdotool "escritorio=1 foco=B activa=B A=v B=v C=o" A B C
    "$TEST_CLIENT" message 32 "$ROOT_ID" _NET_ACTIVE_WINDOW 2 0 0 0 0
    "$TEST_CLIENT" message 32 "${WIDS[A]}" _NET_ACTIVE_WINDOW 5 0 0 0 0
    verdict=FALLA
    wait_for logged "_NET_ACTIVE_WINDOW ignorada: $ROOT_ID no está gestionada" && wait_for logged 'Source(5)' && verdict=OK
    record A60-invalidas "raíz no gestionada; fuente 5 rechazada" "$(grep -cE '_NET_ACTIVE_WINDOW ignorada|Source\(5\)' "$WM_LOG") registros" "$verdict" "registro del WM"
    expect_stays A60-sin-cambios "escritorio=1 foco=B activa=B A=v B=v C=o" A B C
    stop
    ;;
A61)
    start6
    open_xev A
    got=$(xdotool get_desktop)
    record A61-get-desktop 0 "$got" "$(ok_if test "$got" = 0)" "xdotool get_desktop (exige _NET_SUPPORTED)"
    xdotool set_desktop 2
    expect A61-set-desktop "escritorio=3 foco=PointerRoot activa=ninguna A=o" A
    xdotool set_desktop_for_window "${WIDS[A]}" 4
    verdict=FALLA
    wait_for test "$(net_desk A)" = 5 && desk_is 3 && hidden A && verdict=OK
    record A61-oculta-a-oculto "A al escritorio 5, oculta; activo 3" "A en $(net_desk A), $(map_state A); activo $(current_desk)" "$verdict" "xdotool set_desktop_for_window entre escritorios ocultos"
    xdotool set_desktop_for_window "${WIDS[A]}" 2
    expect A61-al-activo "escritorio=3 foco=PointerRoot activa=ninguna A=v" A
    expect_layout A61-al-activo-mosaico "A=$FULL"
    # Valores no válidos: sin cambios.
    "$TEST_CLIENT" message 32 root _NET_CURRENT_DESKTOP 8 0 0 0 0
    "$TEST_CLIENT" message 32 root _NET_CURRENT_DESKTOP 0xffffffff 0 0 0 0
    "$TEST_CLIENT" message 32 "${WIDS[A]}" _NET_CURRENT_DESKTOP 1 0 0 0 0
    "$TEST_CLIENT" message 32 "${WIDS[A]}" _NET_WM_DESKTOP 0xffffffff 2 0 0 0
    "$TEST_CLIENT" message 32 "${WIDS[A]}" _NET_WM_DESKTOP 9 2 0 0 0
    verdict=FALLA
    wait_for logged 'Desk(8)' && wait_for logged 'Desk(9)' && wait_for logged 'NotRoot' &&
        [[ $(grep -c 'Sticky' "$WM_LOG") == 2 ]] && verdict=OK
    record A61-invalidas "Desk(8), Sticky ×2, NotRoot y Desk(9) rechazados" "$(grep -oE 'rechazada para 0x[0-9a-f]+: [A-Za-z]+(\([0-9a-fx]+\))?' "$WM_LOG" | sed 's/rechazada para //' | paste -sd';')" "$verdict" "registro del WM"
    expect_stays A61-sin-cambios "escritorio=3 foco=PointerRoot activa=ninguna A=v" A
    got=$(net_desk A)
    record A61-escritorio-de-A 3 "$got" "$(ok_if test "$got" = 3)" "_NET_WM_DESKTOP de A (de 1 a 8) tras las peticiones rechazadas"
    stop
    ;;
A62)
    start6
    # Ventana nueva con WM_TAKE_FOCUS: no hay pulsación, así que el mensaje
    # lleva la hora del servidor, no CurrentTime (0).
    open_tc T --take-focus
    verdict=FALLA
    wait_for has_message T WM_TAKE_FOCUS && (($(last_time T WM_TAKE_FOCUS) > 0)) && verdict=OK
    record A62-ventana-nueva "WM_TAKE_FOCUS con marca > 0" "$(received T WM_TAKE_FOCUS | head -n1)" "$verdict" "registro de test-client"
    first=$(last_time T WM_TAKE_FOCUS)
    open_xev A
    key super+j
    verdict=FALLA
    wait_for messages_at_least T WM_TAKE_FOCUS 2 && (($(last_time T WM_TAKE_FOCUS) > first)) && verdict=OK
    record A62-pulsacion "segundo WM_TAKE_FOCUS, posterior" "$(received T WM_TAKE_FOCUS | awk '{print $3}' | paste -sd' ')" "$verdict" "Mod4+j: marca de la pulsación"
    # PropertyNotify de WM_PROTOCOLS: sin WM_DELETE_WINDOW, Mod4+w no cierra.
    key super+j
    precondition "foco en A" wait_for focus_is A
    xprop -id "${WIDS[A]}" -remove WM_PROTOCOLS
    precondition "cambio registrado" wait_for logged "ventana ${WIDS[A]}: modelo de entrada Passive, WM_DELETE_WINDOW no (PropertyNotify)"
    key super+w
    precondition "Mod4+w registrado" wait_for logged "Mod4+w: ${WIDS[A]} no anuncia WM_DELETE_WINDOW"
    sleep 0.5
    record A62-protocolos-retirados "A sigue abierta" "A $(map_state A)" "$(ok_if test "$(map_state A)" = v)" "xprop -remove WM_PROTOCOLS y Mod4+w"
    # Solo WM_TAKE_FOCUS: modelo LocallyActive. (xprop -f 32a guarda un único
    # átomo: una lista separada por comas sería un átomo con ese nombre.)
    xprop -id "${WIDS[A]}" -f WM_PROTOCOLS 32a -set WM_PROTOCOLS WM_TAKE_FOCUS
    precondition "cambio registrado" wait_for logged "ventana ${WIDS[A]}: modelo de entrada LocallyActive, WM_DELETE_WINDOW no (PropertyNotify)"
    key super+j
    precondition "foco en T" wait_for focus_is T
    key super+j
    verdict=FALLA
    wait_for logged "foco en ${WIDS[A]} (LocallyActive)" && wait_for grep -q '(WM_TAKE_FOCUS)' "$OUT/$CASE-A.out" && verdict=OK
    record A62-protocolos-anadidos "foco LocallyActive; xev recibe WM_TAKE_FOCUS" "$(grep -c '(WM_TAKE_FOCUS)' "$OUT/$CASE-A.out") mensajes en A" "$verdict" "xprop -set WM_PROTOCOLS WM_TAKE_FOCUS y Mod4+j"
    # Solo WM_DELETE_WINDOW: Mod4+w vuelve a cerrar.
    xprop -id "${WIDS[A]}" -f WM_PROTOCOLS 32a -set WM_PROTOCOLS WM_DELETE_WINDOW
    precondition "cambio registrado" wait_for logged "ventana ${WIDS[A]}: modelo de entrada Passive, WM_DELETE_WINDOW sí (PropertyNotify)"
    key super+w
    verdict=FALLA
    wait_for gone A && verdict=OK
    record A62-cierre-tras-cambio "A cerrada" "A $(map_state A)" "$verdict" "Mod4+w tras volver a anunciar WM_DELETE_WINDOW"
    # Las propiedades que escribe el propio WM no provocan bucles.
    n=$(grep -c 'PropertyNotify' "$WM_LOG")
    record A62-sin-bucles 3 "$n" "$(ok_if test "$n" = 3)" "líneas «(PropertyNotify)» en el registro: solo los tres cambios de WM_PROTOCOLS"
    stop
    ;;
A64)
    start6
    precondition "identificación publicada" wait_for identified
    W=$(check_of -root)
    open_xev A
    go 2
    open_xev B
    go 3
    kill -TERM "$WM_REAL"
    wait_wm 143 'identificación publicada'
    verdict=FALLA
    wait_for vis_is "A=v B=v" A B && [[ $(check_of -root) == "$W" && $(owner) == ninguno ]] && verdict=OK
    record A64-sigterm "ocultas visibles; propiedad obsoleta; WM_S0 libre" "$(vis A B); raíz -> $(check_of -root); WM_S0: $(owner)" "$verdict" "tras SIGTERM al proceso del WM ($WM_REAL); sin manejadores de señales"
    run_bin A64-tras-sigterm 12 "_NET_SUPPORTING_WM_CHECK \\($W, inexistente: obsoleta\\)"
    got=$(check_of -root)
    record A64-tras-sigterm-intacta "$W" "$got" "$(ok_if test "$got" = "$W")" "fail-closed: la propiedad no se elimina automáticamente"
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
    xerrors=$(cat "$OUT/$CASE"*.log | grep 'error X ignorado' | grep -cvE "${XERR_ALLOWED:-^$}")
    allowed=$(cat "$OUT/$CASE"*.log | grep 'error X ignorado' | grep -cE "${XERR_ALLOWED:-^$}")
    record "$CASE-extra-sin-errores-x" 0 "$xerrors" "$([[ $xerrors == 0 ]] && echo OK || echo FALLA)" "líneas «error X ignorado» en los registros del WM (admitidas por el caso: $allowed)"
fi
