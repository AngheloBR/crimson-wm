# shellcheck shell=bash
# Funciones comunes de las pruebas de aceptación de F1 (ADR 0003).
# Requiere BIN (binario de crimson-wm) y OUT (directorio de resultados).

RESULTS=$OUT/results.tsv
CODES=$OUT/codes.txt
# shellcheck disable=SC2034 # la usan los scripts que cargan lib.sh
RUNS=$OUT/runs.txt

# record id esperado obtenido veredicto evidencia
# Veredictos: OK, FALLA u OMITIDA (no ejecutada, con el motivo como evidencia).
record() {
    printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" >>"$RESULTS"
}

# finish id código_esperado código_obtenido patrón_ERE log
# Solo es OK si coinciden el código de salida del programa y el mensaje.
finish() {
    local id=$1 want=$2 got=$3 pattern=$4 log=$5 verdict=FALLA ev
    echo "$id $got" >>"$CODES"
    ev=$(grep -m1 -E -- "$pattern" "$log")
    [[ $got == "$want" && -n $ev ]] && verdict=OK
    [[ -n $ev ]] || ev="(patrón no encontrado) $(head -n1 "$log")"
    record "$id" "$want" "$got" "$verdict" "$ev"
}

# run_bin id código patrón [argumentos de env...] [-- argumentos del binario]
run_bin() {
    local id=$1 want=$2 pattern=$3
    shift 3
    local envargs=() binargs=()
    while (($#)); do
        if [[ $1 == -- ]]; then
            shift
            binargs=("$@")
            break
        fi
        envargs+=("$1")
        shift
    done
    local log=$OUT/$id.log
    env "${envargs[@]}" timeout 10 "$BIN" "${binargs[@]}" >"$log" 2>&1
    finish "$id" "$want" "$?" "$pattern" "$log"
}

# start_wm id: arranca el WM en segundo plano (WM_ID, WM_PID, WM_LOG) con
# `env -i` y solo las variables de WM_ENV (xvfb-env.sh), después de
# comprobar que su PATH no puede resolver ninguna aplicación real. WM_PID es
# el de `timeout`; el WM es su único hijo.
start_wm() {
    WM_ID=$1
    WM_LOG=$OUT/$1.log
    precondition "PATH del WM aislado" isolated
    timeout 30 env -i "${WM_ENV[@]}" "$BIN" >"$WM_LOG" 2>&1 &
    WM_PID=$!
}

# wait_wm código patrón: espera a que el WM termine y comprueba su salida.
wait_wm() {
    wait "$WM_PID"
    finish "$WM_ID" "$1" "$?" "$2" "$WM_LOG"
}

# wait_for orden... : reintenta durante 5 s hasta que la orden tenga éxito.
wait_for() {
    local i
    for ((i = 0; i < 50; i++)); do
        "$@" >/dev/null 2>&1 && return 0
        sleep 0.1
    done
    return 1
}

# output_is valor orden...: la salida de la orden (sin los saltos de línea
# finales) es exactamente el valor. Con wait_for, la orden se vuelve a
# ejecutar en cada intento; `wait_for test "$(orden)" = valor` evaluaría la
# sustitución una sola vez, antes de esperar (ADR 0010, F8-B.2).
output_is() {
    local want=$1
    shift
    [[ $("$@") == "$want" ]]
}

# Comprobaciones sobre la sesión principal y el entorno (A9 en F1). Solo
# lecturas que no conectan con ningún servidor X. Requieren OUT y TMP; WATCH
# lista los procesos de la sesión que deben conservar su PID.
WATCH=${CRIMSON_F1_WATCH:-bspwm sxhkd}
TMP=${TMPDIR:-/tmp}
snapshot() {
    local p
    for p in $WATCH; do pgrep -x "$p" >"$OUT/$p.$1"; done
    pgrep -x Xvfb | sort >"$OUT/xvfb.$1"
    find /tmp -maxdepth 1 -name '.X*-lock' | sort >"$OUT/locks.$1"
    find /tmp/.X11-unix -mindepth 1 | sort >"$OUT/sockets.$1"
    find "$TMP" -maxdepth 1 -name 'xvfb-run.*' | sort >"$OUT/xvfb-run-tmp.$1"
}

# session_checks prefijo: compara las instantáneas «antes» y «despues» y
# busca pánicos en los registros de OUT.
session_checks() {
    local p before after verdict panics panic_logs s
    for p in $WATCH; do
        before=$(paste -sd, "$OUT/$p.antes")
        after=$(paste -sd, "$OUT/$p.despues")
        if [[ -z $before ]]; then
            record "$1-$p" "mismo PID" "no estaba en ejecución" OMITIDA "pgrep -x $p"
        else
            verdict=FALLA
            [[ $before == "$after" ]] && verdict=OK
            record "$1-$p" "mismo PID" "antes $before, después ${after:-ninguno}" "$verdict" "pgrep -x $p"
        fi
    done
    panics=$(awk '$2 == 101' "$CODES" | wc -l)
    panic_logs=$(grep -l 'panicked at' "$OUT"/*.log | wc -l)
    verdict=FALLA
    [[ $panics == 0 && $panic_logs == 0 ]] && verdict=OK
    record "$1-sin-panico" 0 "$panics códigos 101, $panic_logs registros con «panicked at»" "$verdict" "$(wc -l <"$CODES") ejecuciones del binario"
    load_own
    for s in xvfb locks sockets xvfb-run-tmp; do
        residue_check "$1" "$s"
    done
    record "$1-xprop-:0" "sin cambios" "-" OMITIDA "omitida deliberadamente: no se consulta la sesión X11 real"
}

# Residuos de Xvfb (ADR 0010, F8-B.1). Las instantáneas ven todos los Xvfb
# del sistema, también los de otros programas que arrancan o terminan
# durante la batería. Por eso cada recurso se atribuye con la lista de
# recursos propios que anota xvfb-env.sh (un Xvfb por xvfb-run):
# - Residuo propio, siempre FALLA: un Xvfb propio vivo (mismo PID y su
#   cookie en -auth, contra la reutilización de PID); el lock de un display
#   propio con el PID de un Xvfb propio; el socket de un display propio,
#   salvo que su lock lo tenga ya un servidor ajeno; o el directorio
#   temporal de un xvfb-run propio. Se comprueban todos, sean nuevos o no.
# - Recurso nuevo ajeno (OK, se registra): no es de ningún Xvfb propio.
#   Solo se acepta si la lista está completa: una anotación válida por cada
#   xvfb-run. Si no, cualquier recurso nuevo es FALLA (sin atribuir).
OWN_LIST=$OUT/xvfb-propios.tsv
OWN_PIDS=()
OWN_DISPLAYS=()
OWN_DIRS=()
OWN_COMPLETE=0
OWN_SUMMARY=
load_own() {
    local case_id parent pid display auth valid=0 runs
    OWN_PIDS=() OWN_DISPLAYS=() OWN_DIRS=()
    if [[ -f $OWN_LIST ]]; then
        # shellcheck disable=SC2034 # case_id y parent solo documentan la línea
        while IFS=$'\t' read -r case_id parent pid display auth; do
            [[ $pid =~ ^[0-9]+$ && $auth == */xvfb-run.*/Xauthority ]] || continue
            [[ $display =~ ^:([1-9][0-9]*)$ ]] || continue
            OWN_PIDS+=("$pid")
            OWN_DISPLAYS+=("${BASH_REMATCH[1]}")
            OWN_DIRS+=("${auth%/Xauthority}")
            valid=$((valid + 1))
        done <"$OWN_LIST"
    fi
    runs=$(grep -c ' xvfb-run exit: ' "$RUNS")
    OWN_COMPLETE=0
    ((valid == runs)) && OWN_COMPLETE=1
    OWN_SUMMARY="$valid Xvfb propios anotados de $runs xvfb-run"
}

# in_list valor elementos...: el valor es uno de los elementos.
in_list() {
    local want=$1 item
    shift
    for item; do [[ $item == "$want" ]] && return 0; done
    return 1
}

# lock_pid fichero: PID que Xvfb escribe en su lock; falla si no se puede
# leer o no es un número.
lock_pid() {
    local value
    value=$(tr -d ' \n' 2>/dev/null <"$1") || return 1
    [[ $value =~ ^[0-9]+$ ]] && echo "$value"
}

# own_alive pid directorio: el proceso es el Xvfb propio de ese xvfb-run.
own_alive() {
    local args=()
    mapfile -d '' args 2>/dev/null <"/proc/$1/cmdline" || return 1
    [[ ${args[0]:-} == Xvfb ]] && in_list "$2/Xauthority" "${args[@]}"
}

# foreign_lock display: su lock existe y es de un servidor ajeno.
foreign_lock() {
    local pid
    pid=$(lock_pid "/tmp/.X$1-lock") && ! in_list "$pid" "${OWN_PIDS[@]}"
}

# own_left tipo: residuos propios que siguen presentes, uno por línea.
own_left() {
    local i n pid
    for i in "${!OWN_PIDS[@]}"; do
        n=${OWN_DISPLAYS[i]}
        case $1 in
        xvfb) own_alive "${OWN_PIDS[i]}" "${OWN_DIRS[i]}" && echo "${OWN_PIDS[i]}" ;;
        locks) pid=$(lock_pid "/tmp/.X$n-lock") && in_list "$pid" "${OWN_PIDS[@]}" && echo "/tmp/.X$n-lock" ;;
        sockets) [[ -e /tmp/.X11-unix/X$n || -L /tmp/.X11-unix/X$n ]] && ! foreign_lock "$n" && echo "/tmp/.X11-unix/X$n" ;;
        xvfb-run-tmp) [[ -e ${OWN_DIRS[i]} ]] && echo "${OWN_DIRS[i]}" ;;
        esac
    done | sort -u
}
no_own_left() { [[ -z $(own_left "$1") ]]; }

