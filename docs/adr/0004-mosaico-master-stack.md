# ADR 0004: Motor de mosaico master-stack (F2)

- **Estado:** aceptado. Implementado el 2026-10-13 (F2-B). Las pruebas
  unitarias (36 en total, 18 de ellas nuevas) pasaron, y las de aceptación
  A10–A20 pasaron en Xvfb; A21 quedó parcial por el mismo motivo que A9
  (ver «Resultados de las pruebas de aceptación de F2»).
- **Fecha:** 2026-10-13
- **Actualización F5 (2026-10-13):** el mosaico pasa a ser por escritorio:
  cada uno de los ocho escritorios virtuales tiene su propio `Clients`, y
  el WM ya desmapea ventanas para ocultarlas, distinguiendo sus desmapeos de
  las retiradas de los clientes (ADR 0007). Las secciones afectadas lo
  indican.
- **Actualización F6 (2026-10-13):** el WM declara el soporte EWMH y
  publica `_NET_CLIENT_LIST` y `_NET_CLIENT_LIST_STACKING`. Entre los tipos
  de ventana solo declara los que tienen un tratamiento propio (normal,
  diálogo, utilidad y barra de herramientas; ADR 0008).
- **Actualización F7 (2026-10-14):** la geometría que coloca el WM sale de
  `Clients::plan`: el master-stack de las ventanas que no están a pantalla
  completa, más el área completa para la que lo está, que conserva su
  posición en la lista (ADR 0009). *La «pantalla completa» que se cita
  abajo como no atendida está resuelta.*
- **Actualización F8 (2026-10-14):** el valor de `WM_TRANSIENT_FOR` se
  interpreta (ADR 0010). La clasificación no cambia, pero las flotantes se
  suben por familias, cada diálogo encima de su principal, y un diálogo
  flotante se centra sobre ella al gestionarse. `arrange` se llama siempre
  antes de mapear una ventana nueva, también las flotantes.
- **Actualización F9 (2026-10-14):** `clients::classify` devuelve una
  `Disposition` (ADR 0011). `DOCK`, `NOTIFICATION` y `DESKTOP` ya no son
  flotantes: son ventanas fijas, fuera del mosaico y de los escritorios, en
  su capa de la pila (`DESKTOP` por debajo del mosaico). La tabla de
  clasificación de este ADR sigue valiendo para las demás; el «primer tipo
  reconocido» lo calcula ahora `ewmh::TypeAtoms`. Desde F9, `arrange`
  termina con `apply_layers`. *La limitación «Paneles y escritorio» está
  resuelta en parte: siguen sin struts.*
- **Actualización F10 (2026-10-14):** el mosaico usa el área de trabajo:
  la pantalla menos lo que reservan los paneles con `_NET_WM_STRUT_PARTIAL`
  o `_NET_WM_STRUT` (`Clients::plan(work, screen)`). La proporción del 50 %
  se calcula sobre ella. Al cambiar las reservas, el escritorio activo se
  reorganiza al momento y los ocultos al mostrarse; las flotantes no se
  mueven (ADR 0012).
- **Actualización F11 (2026-10-14):** después de `clients::classify`, una
  regla de `CRIMSON_RULES` puede pasar una ventana de mosaico a flotante;
  nunca al revés. La clasificación no cambia y, sin reglas, todo sigue
  como en F10 (ADR 0013).

## Contexto

En F1 el WM mapeaba y configuraba las ventanas tal como lo pedían (ADR
0003). F2 añade el primer diseño en mosaico: las ventanas normales se
colocan automáticamente y se reorganizan al abrirse o cerrarse, y los
diálogos quedan flotantes. El objetivo es un motor pequeño y correcto que
pueda crecer después con escritorios virtuales, otros diseños y foco, sin
adelantar esas abstracciones.

## Decisión

### Módulos

| Módulo | Responsabilidad | E/S X11 |
|---|---|---|
| `src/layout.rs` | `Rect` y `master_stack(area, n)`: la geometría | No |
| `src/clients.rs` | `Clients` (ventanas gestionadas, en orden) y `classify` | No |
| `src/wm.rs` | Bucle de eventos, consultas al servidor, aplicación de la geometría | Sí |
| `src/keys.rs` | Atajo provisional de salida, movido sin cambios desde `wm.rs` | Sí |
| `src/safeguards.rs` | Capa 4: selecciona también `SubstructureNotify` (ADR 0003) | Sí |

