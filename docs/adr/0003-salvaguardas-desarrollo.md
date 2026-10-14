# ADR 0003: Salvaguardas de desarrollo y protección de la sesión principal

- **Estado:** aceptado. Las cuatro capas se implementaron en P4
  (2026-10-11). Las pruebas unitarias U1–U4 (18 pruebas de `cargo test`)
  pasaron. El 2026-10-12 se ejecutaron las pruebas de aceptación en Xvfb con
  el arnés `tests/acceptance/f1/`: **A1–A8 pasaron y A9 quedó parcial**,
  porque la consulta a la sesión `:0` se omitió deliberadamente. Detalles y
  limitaciones en «Resultados de las pruebas de aceptación de F1».
- **Fecha:** 2026-10-11 (resultados de F1 añadidos el 2026-10-12)
- **Actualización F2 (2026-10-13):** la capa 4 selecciona también
  `SubstructureNotify`, que necesita el mosaico (ADR 0004); la regresión de
  F1 sobre el código de F2 se describe en «Regresión de F1 con el código
  de F2».
- **Actualización F3 (2026-10-13):** los atajos de foco (ADR 0005) se
  capturan con la misma regla que el de salida (código 14 si falla), y el WM
  publica `_NET_ACTIVE_WINDOW` en la raíz después de superar las cuatro
  capas y lo borra al salir. Las capas no cambian; la regresión de F1 sobre
  el código de F3 también se describe en esa sección.
- **Actualización F5 (2026-10-13):** los 17 atajos de escritorio (ADR 0007)
  se capturan con la misma regla (código 14 si falla). El WM registra
  `WM_STATE`, `_NET_NUMBER_OF_DESKTOPS`, `_NET_CURRENT_DESKTOP` y
  `_NET_WM_DESKTOP` después de superar las cuatro capas, sigue sin crear
  `_NET_SUPPORTING_WM_CHECK` y pone cada ventana gestionada en su save-set:
  si el WM termina por cualquier motivo, el servidor vuelve a mapear las
  ventanas que tenía ocultas. Las capas no cambian.
- **Actualización F6 (2026-10-13):** la capa 3 se amplía sin relajarse
  (ADR 0008). Sigue rechazando cualquier `_NET_SUPPORTING_WM_CHECK`
  previa, aunque sea obsoleta; el mensaje indica, solo como diagnóstico, si
  la ventana a la que apunta existe. Además rechaza (código 12) si la
  selección `WM_S{pantalla}` de ICCCM §2.8 tiene propietario, sin crear el
  átomo. Solo después de las cuatro capas, de los atajos y de los átomos, el
  WM crea su ventana de comprobación y, con el servidor capturado
  (`GrabServer`), vuelve a comprobar ambas condiciones, adquiere `WM_S0` y
  publica `_NET_SUPPORTING_WM_CHECK` y `_NET_SUPPORTED`. Al salir solo borra
  las propiedades de la raíz si siguen apuntando a su ventana. Tras SIGTERM
  o SIGKILL la propiedad queda obsoleta y el siguiente arranque en ese
  servidor se rechaza (fail-closed) hasta borrarla a mano: el WM nunca la
  elimina por su cuenta. La segunda instancia de A6 se detiene ahora en la
  capa 3; la capa 4 (`BadAccess`) se prueba en A54 con un cliente que solo
  toma `SubstructureRedirect`.
- **Actualización F8-B.1 (2026-10-14):** las comprobaciones de residuos de
  Xvfb (A9 y sus equivalentes en F2–F8) ya no cuentan como residuo
  cualquier recurso nuevo del sistema. Cada caso anota su propio Xvfb, y
  cada recurso se atribuye: los propios que persisten siempre fallan, y los
  ajenos solo se aceptan si todos los Xvfb del arnés están anotados (ADR
  0010, «Aislamiento de las comprobaciones de residuos»).

## Contexto

