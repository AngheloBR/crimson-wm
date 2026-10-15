# ADR 0014: Configuración TOML (F12)

- **Estado:** aceptado. Implementado el 2026-10-15 (F12-B) en la rama
  `feat/f12-toml-config`. Ver «Resultados de las pruebas de aceptación de
  F12».
- **Fecha:** 2026-10-15

![Captura real en Xvfb: sin wm.toml, A ocupa la mitad izquierda y B y V se reparten la pila; con master_percent = 60 y una regla, A ocupa 768 píxeles, B la pila de 512 y V queda flotante encima](../images/crimson-wm-f12-comparison.png)

*Capturas auténticas con el mismo binario de F12 (A147 sin archivo de
configuración y A148 con `master_percent = 60` y la regla `class =
"Viewer"`, `floating = true`). Volcados de `xwd -root` de Xvfb aislados de
1280 × 720; la composición solo añade rótulos, márgenes y bordes, y cada
panel es idéntico píxel a píxel a su captura.*

![Diagrama de F12: dónde se lee la configuración en el arranque, sus cuatro etapas y el código 15, la precedencia de las fuentes, la lectura segura, el esquema, el efecto de master_percent, CRIMSON_RULES y lo aplazado](../images/crimson-wm-f12-config.png)

*Diagrama técnico elaborado a partir del código de F12; no es una captura.
Fuente editable: `docs/images/crimson-wm-f12-config.svg`.*

## Contexto

Hasta F11, los atajos (`keys::BINDINGS`), los programas
(`spawn::PROGRAMS`) y la proporción de la columna maestra
(`layout::MASTER_PERCENT`) eran tablas y constantes compiladas, y las
reglas de ventanas llegaban en la variable provisional `CRIMSON_RULES`
(ADR 0013). F12-A auditó qué valores existen de verdad y propuso una
configuración declarativa limitada a ellos; el usuario aprobó el diseño y
la dependencia antes de F12-B.

## Decisión

### Archivo, momento y alcance

- Archivo `crimson/wm.toml` (deja sitio a futuros `panel.toml` y
  similares), **leído una sola vez** en `main.rs`, después de las capas 1
  (`DISPLAY`) y 2 (token) del ADR 0003 y **antes de conectar con X**. No
  hay recarga en caliente.
- Solo cuatro secciones, las de funciones que existen: `[layout]`,
  `[programs.NOMBRE]`, `[[keybindings]]` y `[[rules]]`. No hay `[general]`
  ni `[focus]`: no tendrían ningún parámetro real.
- **Sin archivo, la configuración es la predeterminada, idéntica a F11**
  (`Config::default()`: 30 atajos, 6 programas, 50 %, ninguna regla). Una
  prueba unitaria lo comprueba atajo por atajo, y otra exige que
  `docs/examples/wm.toml` produzca exactamente la misma `Config`.

> F12-A cifró la tabla predeterminada en 31 atajos. Son **30**: 4 de
> control, 6 de lanzamiento, 16 de escritorios y 4 más (`Mod4+Tab`,
> `Mod4+w`, `Mod4+Shift+w` y `Mod4+f`). La implementación lo detectó con
> la prueba de equivalencia; el código de F11 no cambia.

### Ubicación y precedencia

1. `--config RUTA`: la única fuente; si no existe, error.
2. `$XDG_CONFIG_HOME/crimson/wm.toml`, si la variable es una ruta absoluta.
3. `$HOME/.config/crimson/wm.toml`, si `HOME` es una ruta absoluta.
4. La predeterminada, si no existe ningún archivo.

- `XDG_CONFIG_HOME` **ausente o vacía** no da candidata, sin aviso; una
  ruta **relativa** es inválida según la especificación XDG y se ignora con
  un aviso en el registro. Lo mismo para `HOME`. Si las dos dan la misma
  ruta, se prueba una vez.
- **Gana la primera candidata que existe.** Si existe pero es ilegible o
  inválida, el WM no arranca: nunca se pasa silenciosamente a la siguiente
  ni a la predeterminada. Un enlace simbólico roto cuenta como existente.
  Un componente intermedio que no es un directorio (`ENOTDIR`) o sin
  permiso de búsqueda también es un error, no un archivo ausente.
