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
# shellcheck source=xvfb-env.sh
source "$(dirname "$0")/xvfb-env.sh"

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
    # Desde F6 (ADR 0008) la primera instancia publica
    # _NET_SUPPORTING_WM_CHECK y toma WM_S0: la segunda la detecta ya en la
    # capa 3. La capa 4 (BadAccess) se prueba en A54 de F6.
    precondition "primera instancia identificada" wait_for grep -q 'identificación publicada' "$WM_LOG"
    run_bin A6-segunda 12 '_NET_SUPPORTING_WM_CHECK \(0x[0-9a-f]+, existente\)'
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

    # Extra: desde F2 (ADR 0004) la ventana va en mosaico y ocupa toda la
    # pantalla; su ConfigureRequest se deniega y la geometría no cambia.
    # Es una comprobación negativa con espera fija: la A15 de F2 la
    # confirma con el ConfigureNotify sintético.
    wid=$(xwininfo -name crimson-f1 | awk '/Window id:/ {print $4}')
    tiled="0 0 ${SIZE%x*} ${SIZE#*x}"
    window_geometry() { xwininfo -id "$wid" | awk '/Absolute upper-left X|Absolute upper-left Y|Width:|Height:/ {printf "%s ", $NF}'; }
    tiled_now() { [[ $(window_geometry) == "$tiled " ]]; }
    precondition "ventana en mosaico ($tiled)" wait_for tiled_now
    xdotool windowmove "$wid" 100 120
    xdotool windowsize "$wid" 400 200
    sleep 0.5
    geometry=$(window_geometry)
    geometry=${geometry% }
    verdict=FALLA
    [[ $geometry == "$tiled" ]] && kill -0 "$WM_PID" && verdict=OK
    record A7-extra-configure "$tiled" "$geometry" "$verdict" "xwininfo tras xdotool windowmove/windowsize (denegado en mosaico), WM en marcha"

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