La sesión de trabajo del autor (bspwm + sxhkd) no debe verse afectada
durante el desarrollo. El 2026-10-11 se comprobó que esa sesión usa el
display `:0`. Ejecutar el WM por error contra ella podría interferir con su
funcionamiento, así que las pruebas se harán en servidores X aislados (Xvfb
y, más adelante, Xephyr).

## Decisión

### Capas de protección

Antes de tomar el control de cualquier pantalla, el WM aplica cuatro
comprobaciones, en este orden (`src/main.rs`, función `run`).

#### Capa 1: validación de `DISPLAY` (antes de abrir la conexión)

Política conservadora: **si no se puede demostrar que el destino está
permitido, se rechaza.**

1. **Display número 0:** se rechaza siempre, sea cual sea la pantalla o la
   representación. Ejemplos no exhaustivos, todos rechazados en U1 y en
   A2: `:0`, `:0.0`, `:0.1`, `:00`, `unix/:0`, y también `unix:0` y
   `/tmp/.X11-unix/X0`, que el parser trata como rutas (regla 3).
2. **Destinos remotos o por TCP:** se rechazan. Por ejemplo `localhost:1`,
   `host:1`, `tcp/localhost:1`, `[::1]:1`, o cualquier valor con un host no
   vacío o con un protocolo distinto de `unix`.
3. **Valores malformados o que no puedan validarse con certeza:** se
   rechazan, con el código 10. Incluye los valores que no son Unicode y
   todas las formas de ruta (`/…` y `unix:…`, por ejemplo
   `/tmp/.X11-unix/X1` o `unix:1`), cuyo número de display real no puede
   validarse. El parser de x11rb-protocol 0.14.0 las trata así
   (`parse_display/mod.rs:72-77` y `113-142`):
   - **Si la ruta existe**, o existe al quitarle un sufijo de pantalla
     `.n`, devuelve `protocol = "unix"`, `host` igual a la ruta y display 0,
     sin relación con el número real del servidor. `src/display.rs` la
     rechaza por tener un host no vacío («solo se admiten displays
     locales»).
   - **Si no existe**, devuelve `MalformedValue` y `src/display.rs` la
     rechaza como «DISPLAY malformado».

   La existencia se comprueba con `Path::exists` (`mod.rs:44-46`) en el
   momento de validar, y las rutas relativas dependen del directorio
   actual: `unix:0` busca un archivo llamado `0`. En ambos casos el
   resultado es el código 10; solo el mensaje depende del sistema de
   archivos. `src/display.rs` comprueba, en este orden, el análisis
   (líneas 38-39), el protocolo (41-45), el host no vacío (46-51) y el
   display 0 (52-56).
4. **`DISPLAY` ausente o vacío:** no hay a qué conectar; se termina con el
   código 11.

**Parser (decisión del autor del 2026-10-11, alternativa A).** Para que la
validación y la conexión no interpreten `DISPLAY` de forma distinta:

- `DISPLAY` se lee **una sola vez** y se analiza con el parser público
  `x11rb_protocol::parse_display::parse_display`, accesible mediante
  `x11rb::reexports` (API y comportamiento verificados en el ADR 0002).
- Solo se acepta un resultado con protocolo ausente o `unix`, host vacío y
  display distinto de 0. La capa 1 acepta cualquier número de pantalla
  (`u16`) y no comprueba que exista.
- **La existencia de la pantalla se comprueba en el cliente**, una vez que
  el servidor ha aceptado la conexión. x11rb compara el número con las
  pantallas que lista la respuesta de conexión
  (`x11rb-0.14.0/src/rust_connection/mod.rs:245-248`) y, si no existe,
  devuelve `InvalidScreen`; `src/connect.rs` (líneas 53-56) lo convierte en
  el código 14. `src/main.rs` (líneas 64-67) repite la comprobación, con el
  mismo código. Ninguna prueba de aceptación ha usado todavía una pantalla
  inexistente.
- El parser acepta ceros a la izquierda y el signo `+` (`:05` y `:+5` dan
  el display 5). Se aceptan, porque la conexión usa el número ya analizado
  y no vuelve a interpretar el texto.

