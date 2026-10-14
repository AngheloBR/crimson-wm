# ADR 0006: Lanzamiento de aplicaciones (F4)

- **Estado:** aceptado. Implementado el 2026-10-13 (F4-B). Las 62 pruebas
  unitarias pasaron, y las de aceptación A33–A39 pasaron en Xvfb; A40 quedó
  parcial por el mismo motivo que A9 (ver «Resultados de las pruebas de
  aceptación de F4»).
- **Fecha:** 2026-10-13
- **Actualización F5 (2026-10-13):** `Repeats` también filtra la
  repetición automática de los atajos de escritorio que no son idempotentes
  (`Mod4+Shift+n` y `Mod4+Tab`), sin el intervalo de 300 ms (ADR 0007). Los
  programas lanzados no se ven afectados por los cambios de escritorio, y su
  ventana aparece en el escritorio activo al mapearse.

## Contexto

Las combinaciones de aplicaciones reservadas en F3 (ADR 0005) pasan a
lanzar programas, sin depender de sxhkd, bspwm ni scripts personales. El
ADR 0003 exige además que, desde esta fase, el token del sandbox no llegue
a los procesos que lance el WM.

## Decisión

### Módulos

| Módulo | Cambio |
|---|---|
| `src/spawn.rs` (nuevo) | Tabla de programas (`Program`), construcción del `Command`, `Launcher` con un hilo de supervisión por lanzamiento y límite de lanzamientos supervisados |
| `src/keys.rs` | `Action::Launch(índice)`, los seis atajos, captura opcional de los atajos de lanzamiento y filtro de repetición (`Repeats`) |
| `src/wm.rs` | Atiende `Launch` y los `KeyRelease` de las teclas capturadas |
| `src/main.rs` | Pasa a `wm::run` el display validado (`Target`) |

`error.rs`, `safeguards.rs`, `display.rs`, `connect.rs`, `layout.rs` y
`clients.rs` no cambian. No hay dependencias ni códigos de salida nuevos:
un fallo al lanzar nunca termina el WM.

### Atajos y programas

| Atajo | Nombre | argv |
|---|---|---|
| `Mod4+Return` | terminal | `kitty` |
| `Mod4+d` | lanzador | `rofi -show drun` |
| `Mod4+e` | archivos | `thunar` |
| `Mod4+b` | navegador | `firefox` |
| `Mod4+n` | editor | `kitty -e nvim` |
| `Print` (sin modificadores) | capturas | `flameshot gui` |

- La tabla es fija (`spawn::PROGRAMS`) y los atajos la referencian por
  índice. Una configuración futura sustituirá la tabla sin cambiar el
  modelo de ejecución.
- `argv[0]` se busca en el `PATH` del WM; `nvim` lo busca la propia
  `kitty`. No hay shell, `eval`, interpolación ni expansión de `~`, de
  variables ni de comodines: cada elemento llega literal al programa.
- Los atajos de lanzamiento son **opcionales**: si ninguna tecla produce el
  keysym o el servidor rechaza `GrabKey`, el WM avisa, deshace las
  variantes ya capturadas de esa tecla (para que no quede capturada sin
  acción) y sigue. Los atajos de control (`Mod4+j`, `Mod4+k`,
  `Mod4+Shift+Return` y `Mod4+Shift+Escape`) siguen siendo obligatorios
  (código 14, ADR 0005).
- En el mapa por defecto de Xvfb, `d`, `e`, `b` y `n` son los códigos 40,
  26, 56 y 57, y `Print` los códigos 107 y 218; se capturan ambos.
- Siguen reservadas, sin capturar, `Mod4+h`, `Mod4+l`, `Mod4+w` y
  `Mod4+Shift+w` (ADR 0005).

### Modelo de ejecución

- `Launcher::launch` reserva una plaza de supervisión, crea un hilo con
  nombre `crimson-launch` y pila de 128 KiB, y vuelve enseguida. El bucle
  X11 sigue siendo el único que habla con el servidor.
- El hilo lanza el programa (`Command::spawn`, que solo espera al `exec`) y
  lo espera con `Child::wait`, bloqueado y sin consumir CPU. Así recoge al
  hijo en cuanto termina: no quedan zombis mientras el WM está en marcha, y
  no hacen falta señales, `unsafe` ni sondeo periódico.
- **Límite:** 64 lanzamientos supervisados a la vez (hijos directos, no sus
  descendientes). Por encima, el lanzamiento se rechaza y se registra. La
  plaza se libera al recoger al hijo, o si no llegó a lanzarse.
