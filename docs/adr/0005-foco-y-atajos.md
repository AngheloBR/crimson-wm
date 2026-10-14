# ADR 0005: Foco y navegación por teclado (F3)

- **Estado:** aceptado. Implementado el 2026-10-13 (F3-B). Las 48 pruebas
  unitarias pasaron, y las de aceptación A22–A31 pasaron en Xvfb; A32 quedó
  parcial por el mismo motivo que A9 (ver «Resultados de las pruebas de
  aceptación de F3»).
- **Fecha:** 2026-10-13 (atajo de Zoom cambiado a `Mod4+Shift+Return` y
  combinaciones reservadas añadidas el mismo día, en F3-B.1)
- **Actualización F4 (2026-10-13):** las seis combinaciones de aplicaciones
  ya no están reservadas: lanzan programas y sus atajos son opcionales
  (ADR 0006). Las secciones afectadas lo indican.
- **Actualización F5 (2026-10-13):** el foco, el historial y la navegación
  pasan a ser por escritorio virtual. Los `FocusIn` de ventanas de otro
  escritorio se ignoran. Se añaden 17 atajos obligatorios de escritorio y
  `WM_STATE` (ADR 0007).
- **Actualización F6 (2026-10-13):** `WM_TAKE_FOCUS` ya no se envía con
  `CurrentTime`. Lleva la marca de la pulsación o, si no la hay, la hora del
  servidor; `SetInputFocus` sigue usando `CurrentTime` en ese caso. Los
  cambios de `WM_HINTS` y `WM_PROTOCOLS` se siguen con `PropertyNotify`.
  `Mod4+w` y `Mod4+Shift+w` dejan de estar reservados: cierran ventanas. Se
  atienden las peticiones `_NET_ACTIVE_WINDOW` (ADR 0008).
- **Actualización F7 (2026-10-14):** `Mod4+f`, obligatorio, alterna la
  pantalla completa. Cada decisión de foco recoloca en la pila la ventana a
  pantalla completa del escritorio: arriba si tiene el foco y al fondo si
  no (ADR 0009).

## Contexto

Hasta F2 el WM no gestionaba el foco: el servidor lo hacía seguir al
puntero. F3 añade el foco explícito y la navegación por teclado entre las
ventanas del diseño master-stack (ADR 0004), sin decoraciones, IPC ni
escritorios virtuales.

## Decisión

### Módulos

| Módulo | Cambio |
|---|---|
| `src/keys.rs` | Tabla fija de atajos (`Action`: `Exit`, `FocusNext`, `FocusPrev`, `Zoom`) en lugar del atajo de salida suelto |
| `src/clients.rs` | Historial de foco, orden de navegación, `zoom`, `InputModel`, `input_hint` y la marca `focusable`. Sigue sin E/S |
| `src/wm.rs` | Captura de los atajos, lectura de `WM_HINTS` y `WM_PROTOCOLS`, `SetInputFocus`, `WM_TAKE_FOCUS`, `FocusIn`, `_NET_ACTIVE_WINDOW` y limpieza al salir |

`main.rs`, `safeguards.rs`, `display.rs` y `connect.rs` no cambian. No hay
dependencias nuevas.

### Atajos

| Atajo | Acción |
|---|---|
| `Mod4+j` | Enfoca la siguiente ventana del orden de navegación, con vuelta al principio |
| `Mod4+k` | Enfoca la anterior, con vuelta al final |
| `Mod4+Shift+Return` | Zoom: la ventana enfocada pasa a maestra; si ya lo es, se intercambia con la segunda. El foco no cambia |
| `Mod4+Shift+Escape` | Salida con 0 (ADR 0003) |

En F3, `Mod4+Return` no se captura ni tiene acción: está reservado para la
terminal (ver «Combinaciones reservadas»), y hasta entonces la pulsación
llega a la ventana enfocada. **Desde F4** lanza la terminal (ADR 0006).

- Los modificadores se comparan de forma exacta, sin contar Bloq Mayús ni
  Bloq Num: `Mod4+Shift+j` no hace nada. Todos los atajos se capturan
  también con esos bloqueos activos.
- Todos son obligatorios: si falta la tecla en el mapa o falla `GrabKey`
  (por ejemplo, con `BadAccess` porque otro cliente la tiene), el WM
  termina con 14, como con el atajo de salida en F1. Desde F4, los atajos
  de lanzamiento son la excepción: opcionales, con un aviso (ADR 0006).