**Conexión.** Solo por el socket Unix local `/tmp/.X11-unix/X<N>`, con
`DefaultStream::connect(&ConnectAddress::Socket(..))`. **Nunca por TCP:** no
se usa `RustConnection::connect`, que vuelve a leer `DISPLAY` y prueba
`localhost:6000+N` si el socket falla.

**Autenticación: fallo cerrado (decisión del autor del 2026-10-11).** La
cookie se obtiene con `x11rb_protocol::xauth::get_auth`, que lee el archivo
de `XAUTHORITY` o, si esa variable no está definida, `$HOME/.Xauthority`
(`x11rb-protocol-0.14.0/src/xauth.rs:131-141`), y solo admite
`MIT-MAGIC-COOKIE-1`. Si no hay entrada para ese display, no se puede
determinar el archivo o falla su lectura (por ejemplo, porque no existe),
el WM termina con el código 11 **sin intentar la conexión sin
autenticación** (`src/connect.rs`, líneas 19-34). Si el servidor
rechaza la autenticación, también termina con el código 11. Un servidor sin
cookie registrada no se puede usar, y nunca se recurrirá a `-ac`.

#### Capa 2: marca de sandbox

La propiedad `_CRIMSON_SANDBOX` de la ventana raíz debe coincidir con la
variable de entorno `CRIMSON_SANDBOX_TOKEN`. Solo los scripts de prueba
crean esa marca, con un valor aleatorio generado con `mcookie`. La variable
se comprueba antes de abrir la conexión (`src/main.rs`, línea 60).

- La propiedad debe ser de tipo `STRING` y formato 8, y su contenido debe
  coincidir byte a byte con el valor de la variable (por ejemplo,
  `xprop -root -f _CRIMSON_SANDBOX 8s -set _CRIMSON_SANDBOX "$token"`).
- Una variable ausente, vacía o no Unicode, una propiedad ausente o de otro
  tipo y un valor distinto se rechazan con el código 10.
- Los átomos se consultan con `InternAtom` y `only_if_exists = true`, para
  que las comprobaciones no creen átomos en el servidor. Se aplica también
  a la capa 3.

**Requisito de F4, cumplido:** `CRIMSON_SANDBOX_TOKEN` no se hereda por los
procesos que lanza el WM: se elimina del entorno de cada hijo con
`Command::env_remove`, sin modificar el del WM (`src/spawn.rs`; verificado
en A34, ver ADR 0006).

#### Capa 3: otro gestor presente

Si la raíz tiene la propiedad `_NET_SUPPORTING_WM_CHECK`, el WM se niega a
arrancar con el código 12 (verificado en A5).

El rechazo es **deliberadamente conservador**: basta con que la propiedad
exista, aunque sea obsoleta (por ejemplo, si la dejó un gestor que se
colgó). Ante la duda, el WM no arranca. A partir de F6, cuando Crimson WM
defina esa propiedad, un servidor ya usado por una instancia anterior
también será rechazado. Por eso cada prueba que lo requiera usará un
servidor X nuevo.

#### Capa 4: `BadAccess`

La selección de `SubstructureRedirect` en la raíz se hace con una petición
*checked*. Si otro cliente ya la tiene, el error `BadAccess` se gestiona
explícitamente y el WM termina con el código 12 (verificado en A6).

Desde F2, la misma petición selecciona también `SubstructureNotify`, para
recibir `UnmapNotify` y `DestroyNotify` de las ventanas (ADR 0004). Varios
clientes pueden seleccionar ese bit a la vez: según el protocolo X11, solo
`SubstructureRedirect`, `ResizeRedirect` y `ButtonPress` producen
`BadAccess`, así que la capa no cambia (A6 sigue pasando).

### Registro de conexión

No es una capa de protección, sino información de diagnóstico. El WM
registra a qué servidor se ha conectado (display, fabricante, release, ID
de la raíz y tamaño de pantalla; verificado en A7). Los registros no
vuelcan el entorno: el WM solo lee `DISPLAY` y `CRIMSON_SANDBOX_TOKEN`
(`src/main.rs`, líneas 59-60), y desde F4 `HOME`, como directorio de
trabajo de los programas que lanza (ADR 0006). Nunca imprime el valor del
token.

