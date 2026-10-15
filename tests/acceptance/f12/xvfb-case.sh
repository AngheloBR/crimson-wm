#!/bin/bash
# Una prueba de aceptación de F12 (ADR 0014) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh); las funciones de ventanas, las de F2
# (windows.sh), y las de estado, las mismas que en F11. Como en F4–F11, el
# único directorio del PATH del WM contiene copias de f4/fake.sh: ninguna
# aplicación real puede ejecutarse, tampoco las que nombran las
# configuraciones de prueba.
#
# Las configuraciones se escriben en el XDG_CONFIG_HOME o el HOME temporales
# de la prueba (f1/xvfb-env.sh), o se pasan con --config; nunca se lee la
# del usuario. A147 y A148 hacen capturas reales del Xvfb con `xwd`.
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
open_tc() {
    local name=$1
    shift
    open_window "$name" "$TEST_CLIENT" window "$name" "$@"
}

# Configuración (ADR 0014).
XDG_FILE=$SANDBOX/config/crimson/wm.toml
HOME_FILE=$SANDBOX/home/.config/crimson/wm.toml
# write_config ruta: escribe la entrada estándar en esa ruta con permisos
# 0600.
write_config() {
    mkdir -p "${1%/*}"
    cat >"$1"
    chmod 600 "$1"
}
# start12 [argumentos del binario...]: arranca el WM con los programas
# falsos instalados y registra la evidencia de aislamiento, como start11 en
# F11. WM_ENV se fija antes de llamarla (BASE_ENV por defecto).
BASE_ENV=("${WM_ENV[@]}")
start12() {
    local app found summary='' names expected verdict=FALLA
    install_fakes
    set_marker "$TOKEN"
    start_wm "$CASE" "$@"
    require_ready
    real_pid
    for app in "${APPS[@]}"; do
        found=$(resolve "$app")
        if [[ $found == "$SANDBOX/bin/$app" ]] && cmp -s "$found" "$FAKE"; then
            summary+="$app=falso "
        else
            summary+="$app=¡${found:-ninguno}! "
        fi
    done
    names=$(while IFS= read -r -d '' entry; do printf '%s\n' "${entry%%=*}"; done <"/proc/$WM_REAL/environ" | sort | paste -sd' ')
    expected=$(printf '%s\n' "${WM_ENV[@]%%=*}" | sort | paste -sd' ')
    isolated && [[ $names == "$expected" ]] && verdict=OK
    record "$CASE-aislamiento" "solo programas falsos; entorno mínimo" "${summary% }" "$verdict" "variables del WM (pid $WM_REAL): $names"
}
real_pid() {
    WM_REAL=$(pgrep -P "$WM_PID" -x crimson-wm)
    precondition "pid del WM" test -n "$WM_REAL"
}
# restart id [argumentos...]: otra instancia del WM en el mismo Xvfb.
restart() {
    start_wm "$@"
    require_ready
    real_pid
}
# stop_with teclas: sale con el atajo de salida de la configuración.
stop_with() {
    key "$1"
    wait_wm 0 'atajo de salida recibido'
}
# stop_keep: sale con el atajo predeterminado sin cerrar las ventanas de
# prueba; la instancia siguiente las adopta.
stop_keep() { stop_with super+shift+Escape; }
# config_line: la línea de la configuración cargada, sin el prefijo.
config_line() { grep -m1 '^crimson-wm: configuración: ' "$WM_LOG" | sed 's/^crimson-wm: //'; }
# check_config: --check-config con el entorno del WM, sin DISPLAY,
# XAUTHORITY ni token. Deja la salida en CHECK_OUT y el código en CHECK_RC.
check_config() {
    local -a env_no_x=()
    local v
    for v in "${WM_ENV[@]}"; do
        case $v in DISPLAY=* | XAUTHORITY=* | CRIMSON_SANDBOX_TOKEN=*) ;; *) env_no_x+=("$v") ;; esac
    done
    CHECK_OUT=$(timeout 10 env -i "${env_no_x[@]}" "$BIN" --check-config 2>&1)
    CHECK_RC=$?
    echo "$CASE check_config -> $CHECK_RC" >>"$RUNS"
    printf '%s\n' "$CHECK_OUT" >>"$OUT/$CASE-check.log"
}
# server_state: átomos y propiedades de la raíz, para comprobar que una
# ejecución no dejó nada en el servidor.
server_state() {
    xlsatoms | wc -l
    xprop -root | sort
}
# launches programa: registros del programa falso.
launches() { find "$LOGS" -maxdepth 1 -regex ".*/$1\.[0-9]+" | sort; }
launch_count() { launches "$1" | wc -l; }
launch_count_is() { [[ $(launch_count "$1") == "$2" ]]; }
field() { sed -n "s/^$2=//p" "$1"; }