- Nunca se crea ningún archivo ni directorio.
- El registro indica la fuente: `configuración: RUTA (30 atajos, 6
  programas, maestra al 50 %, 0 reglas)` o `configuración: predeterminada
  (no existe … ni …)`. La línea de la predeterminada no contiene la palabra
  «reglas», como exige A121 de F11.

### Lectura segura (`config::read`)

- `fs::metadata` (sigue enlaces) **antes de abrir**: debe ser un archivo
  regular. Abrir una FIFO sin escritor bloquearía `open(2)`.
- Después de abrir, `fstat` sobre el descriptor: archivo regular, el
  **mismo dispositivo e inodo** que antes, **sin permiso de escritura para
  el grupo ni para otros** (`mode & 0o022 == 0`; el archivo decide qué
  programas se ejecutan con los permisos del usuario) y **como mucho
  64 KiB**, comprobado otra vez al leer (`take(65537)`), por si crece.
- **UTF-8 válido**; el error indica la línea del primer byte inválido.
- Los mensajes describen el problema (tipo de archivo, permisos en octal,
  tamaño, error del sistema) sin el contenido.
- **F12-C:** la apertura usa `O_NONBLOCK | O_NOCTTY | O_CLOEXEC`: una
  sustitución por FIFO entre `metadata` y `open` ya no bloquea el arranque,
  y una sustitución por terminal no le da un terminal de control al WM.
  Se reutiliza `rustix` 1.1.5, ya transitiva de x11rb, como dependencia
  directa con `fs` y `std`, sin añadir paquetes ni features al árbol.

### Análisis TOML (`config::document`)

- **Dependencia:** `toml_parser` 1.1.5 (TOML 1.1), sin la feature
  `unsafe`, que trae solo `winnow` 1.0.4 (ver «Dependencias»).
  `toml_parser` hace el análisis léxico y sintáctico por eventos y la
  decodificación de claves, cadenas (escapes, Unicode) y números; sus
  errores se convierten en «TOML inválido: …» con la posición.
- `document.rs` organiza los eventos en un **árbol con datos propios**
  (`Table`, `Item`, `Value`; sin referencias al texto) y aplica las reglas
  lógicas de la especificación, que `toml_parser` deja al usuario: claves
  repetidas; `[x]` definida dos veces, o sobre una tabla creada con claves
  con punto o en línea; claves con punto que amplían tablas implícitas o
  creadas con claves con punto en la misma sección; tablas en línea y arrays
  estáticos cerrados; `[[x]]` solo sobre un array de tablas. Sus pruebas
  incluyen 45 documentos inválidos de esos tipos.
- Profundidad máxima de arrays y tablas en línea anidados: 8
  (`RecursionGuard`).
- Las fechas y los decimales se reconocen como tipos, sin interpretar su
  valor: ningún campo del esquema los admite, así que siempre se rechazan
  (una fecha con un mes 13 da «debe ser un entero, no una fecha» en lugar
  de un error de sintaxis; el archivo se rechaza igual).

### Esquema y validación (`config::schema`)

- **Claves y tablas desconocidas, tipos incorrectos y valores fuera de
  rango son errores**, en todas las secciones. Los límites de número se
  comprueban antes de validar los elementos.
- **`[layout] master_percent`:** entero de 10 a 90; por defecto 50.
- **`[programs.NOMBRE]`:**
  - nombre: una minúscula ASCII y luego minúsculas, dígitos, `_` o `-`;
    como mucho 32 caracteres; como mucho 64 programas;
  - solo la clave `argv`: de 1 a 32 cadenas de hasta 4096 bytes, sin NUL;
  - `argv[0]`: un nombre que se busca en el `PATH` del WM o una ruta
    absoluta; ni vacío, ni relativo con `/`, ni empezando por `~`, ni con
    `$`. Los demás argumentos se pasan literalmente (`"$HOME"` llega como
    `$HOME`, A137);
  - **no hay** shell, interpolación, expansión de `~`, entorno, directorio
    de trabajo ni usuario configurables, y nada se ejecuta durante el
    análisis. El saneado de F4 (ADR 0006) no cambia.
