#!/bin/bash
# Programa falso de las pruebas de aceptación (ADR 0006). xvfb-case.sh lo
# copia con el nombre de cada aplicación (kitty, rofi, thunar, firefox,
# flameshot y nvim) en el único directorio del PATH del WM; nunca se instala
# en ningún directorio de la sesión.
#
# Registra cómo lo lanzó el WM en ../lanzamientos/NOMBRE.PID y termina, o
# espera los segundos indicados en ../lanzamientos/NOMBRE.espera. No ejecuta
# ningún programa por su nombre: solo órdenes internas de bash y ls, mv y
# sleep por su ruta absoluta.
set -u
name=${0##*/}
log=${0%/*}/../lanzamientos
record=$log/$name.$$

# Descriptores abiertos, antes de abrir ninguno más: ls es un proceso hijo y
# los lee de /proc sin modificarlos.
/usr/bin/ls -l "/proc/$$/fd" >"$record.fds"
# pid (nombre) estado ppid pgid ...
read -r -a stat <"/proc/$$/stat"
sigign=
while read -r key value; do
    [[ $key == SigIgn: ]] && sigign=$value
done <"/proc/$$/status"
presence() { if [[ -v $1 ]]; then echo presente; else echo ausente; fi; }
{
    # Número de argumentos y cada uno entre corchetes («0:[]» sin ninguno).
    printf 'argv=%s:' "$#"
    printf '[%s]' "$@"
    printf '\n'
    printf 'pid=%s\nppid=%s\npgid=%s\n' "$$" "${stat[3]}" "${stat[4]}"
    printf 'cwd='
    pwd -P
    printf 'sigign=%s\n' "$sigign"
    printf 'token=%s\n' "$(presence CRIMSON_SANDBOX_TOKEN)"
    printf 'windowid=%s\n' "$(presence WINDOWID)"
    printf 'startup=%s\n' "$(presence DESKTOP_STARTUP_ID)"
    printf 'display=%s\n' "${DISPLAY-ausente}"
    printf 'xauthority=%s\n' "${XAUTHORITY-ausente}"
    printf 'path=%s\n' "${PATH-ausente}"
    printf 'home=%s\n' "${HOME-ausente}"
    printf 'xdg_runtime_dir=%s\n' "${XDG_RUNTIME_DIR-ausente}"
    printf 'dbus=%s\n' "${DBUS_SESSION_BUS_ADDRESS-ausente}"
} >"$record.tmp"
# Salida y errores heredados del WM: aparecen en su registro.
printf 'programa falso %s: pid %s\n' "$name" "$$"
printf 'programa falso %s: salida de errores\n' "$name" >&2

hold=0
[[ -r $log/$name.espera ]] && read -r hold <"$log/$name.espera"
[[ $hold =~ ^[0-9]+$ ]] || hold=0
# El registro aparece completo, de una vez.
/usr/bin/mv "$record.tmp" "$record"
if ((hold > 0)); then
    # Cualquier señal recibida queda registrada.
    for sig in HUP INT QUIT TERM USR1 USR2; do
        # shellcheck disable=SC2064 # se expande ahora a propósito
        trap "printf '%s\n' $sig >>'$record.senales'; exit 1" "$sig"
    done
    /usr/bin/sleep "$hold" &
    wait "$!"
fi
exit 0
