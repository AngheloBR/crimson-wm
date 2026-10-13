#!/bin/bash
# Una prueba de aceptación de F2 (ADR 0004) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas y la preparación son las de F1 (xvfb-env.sh).
# Uso: xvfb-case.sh BINARIO DIRECTORIO_DE_RESULTADOS CASO
set -u
BIN=$1
OUT=$2
CASE=$3
F1=$(cd "$(dirname "$0")/../f1" && pwd)
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
# shellcheck source=../f1/xvfb-env.sh
source "$F1/xvfb-env.sh"

# shellcheck source=windows.sh
source "$(dirname "$0")/windows.sh"

if [[ $CASE == A20 ]]; then
    precondition "pantalla 1279x719" test "$SIZE" = 1279x719
else
    precondition "pantalla 1280x720" test "$SIZE" = 1280x720
fi

case $CASE in
A10)
    start
    open_window a
    expect_layout A10-una "a=0 0 1280 720"
    border=$(xwininfo -id "${WIDS[a]}" | awk '/Border width:/ {print $NF}')
    record A10-borde 0 "$border" "$([[ $border == 0 ]] && echo OK || echo FALLA)" "xwininfo: Border width"
    stop
    ;;
A11)
    start
    open_window a
    open_window b
    expect_layout A11-dos "a=0 0 640 720" "b=640 0 640 720"
    stop
    ;;
A12)
    start
    open_window a
    open_window b
    open_window c
    expect_layout A12-tres "a=0 0 640 720" "b=640 0 640 360" "c=640 360 640 360"
    stop
    ;;
A13)
    start
    open_window a
    open_window b
    open_window c
    precondition "mosaico de tres ventanas" wait_for layout_is "a=0 0 640 720" "b=640 0 640 360" "c=640 360 640 360"
    kill "${PIDS[a]}"
    unset 'PIDS[a]'
    expect_layout A13-cierre-maestra "b=0 0 640 720" "c=640 0 640 720"
    stop
    ;;
A14)
    start
    open_window a
    open_window b
    open_window c
    precondition "mosaico de tres ventanas" wait_for layout_is "a=0 0 640 720" "b=640 0 640 360" "c=640 360 640 360"
    retire b
    expect_layout A14-retirada "a=0 0 640 720" "c=640 0 640 720"
    xdotool windowmap "${WIDS[b]}"
    expect_layout A14-al-final "a=0 0 640 720" "c=640 0 640 360" "b=640 360 640 360"
    stop
    ;;
A15)
    start
    open_window a
    precondition "ventana a en mosaico" wait_for layout_is "a=0 0 1280 720"
    # xev muestra los ConfigureNotify que recibe la ventana. No hay forma de
    # saber cuándo ha seleccionado los eventos: se espera 1 s.
    xev -id "${WIDS[a]}" -event structure >"$OUT/A15-xev.txt" 2>&1 &
    XEV=$!
    sleep 1
    xdotool windowmove "${WIDS[a]}" 100 120
    xdotool windowsize "${WIDS[a]}" 400 200
    xdotool windowraise "${WIDS[a]}"
    three_synthetic() { (($(grep -c 'synthetic YES' "$OUT/A15-xev.txt") >= 3)); }
    wait_for three_synthetic
    kill "$XEV"
    synthetic=$(grep -c 'synthetic YES' "$OUT/A15-xev.txt")
    real=$(grep -c 'synthetic NO' "$OUT/A15-xev.txt")
    geometry_lines=$(grep -c '(0,0), width 1280, height 720,' "$OUT/A15-xev.txt")
    verdict=FALLA
    [[ $synthetic == 3 && $real == 0 && $geometry_lines == 3 ]] && verdict=OK
    record A15-sinteticos "3 sintéticos (0,0) 1280x720, 0 reales" "$synthetic sintéticos ($geometry_lines con la geometría asignada), $real reales" "$verdict" "xev -event structure tras windowmove, windowsize y windowraise"
    # Tras los tres eventos sintéticos, el WM ya ha atendido las peticiones.
    expect_layout A15-geometria "a=0 0 1280 720"
    stop
    ;;