# Estado de las ventanas y de la raíz (como en F11).
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
raw_desk() {
    local v
    v=$(xprop -id "${WIDS[$1]}" _NET_WM_DESKTOP 2>/dev/null | sed -n 's/^_NET_WM_DESKTOP(CARDINAL) = \([0-9]*\)$/\1/p')
    echo "${v:-ausente}"
}
desk_of() {
    local v
    v=$(raw_desk "$1")
    [[ $v =~ ^[0-9]+$ && $v -lt 8 ]] && echo $((v + 1)) || echo "$v"
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
focus_is() { [[ $(focus_name) == "$1" ]]; }
logged() { grep -qF -- "$1" "$WM_LOG"; }
startup_finished() { ! kill -0 "$WM_PID" 2>/dev/null || xprop -root _NET_SUPPORTING_WM_CHECK | grep -q 'window id #'; }
managed_line() { grep -F "ventana ${WIDS[$1]} gestionada" "$WM_LOG" | tail -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]* //'; }
managed_as() { [[ $(managed_line "$1") == "gestionada ($2)"* ]]; }
rules_line() { grep -F "ventana ${WIDS[$1]}: reglas de las líneas" "$WM_LOG" | tail -n1 | sed 's/^crimson-wm: ventana 0x[0-9a-f]*: //'; }
has_rules_line() { grep -qF "ventana ${WIDS[$1]}: reglas de las líneas" "$WM_LOG"; }
hidden_on() { logged "ventana ${WIDS[$1]} al escritorio $2 regla de la línea $3 (oculta)"; }
open_hidden() {
    local name=$1
    shift
    "$TEST_CLIENT" window "$name" "$@" >"$OUT/$CASE-$name.out" 2>&1 &
    PIDS[$name]=$!
    precondition "ventana $name creada" wait_for grep -q '^ventana 0x' "$OUT/$CASE-$name.out"
    WIDS[$name]=$(sed -n 's/^ventana //p' "$OUT/$CASE-$name.out" | head -n1)
}
open_dialog() {
    local name=$1 owner=${WIDS[$2]}
    shift 2
    open_window "$name" "$TEST_CLIENT" window "$name" --transient-for "$owner" "$@"
}
tree_names() {
    local id n names=''
    while read -r id; do
        n=$(name_of "$id")
        [[ $n != "$id" ]] && names="$n $names"
    done < <(xwininfo -root -children | awk '$1 ~ /^0x/ {print $1}')
    echo "${names% }"
}
order_of() {
    local n s=''
    for n in $(tree_names); do
        [[ " $* " == *" $n "* ]] && s+="$n "
    done
    echo "${s% }"
}
desk_is() { [[ $(current_desk) == "$1" ]]; }

# Capturas (A147 y A148): volcado real del Xvfb y rótulo con datos del
# servidor.
shot() {
    sleep 1
    xwd -root -silent -out "$OUT/$1.xwd" && magick "$OUT/$1.xwd" "$OUT/$1.png"
    printf 'escritorio %s · foco: %s · pila: %s\n' "$(current_desk)" "$(focus_name)" "$(order_of "${@:2}" | sed 's/ / < /g')" >"$OUT/$1.txt"
}
px() { magick "$OUT/$1.png" -format "%[hex:p{$2}]" info:; }
RED=8B0A1F BLACK=1F2328 LIGHT=D0D7DE

precondition "pantalla 1280x720" test "$SIZE" = 1280x720
FULL="0 0 1280 720"
LEFT="0 0 640 720"
RIGHT="640 0 640 720"
# Escena de la comparación: A y B normales y V (class Viewer).
scene() {
    open_tc A --background "$RED"
    open_tc B --background "$BLACK"
    open_tc V --class viewer Viewer --geometry 700 120 460 300 --background "$LIGHT"
}
SCENE_CONFIG='# A148: proporción 60 % y una regla
[layout]
master_percent = 60

[[rules]]
class = "Viewer"
floating = true
'

case $CASE in
A136)
    # Sin configuración: el comportamiento de F11.
    WM_ENV=("${BASE_ENV[@]}")
    start12
    want="configuración: predeterminada (no existe $XDG_FILE ni $HOME_FILE)"
    record A136-fuente "$want" "$(config_line)" "$(ok_if test "$(config_line)" = "$want")" "registro del WM"
    open_tc A
    open_tc B
    expect_layout A136-mosaico "A=$LEFT" "B=$RIGHT"
    key super+j
    expect A136-foco "escritorio=1 foco=A activa=A A=v B=v" A B
    key super+Return
    verdict=FALLA
    wait_for launch_count_is kitty 1 && [[ $(field "$(launches kitty)" argv) == "0:[]" ]] && verdict=OK
    record A136-lanzamiento "kitty falso, argv=0:[]" "$(launch_count kitty) lanzamientos; argv=$(field "$(launches kitty | head -n1)" argv)" "$verdict" "Mod4+Return con la tabla predeterminada"
    n=$(grep -cE "reglas|CRIMSON_RULES|WM_CLASS" "$WM_LOG")
    record A136-sin-reglas 0 "$n" "$(ok_if test "$n" = 0)" "líneas de reglas, CRIMSON_RULES o WM_CLASS en el registro del WM (como A121)"
    check_config
    want="crimson-wm: configuración predeterminada (no existe $XDG_FILE ni $HOME_FILE): válida (30 atajos, 6 programas, maestra al 50 %, 0 reglas)"
    record A136-check "0; $want" "$CHECK_RC; $CHECK_OUT" "$(ok_if test "$CHECK_RC; $CHECK_OUT" = "0; $want")" "--check-config sin DISPLAY, XAUTHORITY ni token"
    stop
    ;;