- **`[[keybindings]]`:**
  - `keys`: modificadores `Shift`, `Control` (`Ctrl`), `Mod1` (`Alt`) y
    `Mod4` (`Super`) y una tecla de la tabla de `keynames.rs`; sin
    espacios; el orden de los modificadores no importa. `Lock`, `Mod2`,
    `Mod3`, `Mod5`, `Hyper`, `Meta` y `NumLock` se rechazan (Bloq Mayús y
    Bloq Num se ignoran siempre; Mod3 y Mod5 no tienen una asignación
    estable: en Xvfb, Mod3 es `ISO_Level5_Shift`);
  - **tabla de teclas curada** (letras, dígitos, edición, flechas,
    puntuación de la fila base, `F1`–`F12`, `Print` y diez teclas
    multimedia `XF86…`), con alias (`Page_Up` = `Prior`). Las letras van en
    minúscula: `J` es un error que sugiere `Shift+j`. Una prueba unitaria
    contrasta cada nombre con `/usr/include/X11/keysymdef.h` y
    `XF86keysym.h`, y A137 comprueba la traducción con el servidor real
    (`F9`, código 75; `XF86AudioMute`, código 121; `Prior`);
  - **las teclas de escritura** (letras, dígitos, puntuación, espacio,
    edición, flechas) exigen `Control`, `Mod1` o `Mod4`, para que un atajo
    no robe la escritura normal; las teclas de función, `Print` y las
    multimedia se admiten solas;
  - `action`: las del núcleo (`exit`, `focus_next`, `focus_prev`, `zoom`,
    `launch` con `program`, `view_desktop` y `move_to_desktop` con
    `desktop` de 1 a 8, `view_previous`, `close`, `kill`, `fullscreen`).
    Falta un parámetro obligatorio, sobra uno que la acción no usa, o el
    programa no existe: error;
  - **duplicados tras normalizar** (máscara y keysym): error, con la línea
    del primero; como mucho 128 atajos;
  - **al menos un atajo `exit`.**
- **Sustitución, no fusión:** `[[keybindings]]` sustituye a la tabla
  predeterminada completa y `[programs]` a la de programas. Con
  `[programs]` y sin `[[keybindings]]`, los atajos predeterminados de
  lanzamiento buscan su programa **por nombre** en la tabla nueva; si falta
  alguno, es un error que lo nombra.
- **`[[rules]]`:** las claves de F11 en TOML (`instance`, `class`, `type`,
  `transient` booleano, `desktop` de 1 a 8, `floating`, `nofocus` y
  `fullscreen` booleanos; `false` equivale a omitirla). Se construyen con
  `rules::window_type` y `rules::rule`, las mismas validaciones que usa el
  analizador de `CRIMSON_RULES`, y se evalúan con el mismo
  `rules::evaluate`. `instance` y `class` de 1 a 256 bytes, sin NUL; como
  mucho 64 reglas. `Rule::line` es la línea de su `[[rules]]`.
- **Formas equivalentes:** tablas con cabecera, claves con punto
  (`layout.master_percent = 60`) y tablas o arrays en línea dan la misma
  `Config`.

### Errores: código 15

- Nuevo `Fatal::Config` con **código de salida 15**, que amplía la tabla
  del ADR 0003. El mensaje es `configuración rechazada: RUTA:LÍNEA:COLUMNA:
  motivo` (sin posición si el problema es del archivo, no del texto). Las
  posiciones señalan la clave en las claves desconocidas y repetidas, y el
  valor en los errores de tipo o rango.
- **Nunca se aplica una configuración parcial:** `Config` se construye
  entera o no se construye, y el WM no ha conectado todavía.
- Las capas 1 y 2 van antes: con `DISPLAY=:0` el código es 10 aunque la
  configuración sea inválida (A146).
- Los mensajes pueden citar claves TOML, nombres de teclas, modificadores,
  acciones, tipos y programas (recortados a 32 caracteres y escapados) y
  números, **nunca argv ni las cadenas de las reglas** (pruebas unitarias
  y A142).

