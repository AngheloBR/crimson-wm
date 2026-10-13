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