A137)
    # Configuración real: 60 %, atajos con Alt, teclas con nombre y
    # programas propios (falsos).
    write_config "$XDG_FILE" <<'EOF'
# A137 (ADR 0014)
[layout]
master_percent = 60

[programs.prueba]
argv = ["kitty", "--perfil", "$HOME", "~", "a b"]

[programs.menu]
argv = ["rofi", "-show", "run"]

[programs.silencio]
argv = ["flameshot", "mute"]

[[keybindings]]
keys = "Alt+Shift+q"
action = "exit"

[[keybindings]]
keys = "Alt+j"
action = "focus_next"

[[keybindings]]
keys = "Mod1+Prior"
action = "zoom"

[[keybindings]]
keys = "Ctrl+Alt+t"
action = "launch"
program = "prueba"

[[keybindings]]
keys = "F9"
action = "launch"
program = "menu"

[[keybindings]]
keys = "XF86AudioMute"
action = "launch"
program = "silencio"

[[keybindings]]
keys = "Alt+Shift+2"
action = "move_to_desktop"
desktop = 2

[[keybindings]]
keys = "Alt+2"
action = "view_desktop"
desktop = 2

[[keybindings]]
keys = "Alt+1"
action = "view_desktop"
desktop = 1
EOF
    WM_ENV=("${BASE_ENV[@]}")
    start12
    want="configuración: $XDG_FILE (9 atajos, 3 programas, maestra al 60 %, 0 reglas)"
    record A137-fuente "$want" "$(config_line)" "$(ok_if test "$(config_line)" = "$want")" "registro del WM"
    open_tc A
    open_tc B
    expect_layout A137-mosaico-2 "A=0 0 768 720" "B=768 0 512 720"
    open_tc C
    expect_layout A137-mosaico-3 "A=0 0 768 720" "B=768 0 512 360" "C=768 360 512 360"
    key alt+j
    expect A137-foco-alt-j "escritorio=1 foco=A activa=A A=v B=v C=v" A B C
    key super+j
    expect_stays A137-sin-super-j "escritorio=1 foco=A activa=A A=v B=v C=v" A B C
    key alt+j
    precondition "foco en B" wait_for focus_is B
    key alt+Prior
    expect_layout A137-zoom-prior "B=0 0 768 720" "A=768 0 512 360" "C=768 360 512 360"
    key ctrl+alt+t
    verdict=FALLA
    # shellcheck disable=SC2016 # $HOME debe llegar literal, sin expandir
    wait_for launch_count_is kitty 1 && [[ $(field "$(launches kitty)" argv) == '4:[--perfil][$HOME][~][a b]' ]] && verdict=OK
    # shellcheck disable=SC2016 # $HOME debe llegar literal, sin expandir
    record A137-argv-literal '4:[--perfil][$HOME][~][a b]' "$(field "$(launches kitty | head -n1)" argv)" "$verdict" "Ctrl+Alt+t: argv del programa falso, sin shell ni expansión"
    key F9
    verdict=FALLA
    wait_for launch_count_is rofi 1 && [[ $(field "$(launches rofi)" argv) == '2:[-show][run]' ]] && verdict=OK
    record A137-tecla-funcion '2:[-show][run]' "$(field "$(launches rofi | head -n1)" argv)" "$verdict" "F9 sin modificadores (keysym 0xffc6, código $(xmodmap -pke | awk '$4 == "F9" {print $2; exit}'))"
    key XF86AudioMute
    verdict=FALLA
    wait_for launch_count_is flameshot 1 && [[ $(field "$(launches flameshot)" argv) == '1:[mute]' ]] && verdict=OK
    record A137-tecla-multimedia '1:[mute]' "$(field "$(launches flameshot | head -n1)" argv)" "$verdict" "XF86AudioMute (keysym 0x1008ff12, código $(xmodmap -pke | awk '$4 == "XF86AudioMute" {print $2; exit}'))"
    # Los atajos predeterminados ya no existen.
    key super+Return super+d Print
    sleep 0.7
    got="kitty=$(launch_count kitty) rofi=$(launch_count rofi) flameshot=$(launch_count flameshot)"
    record A137-sin-predeterminados "kitty=1 rofi=1 flameshot=1" "$got" "$(ok_if test "$got" = "kitty=1 rofi=1 flameshot=1")" "Mod4+Return, Mod4+d e Impr Pant tras sustituir la tabla"
    precondition "foco en B" wait_for focus_is B
    key alt+shift+2
    expect A137-mover "escritorio=1 foco=A activa=A A=v B=o C=v" A B C
    key alt+2
    expect A137-ver "escritorio=2 foco=B activa=B A=o B=v C=o" A B C
    key alt+1
    precondition "escritorio 1" wait_for desk_is 1
    stop_with alt+shift+q
    # Atajo de control obligatorio que no se puede capturar: si dos keysyms
    # de la tabla están en el mismo código de tecla, el WM no arranca (14).
    xmodmap -e "keycode 44 = j k j k"
    write_config "$SANDBOX/colision.toml" <<'EOF'