### `--check-config [RUTA]` y `--config RUTA`

- Sintaxis: `crimson-wm [--config RUTA]`, `crimson-wm --check-config
  [RUTA]`, `crimson-wm --help | --version`. La ruta es un argumento
  separado (no hay `--config=RUTA`), no puede estar vacía ni empezar por
  `-` (se escribe `./-archivo`): así un error tipográfico como
  `--config --help` no se toma por un archivo. Cualquier otro argumento da
  el código 2. `--help` y `--version` siguen aceptando un solo argumento.
- `--check-config` usa la misma búsqueda, lectura, validación y precedencia
  de `CRIMSON_RULES` que el arranque, pero **no lee `DISPLAY`,
  `XAUTHORITY` ni el token, no conecta con X, no lanza nada y no escribe
  ningún archivo**. Imprime en la salida estándar la fuente validada y un
  resumen (y los avisos), y termina con 0 o 15.

### Migración de `CRIMSON_RULES`

- La variable **se conserva en F12**, obsoleta. Si está definida,
  **sustituye por completo** a las `[[rules]]` del archivo, sin fusionar.
  El registro conserva primero las líneas de F11 (`CRIMSON_RULES: N
  reglas` o `CRIMSON_RULES inválida (…): se continúa sin reglas`) y añade
  un aviso de obsolescencia y, si el archivo tenía reglas, que no se
  aplican.
- **Inválida:** como en F11, se descarta entera y el WM arranca **sin
  reglas**; tampoco se usan las del archivo (no hay respaldo silencioso).
  Las garantías de F11 se mantienen: no detiene el WM ni cambia ninguna
  salvaguarda (A145).
- Vacía (`CRIMSON_RULES=`): está definida, así que deja el WM sin reglas.
- **El archivo se valida entero antes**: unas `[[rules]]` inválidas dan
  15 aunque `CRIMSON_RULES` fuera a sustituirlas.
- `spawn` sigue eliminándola del entorno de los hijos.

### Aplicación de la `Config`

- `main.rs` pasa la `Config` completa a `wm::run`:
  - `keys::grab_bindings(conn, root, &config.bindings)`: la política de F4
    no cambia. Los atajos de control son obligatorios (código 14 si no se
    pueden capturar) y los de lanzamiento, opcionales (aviso). **Nuevo:**
    si dos keysyms de la tabla están en el mismo código de tecla (otro
    nivel de la distribución), el servidor no lo impide, porque las dos
    capturas son del mismo cliente; ahora se detecta: gana el primero, y
    si el segundo es de control, el WM no arranca (14, A137).
    Si un modificador del atajo también está asignado a Bloq Num, se
    rechaza el atajo obligatorio (14) o se desactiva el opcional: ese
    modificador se elimina al reconocer los eventos y la acción sería
    inaccesible. A137 lo comprueba con Num Lock en Mod1.
  - `Launcher::new(target, config.programs)`: cada hilo de supervisión
    comparte su `Program` con `Arc`; la tabla no cambia mientras el WM
    está en marcha.
  - `Wm::master_percent` → `Clients::plan` → `layout::master_stack(área,
    n, porcentaje)`.
  - `Wm::rules` como en F11.
- Módulos nuevos: `config.rs` (rutas, lectura, errores, `Config`,
  `CRIMSON_RULES`, `--check-config`), `config/document.rs`,
  `config/schema.rs` y `keynames.rs`. El código propio mantiene
  `forbid(unsafe_code)`.

### Lo que nunca es configurable

- Las salvaguardas del ADR 0003 y el saneado del entorno de los hijos.
- El número de escritorios (8), las políticas ICCCM/EWMH, las capas y los
  límites internos (propiedades, supervisión de lanzamientos, filtro de
  repetición).
- Que exista un atajo de salida (su tecla sí se puede cambiar).

## Dependencias

- `toml_parser` 1.1.5+spec-1.1.0 (`MIT OR Apache-2.0`, MSRV 1.85) con
  `default-features = false, features = ["std"]`, y `winnow` 1.0.4 (`MIT`,
  MSRV 1.65.0) como transitiva. Resueltas **sin red**
  (`cargo build --offline`) desde la caché local de Cargo; las sumas de
  `Cargo.lock` coinciden con el SHA-256 de los `.crate`. `cargo tree`
  muestra solo `toml_parser → winnow`.