- Si no se puede crear el hilo, no se lanza nada: nunca queda un hijo sin
  nadie que lo espere.
- **Hijo:** grupo de procesos propio (`process_group(0)`), así que las
  señales dirigidas al grupo del WM (Ctrl+C en su terminal, `timeout`) no
  le llegan. Entrada desde `/dev/null`; salida y errores heredados del WM.
  Los descriptores del WM, incluido el socket X11, tienen `O_CLOEXEC` y no
  pasan al hijo.
- **Directorio de trabajo:** `HOME` si es una ruta absoluta a un
  directorio; si no, `/`, que siempre existe y no retiene ningún punto de
  montaje.
- **Al salir el WM** (atajo de salida o conexión perdida), los programas
  lanzados siguen en marcha: el WM no les envía ninguna señal, y el sistema
  (init o el subreaper de la sesión) los adopta.

### Repetición

- La repetición automática del servidor (sin XKB) llega como un
  `KeyRelease` y un `KeyPress` de la misma tecla con la misma marca de
  tiempo. Mientras dura la captura activada por la pulsación, el WM recibe
  ambos; se ignora la pulsación emparejada con la liberación anterior.
- Además, cada atajo de lanzamiento ignora las pulsaciones a menos de
  300 ms de la anterior, aceptada o no. Se comparan marcas de tiempo del
  servidor con resta modular (dan la vuelta cada 2^32 ms), sin consultar el
  reloj del sistema.
- Con la repetición de Xvfb (retardo de 660 ms, 25 Hz), mantener
  `Mod4+Return` 1,5 s lanza un solo programa (A38).

### Entorno de los hijos

El entorno es el del WM con estos cambios, aplicados solo al `Command`
(`env_remove` y `env`): el entorno del propio WM no se modifica, y no se
usan `set_var` ni `remove_var` (que en la edición 2024 son `unsafe`).

| Variable | Tratamiento | Motivo |
|---|---|---|
| `CRIMSON_SANDBOX_TOKEN` | Se elimina | ADR 0003: un hijo no debe poder reutilizar la marca del sandbox |
| `DISPLAY` | Se fija a `:N.S`, el display y la pantalla validados por la capa 1 | El hijo va siempre al servidor que gestiona el WM |
| `WINDOWID`, `DESKTOP_STARTUP_ID` | Se eliminan | Describen desde dónde se lanzó el WM, no al hijo |
| `XAUTHORITY` y el resto | Se heredan sin cambios | El hijo se autentica con la misma cookie con la que se conectó el WM (o con `~/.Xauthority`, como él) |

Los registros solo contienen el nombre del programa, `argv[0]`, el pid y el
estado de salida; nunca el resto de argv, variables de entorno ni cookies.

### Configuración futura (no implementada)

Fuera de F4. Propuesta: `$XDG_CONFIG_HOME/crimson-wm/config.toml` con
entradas `[[launch]]` (`key` y `argv`), validadas por completo antes de
aplicarse (argv no vacío, sin cadenas vacías ni NUL, `argv[0]` como nombre
simple o ruta absoluta, sin colisiones con los atajos de control) y con
recarga transaccional. El archivo decide qué se ejecuta, así que deberá
pertenecer al usuario y no ser escribible por otros. Leer TOML requiere
dependencias (`toml`, `serde`) que habrá que revisar y autorizar.

## Verificación

### Aislamiento de las pruebas

Las pruebas nunca ejecutan las aplicaciones reales:

- Desde F4, `start_wm` (`tests/acceptance/f1/lib.sh`) ejecuta el WM con
  `env -i` y solo las variables de `WM_ENV` (`f1/xvfb-env.sh`): `DISPLAY` y
  `XAUTHORITY` del Xvfb de la prueba, el token, y `PATH`, `HOME`,
  `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`,
  `XDG_RUNTIME_DIR` (modo 700) y `DBUS_SESSION_BUS_ADDRESS` apuntando a
  directorios temporales de la prueba. El bus de D-Bus no existe. Se aplica
  también a los arneses de F1, F2 y F3.
- El `PATH` del WM es un único directorio temporal: vacío en F1–F3 (un
  atajo de lanzamiento solo puede fallar) y, en F4, con copias de
  `tests/acceptance/f4/fake.sh` llamadas `kitty`, `rofi`, `thunar`,
  `firefox`, `flameshot` y `nvim`.