- En el Xvfb de pruebas (mapa por defecto) Mod4 es `Super_L`/`Super_R`
  (también `Hyper_L`), Mod2 es `Num_Lock`, y `j`, `k`, `Return` y `Escape`
  son los códigos 44, 45, 36 y 9. No hay otras capturas.
- **Con Xephyr** (pendiente), las pulsaciones pasan antes por el servidor de
  la sesión principal: si sxhkd captura allí alguna de estas combinaciones,
  Xephyr no la recibirá. No se ha leído la configuración de sxhkd.

### Combinaciones reservadas

Crimson WM es independiente de bspwm, sxhkd y los scripts personales del
autor; sus hábitos de teclado solo sirven de referencia. Las combinaciones
siguientes quedan reservadas: **no están implementadas, no se capturan y
el WM no ejecuta ningún programa**. Una prueba unitaria comprueba que la
tabla de atajos no ocupa ninguna.

Aplicaciones, para el lanzador de F4. **Implementadas en F4** con estas
mismas órdenes, como argv sin shell (ADR 0006); ya no están reservadas:

| Atajo | Aplicación | Orden prevista |
|---|---|---|
| `Mod4+Return` | Terminal | `kitty` |
| `Mod4+d` | Lanzador | `rofi -show drun` |
| `Mod4+e` | Archivos | `thunar` |
| `Mod4+b` | Navegador | `firefox` |
| `Mod4+n` | Editor Neovim | `kitty -e nvim` |
| `Print` | Capturas | `flameshot gui` |

Las órdenes son valores iniciales propuestos. F4 definirá cómo se
configuran y se ejecutan de forma segura (como lista `argv`, sin una shell
implícita), y entonces se comprobarán las herramientas instaladas.

Otras funciones, para fases posteriores:

| Atajo | Función prevista |
|---|---|
| `Mod4+h` | Foco hacia la izquierda |
| `Mod4+l` | Foco hacia la derecha |
| `Mod4+w` | Cerrar la ventana |
| `Mod4+Shift+w` | Forzar el cierre, sujeto a una política segura |

*`Mod4+w` y `Mod4+Shift+w` se implementaron en F6 (ADR 0008):
`WM_DELETE_WINDOW` y `KillClient`; solo `Mod4+h` y `Mod4+l` siguen
reservadas.*

### Modelo del foco

- `Clients` guarda un historial de foco (`history`), de la ventana más
  reciente a la más antigua; la primera es la enfocada. Solo contiene
  ventanas gestionadas que admiten foco.
- **Ventanas que admiten foco:** las de mosaico, los diálogos, `UTILITY`,
  `TOOLBAR` y las transitorias. **No lo admiten** `SPLASH`, `MENU`, `DOCK`,
  `DESKTOP`, `NOTIFICATION`, `TOOLTIP`, `POPUP_MENU`, `DROPDOWN_MENU`,
  `COMBO` y `DND`: nunca reciben el foco ni entran en la navegación.
- **Orden de navegación:** el mosaico (maestra y pila de arriba abajo) y
  después las flotantes que admiten foco, en orden de llegada.
- **Al abrirse** una ventana que admite foco, lo recibe después de
  `MapWindow`, en la misma conexión, así que ya es visible.
- **Al desaparecer la enfocada** (`UnmapNotify`, `DestroyNotify` o
  `ReparentNotify`), el foco pasa a la más reciente del historial; si está
  vacío, a la primera del orden de navegación; y si no queda ninguna, el
  foco vuelve a seguir al puntero (`PointerRoot`) y `_NET_ACTIVE_WINDOW`
  pasa a None. Al cerrar un diálogo, el foco vuelve así a la ventana usada
  antes.
- **Al arrancar**, tras adoptar las ventanas visibles, recibe el foco la
  maestra.

### Modelos de entrada (ICCCM §4.1.7)

`WM_HINTS.input` (si falta, se supone verdadero) y la presencia de
`WM_TAKE_FOCUS` en `WM_PROTOCOLS` deciden el modelo:

| Modelo | `input` | `WM_TAKE_FOCUS` | Acción del WM |
|---|---|---|---|
| Passive | sí | no | `SetInputFocus(ventana, revert_to=Parent)` |
| LocallyActive | sí | sí | `SetInputFocus` y el mensaje `WM_TAKE_FOCUS` |
| GloballyActive | no | sí | Solo el mensaje `WM_TAKE_FOCUS`; el cliente decide si toma el foco |
| NoInput | no | no | La ventana queda como activa, pero el foco del teclado va a la raíz, para que las pulsaciones no lleguen a la ventana enfocada antes |

