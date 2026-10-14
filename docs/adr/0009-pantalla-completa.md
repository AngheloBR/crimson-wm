# ADR 0009: Pantalla completa (F7)

- **Estado:** aceptado. Implementado el 2026-10-14 (F7-B). Las 111 pruebas
  unitarias pasaron, y las de aceptación A63 y A65–A74 pasaron en Xvfb; A75
  quedó parcial por el mismo motivo que A9 (ver «Resultados de las pruebas
  de aceptación de F7»).
- **Fecha:** 2026-10-14
- **Corrección F7-B.1 (2026-10-14):** `_NET_WM_STATE` se lee entera y se
  actualiza de forma atómica; desaparece el límite de 64 átomos (ver
  «Lectura y escritura completas de `_NET_WM_STATE`»).
- **Actualización F8 (2026-10-14):** `restack_fullscreen` cuenta la
  familia: si el foco lo tiene un diálogo de la ventana a pantalla
  completa, esta sigue arriba con sus diálogos encima, y solo baja al
  fondo cuando se enfoca una ventana ajena (ADR 0010).
- **Actualización F9 (2026-10-14):** solo admiten la pantalla completa
  los tipos con `Class::fullscreen` (`NORMAL`, `DIALOG`, `UTILITY`,
  `TOOLBAR` y sin tipo); en los demás y en las fijas se rechaza, también el
  estado inicial, y FULLSCREEN se retira de `_NET_WM_STATE`. La enfocada
  queda por encima del `DOCK`; sin foco, baja al fondo pero por encima de
  las `DESKTOP` (ADR 0011).
- **Actualización F10 (2026-10-14):** la pantalla completa sigue ocupando
  la pantalla física, aunque los paneles reserven espacio; al salir, una
  ventana de mosaico vuelve a su hueco del área de trabajo, y el marco de
  respaldo de una flotante se calcula sobre esa área (ADR 0012).

![Captura real en Xvfb: tres ventanas xlogo en master-stack; la segunda a pantalla completa con Mod4+f; y de nuevo la disposición original, idéntica píxel a píxel](../images/crimson-wm-f7-comparison.png)

*Captura auténtica (A74): volcados de `xwd -root` de un Xvfb aislado de
1280 × 720 con Crimson WM en marcha y tres `xlogo` estáticas. La
composición solo añade rótulos, márgenes y bordes; cada panel es idéntico
píxel a píxel a su captura, y los paneles 1 y 3 lo son entre sí.*

![Diagrama de F7: estados normal y pantalla completa, qué se guarda y qué se calcula, ejemplo en mosaico, pila y foco, peticiones _NET_WM_STATE, salida y recuperación, y lo aplazado](../images/crimson-wm-f7-fullscreen.png)

*Diagrama técnico elaborado a partir del código de F7; no es una captura.
Fuente editable: `docs/images/crimson-wm-f7-fullscreen.svg`.*

## Contexto

Navegadores, reproductores y presentaciones piden la pantalla completa con
`_NET_WM_STATE_FULLSCREEN` (EWMH), antes o después de mapear su ventana. En
F6 esa petición se ignoraba y no estaba declarada. F7 la implementa para
ventanas en mosaico y flotantes. Tiene que convivir con los ocho
escritorios, el foco por historial y la publicación EWMH, y devolver cada
ventana exactamente a su estado anterior.

## Decisión

### Modelo de estado

- **`Client.fullscreen: Option<Restore>`** es la única fuente de verdad. La
  geometría (`Clients::plan`) y `_NET_WM_STATE` se derivan de él; no hay
  ninguna copia.
- **En mosaico** (`Restore { frame: None }`): no se guarda nada. La ventana
  sale del cálculo master-stack, pero conserva su posición en la lista, así
  que al salir `plan` le devuelve su hueco y el orden queda intacto.
- **Flotante** (`Restore { frame: Some(Frame) }`): al entrar se guarda su
  marco real con `GetGeometry` (x, y, ancho, alto y borde), que se restaura
  exactamente al salir.
- **A pantalla completa:** el área de la pantalla (`Wm.area`) y borde 0.
- **Como mucho una por escritorio** (nueva invariante de `Workspaces`): la
  última petición válida gana, también al mover una ventana a otro
  escritorio. La anterior sale en la misma operación.
- **Idempotencia:** añadir lo que ya está o quitar lo que no está no cambia
  nada; ni siquiera se registra.

### Módulos