Los módulos sin E/S se prueban con `cargo test` sin servidor X. No se añade
ninguna dependencia ni feature de x11rb; los átomos se registran con la
macro `atom_manager!` de x11rb, disponible sin features.

### Diseño master-stack

- La primera ventana en mosaico ocupa la columna izquierda (la maestra);
  el resto se reparte la altura de la columna derecha (la pila).
- La columna maestra mide el 50 % del ancho (`MASTER_PERCENT`, constante
  hasta que exista una configuración). No hay bordes ni márgenes: el WM
  fija `border_width = 0` y los rectángulos cubren la pantalla sin huecos
  ni solapes.
- Aritmética entera: si la altura no es divisible, los primeros tramos de
  la pila miden un píxel más, de modo que la suma es exacta. Ejemplo con
  1279×719 y tres ventanas: maestra 639×719; pila 640×360 y 640×359.
- Las ventanas nuevas se añaden **al final**. Al retirarse la maestra, la
  siguiente pasa a serlo; una ventana que se vuelve a mapear entra al final.
- **Rangos de X11:** `Rect` usa los tipos del protocolo (`i16` para la
  posición y `u16` para el tamaño), las posiciones se suman con saturación
  y ningún tamaño es 0 (X11 lo rechaza con `BadValue`). En casos
  degenerados (menos de 2 píxeles de ancho o más ventanas en la pila que
  píxeles de alto) los rectángulos miden al menos 1 y se solapan.

### Clasificación

| Ventana | Gestión |
|---|---|
| Override-redirect | No se gestiona (normalmente no produce `MapRequest`) |
| Primer tipo reconocido de `_NET_WM_WINDOW_TYPE` = `NORMAL` | Mosaico |
| Primer tipo reconocido = `DIALOG`, `UTILITY`, `SPLASH`, `TOOLBAR`, `MENU`, `DOCK`, `DESKTOP`, `NOTIFICATION`, `TOOLTIP`, `POPUP_MENU`, `DROPDOWN_MENU`, `COMBO` o `DND` | Flotante |
| Sin tipo reconocido y con `WM_TRANSIENT_FOR` | Flotante (EWMH la trata como un diálogo) |
| Resto | Mosaico |

- Cuenta el primer tipo reconocido, como indica EWMH: la lista está en
  orden de preferencia.
- `WM_TRANSIENT_FOR` se acepta con cualquier tipo de propiedad y un valor de
  32 bits. ICCCM espera el tipo `WINDOW`, pero `xprop -set` la crea como
  `CARDINAL` (comprobado en A17), y la regla permisiva es inocua.
- Las flotantes conservan la geometría que piden y se suben por encima del
  mosaico cada vez que este se reorganiza.
- `WM_NORMAL_HINTS` (tamaños mínimo, máximo o fijo) se ignora en F2.

### Eventos

| Evento | Acción |
|---|---|
| `MapRequest` | Si la ventana no está gestionada, se leen en una sola ida y vuelta sus atributos, `WM_TRANSIENT_FOR` y `_NET_WM_WINDOW_TYPE`, y se clasifica. En mosaico se reorganiza **antes** de mapearla, para que aparezca ya con su tamaño. Después, `MapWindow` |
| `ConfigureRequest` | En mosaico se deniega todo, geometría y apilamiento (las flotantes deben seguir encima), y se envía un `ConfigureNotify` sintético con la geometría asignada, en coordenadas de la raíz y sin borde (ICCCM §4.1.5). A las flotantes y a las no gestionadas se les aplica lo pedido; el servidor genera el `ConfigureNotify` real |
| `UnmapNotify`, `DestroyNotify` | Solo los que llegan por `SubstructureNotify` de la raíz (`event == raíz`), incluido el `UnmapNotify` sintético de retirada (ICCCM §4.1.4). Si la ventana estaba en mosaico, se reorganiza |
| `ReparentNotify` | Si una ventana deja de ser hija de la raíz, se deja de gestionar |
| `KeyPress` del atajo | Termina con 0, como en F1 |
| `Event::Error` | Se registra y se sigue, como en F1 |

- **Idempotencia:** gestionar dos veces la misma ventana no la duplica, y
  retirar una ventana no gestionada no hace nada, así que el `UnmapNotify` y
  el `DestroyNotify` de una misma ventana, o los eventos de ventanas
  ajenas, no alteran el estado. F2 nunca desmapea ventanas por sí mismo, así
  que no necesita distinguir desmapeos propios (lo necesitarán los
  escritorios virtuales). *Resuelto en F5 (ADR 0007).*