[[keybindings]]
keys = "Alt+Shift+q"
action = "exit"

[[keybindings]]
keys = "Alt+j"
action = "focus_next"

[[keybindings]]
keys = "Alt+k"
action = "focus_prev"
EOF
    start_wm "$CASE-colision" --config "$SANDBOX/colision.toml"
    wait "$WM_PID"
    rc=$?
    echo "$CASE-colision $rc" >>"$CODES"
    msg=$(grep -m1 'código de tecla 44 de Mod1+k ya está asignado' "$WM_LOG" | sed 's/^crimson-wm: //')
    record A137-colision "14; el código de tecla 44 de Mod1+k ya está asignado" "$rc; ${msg:-sin mensaje}" "$([[ $rc == 14 && -n $msg ]] && echo OK || echo FALLA)" "xmodmap: keycode 44 = j k; Alt+j y Alt+k obligatorios"
    # Un modificador ignorado al reconocer eventos no puede formar parte
    # de un atajo: de otro modo el WM arranca sin una salida utilizable.
    xmodmap -e 'clear mod2' -e 'add mod1 = Num_Lock'
    write_config "$SANDBOX/numlock.toml" <<'EOF'
[[keybindings]]
keys = "Alt+Shift+q"
action = "exit"
EOF
    start_wm "$CASE-numlock" --config "$SANDBOX/numlock.toml"
    wait_for startup_finished
    if kill -0 "$WM_PID" 2>/dev/null; then
        kill "$WM_PID"
    fi
    wait "$WM_PID"
    rc=$?
    msg=$(grep -m1 'Mod1+Shift+q usa un modificador de Bloq Num' "$WM_LOG")
    record A137-numlock "14; modificador de Bloq Num rechazado" "$rc; ${msg:-sin mensaje}" "$([[ $rc == 14 && -n $msg ]] && echo OK || echo FALLA)" "Num_Lock en Mod1; atajo obligatorio Alt+Shift+q"
    ;;