- Antes de arrancar el WM, `isolated` comprueba que ese directorio solo
  contiene copias exactas de `fake.sh` (sin enlaces) y que ninguna de esas
  seis órdenes se resuelve fuera de él, con un bash nuevo y solo ese
  `PATH`. Si no se cumple, la prueba no se ejecuta.
- El `PATH` del WM siempre está definido y no está vacío: sin él, glibc
  buscaría en su ruta por defecto (`/bin:/usr/bin`), donde están las
  aplicaciones reales.
- Los programas falsos solo usan órdenes internas de bash y `ls`, `mv` y
  `sleep` por su ruta absoluta. Registran su argv, pid, ppid, pgid,
  directorio, `SigIgn`, descriptores y la presencia (no el valor) del
  token, `WINDOWID` y `DESKTOP_STARTUP_ID`.
- Cada caso de F4 registra cómo se resuelve cada orden y los nombres (no
  los valores) de las variables del proceso del WM; y al final, que cada
  pid lanzado tiene el registro de un programa falso.

### Pruebas unitarias nuevas o cambiadas

- U10 (`spawn`, 10 pruebas): tabla de los seis programas (argv exactos, sin
  vacíos, `argv[0]` sin `/`); argv exacto, también con metacaracteres de
  shell como argumento literal; entorno del `Command` (tres variables
  eliminadas, `DISPLAY` fijado y nada más); `DISPLAY` desde el `Target`;
  argv vacío; ejecutable inexistente (`NotFound`, sin proceso); directorio
  de trabajo y su respaldo; límite de 64; descripción del estado de salida;
  recogida de dos hijos reales (`/bin/true`).
- U11 (`keys`, 4 pruebas nuevas y 3 cambiadas): los seis atajos con su argv
  y como opcionales, con un atajo por programa; los de control,
  obligatorios; intervalo de repetición por atajo; repetición automática
  emparejada; desbordamiento del reloj. Las pruebas de reconocimiento,
  bloqueos y modificadores exactos incluyen los atajos de lanzamiento.

### Pruebas de aceptación

- A33. Los seis atajos lanzan su programa falso con el argv exacto; los seis
  terminan y el WM los recoge; ninguna de esas teclas llega a la ventana
  enfocada.
- A34. Con `WINDOWID` y `DESKTOP_STARTUP_ID` en el entorno del WM, el hijo:
  no tiene el token ni esas variables; `DISPLAY` = `:N.0` (el WM tiene
  `:N`); `XAUTHORITY` = la cookie temporal de xvfb-run; `PATH`, `HOME`,
  `XDG_RUNTIME_DIR` y D-Bus de la prueba; stdin = `/dev/null`; salida y
  errores en el registro del WM; solo los descriptores 0, 1, 2 y 255 (el
  script de bash), sin sockets; directorio = `HOME`; grupo propio; padre =
  el WM; `SIGPIPE` no ignorada.
- A35. Sin tecla `Print` en el mapa (`xmodmap`): aviso y el WM arranca.
  `firefox` inexistente (`No such file or directory`) y `thunar` sin
  permiso de ejecución (`Permission denied`): error registrado, sin
  lanzamiento y el WM sigue; después `Mod4+j` y `Mod4+d` funcionan.
- A36. Con un hijo que dura 5 s, el WM coloca dos ventanas en mosaico,
  cambia el foco y lanza otro programa mientras el hijo sigue en marcha.
- A37. Con un hijo vivo: 1 hijo y 2 hilos en el WM. Al terminar ese y otros
  dos: sin hijos ni zombis (`ps --ppid`) y 1 hilo. 66 pulsaciones con los
  hijos esperando: 64 lanzados, 2 rechazados, 64 hijos y 65 hilos; al
  terminarlos con SIGTERM, ningún hijo y 1 hilo.
- A38. Control: Xvfb repite una tecla mantenida. Dos pulsaciones seguidas
  de `Mod4+Return`: un lanzamiento; otra 0,5 s después: el segundo;
  `Mod4+Return` mantenido 1,5 s: solo uno más.
- A39. Al salir el WM, el hijo sigue vivo en su grupo, adoptado por otro
  proceso, sin haber recibido ninguna señal; otro cliente del servidor
  sigue en marcha.
- A40. Como A9, y además: ningún proceso en los grupos de los programas
  falsos ni con la ruta de los resultados, y ningún proceso nuevo llamado
  como alguna de las seis aplicaciones.

Cada caso comprueba además que no se registró ningún error X.

