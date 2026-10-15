#!/bin/bash
# A139–A144 y A146 (parte aislada): configuraciones inválidas, límites y
# salvaguardas, sin ningún servidor X accesible. run.sh lo ejecuta con
# `unshare -rmn`, como tests/acceptance/f1/isolated.sh: un tmpfs vacío tapa
# /tmp/.X11-unix y la red propia oculta los sockets abstractos y TCP.
#
# Cada caso inválido se ejecuta dos veces:
# - en modo normal, con DISPLAY=:57 (sin servidor) y un token: si la
#   configuración se leyera después de intentar conectar, el código sería 11;
#   15 demuestra que se rechaza antes;
# - con --check-config, sin DISPLAY, XAUTHORITY ni token.
#
# Dentro del espacio de nombres el usuario es root, así que el caso de un
# archivo sin permiso de lectura (000) se prueba en Xvfb (A144).
# Uso: isolated.sh BINARIO DIRECTORIO_DE_RESULTADOS
set -u
BIN=$1
OUT=$2
F1=$(cd "$(dirname "$0")/../f1" && pwd)
# shellcheck source=../f1/lib.sh
source "$F1/lib.sh"
trap 'echo "isolated.sh exit: $?" >>"$RUNS"' EXIT

read -r _ outside_uid _ </proc/self/uid_map
if [[ $outside_uid == 0 ]]; then
    echo "GUARDA: no se está en un espacio de nombres de usuario sin privilegios"
    exit 90
fi
interfaces=$(awk -F: 'NR > 2 {gsub(/ /, "", $1); print $1}' /proc/net/dev | paste -sd,)
if [[ $interfaces != lo ]]; then
    echo "GUARDA: la red no está aislada (interfaces: $interfaces)"
    exit 90
fi
if ! mount -t tmpfs -o mode=1777 tmpfs /tmp/.X11-unix; then
    echo "GUARDA: no se pudo aislar /tmp/.X11-unix"
    exit 90
fi
if [[ -n $(find /tmp/.X11-unix -mindepth 1) ]]; then
    echo "GUARDA: /tmp/.X11-unix no está vacío"
    exit 90
fi
record A139-aislado "vacío" "$(find /tmp/.X11-unix -mindepth 1 | wc -l) entradas" OK \
    "tmpfs sobre /tmp/.X11-unix, uid real $outside_uid, interfaces: $interfaces"

EMPTY=$OUT/xauth-vacio
: >"$EMPTY"
common=(DISPLAY=:57 XAUTHORITY="$EMPTY" CRIMSON_SANDBOX_TOKEN=token-de-prueba)
no_x=(-u DISPLAY -u XAUTHORITY -u CRIMSON_SANDBOX_TOKEN)
CFG=$OUT/configuraciones
EXIT_KEY='[[keybindings]]
keys = "Mod4+Shift+Escape"
action = "exit"
'

# cfg id: escribe la entrada estándar en $CFG/id/crimson/wm.toml (0600).
# Se usa al final de una tubería, en una subshell.
cfg() {
    local file=$CFG/$1/crimson/wm.toml
    mkdir -p "${file%/*}"
    cat >"$file"
    chmod 600 "$file"
}
# bad id patrón: el archivo de $CFG/id, en modo normal y con --check-config.
bad() {
    run_bin "$1" 15 "$2" "${common[@]}" XDG_CONFIG_HOME="$CFG/$1"
    run_bin "$1-check" 15 "$2" "${no_x[@]}" XDG_CONFIG_HOME="$CFG/$1" -- --check-config
}
# good id patrón: --check-config con la ruta explícita del archivo.
good() {
    run_bin "$1" 0 "$2" "${no_x[@]}" -- --check-config "$CFG/$1/crimson/wm.toml"
}

