#!/bin/bash
# A150–A164: solo dentro del Xvfb autenticado que comprueba helpers.sh.
set -u
F=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$F/../../.." && pwd)
# shellcheck source=helpers.sh
source "$F/helpers.sh"

config13() {
    sed -e '/^default = /d' -e "/^master_percent = /a default = \"$1\"" "$ROOT/docs/examples/wm.toml" >"$SANDBOX/config-base"
    cat "$SANDBOX/config-base" - <<'EOF' | write_config "$XDG_FILE"

[[keybindings]]
keys = "Mod4+space"
action = "cycle_layout"
EOF
}
check() {
    local want=${2//$'\n'/\\n} got=${3//$'\n'/\\n}
    record "$1" "${want//$'\t'/\\t}" "${got//$'\t'/\\t}" "$(ok_if test "$2" = "$3")" "${4:-observación del servidor}"
}
cycle() { key super+space; }
focus() { xdotool windowactivate --sync "${WIDS[$1]}"; precondition "foco $1" wait_for focus_is "$1"; }
view() { key "super+$1"; precondition "escritorio $1" wait_for desk_is "$1"; }
fullscreen() {
    local atom
    atom=$(xlsatoms -name _NET_WM_STATE_FULLSCREEN | awk '{print $1}')
    precondition 'átomo fullscreen existente' test -n "$atom"
    precondition 'mensaje fullscreen enviado' "$TEST_CLIENT" message 32 "${WIDS[$1]}" _NET_WM_STATE "$2" "$atom" 0 2 0
}
move_ewmh() { "$TEST_CLIENT" message 32 "${WIDS[$1]}" _NET_WM_DESKTOP "$(($2-1))" 2 0 0 0; }
full_is() { xprop -id "${WIDS[$1]}" _NET_WM_STATE | grep -q '_NET_WM_STATE_FULLSCREEN'; }
not_full() { ! full_is "$1"; }
border_of() { xwininfo -id "${WIDS[$1]}" | awk '/Border width:/ {print $3}'; }
client_count() { xprop -root _NET_CLIENT_LIST | grep -o '0x[0-9a-f]*' | wc -l; }
no_connection() { ! grep -q 'conectado a' "$WM_LOG"; }
three() { open_tc A; open_tc B; open_tc C; }
cols3() { expect_layout "$1" 'A=0 0 427 720' 'B=427 0 427 720' 'C=854 0 426 720'; }
master3() { expect_layout "$1" 'A=0 0 640 720' 'B=640 0 640 360' 'C=640 360 640 360'; }
# Solo acepta eventos posteriores al estímulo, nunca mensajes antiguos.
new_notify() { tail -n "+$(($2 + 1))" "$OUT/$CASE-$1.out" | grep -qF -- "$3"; }
space_presses() {
    awk '/^KeyPress event/ {press=1} /^KeyRelease event/ {press=0}
        press && /state 0x40,.*[(]keysym 0x20, space[)]/ {n++}
        END {print n+0}' "$OUT/$CASE-Q.out"
}

case $CASE in
A150)
    # EXPECT: A150 A150-aislamiento A150-f12 A150-30 A150-example A150-default-sin-cycle A150-space-libre
    start13
    three
    master3 A150-f12
    check_config
    check A150-30 '0; 30 atajos' "$CHECK_RC; $(grep -o '30 atajos' <<<"$CHECK_OUT")" 'resumen real de --check-config, además del código'
    key super+space
    master3 A150-default-sin-cycle
    open_window Q stdbuf -oL xev -name Q -event keyboard
    precondition 'foco del observador' wait_for focus_is Q
    key super+space
    precondition 'space llega al cliente' wait_for output_is 1 space_presses
    check A150-space-libre 1 "$(space_presses)" 'xev recibe KeyPress con Mod4; no hay grab predeterminado'
    kill "${PIDS[Q]}"
    precondition 'observador retirado' wait_for output_is 3 client_count
    stop_keep
    git -C "$ROOT" show d7a4c40252e93d96fc5f5c352da89bda0403374c:docs/examples/wm.toml | write_config "$XDG_FILE"
    start_wm A150-example
    require_ready
    master3 A150-ejemplo-geometria
    stop_with super+shift+Escape
    ;;
