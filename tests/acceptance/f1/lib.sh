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

# start_wm id: arranca el WM en segundo plano (WM_ID, WM_PID, WM_LOG).
start_wm() {
    WM_ID=$1
    WM_LOG=$OUT/$1.log
    timeout 30 "$BIN" >"$WM_LOG" 2>&1 &
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
    local p before after verdict panics panic_logs s new
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
    for s in xvfb locks sockets xvfb-run-tmp; do
        new=$(comm -13 "$OUT/$s.antes" "$OUT/$s.despues" | paste -sd' ')
        verdict=FALLA
        [[ -z $new ]] && verdict=OK
        record "$1-sin-residuos-$s" "ninguno nuevo" "${new:-ninguno}" "$verdict" "antes y después: $(paste -sd' ' "$OUT/$s.despues")"
    done
    record "$1-xprop-:0" "sin cambios" "-" OMITIDA "omitida deliberadamente: no se consulta la sesión X11 real"
}
