#!/bin/bash
# Una prueba de aceptación de F1 dentro de un Xvfb nuevo. run.sh lo ejecuta
# como orden de `xvfb-run` (Debian 13), que fija DISPLAY y XAUTHORITY.
# Uso: xvfb-case.sh BINARIO DIRECTORIO_DE_RESULTADOS CASO
set -u
BIN=$1
OUT=$2
CASE=$3
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
HOLD=
on_exit() {
    local rc=$?
    [[ -n $HOLD ]] && kill "$HOLD" 2>/dev/null
    echo "$CASE xvfb-case.sh exit: $rc" >>"$RUNS"
}
trap on_exit EXIT

# Guardas: solo se continúa dentro del Xvfb de este xvfb-run, con su cookie
# temporal, sin TCP y sin -ac. Si algo no encaja, no se conecta nada.
if ! [[ ${DISPLAY:-} =~ ^:[1-9][0-9]*$ ]]; then
    echo "GUARDA: DISPLAY inesperado"
    exit 90
fi
if [[ ${XAUTHORITY:-} != */xvfb-run.*/Xauthority || ! -s $XAUTHORITY ]]; then
    echo "GUARDA: XAUTHORITY no es la cookie temporal de xvfb-run"
    exit 90
fi
XVFB_PID=$(pgrep -P "$PPID" -x Xvfb) || {
    echo "GUARDA: no hay un Xvfb hijo de xvfb-run"
    exit 90
}
mapfile -d '' XVFB_ARGS <"/proc/$XVFB_PID/cmdline"
if [[ ${XVFB_ARGS[0]} != Xvfb || ${XVFB_ARGS[1]} != "$DISPLAY" ]]; then
    echo "GUARDA: el Xvfb hijo no sirve $DISPLAY"
    exit 90
fi
auth_ok=0 nolisten_ok=0 SIZE=
for ((i = 2; i < ${#XVFB_ARGS[@]}; i++)); do
    case ${XVFB_ARGS[i]} in
    -ac | -listen)
        echo "GUARDA: Xvfb arrancado con ${XVFB_ARGS[i]}"
        exit 90
        ;;
    -auth) [[ ${XVFB_ARGS[i + 1]:-} == "$XAUTHORITY" ]] && auth_ok=1 ;;
    -nolisten) [[ ${XVFB_ARGS[i + 1]:-} == tcp ]] && nolisten_ok=1 ;;
    -screen) [[ ${XVFB_ARGS[i + 2]:-} =~ ^([0-9]+x[0-9]+)x[0-9]+$ ]] && SIZE=${BASH_REMATCH[1]} ;;
    esac
done
if ((!auth_ok || !nolisten_ok)) || [[ -z $SIZE ]]; then
    echo "GUARDA: Xvfb sin -auth con la cookie temporal, sin -nolisten tcp o sin -screen"
    exit 90
fi
echo "$CASE: DISPLAY=$DISPLAY, Xvfb pid $XVFB_PID, pantalla $SIZE, -auth <cookie temporal>, -nolisten tcp"