A151)
    # EXPECT: A151 A151-aislamiento A151-uno A151-dos A151-tres A151-cuatro A151-captura A151-fidelidad
    config13 columns; start13
    open_tc A --background 8B0A1F
    expect_layout A151-uno 'A=0 0 1280 720'
    open_tc B --background 1F2328
    expect_layout A151-dos 'A=0 0 640 720' 'B=640 0 640 720'
    open_tc C --background D0D7DE
    cols3 A151-tres
    xwd -root -silent -out "$OUT/columns.xwd"
    magick "$OUT/columns.xwd" "$OUT/columns.png"
    cycle
    master3 A151-master-captura
    xwd -root -silent -out "$OUT/master.xwd"
    magick "$OUT/master.xwd" "$OUT/master.png"
    for kind in master columns; do
        label='Master-stack · 50 %'
        [[ $kind == columns ]] && label=Columns
        magick "$OUT/$kind.png" -background white -gravity north -splice 0x70 -font DejaVu-Sans-Bold -pointsize 32 -fill '#1F2328' -annotate +0+18 "$label" "$OUT/$kind-panel.png"
    done
    magick "$OUT/master-panel.png" "$OUT/columns-panel.png" +append -gravity north -background white -splice 0x70 -font DejaVu-Sans-Bold -pointsize 34 -fill '#8B0A1F' -annotate +0+18 'Crimson WM · F13: capturas reales en Xvfb' "$OUT/comparison.png"
    check A151-captura 2560x860 "$(magick identify -format '%wx%h' "$OUT/comparison.png")" 'solo rótulos añadidos; izquierda Master-stack, derecha Columns'
    magick "$OUT/comparison.png" -crop 1280x720+0+140 +repage "$OUT/check-master.png"
    magick "$OUT/comparison.png" -crop 1280x720+1280+140 +repage "$OUT/check-columns.png"
    same="$(magick compare -metric AE "$OUT/master.png" "$OUT/check-master.png" null: 2>&1) $(magick compare -metric AE "$OUT/columns.png" "$OUT/check-columns.png" null: 2>&1)"
    check A151-fidelidad '0 0' "$same" 'cada panel coincide píxel a píxel con su captura'
    cycle
    open_tc D
    expect_layout A151-cuatro 'A=0 0 320 720' 'B=320 0 320 720' 'C=640 0 320 720' 'D=960 0 320 720'
    stop
    ;;
A152)
    # EXPECT: A152 A152-aislamiento A152-columns A152-master A152-repeat A152-foco A152-orden A152-zoom
    config13 master-stack; start13; three
    cycle; cols3 A152-columns
    check A152-foco C "$(focus_name)"
    cycle; master3 A152-master
    before=$(grep -c 'layout del escritorio' "$WM_LOG")
    xdotool keydown super+space
    sleep 1.2
    xdotool keyup super+space
    cols3 A152-repeat-geometria
    after=$(grep -c 'layout del escritorio' "$WM_LOG")
    check A152-repeat 1 "$((after-before))" 'una pulsación mantenida, repetición del servidor ignorada'
    key super+j
    check A152-orden A "$(focus_name)" 'C→A conserva navegación'
    focus C
    key super+shift+Return
    expect_layout A152-zoom 'C=0 0 427 720' 'A=427 0 427 720' 'B=854 0 426 720'
    stop
    ;;
A153)
    # EXPECT: A153 A153-aislamiento A153-desk1 A153-desk2 A153-anterior A153-vacio A153-inicial
    config13 columns; start13; three
    cols3 A153-inicial
    cycle; master3 A153-desk1
    view 2
    open_tc D; open_tc E; open_tc G
    expect_layout A153-desk2 'D=0 0 427 720' 'E=427 0 427 720' 'G=854 0 426 720'
    key super+Tab
    precondition 'regreso al 1' wait_for desk_is 1
    master3 A153-anterior
    view 3; cycle
    open_tc H; open_tc I; open_tc J
    kill "${PIDS[H]}" "${PIDS[I]}" "${PIDS[J]}"
    precondition 'escritorio vacío' wait_for output_is 6 client_count
    view 2; view 3
    open_tc K; open_tc L; open_tc M
    expect_layout A153-vacio 'K=0 0 640 720' 'L=640 0 640 360' 'M=640 360 640 360'
    stop
    ;;