# attribute tipo recurso: «propio», «ya no existe», «ajeno (…)» o «sin
# atribuir» para un recurso que no estaba en la primera instantánea.
attribute() {
    local type=$1 item=$2 n pid args=()
    case $type in
    xvfb)
        in_list "$item" "${OWN_PIDS[@]}" && { echo propio; return; }
        mapfile -d '' args 2>/dev/null <"/proc/$item/cmdline" || { echo "ya no existe"; return; }
        ((OWN_COMPLETE)) && { echo "ajeno (pid $item, ${args[1]:-?})"; return; }
        ;;
    locks)
        [[ -e $item ]] || { echo "ya no existe"; return; }
        if pid=$(lock_pid "$item"); then
            in_list "$pid" "${OWN_PIDS[@]}" && { echo propio; return; }
            ((OWN_COMPLETE)) && { echo "ajeno (pid $pid)"; return; }
        fi
        ;;
    sockets)
        [[ -e $item || -L $item ]] || { echo "ya no existe"; return; }
        n=${item##*/X}
        in_list "$n" "${OWN_DISPLAYS[@]}" && ! foreign_lock "$n" && { echo propio; return; }
        ((OWN_COMPLETE)) && { echo "ajeno (display :$n)"; return; }
        ;;
    xvfb-run-tmp)
        [[ -e $item ]] || { echo "ya no existe"; return; }
        in_list "$item" "${OWN_DIRS[@]}" && { echo propio; return; }
        ((OWN_COMPLETE)) && { echo "ajeno"; return; }
        ;;
    esac
    echo "sin atribuir"
}

# residue_check prefijo tipo: registra prefijo-sin-residuos-tipo.
residue_check() {
    local id=$1-sin-residuos-$2 type=$2 left item what foreign=() unattributed=() verdict=FALLA
    # xvfb-run no espera a que su Xvfb termine: se le dan hasta 5 s.
    wait_for no_own_left "$type"
    left=$(own_left "$type" | paste -sd' ')
    while read -r item; do
        [[ -n $item ]] || continue
        what=$(attribute "$type" "$item")
        case $what in
        propio | "ya no existe") ;;
        "sin atribuir") unattributed+=("$item") ;;
        *) foreign+=("$item $what") ;;
        esac
    done < <(comm -13 "$OUT/$type.antes" "$OUT/$type.despues")
    [[ -z $left && ${#unattributed[@]} == 0 ]] && verdict=OK
    record "$id" "ningún residuo propio" "${left:-ninguno propio}${unattributed[*]:+; sin atribuir: ${unattributed[*]}}" "$verdict" \
        "$OWN_SUMMARY; nuevos ajenos: $(IFS=,; echo "${foreign[*]:-ninguno}"); después: $(paste -sd' ' "$OUT/$type.despues")"
}