A138)
    # Reglas de F11 desde [[rules]]: las líneas son las del archivo.
    write_config "$XDG_FILE" <<'EOF'
# A138: reglas de F11 en TOML (ADR 0014)
[[rules]]
class = "Chat"
desktop = 3

[[rules]]
class = "Viewer"
floating = true

[[rules]]
instance = "quiet"
nofocus = true

[[rules]]
class = "Chat"
desktop = 4
floating = true

[[rules]]
type = "dialog"
class = "Dlg"
desktop = 5
EOF
    WM_ENV=("${BASE_ENV[@]}")
    start12
    want="configuración: $XDG_FILE (30 atajos, 6 programas, maestra al 50 %, 5 reglas)"
    record A138-fuente "$want" "$(config_line)" "$(ok_if test "$(config_line)" = "$want")" "registro del WM"
    open_tc A
    open_hidden C --class chat Chat --geometry 200 150 500 300
    verdict=FALLA
    wait_for hidden_on C 3 2 && verdict=OK
    record A138-chat "escritorio 3 (línea 2), oculta" "$(rules_line C)" "$verdict" "registro del WM"
    verdict=FALLA
    wait_for logged "ventana ${WIDS[C]}: escritorio 4 de la regla de la línea 14 ignorado: ya lo fija una regla anterior" && verdict=OK
    record A138-primera-gana "escritorio 4 de la línea 14 ignorado" "$(grep -cF "ventana ${WIDS[C]}: escritorio 4 de la regla de la línea 14" "$WM_LOG") registros" "$verdict" "registro del WM"
    want="reglas de las líneas [2, 14]: escritorio 3 (línea 2), flotante (línea 14)"
    record A138-flotante "$want" "$(rules_line C)" "$(ok_if test "$(rules_line C)" = "$want")" "acciones de reglas distintas"
    open_tc V --class viewer Viewer --geometry 300 200 400 300
    expect_layout A138-viewer "A=$FULL" "V=300 200 400 300"
    open_tc Q --class quiet Quiet
    expect_stays A138-nofocus "escritorio=1 foco=V activa=V A=v Q=v V=v" A Q V
    open_dialog D A --class dlg Dlg --geometry 0 0 300 200
    verdict=FALLA
    wait_for logged "ventana ${WIDS[D]}: escritorio 5 de la regla de la línea 19 ignorado: diálogo de ${WIDS[A]}" && [[ $(desk_of D) == 1 ]] && verdict=OK
    record A138-dialogo "escritorio 5 (línea 19) ignorado: diálogo de A" "escritorio $(desk_of D); $(grep -F "ventana ${WIDS[D]}: escritorio 5" "$WM_LOG" | sed 's/^crimson-wm: ventana 0x[0-9a-f]*: //')" "$verdict" "el diálogo sigue a su principal"
    check_config
    want="crimson-wm: $XDG_FILE: configuración válida (30 atajos, 6 programas, maestra al 50 %, 5 reglas)"
    record A138-check "0; $want" "$CHECK_RC; $CHECK_OUT" "$(ok_if test "$CHECK_RC; $CHECK_OUT" = "0; $want")" "--check-config sin X"
    stop
    ;;
A139)
    # TOML mal formado en un Xvfb real: código 15 sin conectar.
    printf '[layout\nmaster_percent = 50\n' | write_config "$XDG_FILE"
    WM_ENV=("${BASE_ENV[@]}")
    install_fakes
    set_marker "$TOKEN"
    before=$(server_state)
    start_wm "$CASE"
    wait_wm 15 'configuración rechazada: .*wm\.toml:1:[0-9]+: TOML inválido'
    n=$(grep -c 'conectado a' "$WM_LOG")
    record A139-xvfb-sin-conexion 0 "$n" "$(ok_if test "$n" = 0)" "líneas «conectado a» en el registro del WM"
    after=$(server_state)
    record A139-xvfb-servidor-intacto "mismos átomos y propiedades de la raíz" "$(head -n1 <<<"$before") → $(head -n1 <<<"$after") átomos" "$(ok_if test "$before" = "$after")" "xlsatoms y xprop -root antes y después"
    ;;
