#!/bin/bash
# Una prueba de aceptación de F3 (ADR 0005) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas y la preparación son las de F1 (xvfb-env.sh) y las
# funciones de ventanas, las de F2 (windows.sh).
# Uso: xvfb-case.sh BINARIO DIRECTORIO_DE_RESULTADOS CASO
set -u
BIN=$1
OUT=$2
CASE=$3
F1=$(cd "$(dirname "$0")/../f1" && pwd)
F2=$(cd "$(dirname "$0")/../f2" && pwd)
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
# shellcheck source=../f1/xvfb-env.sh
source "$F1/xvfb-env.sh"
# shellcheck source=../f2/windows.sh
source "$F2/windows.sh"

ROOT_ID=$(xwininfo -root | awk '/Window id:/ {print $4}')

# Las ventanas de prueba son de xev: sin WM_HINTS (modelo pasivo) y con su
# propio registro de eventos en $OUT/$CASE-nombre.out.
open_xev() { open_window "$1" xev -name "$1"; }

# name_of id: nombre de la ventana (de WIDS), «raíz» o el propio id.
name_of() {
    local n
    [[ $1 == "$ROOT_ID" ]] && { echo raíz; return; }
    [[ $1 == 0x0 ]] && { echo ninguna; return; }
    for n in "${!WIDS[@]}"; do
        [[ ${WIDS[$n]} == "$1" ]] && { echo "$n"; return; }
    done
    echo "$1"
}
# Ventana con el foco real del servidor (GetInputFocus).
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
# Valor de _NET_ACTIVE_WINDOW en la raíz.
active_name() {
    local out
    out=$(xprop -root _NET_ACTIVE_WINDOW)
    case $out in
    *"not found"*) echo ausente ;;
    *"window id # "*) name_of "${out##*# }" ;;
    *) echo "?$out" ;;
    esac
}
focus_state() { echo "foco=$(focus_name) activa=$(active_name)"; }
focus_is() { [[ $(focus_state) == "$1" ]]; }
# expect_focus id «foco=X activa=Y»: espera hasta 5 s a ese estado, con el WM
# en marcha.
expect_focus() {
    local verdict=FALLA
    wait_for focus_is "$2" && kill -0 "$WM_PID" && verdict=OK
    record "$1" "$2" "$(focus_state)" "$verdict" "xdotool getwindowfocus, xprop -root _NET_ACTIVE_WINDOW"
}
# expect_focus_stays id «foco=X activa=Y»: comprobación negativa, tras una
# espera fija de 0,5 s.
expect_focus_stays() {
    local verdict=FALLA
    sleep 0.5
    focus_is "$2" && kill -0 "$WM_PID" && verdict=OK
    record "$1" "$2" "$(focus_state)" "$verdict" "sin cambios tras 0,5 s"
}
key() { xdotool key "$@"; }
three_windows() {
    open_xev A
    open_xev B
    open_xev C
    precondition "mosaico y foco en C" wait_for focus_is "foco=C activa=C"
}
# set_dialog nombre: retira la ventana y le pone _NET_WM_WINDOW_TYPE_DIALOG.
set_dialog() {
    retire "$1"
    xprop -id "${WIDS[$1]}" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE _NET_WM_WINDOW_TYPE_DIALOG
}

precondition "pantalla 1280x720" test "$SIZE" = 1280x720

case $CASE in
A22)
    start
    open_xev A
    expect_focus A22-primera "foco=A activa=A"
    open_xev B
    expect_focus A22-segunda "foco=B activa=B"
    grep -q 'FocusIn event' "$OUT/$CASE-A.out" && fin=OK || fin=FALLA
    record A22-focusin-cliente "FocusIn en A" "$(grep -c 'FocusIn event' "$OUT/$CASE-A.out") FocusIn" "$fin" "registro de xev de A"
    stop
    state=$(focus_state)
    verdict=FALLA
    [[ $state == "foco=PointerRoot activa=ausente" ]] && verdict=OK
    record A22-salida-limpia "foco=PointerRoot activa=ausente" "$state" "$verdict" "tras Mod4+Shift+Escape"
    ;;