- **Geometría guardada:** solo se envía `ConfigureWindow` a las ventanas
  cuya geometría cambia.
- **Adopción:** al arrancar se consultan las hijas de la raíz
  (`QueryTree`) y se gestionan las que ya estaban visibles, con la misma
  clasificación.
- **Orden de arranque:** los átomos se registran después de superar las
  cuatro capas del ADR 0003. Registrarlos crea átomos en el servidor, y un
  servidor rechazado no debe quedar modificado (A4b sigue pasando).
- No hay sondeo periódico: todo responde a eventos.

### Errores

- Las peticiones sin respuesta (`ConfigureWindow`, `MapWindow`,
  `SendEvent`) no se comprueban: si fallan, normalmente con `BadWindow` por
  una carrera, el error llega como evento y se registra.
- En las peticiones con respuesta durante la ejecución, un error X se
  registra («ventana 0x… no gestionada») y la ventana no se gestiona.
- Solo la pérdida de la conexión es fatal (código 13). Los códigos de
  salida del ADR 0003 no cambian: el 14 sigue reservado a los errores del
  arranque (ahora también `InternAtom` y `QueryTree`).

## Pruebas de aceptación de F2

Documentadas el 2026-10-13. Arnés `tests/acceptance/f2/`, que reutiliza
`lib.sh` y `xvfb-env.sh` de F1: cada caso en un Xvfb nuevo de `xvfb-run`
(1280×720, salvo A20), con las mismas guardas, cookie temporal, sin TCP ni
`-ac`, y sin conectar con `:0`. Las ventanas son de `xmessage` (y `xlogo`
en A19); la geometría se lee con `xwininfo`.

**Pruebas unitarias nuevas:**

- U5 (`layout`, 10 pruebas): una, dos y tres ventanas; tamaños impares;
  reparto del resto; área desplazada; casos degenerados; coordenadas
  extremas sin desbordamiento; 10 000 ventanas; y, para todos los tamaños de
  2×1 a 24×24 y hasta 25 ventanas, que los rectángulos quedan dentro del
  área, no se solapan y la cubren.
- U6 (`clients`, 7 pruebas): orden de llegada, relevo de la maestra,
  idempotencia, ventana que vuelve al final, geometría guardada y tabla de
  clasificación.
- U7 (`wm`, 1 prueba): contenido y serialización del `ConfigureNotify`
  sintético.

**Pruebas de aceptación:**

- A10. Una ventana: 0,0 1280×720, borde 0.
- A11. Dos ventanas: 0,0 640×720 y 640,0 640×720.
- A12. Tres ventanas: maestra 640×720; pila 640×360 en y = 0 e y = 360.
- A13. Al matar el cliente de la maestra, la segunda pasa a maestra y la
  tercera ocupa la pila.
- A14. Al desmapear una ventana de la pila se reorganiza; al volver a
  mapearla queda al final.
- A15. `windowmove`, `windowsize` y `windowraise` sobre una ventana en
  mosaico producen exactamente tres `ConfigureNotify` sintéticos con la
  geometría asignada, ninguno real, y la geometría no cambia.
- A16. Un diálogo (`_NET_WM_WINDOW_TYPE_DIALOG`, puesto con la ventana
  desmapeada) queda flotante sin alterar el mosaico, su `ConfigureRequest`
  se aplica y sigue encima tras mapear otra ventana en mosaico.
- A17. Una ventana con `WM_TRANSIENT_FOR` queda flotante.
- A18. Una ventana mapeada antes de arrancar el WM se adopta en mosaico.
- A19. 40 ventanas de `xlogo` terminadas con SIGKILL entre 5 y 95 ms
  después de lanzarse: el WM sigue en marcha y una ventana nueva ocupa toda
  la pantalla (no quedan clientes obsoletos).
- A20. En un Xvfb de 1279×719: 639×719; 640×360 y 640×359.
- A21. Como A9: sin códigos 101 ni «panicked at», bspwm y sxhkd con el
  mismo PID y sin residuos. La consulta de la raíz de `:0` se omite.

Cada caso, salvo A19, comprueba además que no se registró ningún error X.

## Resultados de las pruebas de aceptación de F2