### Alcance y limitaciones

Estas capas son **salvaguardas contra accidentes, no una frontera de
seguridad**, y no ofrecen una protección absoluta:

- **La capa 1 solo protege la sesión principal mientras esta use `:0`.**
  Un gestor de inicio puede asignar otro número de display. En ese caso la
  capa 1 no la protege, aunque las capas 3 y 4 la seguirían protegiendo
  mientras haya otro gestor de ventanas activo en ella.
- **La capa 2 la puede falsificar cualquier proceso local** con acceso a un
  servidor X, porque X11 permite a cualquier cliente escribir propiedades en
  la raíz y no aísla a sus clientes entre sí.
- **Las capas 3 y 4 dependen de que haya otro gestor activo.** No protegen
  un display sin gestor de ventanas.
- Ninguna capa protege frente a una acción deliberada ni frente a un
  proceso malicioso.

### Sin mecanismos de desactivación

Crimson WM **no ofrecerá argumentos de ejecución, variables de entorno ni
opciones de configuración** que desactiven o relajen estas salvaguardas.

- En F1 el binario solo acepta `--help` y `--version`. Cualquier otro
  argumento, o más de uno, termina con el código 2 (verificado en U2 y A1).
- `CRIMSON_SANDBOX_TOKEN` es un requisito adicional para arrancar, no una
  excepción.
- Las salvaguardas solo podrán retirarse en F11 (pruebas en una sesión real
  en otro VT), mediante un cambio de código revisado y un nuevo ADR.

### Entorno de pruebas

- **Xvfb**, mediante `xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp"`.
  Según `/usr/bin/xvfb-run` en Debian 13, esta orden elige un display libre
  a partir de `:99`, crea una cookie temporal con `mcookie` y `xauth`,
  arranca Xvfb con `-auth` y añade `-nolisten tcp` por defecto. La
  repetición en `-s` es una defensa explícita. Nunca se usará `--listen-tcp`.
- **Un servidor nuevo por prueba** cuando la prueba dependa del estado de la
  raíz: marca de sandbox, `_NET_SUPPORTING_WM_CHECK` o `SubstructureRedirect`.
- **Reinicio de Xvfb al quedarse sin clientes.** Observado en las pruebas de
  aceptación de F1 y comprobado de nuevo el 2026-10-12 con Xvfb
  2:21.1.16-1.3+deb13u4 de Debian 13: si no se usa `-noreset`, Xvfb se
  reinicia cuando se desconecta su último cliente y pierde las propiedades
  de la raíz y los átomos creados. Una marca puesta con `xprop -set`, que
  era el único cliente, ya no existe cuando arranca el WM, y este termina
  con el código 10. También puede invalidar en silencio las pruebas
  negativas: una prueba que espera el rechazo de una marca distinta pasaría
  aunque la marca ni siquiera existiese. Para evitarlo:
  - mantener un cliente pasivo conectado durante toda la prueba (por
    ejemplo, `xprop -root -spy` en segundo plano) y confirmar que el
    servidor no se ha reiniciado con una propiedad de control;
  - o, cuando corresponda, arrancar Xvfb con `-noreset` (con `xvfb-run`,
    dentro de `-s`). El estado de la raíz persiste entonces entre clientes,
    así que sigue haciendo falta un servidor nuevo por prueba;
  - en ambos casos, comprobar cada precondición (marca, tipo de la marca,
    `_NET_SUPPORTING_WM_CHECK`) justo antes de ejecutar el WM, y en las
    pruebas que observan ventanas, que el WM sigue en marcha: sin WM, el
    servidor mapea y configura las ventanas por sí mismo.

  Las pruebas de `tests/acceptance/f1/` usan el cliente pasivo. Ninguna de
  estas medidas cambia las salvaguardas del WM.
- **Xephyr** (aplazado): siempre con `-auth <cookie temporal>` y
  `-nolisten tcp`. La cookie se guardará en un directorio temporal bajo
  `$XDG_RUNTIME_DIR`, fuera del repositorio.