# A139: TOML mal formado. El mensaje lleva archivo, línea y columna.
printf '[layout\nmaster_percent = 50\n' | cfg A139-sintaxis
bad A139-sintaxis 'wm\.toml:1:[0-9]+: TOML inválido'
printf '[layout]\nmaster_percent = 50\nmaster_percent = 60\n' | cfg A139-repetida
bad A139-repetida 'wm\.toml:3:1: TOML inválido: la clave «master_percent» está repetida'
printf '[layout]\n[layout]\n' | cfg A139-tabla-repetida
bad A139-tabla-repetida 'wm\.toml:2:[0-9]+: TOML inválido: la tabla \[layout\] ya está definida'

# A140: claves, tablas, tipos y rangos.
printf '[general]\n' | cfg A140-tabla
bad A140-tabla 'clave desconocida «general» en la raíz'
printf '[layout]\ngaps = 5\n' | cfg A140-clave
bad A140-clave 'wm\.toml:2:1: clave desconocida «gaps» en \[layout\]'
printf '[layout]\nmaster_percent = "60"\n' | cfg A140-tipo
bad A140-tipo 'master_percent debe ser un entero, no una cadena'
printf '[layout]\nmaster_percent = 95\n' | cfg A140-rango
bad A140-rango 'wm\.toml:2:18: master_percent = 95 fuera de 10–90'

# A141: atajos.
printf '%s[[keybindings]]\nkeys = "Mod4+j"\naction = "focus_next"\n[[keybindings]]\nkeys = "Super+j"\naction = "zoom"\n' "$EXIT_KEY" | cfg A141-repetido
bad A141-repetido 'wm\.toml:7:[0-9]+: el atajo Mod4\+j está repetido \(ya definido en la línea 4\)'
printf '[[keybindings]]\nkeys = "Mod4+j"\naction = "focus_next"\n' | cfg A141-sin-exit
bad A141-sin-exit 'no tiene ningún atajo con action = "exit"'
printf '[[keybindings]]\nkeys = "Mod4+foo"\naction = "exit"\n' | cfg A141-tecla
bad A141-tecla 'keys: tecla desconocida «foo»'
printf '[[keybindings]]\nkeys = "Lock+Mod4+q"\naction = "exit"\n' | cfg A141-modificador
bad A141-modificador 'modificador «Lock» no admitido'
printf '[[keybindings]]\nkeys = "q"\naction = "exit"\n' | cfg A141-escritura
bad A141-escritura '«q» es una tecla de escritura: necesita Control, Mod1 o Mod4'
printf '%s[[keybindings]]\nkeys = "Mod4+9"\naction = "view_desktop"\ndesktop = 9\n' "$EXIT_KEY" | cfg A141-escritorio
bad A141-escritorio 'desktop = 9 fuera de 1–8'
printf '%s[[keybindings]]\nkeys = "Mod4+x"\naction = "spawn"\n' "$EXIT_KEY" | cfg A141-accion
bad A141-accion 'acción desconocida «spawn»'
printf '%s[[keybindings]]\nkeys = "Mod4+x"\naction = "close"\ndesktop = 2\n' "$EXIT_KEY" | cfg A141-parametro
bad A141-parametro '«desktop» no se usa con la acción close'

# A142: programas.
printf '[programs.t]\nargv = []\n%s' "$EXIT_KEY" | cfg A142-argv-vacio
bad A142-argv-vacio 'argv de «t»: 0 elementos \(de 1 a 32\)'
printf '[programs.t]\nargv = ["kitty", "a\\u0000b"]\n%s' "$EXIT_KEY" | cfg A142-nul
bad A142-nul 'argv de «t»\[1\] contiene un carácter NUL'
printf '%s[[keybindings]]\nkeys = "Mod4+x"\naction = "launch"\nprogram = "nada"\n' "$EXIT_KEY" | cfg A142-sin-definir
bad A142-sin-definir 'el programa «nada» no está definido en \[programs\]'
printf '[programs.t]\nargv = ["~/token-secreto-A142/kitty"]\n%s' "$EXIT_KEY" | cfg A142-tilde
bad A142-tilde 'argv de «t»\[0\] empieza por «~»: no se expande'
# shellcheck disable=SC2016 # el $ es literal: el WM no expande variables
printf '[programs.t]\nargv = ["$TOKEN_SECRETO_A142"]\n%s' "$EXIT_KEY" | cfg A142-variable
bad A142-variable 'contiene «\$»: no se expanden variables'
printf '[programs.t]\nargv = ["token-secreto-A142/kitty"]\n%s' "$EXIT_KEY" | cfg A142-relativa
bad A142-relativa 'argv de «t»\[0\] es una ruta relativa'
printf '[programs.terminal]\nargv = ["kitty"]\n' | cfg A142-reemplazo
bad A142-reemplazo 'no define «lanzador», que usa el atajo predeterminado Mod4\+d'
leaks=$(grep -liE 'token-secreto|TOKEN_SECRETO' "$OUT"/A142-*.log | wc -l)
record A142-sin-valores "0 registros" "$leaks registros" "$([[ $leaks == 0 ]] && echo OK || echo FALLA)" "argv con «token-secreto»: ningún mensaje reproduce argv"