- `WM_TAKE_FOCUS` es un `ClientMessage` de formato 32 con tipo
  `WM_PROTOCOLS`, `data[0] = WM_TAKE_FOCUS` y `data[1]` = marca de tiempo,
  enviado con `SendEvent` a la ventana y máscara vacía (ICCCM §4.2.8). Solo
  se envía a las ventanas que lo anuncian.
- **Marca de tiempo:** la del `KeyPress` del atajo. Cuando el cambio no
  viene de una pulsación (apertura, cierre o arranque), `CurrentTime`: el WM
  no tiene una ventana propia con la que obtener una marca del servidor.
  *Desde F6, `WM_TAKE_FOCUS` lleva en ese caso la hora del servidor,
  obtenida con la ventana de comprobación (ADR 0008).*
- **Lectura robusta:** `WM_HINTS` y `WM_PROTOCOLS` se leen como propiedades
  sin interpretar y se analizan en `clients.rs`. Un valor mal formado o de
  otro tipo cuenta como ausente. No se usa `WmHints::get` de x11rb 0.14,
  cuyo error de análisis llega como `ConnectionError`: el WM lo trataría
  como una conexión perdida (código 13), y un cliente con un `WM_HINTS`
  defectuoso podría terminarlo.

### Cambios de foco hechos por los clientes

El WM selecciona `FocusChange` en las ventanas que admiten foco y sigue los
`FocusIn` legítimos: la ventana pasa a ser la activa, sin volver a llamar a
`SetInputFocus`. Se ignoran:

- **Los antiguos:** el evento lleva el número de secuencia de la última
  petición del WM procesada cuando se generó. Si es menor que el de la
  última petición con la que el WM cambió el foco (`SetInputFocus` o
  `WM_TAKE_FOCUS`), el evento es anterior a esa decisión y ya no describe
  el estado actual.
- Los de las capturas de teclado (modos `Grab`, `Ungrab` y
  `WhileGrabbed`), que producen los propios atajos.
- Los que no sitúan el foco en la ventana (detalles `Pointer`,
  `PointerRoot` y `None`).
- Los de ventanas no gestionadas o ya retiradas. La selección de
  `FocusChange` no se retira al dejar de gestionar una ventana: podría haber
  sido destruida, y la petición produciría un error X.

### Ventana activa: `_NET_ACTIVE_WINDOW`

- El WM la publica en la raíz (tipo `WINDOW`, formato 32) en cada cambio,
  con None si no hay ventana activa, y la borra al salir.
- **Compatibilidad parcial:** el WM no declara soporte EWMH
  (`_NET_SUPPORTED`, `_NET_SUPPORTING_WM_CHECK`) ni publica otras
  propiedades. Algunas herramientas solo leen `_NET_ACTIVE_WINDOW` si el WM
  anuncia que la admite (por ejemplo, `xdotool getactivewindow`).

### Salida

Con `Mod4+Shift+Escape`, el WM borra `_NET_ACTIVE_WINDOW`, devuelve el foco
a `PointerRoot` (el estado inicial del servidor) y espera una respuesta del
servidor antes de terminar. Sin esa espera, otro cliente podía ver todavía
el estado antiguo (observado al desarrollar A22).

### Salvaguardas

- Las capturas, `_NET_ACTIVE_WINDOW`, `WM_PROTOCOLS` y `WM_TAKE_FOCUS` se
  registran después de superar las cuatro capas del ADR 0003; un servidor
  rechazado no queda modificado (A4b sigue pasando).
- No se crea `_NET_SUPPORTING_WM_CHECK`: la capa 3 se comporta igual.
- El foco solo se da a ventanas gestionadas y visibles, o a la raíz.
- Los atajos no ejecutan programas ni usan una shell.
- Errores: `SetInputFocus` o `SendEvent` sobre una ventana que acaba de
  desaparecer producen un error X que se registra sin terminar el WM; su
  `UnmapNotify` o `DestroyNotify` vuelve a colocar el foco. Solo la pérdida
  de la conexión es fatal (13).

## Pruebas de aceptación de F3