- **Nunca se usará `-ac`.**
- **Comprobaciones sobre la sesión principal:** solo lecturas que no
  conectan con ningún servidor X, antes y después de las pruebas:
  - `pgrep -x bspwm` y `pgrep -x sxhkd`, que deben conservar el PID;
  - que no haya servidores Xvfb, locks `/tmp/.X*-lock`, sockets en
    `/tmp/.X11-unix` ni directorios `xvfb-run.*` nuevos (desde F8-B.1:
    ninguno propio y ninguno nuevo sin atribuir; ADR 0010).

  **No se consulta la raíz de `:0`**, por ejemplo con
  `xprop -display :0`, ni se usa ninguna otra herramienta X contra la
  sesión real: conectar con ella no está autorizado (decisión del autor
  del 2026-10-12). Por tanto, las pruebas demuestran que los procesos de la
  sesión principal siguen siendo los mismos, pero **no** que su raíz no
  haya cambiado.
- **A qué servidor se conectó el WM:** la fuente principal es el registro
  del propio WM (display e ID de la raíz), junto con las guardas del arnés,
  que solo continúan dentro del Xvfb hijo de su propio `xvfb-run`. La
  comprobación adicional con `ss -xp`, correlacionando los inodos de los
  sockets, **sigue pendiente**: no se usó en F1, y no se ha verificado qué
  muestra `ss` para el lado cliente de un socket Unix.

### Códigos de salida (F1)

| Código | Causa |
|---|---|
| 0 | Salida normal (atajo provisional `Mod4+Shift+Escape`) |
| 2 | Argumentos inválidos |
| 10 | Display rechazado por las salvaguardas: número 0, no local o malformado (incluidas las formas de ruta y los valores no Unicode); `CRIMSON_SANDBOX_TOKEN` ausente, vacía o no Unicode (antes de conectar); o raíz sin una marca de sandbox válida |
| 11 | `DISPLAY` ausente o vacío; socket inexistente o inaccesible; sin cookie `MIT-MAGIC-COOKIE-1` para el display en el archivo de autorización (`XAUTHORITY` o, si no está definida, `$HOME/.Xauthority`), o imposible determinar o leer ese archivo; o el servidor rechaza la conexión inicial (respuesta `Failed` o `Authenticate`, por ejemplo una cookie errónea) o hay un error de E/S durante ella |
| 12 | Ya hay otro gestor (`_NET_SUPPORTING_WM_CHECK` o `BadAccess`) |
| 13 | Se perdió la conexión X durante la ejecución |
| 14 | Error de protocolo inesperado al arrancar: por ejemplo, la pantalla indicada no existe, ningún código de tecla produce la tecla de un atajo o falla la captura de un atajo (el de salida, desde F3 los de foco del ADR 0005 y desde F5 los de escritorio del ADR 0007, desde F6 los de cierre del ADR 0008 y desde F7 el de pantalla completa del ADR 0009) |
| 101 | `panic` de Rust con `panic = "unwind"` (el valor por defecto, que el proyecto no cambia). Se considera un bug; A9 comprueba que no ocurra |

- **Errores X durante la ejecución:** los debidos a condiciones de carrera
  (`BadWindow`, `BadDrawable`, etc.) llegan como eventos de error; el WM los
  registra y no termina (`src/wm.rs`, líneas 268-273). Desde F2, un error
  X en una consulta con respuesta sobre una ventana (por ejemplo,
  `GetWindowAttributes` de una ventana ya destruida) se registra y la
  ventana no se gestiona (ADR 0004). Solo la pérdida de la conexión lo
  termina, con el código 13 (verificado en A8). **Pendiente:** no se ha
  ejercitado el registro de un error X sin terminar, porque en las pruebas
  no se produjo ninguno; la A19 de F2 lo intenta sin conseguirlo (ver
  ADR 0004).