### Cambios en los arneses anteriores

- F1–F3: el WM arranca con el entorno mínimo y el `PATH` vacío descritos
  arriba (`start_wm`).
- A24 (F3): `Mod4+Return` ya no llega a la ventana enfocada. La
  comprobación `A24-super-return-libre` pasa a ser
  `A24-super-return-capturado`: la ventana no recibe la pulsación, el WM
  registra «no se pudo lanzar «terminal» (kitty)» (su `PATH` está vacío) y
  el mosaico no cambia.

## Resultados de las pruebas de aceptación de F4

Ejecutadas el 2026-10-13 con `tests/acceptance/f4/run.sh` sobre el binario
de depuración (Xvfb 2:21.1.16-1.3+deb13u4, x11-utils 7.7+7,
x11-xserver-utils 7.7+11, xdotool 1:3.20160805.1-5.1, procps 2:4.0.4-9,
diffutils 1:3.10-4). Dos ejecuciones, la segunda tras reforzar la
comprobación de residuos de A40, dieron **79 comprobaciones OK, 0 FALLA y
1 OMITIDA** (`A40-xprop-:0`); código de salida 0. Los resultados
detallados se guardaron fuera del repositorio.

| Prueba | Resultado |
|---|---|
| A33–A39 | Superadas, sin errores X; todos los procesos lanzados fueron programas falsos (81 en total) |
| A40 | **Parcial**: sin pánicos, residuos ni procesos de los programas falsos; bspwm y sxhkd con el mismo PID. La raíz de `:0` no se consultó |

Regresiones, sobre el mismo binario y con el entorno mínimo del WM: F1
46 OK y 1 OMITIDA (A9); F2 47 OK y 1 OMITIDA (A21); F3 61 OK y 1 OMITIDA
(A32), con `A24-super-return-capturado` en lugar de la comprobación
anterior. Las tres terminaron con código 0.

## Limitaciones conocidas

- **`GrabKey` rechazado:** la rama que avisa y deshace la captura de un
  atajo opcional solo está revisada en el código: ninguna herramienta
  instalada permite que otro cliente capture antes esas teclas en Xvfb. Sí
  se prueba la tecla ausente del mapa (A35).
- **Descriptores heredados por el WM:** Rust no cierra los descriptores sin
  `O_CLOEXEC` que el WM recibe de quien lo arranca; esos pasarían a los
  hijos. En las pruebas no hay ninguno (A34).
- **Señales:** el WM sigue sin manejadores (ADR 0003); un SIGTERM lo
  termina sin limpieza, aunque los hijos, en su propio grupo, no lo
  reciben.
- **Tabla fija:** cambiar un programa exige recompilar hasta la fase de
  configuración.
- **Sin notificación de arranque:** no se implementa
  `DESKTOP_STARTUP_ID`/startup-notification; el cursor no indica la
  espera.
- **Mapa de teclado fijo:** tras un `MappingNotify` los atajos no se vuelven
  a capturar (como en F1 y F3).
- **Aplicaciones reales:** nunca se han lanzado en las pruebas, por
  diseño; su comportamiento (D-Bus, instancia única) no está verificado.

## Alternativas consideradas

- **Recoger con `try_wait` en cada evento X:** descartado; un hijo
  terminado seguiría siendo zombi hasta el siguiente evento.
- **`SIGCHLD` o `signalfd`:** requieren `unsafe` o una dependencia nueva.
- **Shell (`sh -c`):** descartada; reintroduce interpolación y expansión.
- **Atajos de lanzamiento obligatorios:** descartado; un teclado sin
  `Print` impediría arrancar el WM.
- **Desactivar la repetición solo por tiempo:** insuficiente; la primera
  repetición llega a los 660 ms, después de los 300 ms del intervalo.
- **XKB «detectable autorepeat»:** requiere activar la extensión XKB de
  x11rb, que no es necesaria para nada más todavía.
- **Directorio de trabajo del WM como respaldo:** descartado; `/` es
  predecible y no retiene ningún punto de montaje.

## Consecuencias

- Se cumple el requisito del ADR 0003 sobre `CRIMSON_SANDBOX_TOKEN`.
- El WM pasa a tener hilos (uno por programa supervisado), que solo
  esperan a su hijo y no comparten estado con el bucle salvo un contador
  atómico.
- Las órdenes nuevas del arnés (`ps`, `cmp`, `xmodmap`) figuran en la
  cabecera de `tests/acceptance/f4/run.sh` y en el `README.md`.