A154)
    # EXPECT: A154 A154-aislamiento A154-origen A154-destino A154-ewmh A154-foco A154-full-move A154-full-desplazada A154-full-pila
    config13 master-stack; start13; three; cycle
    key super+shift+2
    expect_layout A154-origen 'A=0 0 640 720' 'B=640 0 640 720'
    check A154-foco B "$(focus_name)"
    view 2; open_tc D; open_tc E
    expect_layout A154-destino 'C=0 0 640 720' 'D=640 0 640 360' 'E=640 360 640 360'
    move_ewmh E 1
    view 1
    expect_layout A154-ewmh 'A=0 0 427 720' 'B=427 0 427 720' 'E=854 0 426 720'
    fullscreen B 1
    view 2; fullscreen C 1
    expect_layout A154-full-previa 'C=0 0 1280 720'
    view 1; move_ewmh B 2; view 2
    expect_layout A154-full-move 'B=0 0 1280 720' 'C=0 0 640 720' 'D=640 0 640 720'
    precondition 'C deja fullscreen' wait_for not_full C
    check A154-full-desplazada 0 "$(full_is C && echo 1 || echo 0)"
    check A154-full-pila 1 "$(full_is B && echo 1 || echo 0)"
    stop
    ;;
A155)
    # EXPECT: A155 A155-aislamiento A155-strut A155-dinamico A155-oculto A155-retirado A155-workarea
    config13 columns; start13; three
    open_tc P --type DOCK --geometry 0 0 1280 40 --strut 0 0 40 0
    expect_layout A155-strut 'A=0 40 427 680' 'B=427 40 427 680' 'C=854 40 426 680'
    check A155-workarea 8 "$(xprop -root _NET_WORKAREA | grep -o '0, 40, 1280, 680' | wc -l)"
    xprop -id "${WIDS[P]}" -f _NET_WM_STRUT 32c -set _NET_WM_STRUT '0, 0, 60, 0'
    expect_layout A155-dinamico 'A=0 60 427 660' 'B=427 60 427 660' 'C=854 60 426 660'
    view 2; open_tc D; open_tc E
    xprop -id "${WIDS[P]}" -f _NET_WM_STRUT 32c -set _NET_WM_STRUT '0, 0, 20, 0'
    view 1
    expect_layout A155-oculto 'A=0 20 427 700' 'B=427 20 427 700' 'C=854 20 426 700'
    kill "${PIDS[P]}"
    cols3 A155-retirado
    stop
    ;;
A156)
    # EXPECT: A156 A156-aislamiento A156-floating A156-capas A156-foco A156-borde
    config13 columns; start13; three
    open_tc V --type UTILITY --geometry 100 100 400 240 --border 3
    open_tc P --type DOCK --geometry 0 0 1280 20
    open_tc N --type NOTIFICATION --geometry 1050 30 200 80
    open_tc X --type DESKTOP --geometry 0 0 1280 720
    cycle
    expect_layout A156-floating 'V=100 100 400 240'
    check A156-borde 3 "$(border_of V)"
    check A156-foco V "$(focus_name)"
    check A156-capas 'X A V P N' "$(order_of X A V P N)" 'orden inferior→superior, QueryTree'
    stop
    ;;
A157)
    # EXPECT: A157 A157-aislamiento A157-centro A157-sin-recentrar A157-familia A157-retorno
    config13 columns; start13; three
    open_dialog D A --dialog --geometry 0 0 200 100
    expect_layout A157-centro 'D=113 310 200 100'
    cycle
    expect_layout A157-sin-recentrar 'D=113 310 200 100'
    focus A; key super+shift+2
    precondition 'familia oculta' wait_for output_is o map_state D
    view 2
    check A157-familia '2 2' "$(desk_of A) $(desk_of D)"
    focus D
    key super+w
    precondition 'retorno al propietario' wait_for focus_is A
    check A157-retorno A "$(focus_name)"
    stop
    ;;