- **Señales:** en F1 el WM no instala manejadores de señales (no hay código
  de señales en `src/`), así que SIGTERM y SIGINT conservan su
  comportamiento por defecto, aceptable porque el WM aún no guarda estado.
  Un proceso terminado por una señal no devuelve un código de salida: la
  shell muestra 143 (SIGTERM) o 130 (SIGINT). **Pendiente:** ninguna prueba
  ejercita todavía estas señales. La gestión limpia de señales no entró en
  F4 (ADR 0006) y queda para una fase posterior; los programas lanzados
  tienen su propio grupo de procesos y no reciben las señales dirigidas al
  grupo del WM.

## Consecuencias

- Mientras la sesión principal use `:0`, el WM no puede tomar su control
  sin modificar el código (capa 1, verificada en U1 y A2). Las capas 3 y 4
  protegen además cualquier display con otro gestor activo (verificadas en
  Xvfb en A5 y A6). Son protecciones contra accidentes, no
  contra una acción deliberada (ver "Alcance y limitaciones").
- La prueba A8 verificó con Xvfb que cerrar el servidor hace terminar el WM
  con el código 13, y A9 que bspwm y sxhkd conservaron su PID. La misma
  comprobación con Xephyr sigue pendiente.
- **La raíz de `:0` no se consultó en ningún momento**, así que no está
  demostrado que no haya cambiado. Esa consulta no está autorizada (ver
  «Comprobaciones sobre la sesión principal» en «Entorno de pruebas»).
- Las órdenes que usa el arnés, con los paquetes de Debian 13 que las
  aportan, figuran en la cabecera de `tests/acceptance/f1/run.sh` y en la
  tabla de requisitos del `README.md`.

## Pruebas de aceptación de F1

Documentadas el 2026-10-11, antes de implementar F1.

**Alcance de F1:** conectar con las salvaguardas de este ADR; registrar la
conexión; seleccionar `SubstructureRedirect`; atender `MapRequest`
(mapeando la ventana tal cual) y `ConfigureRequest` (aplicando lo pedido),
sin mosaico ni foco; capturar `Mod4+Shift+Escape` en la raíz para salir; y
registrar los errores X sin terminar. Nada más.

**Pruebas unitarias** (`cargo test`, sin servidor X ni ejecutar el
binario):

- U1. Validación de `DISPLAY`: cada ejemplo de las reglas 1 a 4 de la capa 1
  produce el código previsto, y `:1`, `:1.0`, `unix/:5`, `:05` y `:+5` se
  aceptan con el display correcto.
- U2. Argumentos: sin argumentos se arranca; `--help` y `--version` se
  aceptan; cualquier otro argumento, o más de uno, da el código 2.
- U3. Correspondencia entre cada error y su código de salida.
- U4. Comparación de la marca de sandbox y filtrado de modificadores del
  atajo de salida.

**Pruebas de aceptación** (ejecutan el binario en Xvfb; cada una con un
servidor nuevo y comprobaciones de solo lectura sobre la sesión principal
antes y después, sin conectar con `:0`; ver «Entorno de pruebas»):

- A1. `--help` y `--version` terminan con 0; un argumento desconocido, con 2.
- A2. `DISPLAY` ausente o vacío: código 11. `:0`, `:0.0`, `unix/:0`,
  `unix:0`, `/tmp/.X11-unix/X0`, `localhost:1` y `tcp/localhost:1`:
  código 10.
- A3. Display sin servidor: código 11. `XAUTHORITY` apuntando a un archivo
  vacío o sin la cookie del display: código 11.
- A4. Sin `CRIMSON_SANDBOX_TOKEN`, sin marca o con una marca distinta:
  código 10.
- A5. Con `_NET_SUPPORTING_WM_CHECK` presente en la raíz: código 12.
- A6. Con otra instancia ya arrancada en el mismo servidor (y por tanto
  con `SubstructureRedirect` ocupado): la segunda termina con 12.
- A7. Arranque válido: el registro muestra display, fabricante, release,
  ID de la raíz y tamaño; una ventana de `xmessage` queda visible
  (`xwininfo`); `xdotool key super+shift+Escape` hace terminar el WM con 0.
