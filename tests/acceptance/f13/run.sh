#!/bin/bash
# A150–A165 (ADR 0015). Cada caso gráfico crea su Xvfb autenticado.
# Cargo sin red; HOME/configuración y programas falsos aislados como F12.
set -u
F=$(cd "$(dirname "$0")" && pwd)
F1=$(cd "$F/../f1" && pwd)
ROOT=$(cd "$F/../../.." && pwd)
if ((EUID == 0)); then echo 'run.sh: no se ejecuta como root' >&2; exit 2; fi
for tool in xvfb-run Xvfb xauth unshare mount xprop xwininfo xlsatoms xdotool xmodmap mcookie pgrep ps timeout column xwd magick git xev stdbuf; do
    command -v "$tool" >/dev/null || { echo "falta $tool" >&2; exit 2; }
done
# shellcheck source=../f1/binaries.sh
source "$F1/binaries.sh"
ensure_binaries "$ROOT" 1 "${1:-}" || exit 2
unset DISPLAY XAUTHORITY CRIMSON_SANDBOX_TOKEN DBUS_SESSION_BUS_ADDRESS WINDOWID DESKTOP_STARTUP_ID CRIMSON_RULES XDG_CONFIG_HOME
OUT=$(mktemp -d -t crimson-f13.XXXXXX) || exit 2
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
: >"$RESULTS"; : >"$CODES"; : >"$RUNS"
snapshot antes
APPS=(kitty rofi thunar firefox flameshot nvim)
apps_snapshot() {
    local app
    for app in "${APPS[@]}"; do pgrep -x "$app" | sed "s/^/$app /"; done | sort >"$OUT/apps.$1"
}
apps_snapshot antes
unshare -rmn bash "$F/isolated.sh" "$BIN" "$OUT" >"$OUT/isolated.txt" 2>&1
rc=$?
record A164-namespace 0 "$rc" "$([[ $rc == 0 ]] && echo OK || echo FALLA)" 'unshare, sockets ocultos y red aislada'
for c in A150 A151 A152 A153 A154 A155 A156 A157 A158 A159 A160 A161 A162 A163 A163-wide A163-small A164; do
    size=1280x720
    [[ $c == A163 ]] && size=1279x719
    [[ $c == A163-wide ]] && size=3840x1080
    [[ $c == A163-small ]] && size=4x2
    xvfb-run -a -s "-screen 0 ${size}x24 -nolisten tcp" \
        bash "$F/xvfb-case.sh" "$BIN" "$OUT" "$c" "$TEST_CLIENT" >"$OUT/xvfb-$c.txt" 2>&1
    rc=$?
    echo "$c xvfb-run exit: $rc" >>"$RUNS"
    inner=$(awk -v c="$c" '$1==c && $2=="xvfb-case.sh" {print $NF}' "$RUNS")
    want='script=0; xvfb-run=0'
    verdict=FALLA
    [[ $rc == 0 && $inner == 0 ]] && verdict=OK
    if [[ $c == A162 ]]; then
        want='script=0; xvfb-run=0 o 1 al terminar su servidor; WM=13'
        if [[ $inner == 0 && ( $rc == 0 || $rc == 1 ) ]] && grep -qx 'A162-conexion 13' "$CODES"; then verdict=OK; fi
    fi
    record "$c-arnes" "$want" "script=${inner:-sin resultado}; xvfb-run=$rc" "$verdict" 'salida del script y del envoltorio; A162 termina deliberadamente su propio Xvfb'
    n=$(awk -F'\t' -v id="$c-complete" '$1==id{n++} END{print n+0}' "$RESULTS")
    [[ $n == 1 ]] || record "$c-complete" '1 resultado' "$n" FALLA 'caso incompleto'
done
snapshot despues
apps_snapshot despues
session_checks A165
record A165-atribucion 1 "$OWN_COMPLETE" "$([[ $OWN_COMPLETE == 1 ]] && echo OK || echo FALLA)" "$OWN_SUMMARY; una anotación validada por ejecución"
left=$(pgrep -f -- "$OUT/" | paste -sd' ')
record A165-sin-procesos ninguno "${left:-ninguno}" "$([[ -z $left ]] && echo OK || echo FALLA)" 'procesos con rutas propias del arnés'
new=$(comm -13 "$OUT/apps.antes" "$OUT/apps.despues" | paste -sd,)
record A165-sin-apps ninguna "${new:-ninguna}" "$([[ -z $new ]] && echo OK || echo FALLA)" 'pgrep por nombre antes/después'
# Los manifiestos junto a los casos evitan que un aborto elimine cobertura.
while read -r id; do
    n=$(awk -F'\t' -v id="$id" '$1==id{n++} END{print n+0}' "$RESULTS")
    [[ $n == 1 ]] || record "$id" '1 resultado' "$n" FALLA 'resultado ausente o duplicado'
done < <(sed -n 's/^[[:space:]]*# EXPECT: //p' "$F/xvfb-case.sh" "$F/isolated.sh" | tr ' ' '\n' | sort -u)
duplicates=$(cut -f1 "$RESULTS" | sort | uniq -d | paste -sd,)
record A165-identificadores ninguno "${duplicates:-ninguno}" "$([[ -z $duplicates ]] && echo OK || echo FALLA)" 'cada comprobación tiene identificador único'
column -t -s $'\t' "$RESULTS"
ok=$(awk -F'\t' '$4=="OK"{n++} END{print n+0}' "$RESULTS")
failed=$(awk -F'\t' '$4=="FALLA"{n++} END{print n+0}' "$RESULTS")
skipped=$(awk -F'\t' '$4=="OMITIDA"{n++} END{print n+0}' "$RESULTS")
echo "OK: $ok  FALLA: $failed  OMITIDA: $skipped"
echo "Resultados: $OUT"
((failed == 0))