A144)
    # Fallos de lectura en un Xvfb real: código 15 sin conectar. El permiso
    # 000 solo se puede probar fuera del espacio de nombres de isolated.sh.
    WM_ENV=("${BASE_ENV[@]}")
    install_fakes
    set_marker "$TOKEN"
    before=$(server_state)
    printf '[layout]\n' | write_config "$XDG_FILE"
    chmod 000 "$XDG_FILE"
    start_wm "$CASE-xvfb-sin-lectura"
    wait_wm 15 'no se puede abrir: Permission denied'
    chmod 660 "$XDG_FILE"
    start_wm "$CASE-xvfb-escribible"
    wait_wm 15 'lo pueden modificar el grupo u otros usuarios \(permisos 0660\)'
    rm -f "$XDG_FILE"
    # Una FIFO sin escritor bloquearía open(2): se rechaza antes de abrirla
    # (con timeout 30, un bloqueo daría 124).
    mkfifo "$XDG_FILE"
    start_wm "$CASE-xvfb-fifo"
    wait_wm 15 'no es un archivo regular: es una FIFO'
    rm -f "$XDG_FILE"
    n=$(cat "$OUT/$CASE"-xvfb-*.log | grep -c 'conectado a')
    record A144-xvfb-sin-conexion 0 "$n" "$(ok_if test "$n" = 0)" "líneas «conectado a» en los registros del WM"
    after=$(server_state)
    record A144-xvfb-servidor-intacto "mismos átomos y propiedades de la raíz" "$(head -n1 <<<"$before") → $(head -n1 <<<"$after") átomos" "$(ok_if test "$before" = "$after")" "xlsatoms y xprop -root antes y después"
    ;;
A145)
    # Precedencia de las fuentes: --config > XDG > HOME > predeterminada.
    printf '[layout]\nmaster_percent = 30\n' | write_config "$HOME_FILE"
    printf '[layout]\nmaster_percent = 40\n' | write_config "$XDG_FILE"
    printf '[layout]\nmaster_percent = 60\n' | write_config "$SANDBOX/explicita.toml"
    WM_ENV=("${BASE_ENV[@]}")
    start12 --config "$SANDBOX/explicita.toml"
    want="configuración: $SANDBOX/explicita.toml (30 atajos, 6 programas, maestra al 60 %, 0 reglas)"
    record A145-config "$want" "$(config_line)" "$(ok_if test "$(config_line)" = "$want")" "--config gana a XDG y HOME"
    open_tc A
    open_tc B
    expect_layout A145-mosaico-config "A=0 0 768 720" "B=768 0 512 720"
    stop_keep
    restart "$CASE-wm-xdg"
    want="configuración: $XDG_FILE (30 atajos, 6 programas, maestra al 40 %, 0 reglas)"
    record A145-xdg "$want" "$(config_line)" "$(ok_if test "$(config_line)" = "$want")" "XDG gana a HOME"
    expect_layout A145-mosaico-xdg "A=0 0 512 720" "B=512 0 768 720"
    stop_keep
    rm "$XDG_FILE"
    restart "$CASE-wm-home"
    want="configuración: $HOME_FILE (30 atajos, 6 programas, maestra al 30 %, 0 reglas)"
    record A145-home "$want" "$(config_line)" "$(ok_if test "$(config_line)" = "$want")" "sin archivo en XDG"
    expect_layout A145-mosaico-home "A=0 0 384 720" "B=384 0 896 720"
    stop_keep
    # XDG_CONFIG_HOME relativa: se ignora con un aviso y se usa HOME.
    WM_ENV=("${BASE_ENV[@]/#XDG_CONFIG_HOME=*/XDG_CONFIG_HOME=config-relativa}")
    restart "$CASE-wm-xdg-relativa"
    verdict=FALLA
    logged "aviso: XDG_CONFIG_HOME no es una ruta absoluta; se ignora (especificación XDG)" && [[ $(config_line) == "configuración: $HOME_FILE "* ]] && verdict=OK
    record A145-xdg-relativa "aviso; HOME" "$(grep -m1 'XDG_CONFIG_HOME' "$WM_LOG" | sed 's/^crimson-wm: //'); $(config_line)" "$verdict" "XDG_CONFIG_HOME=config-relativa"
    expect_layout A145-mosaico-xdg-relativa "A=0 0 384 720" "B=384 0 896 720"
    stop_keep
    WM_ENV=("${BASE_ENV[@]}")
    rm "$HOME_FILE"
    restart "$CASE-wm-predeterminada"
    want="configuración: predeterminada (no existe $XDG_FILE ni $HOME_FILE)"
    record A145-predeterminada "$want" "$(config_line)" "$(ok_if test "$(config_line)" = "$want")" "sin ningún archivo"
    expect_layout A145-mosaico-predeterminada "A=$LEFT" "B=$RIGHT"
    stop_keep
    # CRIMSON_RULES (obsoleta) sustituye a [[rules]], con avisos.
    printf '[[rules]]\nclass = "Chat"\ndesktop = 3\n' | write_config "$XDG_FILE"
    WM_ENV=("${BASE_ENV[@]}" "CRIMSON_RULES=class=Chat; desktop=2")
    restart "$CASE-wm-crimson-rules"
    open_hidden C --class chat Chat
    verdict=FALLA
    wait_for hidden_on C 2 1 && verdict=OK
    record A145-crimson-rules "escritorio 2 (línea 1 de CRIMSON_RULES)" "$(rules_line C)" "$verdict" "[[rules]] dice escritorio 3"
    want="aviso: CRIMSON_RULES está definida y sustituye a las 1 reglas del archivo de configuración, que no se aplican"
    record A145-sustituye "$want" "$(grep -m1 'sustituye' "$WM_LOG" | sed 's/^crimson-wm: //')" "$(ok_if logged "$want")" "registro del WM"
    want="aviso: CRIMSON_RULES está obsoleta; usa [[rules]] en crimson/wm.toml (ADR 0014)"
    record A145-obsoleta "$want" "$(grep -m1 'obsoleta' "$WM_LOG" | sed 's/^crimson-wm: //')" "$(ok_if logged "$want")" "registro del WM"
    stop_keep
    # Inválida: como en F11, sin reglas; tampoco las del archivo.
    WM_ENV=("${BASE_ENV[@]}" "CRIMSON_RULES=class=Chat; desktop=9")
    restart "$CASE-wm-crimson-rules-invalida"
    want="CRIMSON_RULES inválida (línea 1: escritorio «9» fuera de 1–8): se continúa sin reglas"
    record A145-crimson-rules-invalida "$want" "$(grep -m1 'CRIMSON_RULES' "$WM_LOG" | sed 's/^crimson-wm: //')" "$(ok_if logged "$want")" "la primera línea sobre CRIMSON_RULES es la de F11"
    open_tc E --class chat Chat
    sleep 0.5
    verdict=FALLA
    ! has_rules_line E && [[ $(desk_of E) == 1 ]] && verdict=OK
    record A145-invalida-sin-reglas "escritorio 1, sin reglas" "escritorio $(desk_of E); reglas: $(rules_line E)" "$verdict" "ni CRIMSON_RULES ni [[rules]]"
    stop
    ;;