- A8. Cerrar Xvfb con el WM en marcha: código 13.
- A9. Ninguna prueba produce el código 101, y las comprobaciones sobre la
  sesión principal no cambian (ver «Comprobaciones sobre la sesión
  principal» en «Entorno de pruebas»). En su definición del 2026-10-11 esas
  comprobaciones incluían consultar la raíz de `:0`. Esa parte no está
  autorizada y no se ejecuta, así que A9 solo puede quedar parcial (ver
  «Resultados de las pruebas de aceptación de F1»).

## Resultados de las pruebas de aceptación de F1

Ejecutadas el 2026-10-12 con `tests/acceptance/f1/run.sh` sobre el binario
de depuración. El arnés comprueba que el binario es posterior a `src/`,
`Cargo.toml` y `Cargo.lock`.

- A1, A2 y A3a se ejecutaron con `unshare -rmn`, sin ningún servidor X
  visible.
- El resto, cada una en un Xvfb nuevo lanzado por `xvfb-run`: Xvfb
  2:21.1.16-1.3+deb13u4, una pantalla de 1280x720x24, `-nolisten tcp`,
  cookie temporal y sin `-ac`.
- Resultado del arnés: 46 comprobaciones OK, 0 FALLA y 1 OMITIDA; código de
  salida 0. Los resultados detallados se guardaron fuera del repositorio.

| Prueba | Resultado | Evidencia principal |
|---|---|---|
| A1 | Superada | `--help` y `--version`: 0; `--bogus` y dos argumentos: 2 |
| A2 | Superada | `DISPLAY` ausente o vacío: 11. `:0`, `:0.0`, `:0.1`, `:00`, `unix/:0`, `unix:0`, `/tmp/.X11-unix/X0`, `localhost:1` y `tcp/localhost:1`: 10. Ningún registro de A1–A3a contiene «conectado a» |
| A3 | Superada | Display sin servidor, `XAUTHORITY` vacío, cookie de otro display y cookie errónea: 11. Control: el Xvfb rechaza a `xprop` sin cookie |
| A4 | Superada | Sin token, sin marca, marca distinta y marca `UTF8_STRING` con el valor correcto: 10, con cada precondición verificada antes de ejecutar el WM. En A4b (sin marca), tras el rechazo no existían los átomos `_CRIMSON_SANDBOX` ni `_NET_SUPPORTING_WM_CHECK`, y el átomo de control seguía presente (el servidor no se reinició) |
| A5 | Superada | `_NET_SUPPORTING_WM_CHECK` presente: 12 |
| A6 | Superada | Segunda instancia: 12 (`BadAccess`); la primera sigue en marcha y sale con 0 mediante el atajo |
| A7 | Superada | El registro muestra display, fabricante, release, raíz y tamaño; la ventana de `xmessage` queda `IsViewable` con el WM en marcha; `super+shift+Escape` termina con 0 |
| A8 | Superada | Cerrar Xvfb: 13 («The X11 server closed the connection») |
| A9 | **Parcial** | 28 ejecuciones del binario sin código 101 ni «panicked at»; bspwm y sxhkd con el mismo PID antes y después; sin servidores, locks, sockets ni directorios temporales nuevos. **Omitido deliberadamente:** la consulta de la raíz de `:0`, que formaba parte de A9; esa propiedad **no se ha verificado** |

Comprobaciones adicionales, fuera de los criterios de aceptación:

- `ConfigureRequest` se aplica tal como se pide (100, 120, 400×200).
- `super+Escape` sin Shift no cierra el WM.
- Ningún registro contiene «error X ignorado».

Observaciones sobre la ejecución:

- **Códigos de `xvfb-run`.** En A8, `xvfb-run` termina con 1 aunque la
  prueba pasa: su limpieza (`trap clean_up EXIT` con `set -e`) intenta
  cerrar el Xvfb que la prueba ya ha cerrado. El arnés registra por
  separado el código del programa (13), el del script de la prueba (0) y el
  de `xvfb-run` (1).