A158)
    # EXPECT: A158 A158-aislamiento A158-full A158-conserva A158-sale A158-float-restaura A158-oculto A158-borde-restaura A158-salida-visible A158-salida-estado
    config13 master-stack; start13; three
    fullscreen B 1
    expect_layout A158-full 'B=0 0 1280 720'
    cycle
    check A158-conserva 1 "$(full_is B && echo 1 || echo 0)"
    fullscreen B 0
    cols3 A158-sale
    open_tc V --dialog --geometry 100 110 400 240 --border 2
    fullscreen V 1
    expect_layout A158-float-full 'V=0 0 1280 720'
    cycle; fullscreen V 0
    expect_layout A158-float-restaura 'V=100 110 400 240'
    check A158-borde-restaura 2 "$(border_of V)"
    # Escritorio 1 queda Columns y con fullscreen; el activo 2 es Master.
    cycle; fullscreen B 1
    expect_layout A158-full-antes-ocultar 'B=0 0 1280 720'
    view 2
    stop_keep
    # release coloca el escritorio oculto con SU layout, no el activo.
    expect_geometry_after_exit() { layout_is 'A=0 0 427 720' 'B=427 0 427 720' 'C=854 0 426 720'; }
    precondition 'geometría tras release' wait_for expect_geometry_after_exit
    check A158-oculto '427 0 427 720' "$(geometry B)"
    check A158-salida-visible v "$(map_state B)" 'la salida vuelve a mapear los clientes ocultos'
    check A158-salida-estado '_NET_WM_STATE(ATOM) = ' "$(xprop -id "${WIDS[B]}" _NET_WM_STATE)" 'fullscreen eliminado tras release'
    kill "${PIDS[@]}" 2>/dev/null
    ;;
A159)
    # EXPECT: A159 A159-aislamiento A159-nofocus A159-floating A159-oculta A159-no-reevalua A159-env
    config13 columns
    cat <<'EOF' >>"$XDG_FILE"
[[rules]]
class = "Viewer"
floating = true
nofocus = true
[[rules]]
class = "Chat"
desktop = 2
EOF
    start13; open_tc A
    open_tc V --class viewer Viewer --geometry 100 100 400 240
    check A159-nofocus A "$(focus_name)"
    expect_layout A159-floating 'V=100 100 400 240'
    open_hidden H --class chat Chat
    precondition 'regla de escritorio' wait_for output_is 2 desk_of H
    check A159-oculta o "$(map_state H)"
    before=$(grep -c ': reglas de las líneas' "$WM_LOG")
    cycle; cycle
    after=$(grep -c ': reglas de las líneas' "$WM_LOG")
    check A159-no-reevalua "$before" "$after"
    stop_keep
    WM_ENV=("${BASE_ENV[@]}" 'CRIMSON_RULES=class=Viewer; desktop=3')
    start_wm A159-env
    require_ready
    open_hidden Q --class viewer Viewer
    precondition 'regla de entorno' wait_for output_is 3 desk_of Q
    check A159-env-desktop 3 "$(desk_of Q)"
    stop_with super+shift+Escape
    kill "${PIDS[@]}" 2>/dev/null
    ;;
A160)
    # EXPECT: A160-check A160 A160-sin-conexion A160-servidor A160-parametro
    config13 columns
    before=$(server_state)
    check_config
    check A160-check 0 "$CHECK_RC"
    printf '[layout]\ndefault = "monocle"\n' | write_config "$XDG_FILE"
    set_marker "$TOKEN"
    before=$(server_state)
    start_wm A160
    wait_wm 15 'default debe ser'
    check A160-sin-conexion 1 "$(no_connection && echo 1 || echo 0)"
    check A160-servidor "$before" "$(server_state)" 'mismos átomos y propiedades'
    printf '[[keybindings]]\nkeys="Mod4+space"\naction="cycle_layout"\ndesktop=2\n' | write_config "$XDG_FILE"
    start_wm A160-parametro
    wait_wm 15 'desktop.*no se usa'
    ;;