Documentadas el 2026-10-13. Arnés `tests/acceptance/f3/`, que reutiliza
`f1/lib.sh`, `f1/xvfb-env.sh` y `f2/windows.sh`: cada caso en un Xvfb nuevo
de 1280×720, con las mismas guardas, cookie temporal, sin TCP ni `-ac`, y
sin conectar con `:0`.

- **Foco real:** `xdotool getwindowfocus` (`GetInputFocus` del servidor).
- **Ventana activa:** `xprop -root _NET_ACTIVE_WINDOW`.
- **Clientes:** ventanas de `xev` (sin `WM_HINTS`: modelo pasivo), que
  registran sus propios `FocusIn` y `ClientMessage`, y `xclock`
  (`input=False`: modelo sin entrada).

**Pruebas unitarias nuevas o cambiadas:**

- U8 (`clients`, 8 pruebas): historial de foco; foco tras retirar la
  enfocada, otra o la última; sin historial; orden de navegación cíclico
  con flotantes y sin las ventanas que no admiten foco; `zoom` y sus casos
  sin efecto; tabla de `InputModel`; campo `input` de `WM_HINTS`. La tabla
  de clasificación incluye la marca `focusable` y el modelo de entrada.
- U9 (`keys`, 4 pruebas): cada atajo de la tabla, con Bloq Mayús, Bloq Num
  y botones; modificadores exactos (`Mod4+Return` sin acción); tabla sin
  atajos repetidos ni combinaciones reservadas. Sustituyen a la prueba del
  atajo de salida de F1.
- U7 (`wm`): además del `ConfigureNotify` sintético, la serialización del
  mensaje `WM_TAKE_FOCUS`.

**Pruebas de aceptación:**

- A22. Una ventana nueva recibe el foco (A y luego B); A registra su
  `FocusIn`. Tras salir: foco en `PointerRoot` y sin `_NET_ACTIVE_WINDOW`.
- A23. Con A, B y C (foco en C): `Mod4+j` lleva a A (vuelta) y a B;
  `Mod4+k`, a A y a C (vuelta).
- A24. `Mod4+Return` no se captura: la ventana enfocada (C) registra la
  pulsación en su `xev` y el mosaico no cambia. (Desde F4 esta
  comprobación es `A24-super-return-capturado`: C no recibe la pulsación y
  el lanzamiento falla porque el `PATH` del WM está vacío; ver ADR 0006.) `Mod4+Shift+Return` sobre C
  la pone de maestra (C, A, B) con el foco en C, sin que la pulsación llegue
  a C; repetido, A vuelve a maestra (A, C, B) y el foco sigue en C.
- A25. Con el historial B, C, A, al cerrar B el foco pasa a C y el mosaico
  se reorganiza.
- A26. Al desmapear la enfocada (B), el foco pasa a A. Sin ventanas:
  `PointerRoot` y `_NET_ACTIVE_WINDOW` = None.
- A27. Un diálogo recibe el foco al mapearse sin alterar el mosaico; la
  navegación pasa de A (mosaico) a D (flotante) y vuelve; al cerrarlo, el
  foco vuelve a A.
- A28. `xclock` (sin entrada): es la activa, pero el foco real está en la
  raíz; `Mod4+k` lleva a A.
- A29. Una ventana que anuncia `WM_TAKE_FOCUS` (puesto con `xprop` mientras
  está desmapeada) recibe el foco y un `ClientMessage` `WM_PROTOCOLS`
  formato 32 `WM_TAKE_FOCUS`; la ventana que no lo anuncia no lo recibe.
- A30. Tras `xdotool windowfocus A` (foco puesto por un cliente), A es la
  activa y `Mod4+j` lleva a B.
- A31. `Mod4+j` con Bloq Mayús y `Mod4+k` con Bloq Num activos funcionan;
  `Mod4+Shift+j` no cambia el foco (espera fija de 0,5 s);
  `Mod4+Shift+Return` con Bloq Mayús activo hace el Zoom.
- A32. Como A9: sin códigos 101 ni «panicked at», bspwm y sxhkd con el mismo
  PID y sin residuos. La consulta de la raíz de `:0` se omite.

Cada caso comprueba además que no se registró ningún error X.

## Resultados de las pruebas de aceptación de F3