- `toml_parser` declara `forbid(unsafe_code)` salvo con su feature
  `unsafe`, que no está activa: ninguno de sus 42 bloques `unsafe` se
  compila. `winnow` no tiene ese lint y compila sus 24 bloques `unsafe`
  (cortes sin comprobación de límites en `src/stream/`); **no se han
  auditado**. Detalles en [docs/dependencies.md](../dependencies.md).
- No se añaden `serde`, `toml` ni `toml_edit`.
- F12-C declara también `rustix` 1.1.5 como directa, con las features `fs`
  y `std` que ya activaba x11rb. Versiones, licencias y paquetes resueltos
  permanecen iguales; solo cambia la arista directa en `Cargo.lock`.

## Recarga (diseñada, no implementada)

Necesita un disparador: una señal (con `unsafe` propio o una dependencia
como `signal-hook`) o la IPC prevista. F12 deja preparado el modelo: una
`Config` inmutable, una tabla de atajos que se puede reconstruir y un
`Launcher` y unas reglas que se pueden sustituir de una vez. Diseño
futuro: validar la candidata entera; capturar los atajos nuevos y, si
alguno obligatorio falla, deshacer lo capturado y conservar el estado
anterior; liberar los antiguos; sustituir reglas (solo afectan a ventanas
futuras) y programas (los procesos lanzados no cambian); volver a
organizar si cambia `master_percent`.

## Seguridad

- **Superficie nueva:** un archivo del usuario. Límite de confianza: el
  archivo puede ejecutar programas como el usuario, igual que su shell, así
  que se exige que solo el usuario (o root) pueda escribirlo y que sea un
  archivo regular. **No se comprueba el propietario** (la biblioteca
  estándar no da el UID efectivo sin `unsafe` ni una dependencia nueva) ni
  los permisos de los directorios padre: un directorio de configuración
  escribible por otros sigue siendo un riesgo, como para cualquier programa
  del usuario.
- **Sustituciones:** `O_NONBLOCK` cierra el bloqueo por FIFO entre
  `metadata` y `open`; el tipo y el `dev`/inodo se comprueban sobre el
  descriptor. No se proporciona una instantánea atómica del contenido ni
  protección contra quien pueda modificarlo en el mismo inodo. Un
  atacante con escritura en los directorios padre aún controla qué
  configuración presenta; tampoco se garantiza una apertura sin efectos
  de todo dispositivo que pudiera sustituir al archivo.
- **Analizador:** entradas de 64 KiB como mucho, profundidad 8, límites de
  número antes de validar, sin pánicos (los eventos inesperados son
  errores), sin expresiones regulares ni ejecución de órdenes.
- **Privacidad:** los mensajes no reproducen argv ni las cadenas de las
  reglas.
- La configuración no puede cambiar el display, el token, la marca, el
  entorno de los hijos ni sus privilegios. `--check-config` no abre ninguna
  conexión X.

## Alternativas descartadas

- **`toml` + `serde` (derive):** ergonómico, pero unos diez crates más y
  macros procedurales, y `toml` no estaba en la caché local (necesitaba
  red).
- **`toml_edit`:** conserva el formato para editar archivos; es más
  superficie para una lectura de solo lectura.
- **Un analizador propio de un subconjunto de TOML:** sin dependencias,
  pero rechazaría TOML válido o divergiría de la especificación.
- **Usar la predeterminada con un aviso si el archivo es inválido:** un
  error tipográfico cambiaría los atajos sin que nadie lo viera (no hay
  panel ni IPC). Se prefiere no arrancar, antes de conectar.
- **Fusionar las tablas del archivo con las predeterminadas:** el resultado
  dependería de qué atajos se nombran; se prefiere la sustitución
  explícita.
- **Que `CRIMSON_RULES` y `[[rules]]` a la vez fueran un error:** rompería
  el arnés de F11 y a quien use la variable durante la transición.