A146)
    # Salvaguardas con configuración, en un Xvfb real.
    printf '[layout]\nmaster_percent = 60\n' | write_config "$XDG_FILE"
    WM_ENV=("${BASE_ENV[@]}")
    install_fakes
    # Configuración válida y marca de otro token: la capa 2 lo rechaza (10).
    set_marker "otro-token"
    start_wm "$CASE-marca-invalida"
    wait_wm 10 'marca de sandbox'
    # Configuración inválida y marca de otro token: la configuración se
    # rechaza antes de conectar, y por tanto antes de mirar la marca (15).
    printf '[layout\n' | write_config "$XDG_FILE"
    start_wm "$CASE-orden"
    wait_wm 15 'TOML inválido'
    n=$(grep -c 'conectado a' "$WM_LOG")
    precondition "A146-orden sin conexión" test "$n" = 0
    printf '[layout]\nmaster_percent = 60\n' | write_config "$XDG_FILE"
    before=$(server_state)
    check_config
    want="crimson-wm: $XDG_FILE: configuración válida (30 atajos, 6 programas, maestra al 60 %, 0 reglas)"
    record A146-check-en-xvfb "0; $want" "$CHECK_RC; $CHECK_OUT" "$(ok_if test "$CHECK_RC; $CHECK_OUT" = "0; $want")" "--check-config con un servidor X disponible"
    after=$(server_state)
    n=$(grep -c 'conectado a' "$OUT/$CASE-check.log")
    record A146-check-sin-conexion "sin conexión; servidor intacto" "$n líneas «conectado a»; $(head -n1 <<<"$before") → $(head -n1 <<<"$after") átomos" "$([[ $n == 0 && $before == "$after" ]] && echo OK || echo FALLA)" "xlsatoms y xprop -root antes y después"
    ;;