N=${DISPLAY#:}
TOKEN=$(mcookie)
export CRIMSON_SANDBOX_TOKEN=$TOKEN

# Xvfb se reinicia, y borra propiedades y átomos de la raíz, cuando se
# desconecta su último cliente (ADR 0003). Un cliente pasivo mantiene el
# servidor vivo durante toda la prueba; se confirma con una propiedad de
# control, que también sirve para detectar un reinicio inesperado.
xprop -root -spy >/dev/null 2>&1 &
HOLD=$!
holding() {
    kill -0 "$HOLD" &&
        xprop -root -f _CRIMSON_CONTROL 8s -set _CRIMSON_CONTROL 1 &&
        xprop -root _CRIMSON_CONTROL | grep -qx '_CRIMSON_CONTROL(STRING) = "1"'
}
if ! wait_for holding; then
    record "$CASE-precondicion" "servidor sin reinicios" "no confirmado" FALLA "_CRIMSON_CONTROL no persiste"
    exit 1
fi

# precondition descripción orden...: registra un fallo y aborta si no se cumple.
precondition() {
    local what=$1
    shift
    if ! "$@" >/dev/null 2>&1; then
        record "$CASE-precondicion" "$what" "no se cumple" FALLA "$*"
        exit 1
    fi
}
marker_is() { xprop -root _CRIMSON_SANDBOX | grep -qxF "_CRIMSON_SANDBOX($1) = \"$2\""; }
marker_differs() { xprop -root _CRIMSON_SANDBOX | grep -q '^_CRIMSON_SANDBOX(STRING) = ' && ! xprop -root _CRIMSON_SANDBOX | grep -qF -- "$TOKEN"; }
wm_check_present() { xprop -root _NET_SUPPORTING_WM_CHECK | grep -q '= 1$'; }
no_marker_atom() { [[ -z $(xlsatoms -name _CRIMSON_SANDBOX) ]]; }
set_marker() {
    xprop -root -f _CRIMSON_SANDBOX 8s -set _CRIMSON_SANDBOX "$1"
    precondition "marca presente" marker_is STRING "$1"
}
ready() { grep -q 'salvaguardas superadas' "$WM_LOG" && kill -0 "$WM_PID"; }
# require_ready: aborta la prueba si el WM no llega a gestionar la raíz.
require_ready() {
    if ! wait_for ready; then
        kill -0 "$WM_PID" 2>/dev/null && kill "$WM_PID"
        wait "$WM_PID"
        finish "$WM_ID" "en marcha" "$?" 'salvaguardas superadas' "$WM_LOG"
        record "$CASE-precondicion" "WM en marcha" "no arrancó" FALLA "ver $WM_ID.log"
        exit 1
    fi
}

case $CASE in
A3b)
    set_marker "$TOKEN"
    : >"$OUT/xauth-vacio-b"
    run_bin A3b-xauthority-vacio 11 'no hay cookie MIT-MAGIC-COOKIE-1' XAUTHORITY="$OUT/xauth-vacio-b"
    ;;
A3c)
    set_marker "$TOKEN"
    other=$((N == 1 ? 2 : 1))
    xauth -f "$OUT/xauth-otro" add ":$other" . "$(mcookie)"
    precondition "cookie solo para :$other" test -s "$OUT/xauth-otro"
    run_bin A3c-cookie-otro-display 11 'no hay cookie MIT-MAGIC-COOKIE-1' XAUTHORITY="$OUT/xauth-otro"
    ;;
A3d)
    set_marker "$TOKEN"
    xauth -f "$OUT/xauth-erronea" add ":$N" . "$(mcookie)"
    precondition "cookie errónea para :$N" test -s "$OUT/xauth-erronea"
    run_bin A3d-cookie-erronea 11 'rechazó la conexión' XAUTHORITY="$OUT/xauth-erronea"
    ;;
A3e)
    # Control: el propio servidor exige autenticación (no hay -ac).
    : >"$OUT/xauth-vacio-e"
    XAUTHORITY="$OUT/xauth-vacio-e" timeout 10 xprop -root >"$OUT/A3e-control.log" 2>&1
    got=$?
    verdict=FALLA
    ((got != 0)) && grep -q 'unable to open display' "$OUT/A3e-control.log" && verdict=OK
    record A3e-control-servidor "≠0" "$got" "$verdict" "$(paste -sd' ' "$OUT/A3e-control.log")"
    ;;
A4a)
    set_marker "$TOKEN"
    run_bin A4a-sin-token 10 'CRIMSON_SANDBOX_TOKEN no está definida' -u CRIMSON_SANDBOX_TOKEN
    ;;
A4b)
    precondition "átomo de la marca inexistente" no_marker_atom
    run_bin A4b-sin-marca 10 'marca de sandbox válida'
    atoms=$(xlsatoms -name _CRIMSON_SANDBOX; xlsatoms -name _NET_SUPPORTING_WM_CHECK)
    control=$(xlsatoms -name _CRIMSON_CONTROL | wc -l)
    verdict=FALLA
    [[ -z $atoms && $control == 1 ]] && verdict=OK
    record A4b-no-crea-atomos "vacío" "${atoms:-vacío}" "$verdict" "xlsatoms tras el rechazo; átomo de control presente: $control (sin reinicio)"
    ;;