A16)
    start
    open_window a
    open_window d
    precondition "mosaico de dos ventanas" wait_for layout_is "a=0 0 640 720" "d=640 0 640 720"
    retire d
    xprop -id "${WIDS[d]}" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE _NET_WM_WINDOW_TYPE_DIALOG
    precondition "d es un diálogo" eval "xprop -id ${WIDS[d]} _NET_WM_WINDOW_TYPE | grep -q '= _NET_WM_WINDOW_TYPE_DIALOG$'"
    xdotool windowmap "${WIDS[d]}"
    precondition "d gestionada como flotante" wait_for grep -q "ventana ${WIDS[d]} gestionada (flotante)" "$WM_LOG"
    expect_layout A16-flotante "a=0 0 1280 720" "d=640 0 640 720"
    xdotool windowmove "${WIDS[d]}" 100 100
    expect_layout A16-configure-aplicado "d=100 100 640 720"
    open_window c
    precondition "mosaico de a y c" wait_for layout_is "a=0 0 640 720" "c=640 0 640 720"
    # xwininfo -children lista las hijas de la raíz de arriba abajo.
    order=$(xwininfo -root -children | awk '/"(a|c|d)": / {gsub(/[":]/, "", $2); printf "%s%s", sep, $2; sep = " "}')
    verdict=FALLA
    [[ $order == "d c a" ]] && verdict=OK
    record A16-encima "d c a" "$order" "$verdict" "xwininfo -root -children (de arriba abajo) tras mapear c"
    stop
    ;;
A17)
    start
    open_window a
    open_window t
    precondition "mosaico de dos ventanas" wait_for layout_is "a=0 0 640 720" "t=640 0 640 720"
    retire t
    # xprop crea la propiedad con tipo CARDINAL; el WM acepta cualquier
    # valor de 32 bits (ADR 0004).
    xprop -id "${WIDS[t]}" -f WM_TRANSIENT_FOR 32x -set WM_TRANSIENT_FOR "${WIDS[a]}"
    xdotool windowmap "${WIDS[t]}"
    precondition "t gestionada" wait_for grep -q "ventana ${WIDS[t]} gestionada" "$WM_LOG"
    kind=$(grep -o "ventana ${WIDS[t]} gestionada ([a-z]*)" "$WM_LOG" | tail -n1)
    verdict=FALLA
    [[ $kind == *"(flotante)" ]] && layout_is "a=0 0 1280 720" && verdict=OK
    record A17-flotante "flotante; a=0 0 1280 720" "${kind:-sin registro}; a=$(geometry a)" "$verdict" "WM_TRANSIENT_FOR($(xprop -id "${WIDS[t]}" WM_TRANSIENT_FOR | sed 's/^WM_TRANSIENT_FOR(\([A-Z]*\)).*/\1/'))"
    stop
    ;;
A18)
    set_marker "$TOKEN"
    # Sin WM, el servidor mapea la ventana con la geometría pedida.
    open_window pre
    precondition "ventana previa sin gestionar" wait_for layout_is "pre=10 10 200 100"
    start_wm A18
    require_ready
    expect_layout A18-adopcion "pre=0 0 1280 720"
    open_window b
    expect_layout A18-tras-adopcion "pre=0 0 640 720" "b=640 0 640 720"
    stop
    ;;
A19)
    start
    # Ventanas que desaparecen mientras el WM las gestiona, con esperas de
    # 5 a 95 ms antes de matar el cliente.
    for ((i = 0; i < 40; i++)); do
        xlogo -geometry 100x100 >/dev/null 2>&1 &
        sleep "0.0$((i % 10))5"
        kill -KILL "$!" 2>/dev/null
    done
    wait_for eval '! pgrep -x xlogo -P $$'
    sleep 0.5
    if kill -0 "$WM_PID" 2>/dev/null; then
        record A19-sigue-en-marcha "en marcha" "en marcha" OK "kill -0 tras 40 ventanas de xlogo terminadas con SIGKILL"
    else
        record A19-sigue-en-marcha "en marcha" "terminó" FALLA "ver A19.log"
    fi
    # Sin clientes obsoletos, una ventana nueva ocupa toda la pantalla.
    open_window a
    expect_layout A19-estado-coherente "a=0 0 1280 720"
    xerrors=$(grep -c 'error X ignorado' "$WM_LOG")
    unmanaged=$(grep -c 'no gestionada' "$WM_LOG")
    record A19-registro-errores "registrados sin terminar" "$xerrors errores X, $unmanaged ventanas no gestionadas" "$(kill -0 "$WM_PID" && echo OK || echo FALLA)" "informativo: depende de la temporización"
    stop
    ;;
A20)
    start
    open_window a
    open_window b
    open_window c
    expect_layout A20-impar "a=0 0 639 719" "b=639 0 640 360" "c=639 360 640 359"
    stop
    ;;
*)
    echo "caso desconocido: $CASE"
    exit 2
    ;;
esac

# Ningún error X debe haberse registrado (salvo en A19, que los provoca).
if [[ $CASE != A19 && -n ${WM_LOG:-} ]]; then
    xerrors=$(grep -c 'error X ignorado' "$WM_LOG")
    record "$CASE-extra-sin-errores-x" 0 "$xerrors" "$([[ $xerrors == 0 ]] && echo OK || echo FALLA)" "líneas «error X ignorado» en $WM_ID.log"
fi