| Módulo | Cambio |
|---|---|
| `src/clients.rs` | `Frame`, `Restore`, `Frame::fallback`, `Client.fullscreen`, `Removed.fullscreen`, `fullscreen()`, `plan(area)` y `WindowInfo.fullscreen` |
| `src/workspaces.rs` | `enter_fullscreen` y `leave_fullscreen` (una por escritorio, idempotentes), `desk_mut`, y `move_window`, que conserva el estado y devuelve la ventana desplazada (`displaced`) |
| `src/ewmh.rs` | `Request::Fullscreen`, `StateAction` (REMOVE, ADD y TOGGLE, con `apply`), `with_fullscreen` y `Invalid::{Action, UnsupportedState}`; `_NET_SUPPORTED` pasa a 16 átomos |
| `src/keys.rs` | `Action::Fullscreen`: `Mod4+f`, obligatorio (código 14) y con filtro de repetición automática |
| `src/wm.rs` | `set_fullscreen` (único punto que cambia el estado), `restore_window`, `frame_of`, `write_fullscreen_state`, `restack_fullscreen`, `place`; `arrange` a partir de `plan`; integración con `MapRequest`, adopción, `ConfigureRequest`, `forget`, movimientos, peticiones EWMH y `release` |
| `src/layout.rs` | Sin cambios: `master_stack` recibe solo las ventanas que no están a pantalla completa |
| `examples/test-client.rs` | `--fullscreen`, `--state`, `--dialog`, `--geometry` y `--border`, y registro de los `ConfigureNotify` recibidos |
| `tools/render-svg.py` (nuevo) | Renderizador SVG → PNG reproducible (ver «Documentación visual») |

### Geometría, pila y foco

- **`arrange`:**
  1. aplica `plan` al escritorio activo, enviando solo la geometría que
     cambia;
  2. sube las flotantes que no están a pantalla completa;
  3. coloca en la pila la de pantalla completa.
- **`restack_fullscreen`:** la ventana a pantalla completa del escritorio
  activo sube por encima de todo si tiene el foco y baja al fondo si no.
  Como las demás ventanas en mosaico se reorganizan detrás, la enfocada
  (en mosaico, flotante o un diálogo nuevo) queda visible. Se llama tras
  cada decisión de foco, del WM o de un cliente (`FocusIn`).
- **El foco no cambia** al entrar ni al salir de pantalla completa.
- **`ConfigureRequest`** de una ventana a pantalla completa, en mosaico o
  flotante: se deniega con un `ConfigureNotify` sintético del área (ICCCM
  §4.1.5), sin tocar el marco guardado.

### Escritorios

- **El estado es de la ventana:** se oculta y se muestra con su escritorio.
  Una petición para una ventana de un escritorio oculto se acepta sin
  cambiar el escritorio activo ni el foco, sea cual sea su origen. En una
  flotante se guarda su marco al momento, y la geometría de pantalla
  completa se aplica al mostrarse.
- **Mover** (`Mod4+Shift+n` o `_NET_WM_DESKTOP`) conserva el estado. Si el
  destino ya tenía otra ventana a pantalla completa, esa sale (la última
  gana), sin cambiar de escritorio.
- **Al cerrar, destruir o retirar** una ventana a pantalla completa, su
  escritorio se reorganiza y el foco va a la siguiente del historial, como
  en F5.

### EWMH: `_NET_WM_STATE`

- **Peticiones:** `data[0]` = 0 REMOVE, 1 ADD o 2 TOGGLE (otra acción se
  rechaza), y `_NET_WM_STATE_FULLSCREEN` en `data[1]` o en `data[2]`. La
  fuente (`data[3]`) debe ser 0, 1 o 2. Si ninguno de los dos estados es
  FULLSCREEN, la petición se rechaza (`UnsupportedState`). Si el otro estado
  no es FULLSCREEN, se ignora.
- **Estado inicial:** si `_NET_WM_STATE` contiene FULLSCREEN al llegar el
  `MapRequest`, la ventana se gestiona ya a pantalla completa. Una flotante
  guarda el marco que pidió al crearse.
- **Política de los estados ajenos:**
  - el WM nunca reescribe la propiedad desde su memoria;
  - para cambiar FULLSCREEN la lee entera, quita o añade solo ese átomo y
    la escribe con los demás en su orden (sección siguiente);
  - no interpreta los átomos ajenos (`SKIP_TASKBAR`, `MAXIMIZED_*`, etc.),
    no los declara en `_NET_SUPPORTED` y no los activa;
  - un cliente no puede añadirlos por petición: no se aplican ni se
    escriben.
