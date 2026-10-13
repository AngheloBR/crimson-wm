# shellcheck shell=bash
# Funciones para abrir ventanas y comprobar su geometría en las pruebas de
# F2 y F3. Se cargan después de f1/lib.sh y f1/xvfb-env.sh, con OUT, CASE,
# TOKEN y las funciones de esos archivos definidas.

declare -A PIDS WIDS

viewable() { xwininfo -name "$1" | grep -q 'Map State: IsViewable'; }
# open_window nombre [orden...]: lanza la orden (por defecto, un xmessage
# con ese título), guarda su salida en $OUT/$CASE-nombre.out y espera a que
# la ventana sea visible. Con el WM en marcha, ya está colocada al mapearse.
open_window() {
    local name=$1
    shift
    (($#)) || set -- xmessage -title "$name" -geometry 200x100+10+10 "$name"
    "$@" >"$OUT/$CASE-$name.out" 2>&1 &
    PIDS[$name]=$!
    precondition "ventana $name visible" wait_for viewable "$name"
    WIDS[$name]=$(xwininfo -name "$name" | awk '/Window id:/ {print $4}')
}
# geometry nombre: «x y ancho alto» de la ventana.
geometry() {
    xwininfo -id "${WIDS[$1]}" 2>/dev/null |
        awk '/Absolute upper-left X|Absolute upper-left Y|Width:|Height:/ {printf "%s%s", sep, $NF; sep = " "}'
}
layout_is() {
    local pair
    for pair in "$@"; do
        [[ $(geometry "${pair%%=*}") == "${pair#*=}" ]] || return 1
    done
}
# expect_layout id nombre=geometría...: espera hasta 5 s a que cada ventana
# tenga su geometría, con el WM en marcha (sin WM, el servidor aplicaría
# cualquier geometría pedida).
expect_layout() {
    local id=$1 verdict=FALLA got pair
    shift
    wait_for layout_is "$@" && kill -0 "$WM_PID" && verdict=OK
    got=$(for pair in "$@"; do printf '%s=%s; ' "${pair%%=*}" "$(geometry "${pair%%=*}")"; done)
    record "$id" "$*" "${got%; }" "$verdict" "xwininfo, WM en marcha"
}
# retire nombre: desmapea la ventana y espera a que el WM deje de
# gestionarla. Así se le pueden poner propiedades antes de volver a mapearla.
retire() {
    xdotool windowunmap "${WIDS[$1]}"
    precondition "ventana $1 retirada" wait_for grep -q "ventana ${WIDS[$1]} retirada" "$WM_LOG"
}
start() {
    set_marker "$TOKEN"
    start_wm "$CASE"
    require_ready
}
stop() {
    xdotool key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    ((${#PIDS[@]})) && kill "${PIDS[@]}" 2>/dev/null
}