A4c)
    set_marker "$(mcookie)"
    precondition "marca STRING distinta del token" marker_differs
    run_bin A4c-marca-distinta 10 'marca de sandbox válida'
    ;;
A4d)
    xprop -root -f _CRIMSON_SANDBOX 8u -set _CRIMSON_SANDBOX "$TOKEN"
    precondition "marca UTF8_STRING con el valor correcto" marker_is UTF8_STRING "$TOKEN"
    run_bin A4d-marca-tipo-utf8 10 'marca de sandbox válida'
    ;;
A5)
    set_marker "$TOKEN"
    xprop -root -f _NET_SUPPORTING_WM_CHECK 32c -set _NET_SUPPORTING_WM_CHECK 1
    precondition "_NET_SUPPORTING_WM_CHECK presente" wm_check_present
    run_bin A5-otro-wm-check 12 '_NET_SUPPORTING_WM_CHECK'
    ;;
A6)
    set_marker "$TOKEN"
    start_wm A6-primera
    require_ready
    run_bin A6-segunda 12 'BadAccess'
    precondition "la primera instancia sigue en marcha" kill -0 "$WM_PID"
    xdotool key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    ;;
A7)
    set_marker "$TOKEN"
    start_wm A7
    require_ready
    pattern="conectado a :$N\\.0 \\(fabricante \".+\", release [0-9]+, raíz 0x[0-9a-f]+, $SIZE\\)"
    ev=$(grep -m1 -E -- "$pattern" "$WM_LOG")
    record A7-registro "línea de conexión" "${ev:+presente}" "$([[ -n $ev ]] && echo OK || echo FALLA)" "${ev:-$(head -n2 "$WM_LOG")}"

    xmessage -title crimson-f1 -geometry 300x100+10+10 "Crimson WM F1" &
    XM=$!
    viewable() { xwininfo -name crimson-f1 | grep -q 'Map State: IsViewable'; }
    # Sin WM el servidor mapearía la ventana por sí mismo: el WM debe seguir vivo.
    if wait_for viewable && kill -0 "$WM_PID"; then
        record A7-mapeo IsViewable IsViewable OK "$(xwininfo -name crimson-f1 | grep -m1 'Window id')"
    else
        record A7-mapeo IsViewable "$(xwininfo -name crimson-f1 2>&1 | grep -m1 -E 'Map State|error')" FALLA "MapRequest no atendido o WM terminado"
    fi

    # Extra: ConfigureRequest aplicado tal como se pide.
    wid=$(xwininfo -name crimson-f1 | awk '/Window id:/ {print $4}')
    xdotool windowmove "$wid" 100 120
    xdotool windowsize "$wid" 400 200
    configured() {
        xwininfo -id "$wid" | grep -Eq 'Absolute upper-left X: +100$' &&
            xwininfo -id "$wid" | grep -Eq 'Absolute upper-left Y: +120$' &&
            xwininfo -id "$wid" | grep -Eq 'Width: 400$' &&
            xwininfo -id "$wid" | grep -Eq 'Height: 200$'
    }
    wait_for configured
    geometry=$(xwininfo -id "$wid" | awk '/Absolute upper-left X|Absolute upper-left Y|Width:|Height:/ {printf "%s ", $NF}')
    geometry=${geometry% }
    verdict=FALLA
    [[ $geometry == '100 120 400 200' ]] && kill -0 "$WM_PID" && verdict=OK
    record A7-extra-configure "100 120 400 200" "$geometry" "$verdict" "xwininfo tras xdotool windowmove/windowsize, WM en marcha"

    # Extra: Mod4+Escape sin Shift no debe cerrar el WM.
    xdotool key super+Escape
    sleep 0.5
    if kill -0 "$WM_PID" 2>/dev/null; then
        record A7-extra-sin-shift "sigue en marcha" "sigue en marcha" OK "kill -0 tras super+Escape"
    else
        record A7-extra-sin-shift "sigue en marcha" "terminó" FALLA "super+Escape cerró el WM"
    fi

    xdotool key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    kill "$XM" 2>/dev/null
    ;;
A8)
    set_marker "$TOKEN"
    start_wm A8
    require_ready
    kill -TERM "$XVFB_PID"
    wait_wm 13 'se perdió la conexión'
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