- **Al retirar una ventana a pantalla completa** solo se quita FULLSCREEN;
  sus estados ajenos se quedan (A70).
- **`_NET_SUPPORTED`** añade `_NET_WM_STATE` y `_NET_WM_STATE_FULLSCREEN`,
  y nada más.

### Lectura y escritura completas de `_NET_WM_STATE`

**Causa del límite anterior** (F7-B): `write_fullscreen_state` hacía tres
cosas mal:
- leía la propiedad con `long_length = 64` y no miraba `bytes_after`, así
  que con más de 64 átomos solo veía los primeros, y la escritura `REPLACE`
  borraba el resto;
- pedía el tipo `ATOM`: si la propiedad tenía otro tipo, el servidor no
  devolvía datos, el WM la trataba como vacía y la sobrescribía;
- la detección inicial (`window_info`) tenía el mismo límite.

**Solución (F7-B.1):**
- **Lectura completa:** una sola petición
  `GetProperty(_NET_WM_STATE, AnyPropertyType, 0, MAX_STATE_ATOMS)`, con un
  máximo de 1024 átomos (4 KiB), que acota la memoria aunque la propiedad
  sea enorme.
- **Validación** (`ewmh::StateRead::atoms`), sin E/S:
  - ausente: lista vacía;
  - de otro tipo o formato: `Malformed`;
  - con `bytes_after > 0` (más de 1024 átomos, lectura incompleta):
    `TooLarge`.

  En ambos casos de error **no se escribe nada**, ni siquiera en parte, y
  se registra «`_NET_WM_STATE` de 0x… no se modifica (…): se conserva tal
  cual».
- **Decisión** (`ewmh::update_state`, sin E/S): `None` si no hay que
  cambiar nada (idempotente), o el valor nuevo. Al activar, un FULLSCREEN
  que ya estaba se queda en su sitio, y uno repetido queda una sola vez; si
  no estaba, se añade al final. Al desactivar, se quitan todas sus
  apariciones. Los demás átomos se conservan todos, en su orden.
- **Atomicidad:** lectura, decisión y escritura se hacen con el servidor
  capturado (`GrabServer`), así que ningún cliente puede cambiar la
  propiedad entre medias.
- **Coherencia:**
  - **Al entrar**, la propiedad se actualiza antes que el modelo: si se
    rechaza, o la ventana ya no existe, la ventana no entra en pantalla
    completa.
  - **Al salir** (o al retirarse la ventana), la geometría se restaura
    siempre. Si la propiedad ya no se puede reescribir, se deja como está.
    Solo ocurre si el cliente la cambió después del mapeo, cosa que EWMH no
    permite.
- **Detección inicial:** el `MapRequest` y la adopción usan la misma
  lectura completa. Una propiedad inválida o incompleta no cuenta como
  petición de pantalla completa y se registra.

**Garantías:**
- Cuando el WM escribe `_NET_WM_STATE`, la propiedad contiene exactamente
  los átomos que tenía (hasta 1024), en el mismo orden, con FULLSCREEN
  añadido o quitado.
- Nunca escribe una propiedad que no haya leído entera ni una de otro tipo.
- Ningún otro cliente se intercala entre la lectura y la escritura.

### Salida y recuperación

- **Salida normal** (`Mod4+Shift+Escape`, `SelectionClear` o error tras la
  identificación): antes de la limpieza de F6, todas las ventanas a
  pantalla completa de todos los escritorios vuelven a su hueco (calculado
  con `place`, también en los ocultos) o a su marco, y pierden FULLSCREEN.
- **SIGTERM o SIGKILL:**
  - **no hay restauración** (sin manejadores de señales): quedan con el
    área y FULLSCREEN, y el save-set vuelve a mostrar las ocultas;
  - el siguiente arranque en ese servidor se rechaza (código 12,
    fail-closed, ADR 0008) hasta borrar a mano `_NET_SUPPORTING_WM_CHECK`;
  - después, la adopción respeta FULLSCREEN. Si varias ventanas del
    escritorio 1 lo piden, se recorren de abajo arriba y gana la más alta
    de la pila;
  - una flotante adoptada cuyo marco ya ocupa el área no guarda ese marco:
    al salir de pantalla completa recibe uno de respaldo, centrado y de 2/3
    del área (853 × 480 en 1280 × 720), sin borde. **Su geometría original
    no se recupera** tras una terminación abrupta.