Ejecutadas el 2026-10-13 con `tests/acceptance/f3/run.sh` sobre el binario
de depuración (Xvfb 2:21.1.16-1.3+deb13u4, x11-utils 7.7+7, x11-apps
7.7+11+b1, xdotool 1:3.20160805.1-5.1). Tras el cambio del Zoom a
`Mod4+Shift+Return` (F3-B.1), tres ejecuciones consecutivas dieron el
mismo resultado: **61 comprobaciones OK, 0 FALLA y 1 OMITIDA**
(`A32-xprop-:0`); código de salida 0. Antes del cambio, con 58
comprobaciones, el arnés también pasó tres veces, y el control negativo
con `/bin/false` como binario dio 61 FALLA y código de salida 1. Los
resultados detallados se guardaron fuera del repositorio.

| Prueba | Resultado |
|---|---|
| A22–A31 | Superadas: foco real y `_NET_ACTIVE_WINDOW` coherentes en cada paso, sin errores X |
| A32 | **Parcial**: sin pánicos ni residuos; bspwm y sxhkd con el mismo PID. La raíz de `:0` no se consultó |

Las regresiones de F1 (46 OK) y F2 (47 OK) se ejecutaron sobre el código de
F3-B, antes del cambio del Zoom, y se describen en los ADR 0003 y 0004. No
se repitieron después: el cambio solo afecta a una entrada de la tabla de
atajos, que esos arneses no usan, y el arranque con la tabla nueva lo
ejercitan todos los casos de F3.

## Limitaciones conocidas

- **Modelo globalmente activo:** solo tiene pruebas unitarias. `xprop -set`
  crea `WM_HINTS` con tipo `INTEGER`, que el WM ignora, y ninguna
  herramienta instalada crea una ventana con `input=False` y
  `WM_TAKE_FOCUS`.
- **`FocusIn` antiguos:** la regla por número de secuencia solo está
  revisada en el código; las pruebas no provocan esa carrera.
- **Marcas de tiempo:** `CurrentTime` en los cambios que no vienen de una
  pulsación (ver «Modelos de entrada»). *Corregido para `WM_TAKE_FOCUS` en
  F6 (ADR 0008).*
- **Sin ratón:** mover el puntero ya no cambia el foco y no hay clic para
  enfocar.
- **Propiedades fijas:** `WM_HINTS` y `WM_PROTOCOLS` se leen al gestionar la
  ventana; sus cambios posteriores (`PropertyNotify`) no se atienden. *Resuelto en F6 (ADR 0008).*
- **Mapa de teclado fijo:** tras un `MappingNotify` los atajos no se vuelven
  a capturar (como en F1).
- **Sin prevención del robo de foco:** toda ventana nueva que admite foco lo
  recibe, y los cambios hechos por los clientes se aceptan.
- **EWMH parcial:** solo `_NET_ACTIVE_WINDOW`; tampoco hay `WM_STATE`.
  *Desde F5 se publican `WM_STATE` y las propiedades de escritorio (ADR
  0007), sin declarar soporte EWMH.*
- **Clientes probados:** solo `xev` y `xclock`, en Xvfb; no se ha probado
  con aplicaciones reales ni con Xephyr.

## Alternativas consideradas

- **Foco a la vecina en el diseño al cerrar la enfocada:** descartado; el
  historial devuelve el foco a la ventana usada antes, lo que funciona
  mejor al cerrar diálogos.
- **Imponer siempre el foco del WM** (devolverlo si un cliente lo cambia):
  descartado; ICCCM permite que los clientes se den el foco.
- **Avisar y seguir si falla la captura de un atajo:** descartado; un WM
  sin parte de sus atajos sería confuso, y el código 14 ya cubre ese caso.
  F4 lo adopta solo para los atajos de lanzamiento (ADR 0006).
- **Zoom sin efecto sobre la maestra:** descartado; el intercambio con la
  segunda (como dwm) es más útil.
- **Zoom con `Mod4+Return`** (como dwm), la primera propuesta: sustituido
  por `Mod4+Shift+Return` para dejar `Mod4+Return` a la terminal (F4).
- **`WmHints::get` de x11rb:** descartado por cómo informa de los errores
  de análisis (ver «Modelos de entrada»).

## Consecuencias

- `ConnectionError` sigue siendo la única causa de terminación durante la
  ejecución; toda propiedad de un cliente se analiza en el propio WM.
- El foco explícito sustituye al comportamiento del servidor (el foco
  seguía al puntero); el clic para enfocar llegará con el ratón.
- Las órdenes nuevas del arnés (`xclock`, de x11-apps) figuran en la
  cabecera de `tests/acceptance/f3/run.sh` y en el `README.md`.