A161)
    # EXPECT: A161 A161-aislamiento A161-columns A161-real A161-sintetico A161-oculto A161-oculto-notify
    config13 master-stack; start13; three
    before=$(wc -l <"$OUT/$CASE-B.out")
    cycle; cols3 A161-columns
    precondition 'ConfigureNotify real nuevo' wait_for new_notify B "$before" "ConfigureNotify real ${WIDS[B]} 427 0 427 720"
    check A161-real '427 0 427 720' "$(geometry B)"
    before=$(wc -l <"$OUT/$CASE-B.out")
    xdotool windowsize "${WIDS[B]}" 300 200
    precondition 'ConfigureNotify sintético nuevo' wait_for new_notify B "$before" "ConfigureNotify sintético ${WIDS[B]} 427 0 427 720"
    check A161-sintetico '427 0 427 720' "$(geometry B)"
    key super+shift+2
    # C mantiene su última geometría real hasta mostrar su escritorio.
    old=$(geometry C)
    before=$(wc -l <"$OUT/$CASE-C.out")
    xdotool windowsize "${WIDS[C]}" 200 100
    precondition 'ConfigureNotify oculto nuevo' wait_for new_notify C "$before" "ConfigureNotify sintético ${WIDS[C]} $old"
    check A161-oculto "$old" "$(geometry C)"
    check A161-oculto-notify 1 "$(new_notify C "$before" "ConfigureNotify sintético ${WIDS[C]} $old" && echo 1 || echo 0)" 'la geometría comunicada coincide con la real, no con un plan pendiente'
    stop
    ;;
A162)
    # EXPECT: A162 A162-aislamiento A162-adopta A162-retirada A162-nueva A162-conexion
    config13 columns
    # Antes del WM: adopción de clientes ya mapeados.
    open_tc A; open_tc B; open_tc C
    start13; cols3 A162-adopta
    "$TEST_CLIENT" withdraw "${WIDS[B]}"
    expect_layout A162-retirada 'A=0 0 640 720' 'C=640 0 640 720'
    kill "${PIDS[C]}"
    expect_layout A162-una 'A=0 0 1280 720'
    open_tc D
    expect_layout A162-nueva 'A=0 0 640 720' 'D=640 0 640 720'
    stop_keep
    # El servidor de esta prueba, nunca uno ajeno.
    start_wm A162-conexion
    require_ready
    kill "$XVFB_PID"
    wait_wm 13 'conexión|Connection|conexi'
    kill "${PIDS[@]}" 2>/dev/null
    ;;
A163)
    # EXPECT: A163 A163-aislamiento A163-impar
    config13 columns; start13; three
    case $SIZE in
    1279x719) expect_layout A163-impar 'A=0 0 427 719' 'B=427 0 426 719' 'C=853 0 426 719' ;;
    *) record A163-impar '1279x719' "$SIZE" FALLA 'pantalla inesperada' ;;
    esac
    stop
    ;;
A163-wide)
    # EXPECT: A163-wide A163-wide-aislamiento A163-ancho
    config13 columns; start13; three
    expect_layout A163-ancho 'A=0 0 1280 1080' 'B=1280 0 1280 1080' 'C=2560 0 1280 1080'
    stop
    ;;
A163-small)
    # EXPECT: A163-small A163-small-aislamiento A163-diminuto
    config13 columns; start13; three; open_tc D; open_tc E
    expect_layout A163-diminuto 'A=0 0 1 2' 'B=1 0 1 2' 'C=2 0 1 2' 'D=3 0 1 2' 'E=3 0 1 2'
    stop
    ;;
A164)
    # EXPECT: A164 A164-marca A164-check
    config13 columns
    install_fakes
    set_marker incorrecta
    start_wm A164-marca
    wait_wm 10 'marca de sandbox válida'
    printf '[layout]\ndefault=123\n' | write_config "$XDG_FILE"
    start_wm A164
    wait_wm 15 'default debe ser una cadena'
    check_config
    check A164-check 15 "$CHECK_RC"
    ;;
*) echo "caso desconocido: $CASE" >&2; exit 2 ;;
esac

record "$CASE-complete" 'caso terminado' 'caso terminado' OK 'sin abortar guardas ni precondiciones'