### Carreras X11

- **Ventana destruida entre la petición y el cambio:** `GetGeometry` o la
  lectura de `_NET_WM_STATE` fallan y no se cambia nada; su `DestroyNotify`
  la olvida. La escritura y la configuración de restauración descartan sus
  errores (`ignore_error`): la ventana puede haber desaparecido.
- **`ClientMessage` de una ventana aún no gestionada** (antes del mapeo):
  se ignora y se registra. EWMH pide que antes del mapeo el cliente ponga
  la propiedad, y esa sí se lee en el `MapRequest`.
- **`ConfigureRequest` justo después de pedir la pantalla completa:** se
  deniega con el área.
- **Peticiones seguidas:** se resuelven una a una en el bucle de eventos y
  dejan el último estado.

## Verificación

### Pruebas unitarias (111 en total, 17 más que en F6)

- **U17** (`clients`, 4; `workspaces`, 3):
  - `plan` sin pantalla completa, con una ventana en mosaico (las demás se
    reparten el área y ella recupera su hueco) y con una flotante;
  - `unmanage` de una ventana a pantalla completa;
  - marco de respaldo, también con áreas impares o mínimas;
  - una por escritorio, «la última gana» e idempotencia;
  - escritorios independientes y escritorio oculto, y que una flotante
    pierda la geometría de pantalla completa al salir;
  - mover con desplazamiento.

  Las invariantes de las 5 000 operaciones pseudoaleatorias incluyen ahora
  entrar y salir de pantalla completa.
- **U18** (`ewmh`, 3, y la lista ampliada; `keys`, 1):
  - decodificación de `_NET_WM_STATE`: acciones, FULLSCREEN en `data[1]` o
    `data[2]`, estado no gestionado, acción 3 y fuente 9;
  - acciones idempotentes;
  - valor de la propiedad con los estados ajenos conservados, sin
    repeticiones;
  - `_NET_SUPPORTED` de 16 átomos;
  - `Mod4+f` (código 41), obligatorio y sin conflictos.
- **U18, ampliación de F7-B.1** (`ewmh`, 6 pruebas):
  - propiedad ausente o vacía;
  - solo FULLSCREEN e idempotencia;
  - FULLSCREEN al inicio, en medio y al final, y repetido;
  - 200 estados ajenos y justo en el límite de 1024;
  - mal formada (tipo `CARDINAL`, formato 8);
  - lectura incompleta (`bytes_after > 0`), sin escritura parcial y sin
    contar como petición.

### Pruebas de aceptación (`tests/acceptance/f7/`)

Mismo aislamiento que en F4–F6. Solo `xev`, `xlogo` y `test-client`; nunca
aplicaciones reales.

| Id | Comprobación |
|---|---|
| A63 | `Mod4+f` en mosaico: B ocupa el área, A y C se reparten el mosaico detrás, B arriba y enfocada, `_NET_WM_STATE`; de nuevo `Mod4+f`: mosaico y orden idénticos, sin FULLSCREEN y con el mismo foco; mantener `Mod4+f` 1,5 s cambia una sola vez |
| A65 | ADD (fuente 2), ADD repetido (sin efecto), TOGGLE (fuentes 1 y 0), REMOVE sin estado (sin efecto) y REMOVE; `MAXIMIZED_VERT`, acción 3 y raíz rechazados; FULLSCREEN en `data[2]` con otro estado en `data[1]`: solo FULLSCREEN |
| A66 | Flotante con marco 100, 80, 300 × 200 y borde 3: el área con borde 0, y de vuelta exactamente a su marco |
| A67 | 20 `Mod4+f` en mosaico y 20 TOGGLE en una flotante: geometrías finales idénticas a las iniciales |
| A68 | Pantalla completa independiente en los escritorios 1 y 2; la segunda petición en el escritorio 2 gana; una petición para una ventana del escritorio oculto no cambia el activo ni el foco; al volver, la ventana a pantalla completa sin foco queda al fondo |
| A69 | `Mod4+Shift+3` conserva el estado y desplaza la del destino; el origen se reorganiza y el foco pasa a A; `xdotool set_desktop_for_window` la mueve sin cambiar de escritorio |
| A70 | Cerrar con `Mod4+w`, destruir (`kill`) y retirar una ventana a pantalla completa: mosaico y foco restaurados; un estado ajeno (`SKIP_TASKBAR`) se conserva al entrar y al retirarse. Desde F7-B.1: 100 estados ajenos se conservan en su orden al entrar y al salir; con 1100 átomos (`TooLarge`) y con una propiedad `CARDINAL` (`Malformed`), la propiedad queda intacta y la ventana no entra en pantalla completa |
| A71 | `_NET_WM_STATE` antes del mapeo, en mosaico y flotante; `ConfigureRequest` denegada con el `ConfigureNotify` sintético del área; la flotante vuelve al marco que pidió al crearse |
| A72 | Pila: arriba con el foco; al fondo al enfocar otra; vuelve arriba; un diálogo nuevo queda encima; al cerrarlo, vuelve arriba. `_NET_CLIENT_LIST_STACKING` coincide con `QueryTree` |
| A73 | Salida normal: huecos y marco restaurados y sin FULLSCREEN, en los dos escritorios. SIGTERM: el área y FULLSCREEN se conservan y las ventanas quedan visibles; el reinicio se rechaza (12). Tras borrar la propiedad, la adopción da FULLSCREEN a la más alta de la pila y la flotante sale al marco de respaldo |
| A74 | Captura real (abajo) |
| A75 | Como A9: sin pánicos ni residuos, sin programas falsos en marcha ni aplicaciones nuevas, y bspwm y sxhkd con el mismo PID |