- **`--config=RUTA` o rutas que empiezan por `-`:** más ambigüedad para
  ningún caso de uso real.

## Consecuencias y limitaciones

- Sin archivo, el comportamiento es el de F11 (A136 y regresiones F1–F11).
- Los arneses de F1–F11 no leen nunca la configuración del desarrollador:
  los casos de Xvfb ya usaban `HOME` y `XDG_CONFIG_HOME` temporales, y
  `run_bin` (las ejecuciones de `f1/lib.sh` fuera de Xvfb, también con
  `unshare`) ahora fija `HOME` y `XDG_CONFIG_HOME` a un directorio vacío
  de la prueba. `start_wm` acepta argumentos para el binario.
- `Alt` y `Super` suponen la asignación de modificadores habitual.
- **Pendiente** (no se presenta como implementado): recarga; escritorios
  configurables; bordes, márgenes y colores; ratón; políticas de foco;
  otros diseños; scratchpads; geometría en reglas; comodines; eliminar
  `CRIMSON_RULES`; comprobar el propietario del archivo.

## Verificación

### Pruebas unitarias (U24; 200 en total, 55 más que en F11)

- `config` (U24): equivalencia con F11 y con `docs/examples/wm.toml`;
  esquema completo; formas TOML equivalentes; claves, tablas y tipos
  incorrectos; límites exactos y excedidos (`master_percent` 10/90 y 9/91,
  64/65 programas, 128/129 atajos, 32/33 elementos de argv, 4096/4097
  bytes, nombres de 32/33, 64/65 reglas, 256/257 bytes, 65536/65537 bytes
  de archivo); duplicados normalizados; acciones y parámetros; atajo de
  salida; sustitución de tablas; programas inválidos; reglas equivalentes a
  `CRIMSON_RULES` (mismas decisiones); reglas inválidas; precedencia de
  `CRIMSON_RULES`; líneas y columnas; rutas XDG y HOME; precedencia de
  archivos; permisos, tipos de archivo, enlaces, UTF-8, sin lectura, FIFO;
  ausencia de escrituras; informe de `--check-config`; mensajes sin
  valores privados.
- `config::document`: escalares, tablas, claves con punto, arrays de
  tablas, arrays e inline, 45 documentos inválidos, posiciones,
  profundidad.
- `keynames`: combinaciones, alias, orden, errores, letras y dígitos,
  etiquetas, tabla sin repetidos y contraste con las cabeceras de X11.
- `cli`: `--config`, `--check-config` y sus errores; `layout`: 60 %, 10 %
  y 90 %, cobertura sin solapes con cinco proporciones, valores fuera de
  rango sin tamaños nulos; `error`: código 15.

### Pruebas de aceptación (`tests/acceptance/f12/`)

| Caso | Qué comprueba |
|---|---|
| A136 | Sin archivo: fuente predeterminada, mosaico al 50 %, `Mod4+j`, `Mod4+Return` lanza el kitty falso, ningún registro de reglas, `--check-config` |
| A137 | `master_percent = 60` (768/512), atajos con Alt, `Mod1+Prior`, `F9` y `XF86AudioMute` con el servidor real, argv literal, sin los atajos predeterminados, escritorios; colisión de códigos de tecla → 14 |
| A138 | `[[rules]]` de F11 desde el archivo: escritorio, primera regla gana, flotante, `nofocus`, diálogo; líneas del archivo |
| A139 | TOML mal formado en Xvfb: 15, sin conexión, servidor intacto; y en `unshare` (sintaxis, clave y tabla repetidas) |
| A140 | Tabla y clave desconocidas, tipo y rango |
| A141 | Atajos repetidos tras normalizar, sin `exit`, tecla, modificador, tecla de escritura, escritorio, acción, parámetro sobrante |
| A142 | Programas: argv vacío, NUL, sin definir, `~`, `$`, ruta relativa, sustitución incompleta; ningún mensaje reproduce argv |
| A143 | Límites exactos (válidos) y excedidos: 64 KiB, 128 atajos, 64 programas, 64 reglas, 32 argv |
| A144 | Directorio, enlace roto, no UTF-8, 0664, FIFO, `--config` ausente, sin respaldo de XDG a HOME; en Xvfb: 000, 0660 y FIFO, sin conexión |
| A145 | Precedencia `--config` > XDG > HOME > predeterminada, XDG relativa; `CRIMSON_RULES` sustituye a `[[rules]]` con avisos; inválida, sin reglas |
| A146 | `:0` y falta de token con configuración válida o inválida → 10; marca inválida → 10; configuración inválida antes que la marca → 15; `--check-config` sin X y en Xvfb, sin conectar |
| A147 | Panel 1 de la comparación, sin configuración |
| A148 | Panel 2 con `master_percent = 60` y Viewer flotante, y composición |
| A149 | Estado posterior, pánicos, residuos y procesos (como A9) |