Ejecutadas el 2026-10-13 con `tests/acceptance/f2/run.sh` sobre el binario
de depuración: Xvfb 2:21.1.16-1.3+deb13u4, x11-apps 7.7+11+b1, x11-utils
7.7+7 y xdotool 1:3.20160805.1-5.1. Cinco ejecuciones consecutivas (la
primera con una versión de A19 de 20 ventanas y SIGTERM) dieron el mismo
resultado: **47 comprobaciones OK, 0 FALLA y 1 OMITIDA** (`A21-xprop-:0`);
código de salida 0. Los resultados detallados se guardaron fuera del
repositorio.

| Prueba | Resultado |
|---|---|
| A10–A18, A20 | Superadas, con la geometría exacta esperada y sin errores X |
| A19 | Superada: el WM siguió en marcha y el estado quedó coherente. Ver la limitación sobre las carreras |
| A21 | **Parcial**: sin pánicos ni residuos; bspwm y sxhkd con el mismo PID. La raíz de `:0` no se consultó |

La regresión de F1 sobre el mismo código se describe en el ADR 0003
(«Regresión de F1 con el código de F2»).

**Regresión de F2 con el código de F3** (ADR 0005), el 2026-10-13: **47
OK, 0 FALLA y 1 OMITIDA**; código de salida 0. En F3, las funciones de
ventanas del arnés pasaron de `f2/xvfb-case.sh` a `f2/windows.sh`, para
reutilizarlas en `tests/acceptance/f3/`; `open_window` admite ahora otra
orden en lugar de `xmessage` y guarda su salida en el directorio de
resultados. Los identificadores de las comprobaciones no cambian.

## Limitaciones conocidas

- **Carreras con `BadWindow`:** en A19 ninguna ventana desapareció entre el
  `MapRequest` y las consultas del WM (0 errores X y 0 ventanas no
  gestionadas en las cinco ejecuciones), así que esas ramas solo están
  revisadas en el código, no ejercitadas.
- **`ReparentNotify`:** ninguna prueba reparenta una ventana.
- **Espera fija en A15:** `xev` no indica cuándo ha seleccionado los
  eventos; se espera 1 s antes de enviar las peticiones.
- **Sin foco ni navegación por teclado:** el foco sigue al puntero, como en
  un servidor sin WM. *Resuelto en F3 (ADR 0005).*
- **Sin `WM_STATE` ni EWMH propios** (`_NET_SUPPORTING_WM_CHECK`,
  `_NET_CLIENT_LIST`…): algunas aplicaciones y herramientas los esperan.
  Desde F3 solo se publica `_NET_ACTIVE_WINDOW` (ADR 0005); desde F5,
  también `WM_STATE` y las propiedades de escritorio del ADR 0007.
- **Paneles y escritorio:** `DOCK` y `DESKTOP` quedan flotantes; no se
  reserva espacio para paneles (struts) ni se envía el escritorio al fondo.
- **Pantalla completa** (`_NET_WM_STATE_FULLSCREEN`): no se atiende; esas
  ventanas van en mosaico.
- **`WM_NORMAL_HINTS`:** se ignora; una ventana de tamaño fijo se estira a
  su celda.
- **Una pantalla fija:** el área se lee al arrancar; no hay RandR ni varios
  monitores.
- **Clientes probados:** solo `xmessage` y `xlogo`, en Xvfb y durante
  segundos; no se ha probado con aplicaciones reales ni con Xephyr.

## Alternativas consideradas

- **Ventana nueva como maestra** (convención de dwm): descartada por
  decisión del autor; la primera ventana sigue siendo la maestra.
- **Proporción configurable, márgenes y bordes:** aplazados hasta que exista
  la configuración.
- **Árbol de particiones (como bspwm):** más general, pero innecesario para
  un único diseño; la lista ordenada basta y no impide añadir otros diseños.
- **`HashMap` para los clientes:** prematuro para el número de ventanas de
  un escritorio; el `Vec` conserva además el orden.

## Consecuencias

- `ConfigureRequest` deja de aplicarse tal cual a las ventanas en mosaico:
  la comprobación `A7-extra-configure` de F1 se redefinió (ADR 0003).
- Añadir escritorios virtuales exigirá distinguir los desmapeos propios de
  los del cliente (hecho en F5, ADR 0007); añadir otros diseños, una función por diseño con la
  misma firma que `master_stack`.
- Las nuevas órdenes del arnés (`xlogo`, de x11-apps, y `xev`, de
  x11-utils) figuran en la cabecera de `tests/acceptance/f2/run.sh` y en
  `docs/README-technical-es.md`.
