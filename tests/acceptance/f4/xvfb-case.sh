#!/bin/bash
# Una prueba de aceptación de F4 (ADR 0006) dentro de un Xvfb nuevo. run.sh
# lo ejecuta como orden de `xvfb-run` (Debian 13), que fija DISPLAY y
# XAUTHORITY. Las guardas, la preparación y el entorno aislado del WM son
# los de F1 (xvfb-env.sh) y las funciones de ventanas, las de F2
# (windows.sh).
#
# Los programas falsos (fake.sh) se copian en $SANDBOX/bin, el único
# directorio del PATH del WM, y registran cada lanzamiento en $LOGS.
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

LOGS=$SANDBOX/lanzamientos
# Ningún programa falso sobrevive a la prueba, aunque esta se interrumpa.
trap 'rc=$?; stop_fakes; (exit $rc); on_exit' EXIT

# install_fakes aplicación...: copia fake.sh con esos nombres.
install_fakes() {
    local app
    for app in "$@"; do
        cp "$FAKE" "$SANDBOX/bin/$app" && chmod 755 "$SANDBOX/bin/$app"
    done
}
# records aplicación: registros completos de sus lanzamientos (NOMBRE.PID).
records() {
    local f
    for f in "$LOGS/$1".*; do
        [[ ${f##*.} =~ ^[0-9]+$ ]] && echo "$f"
    done
}
has_record() { [[ -n $(records "$1") ]]; }
count() { records "$1" | wc -l; }
total() {
    local app n=0
    for app in "${APPS[@]}"; do n=$((n + $(count "$app"))); done
    echo "$n"
}
field() { sed -n "s/^$2=//p" "$1"; }
# launched_with aplicación argv: algún lanzamiento con ese argv exacto
# (formato de fake.sh: «N:[arg1][arg2]»).
launched_with() {
    local f
    while read -r f; do
        grep -qxF "argv=$2" "$f" && return 0
    done < <(records "$1")
    return 1
}
argvs() {
    local f
    while read -r f; do field "$f" argv; done < <(records "$1") | paste -sd';'
}
# stop_fakes: termina los programas falsos de esta prueba que sigan en
# marcha, con todo su grupo de procesos. Solo si el pid sigue siendo uno de
# ellos (por su línea de órdenes).
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
# Hijos e hilos del proceso del WM (no el de timeout).
children() { ps -o pid= --ppid "$WM_REAL" | paste -sd' '; }
threads() {
    local t=("/proc/$WM_REAL/task"/*)
    echo "${#t[@]}"
}
key() { xdotool key "$@"; }
focused() { [[ $(xdotool getwindowfocus 2>/dev/null) == "$((WIDS[$1]))" ]]; }
open_xev() { open_window "$1" xev -name "$1"; }
ok_if() { if "$@"; then echo OK; else echo FALLA; fi; }

# start4: arranca el WM, como start, y registra la evidencia de aislamiento:
# cómo se resuelve cada aplicación con su PATH y qué variables tiene (solo
# los nombres; los valores no se muestran).
start4() {
    local app found summary='' names expected verdict=FALLA
    start
    WM_REAL=$(pgrep -P "$WM_PID" -x crimson-wm)
    precondition "pid del WM" test -n "$WM_REAL"
    for app in "${APPS[@]}"; do
        found=$(resolve "$app")
        if [[ -z $found ]]; then
            summary+="$app=ninguno "
        elif [[ $found == "$SANDBOX/bin/$app" ]] && cmp -s "$found" "$FAKE"; then
            summary+="$app=falso "
        else
            summary+="$app=¡$found! "
        fi
    done
    names=$(tr '\0' '\n' <"/proc/$WM_REAL/environ" | cut -d= -f1 | sort | paste -sd' ')
    expected=$(printf '%s\n' "${WM_ENV[@]%%=*}" | sort | paste -sd' ')
    isolated && [[ $names == "$expected" ]] &&
        tr '\0' '\n' <"/proc/$WM_REAL/environ" | grep -qxF "PATH=$SANDBOX/bin" && verdict=OK
    record "$CASE-aislamiento" "solo programas falsos; PATH=\$SANDBOX/bin; entorno mínimo" "${summary% }" "$verdict" "variables del WM (pid $WM_REAL): $names"
}
# check_launch id teclas aplicación argv: pulsa el atajo y espera el
# lanzamiento del programa falso con ese argv exacto.
check_launch() {
    local verdict=FALLA
    key "$2"
    wait_for launched_with "$3" "$4" && kill -0 "$WM_PID" && verdict=OK
    record "$1" "$3 $4" "$3 $(argvs "$3")" "$verdict" "$2; registro del programa falso, WM en marcha"
}

precondition "pantalla 1280x720" test "$SIZE" = 1280x720

case $CASE in
A33)
    install_fakes "${APPS[@]}"
    start4
    open_xev W
    precondition "foco en W" wait_for focused W
    check_launch A33-terminal super+Return kitty "0:[]"
    check_launch A33-lanzador super+d rofi "2:[-show][drun]"
    check_launch A33-archivos super+e thunar "0:[]"
    check_launch A33-navegador super+b firefox "0:[]"
    check_launch A33-editor super+n kitty "2:[-e][nvim]"
    check_launch A33-capturas Print flameshot "1:[gui]"
    # Los seis terminan enseguida y el WM los recoge.
    all_done() { [[ $(grep -c 'terminó con el código 0' "$WM_LOG") == 6 ]]; }
    verdict=FALLA
    wait_for all_done && [[ $(total) == 6 && $(grep -c '» lanzado (pid' "$WM_LOG") == 6 ]] && verdict=OK
    record A33-total "6 lanzamientos, 6 terminados" "$(total) registros; $(grep -c '» lanzado (pid' "$WM_LOG") lanzados, $(grep -c 'terminó con el código 0' "$WM_LOG") terminados" "$verdict" "registros de los programas falsos y del WM"
    # Ninguna de las teclas capturadas llega a la ventana enfocada; sí el
    # modificador (Super_L, código 133).
    leaked=$(grep -cE 'keycode (36|40|26|56|57|107|218) ' "$OUT/$CASE-W.out")
    record A33-teclas-capturadas 0 "$leaked" "$([[ $leaked == 0 ]] && echo OK || echo FALLA)" "eventos de Return, d, e, b, n y Print en el registro de xev de W; Super_L: $(grep -c 'keycode 133 ' "$OUT/$CASE-W.out")"
    stop
    ;;
A34)
    install_fakes "${APPS[@]}"
    # Variables que el WM tiene y no debe pasar a los hijos.
    WM_ENV+=(WINDOWID=4242 DESKTOP_STARTUP_ID=crimson-prueba)
    start4
    wm_env_has() { tr '\0' '\n' <"/proc/$WM_REAL/environ" | grep -qx "$1"; }
    verdict=FALLA
    wm_env_has "DISPLAY=:$N" && wm_env_has 'CRIMSON_SANDBOX_TOKEN=.*' && wm_env_has 'WINDOWID=.*' &&
        wm_env_has 'DESKTOP_STARTUP_ID=.*' && verdict=OK
    record A34-entorno-del-wm "DISPLAY=:$N, con token, WINDOWID y DESKTOP_STARTUP_ID" "$([[ $verdict == OK ]] && echo coincide || echo no coincide)" "$verdict" "/proc/$WM_REAL/environ (solo se comparan; no se muestran los valores)"
    key super+Return
    precondition "terminal lanzada" wait_for has_record kitty
    R=$(records kitty | head -n1)
    pid=$(field "$R" pid)
    fd_target() { awk -v fd="$1" '$(NF-1) == "->" && $(NF-2) == fd {print $NF}' "$R.fds"; }

    got=$(field "$R" token)
    record A34-token ausente "$got" "$(ok_if test "$got" = ausente)" "CRIMSON_SANDBOX_TOKEN en el hijo"
    got=$(field "$R" display)
    record A34-display ":$N.0" "$got" "$(ok_if test "$got" = ":$N.0")" "DISPLAY del hijo, fijado desde el Target validado (el WM tiene :$N)"
    if [[ $(field "$R" xauthority) == "$XAUTHORITY" ]]; then got="la cookie temporal de xvfb-run"; else got="otra"; fi
    record A34-xauthority "la cookie temporal de xvfb-run" "$got" "$(ok_if test "$got" = "la cookie temporal de xvfb-run")" "XAUTHORITY heredado (ruta comparada, no mostrada; el contenido nunca se lee)"
    got="WINDOWID $(field "$R" windowid), DESKTOP_STARTUP_ID $(field "$R" startup)"
    record A34-variables-eliminadas "WINDOWID ausente, DESKTOP_STARTUP_ID ausente" "$got" "$(ok_if test "$got" = "WINDOWID ausente, DESKTOP_STARTUP_ID ausente")" "entorno del hijo"
    verdict=FALLA
    [[ $(field "$R" path) == "$SANDBOX/bin" && $(field "$R" home) == "$SANDBOX/home" &&
        $(field "$R" xdg_runtime_dir) == "$SANDBOX/runtime" &&
        $(field "$R" dbus) == "unix:path=$SANDBOX/runtime/sin-bus" ]] && verdict=OK
    record A34-entorno-aislado "PATH, HOME, XDG_RUNTIME_DIR y D-Bus de la prueba" "PATH=$(field "$R" path); HOME=$(field "$R" home); XDG_RUNTIME_DIR=$(field "$R" xdg_runtime_dir); D-Bus=$(field "$R" dbus)" "$verdict" "heredados del WM sin cambios; el bus no existe: $([[ -e $SANDBOX/runtime/sin-bus ]] && echo existe || echo no existe)"
    got=$(fd_target 0)
    record A34-stdin /dev/null "$got" "$(ok_if test "$got" = /dev/null)" "descriptor 0 del hijo"
    verdict=FALLA
    out_line=$(grep -c "programa falso kitty: pid $pid" "$WM_LOG")
    err_line=$(grep -c "programa falso kitty: salida de errores" "$WM_LOG")
    [[ $(fd_target 1) == "$WM_LOG" && $(fd_target 2) == "$WM_LOG" && $out_line == 1 && $err_line == 1 ]] && verdict=OK
    record A34-salida "1 y 2 heredados del WM" "1 -> $(basename "$(fd_target 1)"), 2 -> $(basename "$(fd_target 2)")" "$verdict" "líneas del hijo en $WM_ID.log: salida $out_line, errores $err_line"
    fds=$(awk '$(NF-1) == "->" {print $(NF-2)}' "$R.fds" | sort -n | paste -sd' ')
    sockets=$(grep -c 'socket:' "$R.fds")
    verdict=FALLA
    [[ $fds == "0 1 2 255" && $sockets == 0 ]] && verdict=OK
    record A34-descriptores "0 1 2 255, sin sockets" "$fds; sockets: $sockets" "$verdict" "ls -l /proc/PID/fd en el hijo (255: el propio script de bash); 255 -> $(basename "$(fd_target 255)")"
    got=$(field "$R" cwd)
    want=$(cd "$SANDBOX/home" && pwd -P)
    record A34-cwd "HOME del WM" "$got" "$(ok_if test "$got" = "$want")" "pwd -P en el hijo"
    read -r -a wm_stat <"/proc/$WM_REAL/stat"
    pgid=$(field "$R" pgid)
    verdict=FALLA
    [[ $pgid == "$pid" && $pgid != "${wm_stat[4]}" ]] && verdict=OK
    record A34-grupo "grupo propio (pgid = pid)" "pid $pid, pgid $pgid" "$verdict" "grupo del WM: ${wm_stat[4]}"
    got=$(field "$R" ppid)
    record A34-padre "$WM_REAL" "$got" "$(ok_if test "$got" = "$WM_REAL")" "ppid del hijo: el proceso del WM"
    sigign=$(field "$R" sigign)
    verdict=FALLA
    [[ $sigign =~ ^[0-9a-f]{16}$ ]] && (((16#$sigign & 0x1000) == 0)) && verdict=OK
    record A34-sigpipe "SIGPIPE no ignorada" "SigIgn $sigign" "$verdict" "señal 13 (SIGPIPE, máscara 0x1000) en SigIgn de /proc/PID/status; el WM sí la ignora (Rust)"
    stop
    ;;
A35)
    # Sin firefox, thunar sin permiso de ejecución y sin ninguna tecla Print.
    install_fakes kitty rofi thunar flameshot nvim
    chmod 644 "$SANDBOX/bin/thunar"
    xmodmap -e 'keycode 107 =' -e 'keycode 218 ='
    no_print() { ! xmodmap -pke | grep -qw Print; }
    precondition "ninguna tecla produce Print" no_print
    precondition "firefox no se resuelve" test -z "$(resolve firefox)"
    start4
    warning='aviso: ningún código de tecla produce la tecla de Print; atajo desactivado'
    verdict=FALLA
    grep -qF "$warning" "$WM_LOG" && kill -0 "$WM_PID" && verdict=OK
    record A35-sin-tecla "aviso y WM en marcha" "$(grep -cF "$warning" "$WM_LOG") avisos" "$verdict" "$(grep -m1 -oF "$warning" "$WM_LOG")"
    # check_error id teclas aplicación nombre
    check_error() {
        local pattern="no se pudo lanzar «$4» ($3): " verdict=FALLA
        key "$2"
        wait_for grep -qF "$pattern" "$WM_LOG" && sleep 0.3 && kill -0 "$WM_PID" &&
            ! has_record "$3" && verdict=OK
        record "$1" "error registrado, WM en marcha, sin lanzamiento" "$(grep -m1 -F "$pattern" "$WM_LOG" | sed 's/^crimson-wm: //')" "$verdict" "$2; registros de $3: $(count "$3")"
    }
    check_error A35-inexistente super+b firefox navegador
    check_error A35-sin-permiso super+e thunar archivos
    open_xev A
    open_xev B
    precondition "foco en B" wait_for focused B
    key super+j
    verdict=FALLA
    wait_for focused A && kill -0 "$WM_PID" && verdict=OK
    record A35-sigue-en-marcha "Mod4+j enfoca A" "foco en $( focused A && echo A || echo otra)" "$verdict" "xdotool getwindowfocus tras los errores de lanzamiento"
    check_launch A35-sigue-lanzando super+d rofi "2:[-show][drun]"
    stop
    ;;
A36)
    install_fakes "${APPS[@]}"
    echo 5 >"$LOGS/kitty.espera"
    start4
    key super+Return
    precondition "terminal lanzada" wait_for has_record kitty
    K=$(field "$(records kitty | head -n1)" pid)
    verdict=FALLA
    kill -0 "$K" && [[ $(threads) == 2 ]] && verdict=OK
    record A36-hijo-en-marcha "hijo vivo; 2 hilos" "pid $K; $(threads) hilos" "$verdict" "el hijo espera 5 s; /proc/PID/task del WM"
    open_xev A
    open_xev B
    expect_layout A36-mosaico "A=0 0 640 720" "B=640 0 640 720"
    key super+j
    verdict=FALLA
    wait_for focused A && kill -0 "$WM_PID" && verdict=OK
    record A36-navegacion "Mod4+j enfoca A" "foco en $(focused A && echo A || echo otra)" "$verdict" "xdotool getwindowfocus"
    check_launch A36-otro-lanzamiento super+d rofi "2:[-show][drun]"
    verdict=FALLA
    kill -0 "$K" && verdict=OK
    record A36-hijo-sigue "vivo durante las comprobaciones" "$(kill -0 "$K" && echo vivo || echo terminado)" "$verdict" "el bucle X11 atendió ventanas, foco y otro lanzamiento mientras el hijo seguía en marcha"
    stop
    ;;
A37)
    install_fakes "${APPS[@]}"
    echo 2 >"$LOGS/kitty.espera"
    start4
    key super+Return
    precondition "terminal lanzada" wait_for has_record kitty
    K=$(field "$(records kitty | head -n1)" pid)
    verdict=FALLA
    kill -0 "$K" && [[ $(children) == "$K" && $(threads) == 2 ]] && verdict=OK
    record A37-supervision "1 hijo, 2 hilos" "hijos: $(children); hilos: $(threads)" "$verdict" "ps --ppid y /proc/PID/task del WM mientras el hijo vive"
    key super+d
    key super+e
    reaped() { ! kill -0 "$K" 2>/dev/null && [[ -z $(children) && $(threads) == 1 ]] && [[ $(grep -c 'terminó con el código 0' "$WM_LOG") == 3 ]]; }
    verdict=FALLA
    wait_for reaped && verdict=OK
    record A37-sin-zombis "sin hijos ni zombis; 1 hilo" "hijos: $(children); hilos: $(threads); terminados: $(grep -c 'terminó con el código 0' "$WM_LOG")" "$verdict" "ps --ppid (incluye zombis) y /proc/PID/task del WM"
    # Límite de 64: 66 pulsaciones de los seis atajos por turno, cada atajo
    # a más de 300 ms del anterior; todos los hijos esperan 30 s.
    for app in "${APPS[@]}"; do echo 30 >"$LOGS/$app.espera"; done
    before=$(total)
    keys=(super+Return super+d super+e super+b super+n Print)
    for ((i = 0; i < 66; i++)); do
        key "${keys[i % 6]}"
        sleep 0.06
    done
    at_limit() {
        [[ $(($(total) - before)) == 64 && $(grep -c 'ya hay 64 programas en supervisión' "$WM_LOG") == 2 ]]
    }
    verdict=FALLA
    wait_for at_limit && sleep 0.3 && at_limit && [[ $(children | wc -w) == 64 && $(threads) == 65 ]] && verdict=OK
    record A37-limite "64 lanzados, 2 rechazados" "$(($(total) - before)) lanzados, $(grep -c 'ya hay 64 programas en supervisión' "$WM_LOG") rechazados; hijos: $(children | wc -w); hilos: $(threads)" "$verdict" "$(grep -m1 -o 'no se lanza «[a-z]*»: ya hay 64 programas en supervisión' "$WM_LOG")"
    stop_fakes
    verdict=FALLA
    all_reaped() { [[ -z $(children) && $(threads) == 1 ]]; }
    wait_for all_reaped && kill -0 "$WM_PID" && verdict=OK
    record A37-recogidos-tras-limite "sin hijos; 1 hilo" "hijos: $(children | wc -w); hilos: $(threads)" "$verdict" "tras enviar SIGTERM a los 64 grupos; terminados con el código 1 (trap del programa falso): $(grep -c 'terminó con el código 1' "$WM_LOG")"
    stop
    ;;
A38)
    install_fakes "${APPS[@]}"
    start4
    open_xev W
    precondition "foco en W" wait_for focused W
    # Control: Xvfb repite una tecla mantenida (a, sin captura, llega a W).
    xdotool keydown a
    sleep 1.5
    xdotool keyup a
    repeats=$(grep -c 'keycode 38 (keysym 0x61, a)' "$OUT/$CASE-W.out")
    record A38-control-autorrepeticion "más de 2 eventos" "$repeats eventos de a en W" "$( ((repeats > 2)) && echo OK || echo FALLA)" "xdotool keydown a durante 1,5 s (retardo 660 ms, 25 Hz)"
    key super+Return super+Return
    sleep 0.5
    got=$(count kitty)
    record A38-doble-pulsacion 1 "$got" "$(ok_if test "$got" = 1)" "dos pulsaciones seguidas de Mod4+Return (xdotool, 12 ms)"
    key super+Return
    verdict=FALLA
    wait_for test "$(count kitty)" = 2 && verdict=OK
    record A38-pasado-el-intervalo 2 "$(count kitty)" "$verdict" "tercera pulsación, 0,5 s después"
    sleep 0.4
    xdotool keydown super+Return
    sleep 1.5
    xdotool keyup super+Return
    sleep 0.5
    got=$(count kitty)
    record A38-tecla-mantenida 3 "$got" "$(ok_if test "$got" = 3)" "Mod4+Return mantenido 1,5 s: una sola pulsación más; Return en W: $(grep -c 'keycode 36 ' "$OUT/$CASE-W.out")"
    stop
    ;;
A39)
    install_fakes "${APPS[@]}"
    echo 30 >"$LOGS/kitty.espera"
    start4
    open_xev A
    key super+Return
    precondition "terminal lanzada" wait_for has_record kitty
    R=$(records kitty | head -n1)
    K=$(field "$R" pid)
    xdotool key super+shift+Escape
    wait_wm 0 'atajo de salida recibido'
    sleep 0.5
    read -r -a k_stat <"/proc/$K/stat"
    verdict=FALLA
    kill -0 "$K" && [[ ${k_stat[4]} == "$K" && ${k_stat[3]} != "$WM_REAL" ]] && verdict=OK
    record A39-hijo-sobrevive "vivo, en su grupo, adoptado por otro proceso" "pid $K vivo: $(kill -0 "$K" && echo sí || echo no); pgid ${k_stat[4]}; padre ${k_stat[3]} ($(ps -o comm= -p "${k_stat[3]}"))" "$verdict" "tras la salida del WM (pid $WM_REAL)"
    verdict=FALLA
    [[ ! -e $R.senales ]] && verdict=OK
    record A39-sin-senales ninguna "$(paste -sd' ' "$R.senales" 2>/dev/null || echo ninguna)" "$verdict" "señales registradas por el programa falso (HUP, INT, QUIT, TERM, USR1, USR2)"
    verdict=FALLA
    kill -0 "${PIDS[A]}" && xwininfo -id "${WIDS[A]}" >/dev/null 2>&1 && verdict=OK
    record A39-clientes "xev A en marcha" "$(kill -0 "${PIDS[A]}" && echo en marcha || echo terminado)" "$verdict" "otro cliente del servidor, tras la salida del WM"
    kill "${PIDS[@]}" 2>/dev/null
    ;;
*)
    echo "caso desconocido: $CASE"
    exit 2
    ;;
esac

if [[ -n ${WM_LOG:-} ]]; then
    # Cada proceso que el WM lanzó es un programa falso que dejó su registro.
    n=0 unknown=
    while read -r pid; do
        n=$((n + 1))
        compgen -G "$LOGS/*.$pid" >/dev/null || unknown+=" $pid"
    done < <(grep -o '» lanzado (pid [0-9]*)' "$WM_LOG" | grep -o '[0-9]*')
    record "$CASE-extra-solo-falsos" "todos con registro de un programa falso" "$n lanzados; sin registro:${unknown:- ninguno}" "$([[ -z $unknown ]] && echo OK || echo FALLA)" "líneas «lanzado» de $WM_ID.log y $LOGS"
    # Ningún error X debe haberse registrado durante la ejecución del WM.
    xerrors=$(grep -c 'error X ignorado' "$WM_LOG")
    record "$CASE-extra-sin-errores-x" 0 "$xerrors" "$([[ $xerrors == 0 ]] && echo OK || echo FALLA)" "líneas «error X ignorado» en $WM_ID.log"
fi