En `unshare -rmn` cada caso inválido se ejecuta en modo normal con
`DISPLAY=:57` (sin servidor): si la configuración se leyera después de
intentar conectar, el código sería 11; es 15. Dentro del espacio de nombres
el usuario es root y puede leer un archivo 000, así que ese caso va en
Xvfb.

## Resultados de las pruebas de aceptación de F12

Ejecuciones del 2026-10-15 en Xvfb con el binario de depuración de F12 y
la toolchain de Debian (`env PATH=/usr/bin:/bin`). Los arneses pidieron
los binarios a Cargo.

- **Unitarias:** 200 OK, 0 fallos. Las comprobaciones que pueden omitirse
  según el sistema (contraste con las cabeceras de X11, FIFO, socket,
  archivo 000) se ejecutaron todas.
- **F12:** 186 OK, 0 FALLA y 1 OMITIDA (`A149-xprop-:0`, la consulta de la
  raíz de `:0`, que se omite deliberadamente, como A9).
- **Regresiones F1–F11**, con el binario de F12, todas con 0 FALLA y 1
  OMITIDA por el mismo motivo: F1, 46 OK; F2, 47 OK; F3, 61 OK; F4, 79 OK;
  F5, 142 OK; F6, 122 OK; F7, 123 OK; F8, 106 OK; F9, 166 OK; F10, 140 OK;
  F11, 132 OK. Son los mismos recuentos que en F11. F9, F10 y F11 usaron
  sus binarios históricos (`CRIMSON_F8_BIN`, `CRIMSON_F9_BIN` y
  `CRIMSON_F10_BIN`) solo para sus paneles de comparación.
- **Defecto del arnés corregido durante F12-B,** en la primera ejecución
  de F12 (172 OK, 14 FALLA, ninguna del WM): `isolated.sh` asignaba una
  variable dentro de una tubería (una subshell) y se detenía en A144; los
  14 FALLA eran comprobaciones sin resultado. Después, F12 pasó entera.

Son los resultados de esas ejecuciones concretas, no una garantía para
cambios posteriores.

La auditoría independiente F12-C, las correcciones y las nuevas ejecuciones
se registran en [Auditoría F12-C](../f12-audit.md).

## Documentación visual

### Captura real (A147 y A148)

- **Escena:** A (rojo, `8B0A1F`) y B (negro, `1F2328`), sin `WM_CLASS`; V
  (`viewer`, `Viewer`; gris claro, `D0D7DE`; pide 700, 120, 460 × 300).
- **Panel 1 (sin `wm.toml`):** las tres en mosaico al 50 %: A 0 0 640 720,
  B 640 0 640 360 y V 640 360 640 360; foco en V.
- **Panel 2 (`master_percent = 60` y regla `Viewer` flotante):** A
  0 0 768 720, B 768 0 512 720 y V flotante en 700, 120, 460 × 300, encima;
  foco en V.
- **Comprobaciones:** colores en puntos conocidos; dos capturas de
  1280 × 720; cada panel idéntico a su captura (0 píxeles distintos).
  Composición de 2688 × 1024. SHA-256 de `crimson-wm-f12-comparison.png`:
  `b722e7099877db17f60400b8d19864c57407322c56f483cf8ae058f144bc8692`.

### Diagrama

`crimson-wm-f12-config.svg`, renderizado con `tools/render-svg.py …
--scale 2` a 2800 × 3000.