### Cambios en los arneses anteriores

- **F6 A53:** la lista esperada de `_NET_SUPPORTED` incluye los dos átomos
  nuevos.
- **`run.sh` de F5, F6 y F7:** el cliente de prueba se compara solo con
  `examples/` y `Cargo.*`. No depende de `src/`, y Cargo no lo vuelve a
  enlazar cuando solo cambia `src/`, así que la comprobación anterior lo
  daba por desfasado.

## Resultados de las pruebas de aceptación de F7

Ejecutadas el 2026-10-14 con `tests/acceptance/f7/run.sh` sobre el binario
de depuración (Xvfb 2:21.1.16-1.3+deb13u4, x11-utils 7.7+7,
x11-xserver-utils 7.7+11, x11-apps (`xwd`, `xlogo`), xdotool
1:3.20160805.1-5.1, ImageMagick 7).

La ejecución de F7-B dio **119 comprobaciones OK, 0 FALLA y 1 OMITIDA**
(`A75-xprop-:0`), con código de salida 0 y todos los `xvfb-run` con 0. La
primera solo tuvo un fallo del arnés: un identificador de comprobación
duplicado en A73, que se corrigió.

**F7-B.1:**
- **F7:** **123 OK, 0 FALLA y 1 OMITIDA**: las 119 anteriores más las cuatro
  nuevas de A70.
- **F6** se repitió porque cambió la lectura de `_NET_WM_STATE` en cada
  `MapRequest`: 122 OK y 1 OMITIDA.
- **F1–F5** no se repitieron: la corrección solo afecta a esa propiedad.
- **Imagen de comparación:** la de A74 en esta ejecución tiene 0 píxeles
  distintos de la del repositorio. Las imágenes no se regeneraron.
- **Clippy:** se ejecutó con el de Debian (0.1.85), invocado directamente.
  En el entorno apareció rustup (toolchain por defecto 1.99), y `cargo
  clippy` buscaría primero sus binarios en `~/.cargo/bin`. Ese clippy más
  reciente señala dos lints nuevos que no corresponden a la toolchain del
  proyecto (ADR 0001); uno de ellos propone una API que no existe en Rust
  1.85.

Regresiones, sobre el mismo binario:
- **F1:** 46 OK y 1 OMITIDA. Como siempre, el `xvfb-run` de A8 termina con
  1 (ADR 0003).
- **F2:** 47 OK y 1 OMITIDA.
- **F3:** 61 OK y 1 OMITIDA.
- **F4:** 79 OK y 1 OMITIDA.
- **F5:** 142 OK y 1 OMITIDA.
- **F6:** 122 OK y 1 OMITIDA, con la lista de `_NET_SUPPORTED` ajustada.

Todas terminaron con código 0.

## Documentación visual

### Captura real (A74)

La genera el propio arnés, con el aislamiento de las demás pruebas:
1. Tres `xlogo` estáticas en la paleta de Crimson (rojo sobre blanco,
   blanco sobre negro y negro sobre gris) en master-stack.
2. `xwd -root` en tres momentos: antes, tras `Mod4+f` sobre la segunda
   ventana y tras volver a pulsarlo.