A23)
    start
    three_windows
    key super+j
    expect_focus A23-siguiente-vuelta "foco=A activa=A"
    key super+j
    expect_focus A23-siguiente "foco=B activa=B"
    key super+k
    expect_focus A23-anterior "foco=A activa=A"
    key super+k
    expect_focus A23-anterior-vuelta "foco=C activa=C"
    stop
    ;;
A24)
    start
    three_windows
    # Desde F4 (ADR 0006), Mod4+Return lanza la terminal y la pulsación ya
    # no llega a la ventana enfocada. En F3 el PATH del WM está vacío: el
    # lanzamiento falla, se registra y ni el WM ni el mosaico cambian.
    key super+Return
    launch_failed() { grep -q 'no se pudo lanzar «terminal» (kitty)' "$WM_LOG"; }
    verdict=FALLA
    wait_for launch_failed && kill -0 "$WM_PID" &&
        ! grep -q 'keycode 36 (keysym 0xff0d, Return)' "$OUT/$CASE-C.out" &&
        layout_is "A=0 0 640 720" "B=640 0 640 360" "C=640 360 640 360" && verdict=OK
    record A24-super-return-capturado "C no recibe Mod4+Return; lanzamiento fallido registrado; mosaico A, B, C" "$(grep -c 'keycode 36 (keysym 0xff0d, Return)' "$OUT/$CASE-C.out") eventos de Return en C; $(grep -o 'no se pudo lanzar «terminal» (kitty): [^(]*' "$WM_LOG"); A=$(geometry A)" "$verdict" "registros de xev de C y del WM"
    key super+shift+Return
    expect_layout A24-maestra "C=0 0 640 720" "A=640 0 640 360" "B=640 360 640 360"
    expect_focus A24-foco "foco=C activa=C"
    # Mod4+Shift+Return sí se captura: no llega a la ventana.
    grabbed=$(grep -c 'state 0x41, keycode 36 (keysym 0xff0d, Return)' "$OUT/$CASE-C.out")
    record A24-zoom-capturado 0 "$grabbed" "$([[ $grabbed == 0 ]] && echo OK || echo FALLA)" "pulsaciones de Mod4+Shift+Return en el registro de xev de C"
    key super+shift+Return
    expect_layout A24-intercambio "A=0 0 640 720" "C=640 0 640 360" "B=640 360 640 360"
    expect_focus A24-foco-intercambio "foco=C activa=C"
    stop
    ;;
A25)
    start
    three_windows
    key super+k
    precondition "foco en B" wait_for focus_is "foco=B activa=B"
    kill "${PIDS[B]}"
    unset 'PIDS[B]'
    # Historial: B, C, A. Al cerrarse B vuelve C.
    expect_focus A25-historial "foco=C activa=C"
    expect_layout A25-mosaico "A=0 0 640 720" "C=640 0 640 720"
    stop
    ;;
A26)
    start
    open_xev A
    open_xev B
    precondition "foco en B" wait_for focus_is "foco=B activa=B"
    xdotool windowunmap "${WIDS[B]}"
    expect_focus A26-desmapeo "foco=A activa=A"
    kill "${PIDS[A]}" "${PIDS[B]}"
    PIDS=()
    expect_focus A26-sin-ventanas "foco=PointerRoot activa=ninguna"
    stop
    ;;
A27)
    start
    open_xev A
    open_xev D
    set_dialog D
    precondition "foco de vuelta en A" wait_for focus_is "foco=A activa=A"
    xdotool windowmap "${WIDS[D]}"
    precondition "D gestionada como flotante" wait_for grep -q "ventana ${WIDS[D]} gestionada (flotante)" "$WM_LOG"
    expect_focus A27-foco-dialogo "foco=D activa=D"
    expect_layout A27-mosaico "A=0 0 1280 720"
    # Orden de navegación: A (mosaico) y D (flotante).
    key super+j
    expect_focus A27-navegacion-mosaico "foco=A activa=A"
    key super+j
    expect_focus A27-navegacion-flotante "foco=D activa=D"
    kill "${PIDS[D]}"
    unset 'PIDS[D]'
    expect_focus A27-cierre-dialogo "foco=A activa=A"
    stop
    ;;