# A143: límites exactos y excedidos.
pad() { # pad bytes: comentario hasta ese tamaño total de archivo
    printf '#'
    head -c "$(($1 - 2))" /dev/zero | tr '\0' 'x'
    printf '\n'
}
pad 65536 | cfg A143-tamano-exacto
good A143-tamano-exacto 'configuración válida \(30 atajos'
pad 65537 | cfg A143-tamano-excedido
bad A143-tamano-excedido 'ocupa 65537 bytes o más \(máximo 65536\)'
bindings() { # bindings n: n atajos distintos, el primero de salida
    local n=1 m4 a b f
    printf '%s' "$EXIT_KEY"
    for m4 in "" "Mod4+"; do
        for a in "" "Control+" "Mod1+" "Control+Mod1+"; do
            for b in "" "Shift+"; do
                for f in {1..12}; do
                    ((n < $1)) || return 0
                    printf '[[keybindings]]\nkeys = "%s%s%sF%s"\naction = "zoom"\n' "$m4" "$a" "$b" "$f"
                    n=$((n + 1))
                done
            done
        done
    done
}
bindings 128 | cfg A143-atajos-128
good A143-atajos-128 'configuración válida \(128 atajos'
bindings 129 | cfg A143-atajos-129
bad A143-atajos-129 'keybindings: 129 elementos \(máximo 128\)'
programs() { # programs n
    local i
    for ((i = 0; i < $1; i++)); do printf '[programs.p%s]\nargv = ["kitty"]\n' "$i"; done
    printf '%s' "$EXIT_KEY"
}
programs 64 | cfg A143-programas-64
good A143-programas-64 'configuración válida \(1 atajos, 64 programas'
programs 65 | cfg A143-programas-65
bad A143-programas-65 '\[programs\]: 65 programas \(máximo 64\)'
rules() { # rules n
    local i
    for ((i = 0; i < $1; i++)); do printf '[[rules]]\nclass = "C%s"\nfloating = true\n' "$i"; done
}
rules 64 | cfg A143-reglas-64
good A143-reglas-64 'configuración válida \(30 atajos, 6 programas, maestra al 50 %, 64 reglas\)'
rules 65 | cfg A143-reglas-65
bad A143-reglas-65 'rules: 65 elementos \(máximo 64\)'
argv() { # argv n
    local i items='"kitty"'
    for ((i = 1; i < $1; i++)); do items+=', "a"'; done
    printf '[programs.t]\nargv = [%s]\n%s' "$items" "$EXIT_KEY"
}
argv 32 | cfg A143-argv-32
good A143-argv-32 'configuración válida \(1 atajos, 1 programas'
argv 33 | cfg A143-argv-33
bad A143-argv-33 'argv de «t»: 33 elementos \(de 1 a 32\)'