- **Primera ejecución fallida.** Una ejecución previa, con una versión
  anterior del arnés, falló en A5–A8 y dio falsos positivos en A4 y A7 por
  el reinicio de Xvfb descrito en «Entorno de pruebas». Solo se corrigió el
  arnés; el código del WM y los criterios no cambiaron.
- **Control negativo.** Con `/bin/false` como binario, el arnés da 38
  comprobaciones FALLA y termina con 1. Detecta también las comprobaciones
  sin resultado.

**Limitaciones: pendiente de validar.**

- **La raíz de `:0`** no se ha consultado (A9 parcial).
- **Xephyr:** no se ha probado (aplazado a P6).
- **Pantallas:** solo se probó una pantalla, de 1280x720.
- **Teclado:**
  - solo se usó el mapa de teclado por defecto de Xvfb, donde `super`
    corresponde a Mod4;
  - las variantes del atajo con Bloq Mayús y Bloq Num solo tienen pruebas
    unitarias.
- **Señales:** no se ha probado SIGTERM ni SIGINT.
- **Uso real:** no hay ejecuciones prolongadas ni aplicaciones reales; solo
  `xmessage`, durante unos segundos.
- **Errores X durante la ejecución:** no se produjo ninguno, así que no se
  ha ejercitado su registro sin terminar el proceso.
- **A2:**
  - que no se abre ninguna conexión lo garantizan el aislamiento y el orden
    del código; no se ha observado con trazas de llamadas al sistema;
  - en el arnés, las formas de ruta solo recorren la rama «malformado»,
    porque el tmpfs oculta `/tmp/.X11-unix/X0`;
  - la rama de ruta existente la recorre la prueba unitaria
    `display_0_se_rechaza_en_cualquier_forma` solo cuando
    `/tmp/.X11-unix/X0` existe en el sistema que la ejecuta, como en el
    equipo de desarrollo el 2026-10-12.
- **`ss -xp`:** no se ha usado.
- **Pantalla inexistente (código 14):** solo se ha comprobado leyendo el
  código; ninguna prueba la ha ejercitado.
- **`$HOME/.Xauthority`:** su uso cuando `XAUTHORITY` no está definida solo
  se ha comprobado leyendo el código; todas las ejecuciones que llegaron a
  conectar tenían `XAUTHORITY` definida.
- **Portabilidad del arnés:** sus guardas dependen del `xvfb-run` de
  Debian 13; en otro entorno fallan de forma cerrada.

## Regresión de F1 con el código de F2

Ejecutada el 2026-10-13 con `tests/acceptance/f1/run.sh` sobre el binario
de depuración de F2 (mismo entorno que en «Resultados de las pruebas de
aceptación de F1»): **46 comprobaciones OK, 0 FALLA y 1 OMITIDA**
(`A9-xprop-:0`); código de salida 0. Se mantienen A1–A8 y A9 parcial.

Cambios en el arnés de F1 hechos en F2:

- **`A7-extra-configure` se redefinió.** En F2 la ventana de `xmessage` va
  en mosaico y ocupa toda la pantalla, y su `ConfigureRequest` se deniega
  (ICCCM §4.1.5), así que la comprobación espera ahora que la geometría no
  cambie (0, 0, 1280×720) en lugar de 100, 120, 400×200. Es una
  comprobación negativa con una espera fija de 0,5 s; la A15 de F2 la
  confirma observando los `ConfigureNotify` sintéticos.
- **Código común extraído, sin cambios de comportamiento:** las guardas y la
  preparación de cada Xvfb pasaron de `xvfb-case.sh` a `xvfb-env.sh`, y las
  comprobaciones de A9 a la función `session_checks` de `lib.sh`, para
  reutilizarlas en `tests/acceptance/f2/`. Los identificadores de las
  comprobaciones no cambian.

La misma regresión, repetida el 2026-10-13 sobre el binario de depuración
de F3 (ADR 0005), dio el mismo resultado: **46 OK, 0 FALLA y 1 OMITIDA**;
código de salida 0. El arnés de F1 no cambió en F3.