A28)
    start
    open_xev A
    # xclock declara input=False sin WM_TAKE_FOCUS: modelo sin entrada.
    open_window K xclock -title K
    precondition "K sin entrada" grep -q "foco en ${WIDS[K]} (NoInput)" "$WM_LOG"
    expect_focus A28-sin-entrada "foco=raíz activa=K"
    key super+k
    expect_focus A28-navegacion "foco=A activa=A"
    stop
    ;;
A29)
    start
    open_xev A
    open_xev T
    retire T
    xprop -id "${WIDS[T]}" -f WM_PROTOCOLS 32a -set WM_PROTOCOLS WM_TAKE_FOCUS
    precondition "T anuncia WM_TAKE_FOCUS" eval "xprop -id ${WIDS[T]} WM_PROTOCOLS | grep -qx 'WM_PROTOCOLS(ATOM): protocols  WM_TAKE_FOCUS'"
    precondition "foco de vuelta en A" wait_for focus_is "foco=A activa=A"
    xdotool windowmap "${WIDS[T]}"
    expect_focus A29-foco "foco=T activa=T"
    take_focus() { grep -q 'message_type 0x[0-9a-f]* (WM_PROTOCOLS), format 32, message 0x[0-9a-f]* (WM_TAKE_FOCUS)' "$OUT/$CASE-T.out"; }
    verdict=FALLA
    wait_for take_focus && grep -q "foco en ${WIDS[T]} (LocallyActive)" "$WM_LOG" && verdict=OK
    record A29-wm-take-focus "ClientMessage WM_TAKE_FOCUS, formato 32" "$(grep -c 'WM_TAKE_FOCUS' "$OUT/$CASE-T.out") mensajes" "$verdict" "registro de xev de T; WM: $(grep -o "foco en ${WIDS[T]} ([A-Za-z]*)" "$WM_LOG" | tail -n1)"
    # A, que no lo anuncia, no recibe el mensaje.
    nothing=$(grep -c 'WM_TAKE_FOCUS' "$OUT/$CASE-A.out")
    record A29-solo-si-lo-anuncia 0 "$nothing" "$([[ $nothing == 0 ]] && echo OK || echo FALLA)" "mensajes WM_TAKE_FOCUS en el registro de xev de A"
    stop
    ;;
A30)
    start
    three_windows
    # Un cliente se da el foco: el WM lo sigue.
    xdotool windowfocus "${WIDS[A]}"
    expect_focus A30-cliente "foco=A activa=A"
    key super+j
    expect_focus A30-navegacion "foco=B activa=B"
    stop
    ;;
A31)
    start
    open_xev A
    open_xev B
    precondition "foco en B" wait_for focus_is "foco=B activa=B"
    key Caps_Lock
    key super+j
    expect_focus A31-bloq-mayus "foco=A activa=A"
    key Caps_Lock
    key Num_Lock
    key super+k
    expect_focus A31-bloq-num "foco=B activa=B"
    key Num_Lock
    key super+shift+j
    expect_focus_stays A31-sin-shift "foco=B activa=B"
    key Caps_Lock
    key super+shift+Return
    expect_layout A31-zoom-bloq-mayus "B=0 0 640 720" "A=640 0 640 720"
    key Caps_Lock
    stop
    ;;
*)
    echo "caso desconocido: $CASE"
    exit 2
    ;;
esac

# Ningún error X debe haberse registrado durante la ejecución del WM.
if [[ -n ${WM_LOG:-} ]]; then
    xerrors=$(grep -c 'error X ignorado' "$WM_LOG")
    record "$CASE-extra-sin-errores-x" 0 "$xerrors" "$([[ $xerrors == 0 ]] && echo OK || echo FALLA)" "líneas «error X ignorado» en $WM_ID.log"
fi