A147)
    # Panel 1 de la comparación: sin configuración.
    WM_ENV=("${BASE_ENV[@]}")
    start12
    scene
    expect_layout A147-mosaico "A=$LEFT" "B=640 0 640 360" "V=640 360 640 360"
    shot "$CASE-1" A B V
    colors="$(px "$CASE-1" 100,300) $(px "$CASE-1" 900,100) $(px "$CASE-1" 900,600)"
    want="$RED $BLACK $LIGHT"
    record A147-colores "$want" "$colors" "$(ok_if test "$colors" = "$want")" "A, B y V en mosaico al 50 %"
    stop
    ;;
A148)
    # Panel 2: master_percent = 60 y Viewer flotante; composición.
    write_config "$XDG_FILE" <<<"$SCENE_CONFIG"
    WM_ENV=("${BASE_ENV[@]}")
    start12
    scene
    expect_layout A148-mosaico "A=0 0 768 720" "B=768 0 512 720" "V=700 120 460 300"
    shot "$CASE-2" A B V
    colors="$(px "$CASE-2" 100,300) $(px "$CASE-2" 1000,600) $(px "$CASE-2" 900,250)"
    want="$RED $BLACK $LIGHT"
    record A148-colores "$want" "$colors" "$(ok_if test "$colors" = "$want")" "A al 60 %, B en la pila, V flotante encima"
    first=$OUT/A147-1.png
    if [[ ! -s $first ]]; then
        for id in A148-dimensiones A148-composicion; do
            record "$id" "dos capturas" "falta la de A147" OMITIDA "A147 no produjo su captura"
        done
    else
        cp "$first" "$OUT/$CASE-1.png"
        cp "$OUT/A147-1.txt" "$OUT/$CASE-1.txt"
        dims=$(for i in 1 2; do magick identify -format '%wx%h ' "$OUT/$CASE-$i.png"; done)
        record A148-dimensiones "1280x720 1280x720" "${dims% }" "$(ok_if test "${dims% }" = "1280x720 1280x720")" "magick identify de las dos capturas"
        labels=("1 · Sin wm.toml: valores predeterminados (F11)" "2 · wm.toml: master_percent = 60 y Viewer flotante")
        for i in 1 2; do
            magick "$OUT/$CASE-$i.png" -bordercolor '#1F2328' -border 2 -background white \
                -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 34 -fill '#1F2328' \
                -annotate +0+14 "${labels[i - 1]}" -font DejaVu-Sans -pointsize 26 -fill '#57606A' \
                -annotate +0+62 "$(cat "$OUT/$CASE-$i.txt")" +repage "$OUT/$CASE-panel-$i.png"
        done
        magick "$OUT/$CASE-panel-1.png" "$OUT/$CASE-panel-2.png" \
            -background white -splice 40x0 +append +repage -gravity east -splice 40x0 +repage \
            -gravity north -splice 0x110 -font DejaVu-Sans-Bold -pointsize 46 -fill '#8B0A1F' \
            -annotate +0+28 "Crimson WM · F12: configuración TOML (capturas reales en Xvfb)" \
            -gravity south -splice 0x80 -font DejaVu-Sans -pointsize 28 -fill '#57606A' \
            -annotate +0+24 "Mismo binario. A: rojo · B: negro · V (class Viewer): gris claro. Volcados de xwd -root, sin retoques." \
            +repage "$OUT/$CASE-comparison.png"
        same=''
        for i in 1 2; do
            x=$((40 + (i - 1) * (1284 + 40) + 2))
            magick "$OUT/$CASE-comparison.png" -crop "1280x720+$x+222" +repage "$OUT/$CASE-check-$i.png"
            same+="$(magick compare -metric AE "$OUT/$CASE-$i.png" "$OUT/$CASE-check-$i.png" null: 2>&1) "
        done
        record A148-composicion "0 0" "${same% }" "$(ok_if test "${same% }" = "0 0")" "píxeles distintos entre cada captura y su panel ($(magick identify -format '%wx%h' "$OUT/$CASE-comparison.png")); rótulos: $(cat "$OUT/$CASE"-[12].txt | paste -sd'|')"
        sha256sum "$OUT/$CASE"-[12].png "$OUT/$CASE-comparison.png" | sed "s#$OUT/##" >"$OUT/$CASE-sha256.txt"
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