# A144: lectura (salvo el permiso 000, que se prueba en Xvfb).
mkdir -p "$CFG/A144-directorio/crimson/wm.toml"
bad A144-directorio 'no es un archivo regular: es un directorio'
mkdir -p "$CFG/A144-enlace-roto/crimson"
ln -s "$CFG/no-existe.toml" "$CFG/A144-enlace-roto/crimson/wm.toml"
bad A144-enlace-roto 'es un enlace simbólico a un destino que no existe'
printf '[layout]\n# \xe9\n' | cfg A144-no-utf8
bad A144-no-utf8 'no es UTF-8 válido \(línea 2\)'
printf '[layout]\n' | cfg A144-escribible
chmod 664 "$CFG/A144-escribible/crimson/wm.toml"
bad A144-escribible 'lo pueden modificar el grupo u otros usuarios \(permisos 0664\)'
mkdir -p "$CFG/A144-fifo/crimson"
mkfifo "$CFG/A144-fifo/crimson/wm.toml"
bad A144-fifo 'no es un archivo regular: es una FIFO'
run_bin A144-config-ausente 15 'no-existe\.toml: no existe' "${common[@]}" -- --config "$CFG/no-existe.toml"
run_bin A144-check-ausente 15 'no-existe\.toml: no existe' "${no_x[@]}" -- --check-config "$CFG/no-existe.toml"
# XDG inválido y HOME válido: no se pasa a HOME.
printf '[layout]\nmaster_percent = 5\n' | cfg A144-sin-respaldo
mkdir -p "$CFG/A144-home/.config/crimson"
printf '[layout]\n' >"$CFG/A144-home/.config/crimson/wm.toml"
chmod 600 "$CFG/A144-home/.config/crimson/wm.toml"
run_bin A144-sin-respaldo 15 'A144-sin-respaldo/crimson/wm\.toml:2:18: master_percent = 5' "${common[@]}" XDG_CONFIG_HOME="$CFG/A144-sin-respaldo" HOME="$CFG/A144-home"
run_bin A144-sin-respaldo-check 15 'A144-sin-respaldo/crimson/wm\.toml:2:18' "${no_x[@]}" XDG_CONFIG_HOME="$CFG/A144-sin-respaldo" HOME="$CFG/A144-home" -- --check-config

# A146: la configuración no cambia el orden de las salvaguardas.
printf '[layout]\nmaster_percent = 60\n' | cfg A146-valida
printf '[layout\n' | cfg A146-invalida
run_bin A146-display-0-valida 10 'display 0 se rechaza siempre' DISPLAY=:0 XAUTHORITY="$EMPTY" CRIMSON_SANDBOX_TOKEN=token-de-prueba XDG_CONFIG_HOME="$CFG/A146-valida"
run_bin A146-display-0-invalida 10 'display 0 se rechaza siempre' DISPLAY=:0 XAUTHORITY="$EMPTY" CRIMSON_SANDBOX_TOKEN=token-de-prueba XDG_CONFIG_HOME="$CFG/A146-invalida"
run_bin A146-sin-token-invalida 10 'CRIMSON_SANDBOX_TOKEN no está definida' -u CRIMSON_SANDBOX_TOKEN DISPLAY=:57 XAUTHORITY="$EMPTY" XDG_CONFIG_HOME="$CFG/A146-invalida"
# --check-config no lee DISPLAY: ni :0 ni un valor malformado lo afectan.
run_bin A146-check-display-0 0 'wm\.toml: configuración válida \(30 atajos, 6 programas, maestra al 60 %' -u CRIMSON_SANDBOX_TOKEN DISPLAY=:0 XDG_CONFIG_HOME="$CFG/A146-valida" -- --check-config
run_bin A146-check-display-basura 0 'configuración válida' -u CRIMSON_SANDBOX_TOKEN DISPLAY=tcp/otro:0 XDG_CONFIG_HOME="$CFG/A146-valida" -- --check-config

connected=$(grep -l 'conectado a' "$OUT"/A139-*.log "$OUT"/A14[0-46]-*.log | wc -l)
total=$(find "$OUT" -maxdepth 1 -name 'A1[34][0-9]-*.log' | wc -l)
verdict=FALLA
[[ $connected == 0 && $total -gt 0 ]] && verdict=OK
record A146-sin-conexion 0 "$connected" "$verdict" "registros aislados de A139–A146 con «conectado a» ($total registros)"