3. ImageMagick convierte los XWD a PNG y compone la comparación con
   rótulos, márgenes y bordes, sin retocar el contenido.

Validaciones registradas en la ejecución:
- **Geometrías** (`xwininfo`) en cada momento: L1 0 0 640 720, L2 640 0
  640 360 y L3 640 360 640 360; luego L2 0 0 1280 720; luego de nuevo las
  primeras.
- **Dimensiones:** las tres capturas, de 1280 × 720.
- **Pantalla completa:** las cuatro esquinas de la captura 2 tienen el
  fondo de L2 (`#1F2328`).
- **Restauración:** 0 píxeles distintos entre «antes» y «restaurado».
- **Composición** (4012 × 984): cada panel es idéntico píxel a píxel a su
  captura, comparando el recorte en la posición conocida.
- **Sumas SHA-256:** se guardan en los resultados. Las de las capturas 1 y
  3 difieren solo por los metadatos de fecha del PNG; sus píxeles son
  idénticos. La de `docs/images/crimson-wm-f7-comparison.png` es
  `cc9015f30f42d52f4ff79f2e6f344a439d0823860f942608e18d2e05763a6fb6`, la
  misma que la de la ejecución definitiva.

Las tres capturas sin componer quedan como evidencia temporal en el
directorio de resultados; en el repositorio solo se registra la
comparación.

### Diagrama y renderizador

- `docs/images/crimson-wm-f7-fullscreen.svg` (fuente editable) y su PNG
  (2800 × 3120, escala 2).
- **`tools/render-svg.py`:**
  - llama con `ctypes` a librsvg (`librsvg-2.so.2`), cairo y GObject del
    sistema; sin módulos de Python adicionales, sin instalar nada y sin red;
  - recibe rutas explícitas (`ENTRADA.svg SALIDA.png [--scale N]`) y no
    contiene rutas personales;
  - termina con 2 si falta una biblioteca o un argumento no es válido, y
    con 1 si el SVG no se carga o el PNG no se escribe;
  - reproduce el PNG de F6 sin ningún píxel distinto.
- `docs/images/crimson-wm-overview.svg` se actualiza a F7. Las imágenes de
  F2 y F6 no cambian.

## Limitaciones conocidas

- **Tras SIGTERM o SIGKILL** no hay restauración. Una flotante adoptada sin
  marco conocido sale al marco de respaldo (centrado, 2/3), no al original.
- **Área fija:** la de la pantalla al arrancar. Sin paneles ni struts, que
  la pantalla completa cubriría; sin RandR ni varios monitores
  (`_NET_WM_FULLSCREEN_MONITORS`).
- **Ventanas sin foco** (avisos, paneles) a pantalla completa: se aplica,
  pero solo suben si reciben el foco.
- **`_NET_WM_STATE` de más de 1024 átomos o mal formada:** el WM no la
  modifica, así que esa ventana no puede entrar en pantalla completa. Si un
  cliente la estropea después del mapeo (contra EWMH), al salir de pantalla
  completa se restaura la geometría pero la propiedad se deja como está.
- **Otros estados** (maximizar, ABOVE, STICKY, etc.): se ignoran; para el
  cliente parece que el WM no los admite.
- **`_NET_ACTIVE_WINDOW`** sobre una ventana a pantalla completa sin foco
  la sube al enfocarla, pero no sube una flotante tapada que no lo esté
  (ADR 0008).

## Alternativas consideradas

- **Reservar el hueco de la ventana en el mosaico:** descartado; al perder
  el foco, la ventana quedaría a la vista en ese hueco.
- **Varias ventanas a pantalla completa por escritorio:** descartado; el
  orden de pila dejaría de ser predecible.
- **Siempre encima, o salir de pantalla completa al cambiar el foco:**
  descartado. EWMH recomienda subirla cuando tiene el foco, y salir sería
  sorprendente.
- **Reescribir `_NET_WM_STATE` desde la memoria del WM:** descartado; se
  perderían los estados del cliente que leen los paneles.
- **Guardar en el WM la geometría de todas las flotantes:** innecesario;
  basta guardarla al entrar.

## Consecuencias

- `arrange` pasa a ser el plan de `Clients` más el apilamiento.
  `Clients::plan` es la única fuente de geometría de lo que coloca el WM.
- `_NET_SUPPORTED` crece a 16 átomos y A53 de F6 se ajusta.
- A partir de F7 hay una captura real por fase cuando aporta evidencia, y
  `tools/render-svg.py` hace reproducibles los PNG de los diagramas.
