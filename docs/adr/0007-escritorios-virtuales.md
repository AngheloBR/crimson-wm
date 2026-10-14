# ADR 0007: Escritorios virtuales (F5)

- **Estado:** aceptado. Implementado el 2026-10-13 (F5-B). Las 80 pruebas
  unitarias pasaron, y las de aceptación A41–A51 pasaron en Xvfb; A52 quedó
  parcial por el mismo motivo que A9 (ver «Resultados de las pruebas de
  aceptación de F5»).
- **Fecha:** 2026-10-13
- **Actualización F6 (2026-10-13):** el soporte EWMH pasa a estar
  declarado (`_NET_SUPPORTING_WM_CHECK`, `_NET_SUPPORTED`) y se atienden las
  peticiones `_NET_CURRENT_DESKTOP` y `_NET_WM_DESKTOP` (ADR 0008). En A41,
  `A41-ewmh-parcial` pasa a ser `A41-ewmh-declarado`. En A51, el reinicio
  tras SIGTERM comprueba primero el rechazo por la propiedad obsoleta
  (`A51-tras-sigterm`) y el arnés la borra antes de seguir. Las secciones
  afectadas lo indican.
- **Actualización F7 (2026-10-14):** cada escritorio puede tener una
  ventana a pantalla completa; el estado viaja con la ventana al moverla y
  la última gana en el destino (ADR 0009). La salida normal restaura antes
  las geometrías de todas ellas.

## Contexto

Hasta F4 todas las ventanas gestionadas estaban siempre visibles en un
único mosaico (ADR 0004), con un único historial de foco (ADR 0005). F5
añade ocho escritorios fijos, en un solo monitor, sin barras, sticky, IPC ni
EWMH completo. Ocultar ventanas obliga a resolver lo que el ADR 0004 dejó
pendiente: distinguir los desmapeos del propio WM de las retiradas de los
clientes.

## Decisión

### Módulos

| Módulo | Cambio |
|---|---|
| `src/workspaces.rs` (nuevo) | `Workspaces`: ocho `Clients`, el escritorio activo y el anterior; cambiar, volver y mover la enfocada. Sin E/S |
| `src/clients.rs` | `take` (saca una ventana sin olvidar su clasificación ni su geometría), `adopt` y `windows`. `unmanage` usa `take` |
| `src/keys.rs` | `Action::View`, `MoveTo` y `ViewPrevious`, los 17 atajos y `Repeats::autorepeat` |
| `src/wm.rs` | `Workspaces` en lugar de `Clients`; mostrar y ocultar (`show`, `hide`), movimiento, `OwnUnmaps`, save-set, `WM_STATE` y propiedades EWMH; motivo de cada retirada en el registro |
| `src/main.rs` | Solo el comentario de la fase |
| `examples/test-client.rs` (nuevo) | Cliente auxiliar de las pruebas (ver «Verificación»); no forma parte del WM |

`error.rs`, `safeguards.rs`, `display.rs`, `connect.rs`, `layout.rs` y
`spawn.rs` no cambian. No hay dependencias ni códigos de salida nuevos.

### Modelo

- `Workspaces { desks: [Clients; 8], current, previous }`. Cada
  escritorio es un `Clients` completo, así que el orden master-stack, el
  historial de foco, la navegación (`neighbour`), el Zoom y `fallback` son
  por escritorio sin cambiar su código.
- Índices internos 0–7 (como `_NET_CURRENT_DESKTOP`); el usuario ve 1–8.
- Invariantes, comprobados en las pruebas tras cada operación: cada ventana
  está en un único escritorio; `current` < 8; `previous` ≠ `current`; el
  historial de cada escritorio solo contiene ventanas suyas que admiten
  foco.
- Una ventana nueva, también un diálogo, va al escritorio activo. Al
  arrancar, las ventanas visibles se adoptan en el escritorio 1. Un
  `_NET_WM_DESKTOP` que traiga el cliente se ignora.

### Atajos

| Atajo | Acción |
|---|---|
| `Mod4+1`…`Mod4+8` | Activa ese escritorio y enfoca su ventana más reciente. Sin efecto si ya es el activo |
| `Mod4+Shift+1`…`Mod4+Shift+8` | Lleva la ventana enfocada al final de ese escritorio, donde pasa a ser la enfocada, **sin cambiar** de escritorio. El foco pasa a la siguiente del historial del activo. Sin efecto sin ventana enfocada o con el mismo escritorio |
| `Mod4+Tab` | Vuelve al escritorio anterior; repetido, alterna entre los dos. Sin efecto antes del primer cambio |

- Los 17 son **obligatorios**: si no se pueden capturar, el WM termina con
  el código 14 (como los atajos de control del ADR 0005).
- `Mod4+Shift+n` se captura con el keysym del dígito: los atajos se
  reconocen por código de tecla y modificadores, no por el carácter que da
  la tecla con Shift. En el mapa de Xvfb, 1–8 son los códigos 10–17 y Tab
  el 23.
- `Mod4+j`, `Mod4+k` y `Mod4+Shift+Return` solo actúan en el escritorio
  activo. Siguen reservadas, sin capturar, `Mod4+h`, `Mod4+l`, `Mod4+w` y
  `Mod4+Shift+w`.
- **Repetición automática:** mover y volver no son idempotentes. Mantener
  `Mod4+Shift+n` se llevaría una ventana tras otra y mantener `Mod4+Tab`
  alternaría sin parar, así que ignoran la repetición del servidor (la
  liberación y la pulsación con la misma marca de tiempo, ADR 0006).
  `Mod4+n` repetido no tiene efecto. Sin el intervalo de 300 ms de los
  lanzamientos: un cambio rápido deliberado sí se atiende.

### Ocultar y mostrar

Ocultar es `UnmapWindow`; mostrar, `MapWindow` (el WM tiene la
redirección, así que no genera `MapRequest`). Un cambio de escritorio envía,
en este orden y por la misma conexión:

1. la geometría del mosaico del escritorio nuevo, para que las ventanas
   aparezcan ya colocadas, y la subida de sus flotantes;
2. `WM_STATE` = NormalState y `MapWindow` de sus ventanas, antes de ocultar
   las anteriores, para no dejar ver la raíz;
3. el foco, antes de ocultar: si se desmapeara primero la ventana enfocada,
   el servidor devolvería el foco a la raíz;
4. `UnmapWindow` y `WM_STATE` = IconicState de las ventanas del escritorio
   anterior;
5. `_NET_CURRENT_DESKTOP`.

Mover una ventana publica su `_NET_WM_DESKTOP`, enfoca la siguiente del
historial, la oculta y reorganiza el escritorio activo. Conserva su
clasificación y su última geometría, que sigue siendo la real: si pide un
cambio estando oculta, el `ConfigureNotify` sintético lleva esa geometría
(ICCCM §4.1.5); al mostrarse se recoloca. Las flotantes conservan su
posición, que el servidor mantiene aunque estén desmapeadas.

### Desmapeos propios (`OwnUnmaps`)

El WM anota, por cada `UnmapWindow` con el que oculta una ventana, el par
(ventana, número de secuencia de la petición). Un `UnmapNotify` recibido
por la raíz es del WM si coincide exactamente con un par anotado (que se
consume); cualquier otro es una retirada del cliente.

Antes de adoptarlo se comprobaron sus garantías:

- **Qué número lleva un evento.** Cada evento lleva los 16 bits bajos del
  número de secuencia de la última petición **de la conexión del WM** que
  el servidor ha procesado, sea quien sea el cliente que lo causa. El
  número no identifica el origen, así que no basta con él. Lo que lo hace
  fiable es este razonamiento:
  - una ventana gestionada que no es override-redirect, una vez
    desmapeada, solo puede volver a mapearse a través del WM
    (SubstructureRedirect). Entre el desmapeo del WM y su siguiente
    petición ningún otro cliente puede generar un `UnmapNotify` real de esa
    ventana;
  - si el cliente la desmapea antes de que el servidor procese el
    desmapeo del WM, su evento lleva un número anterior (es una retirada) y
    el desmapeo del WM no genera ninguno.
- **Eventos de otras conexiones.** Por lo anterior, el número de secuencia
  de esos eventos es el del WM, nunca el del cliente que los causa.
- **Desbordamiento.** x11rb reconstruye números de 64 bits a partir de los
  16 del protocolo
  (`x11rb-protocol-0.14.0/src/connection/mod.rs:158`). Para que la
  reconstrucción sea inequívoca, fuerza una petición con respuesta
  (`GetInputFocus`) antes de que haya 2¹⁶−1 peticiones sin respuesta
  pendientes (`connection/mod.rs:102`). El WM compara solo esos números de
  64 bits (`wait_for_event_with_sequence` y
  `VoidCookie::sequence_number`).
- **Sintéticos.** El servidor también pone el número de secuencia del WM
  en un evento enviado con `SendEvent`, así que podría coincidir con un par
  anotado. Por eso un `UnmapNotify` sintético (bit 0x80 del código) nunca
  se compara: siempre es una retirada (ICCCM §4.1.4) y no consume ningún
  par.
- **Duplicados, cambios rápidos y destrucción.**
  - Cada par se consume una sola vez.
  - Una ventana puede tener varios pares pendientes (1→2→1→2).
  - Los eventos llegan en orden de número de secuencia, así que, al
    recibir un `UnmapNotify`, se descartan los pares anteriores: su evento
    ya no llegará.
  - Al olvidar una ventana (retirada o destrucción) se descartan sus pares,
    y un identificador reutilizado no los hereda.
  - Un `UnmapWindow` sobre una ventana ya destruida da `BadWindow`, que se
    registra como error X; su `DestroyNotify` la olvida.

Se descartó un contador de desmapeos pendientes por ventana: si el cliente
se retira justo antes del desmapeo del WM, el contador consumiría el evento
del cliente y la ventana retirada seguiría gestionada.

Al dejar de gestionar una ventana se registra el motivo:
- desmapeada por el cliente;
- desmapeada por el cliente con un desmapeo del WM pendiente (la carrera
  anterior);
- `UnmapNotify` sintético;
- destruida;
- con otro padre.

### Save-set y retiradas

- Al gestionar una ventana, `ChangeSaveSet(Insert)`. Cuando la conexión
  del WM se cierra por cualquier motivo (salida, SIGTERM, SIGKILL o fallo),
  el servidor mapea las ventanas desmapeadas de su save-set. Así ninguna
  queda oculta aunque el WM siga sin manejadores de señales.
- Al dejar de gestionarla por un `UnmapNotify` o un `ReparentNotify`:
  - `ChangeSaveSet(Delete)`, para que una ventana retirada no reaparezca
    al cerrarse el WM;
  - se borran `WM_STATE` (ICCCM §4.1.3.1) y `_NET_WM_DESKTOP`.

  Si el cliente acaba de terminar, la ventana ya no existe: esos errores se
  descartan con `VoidCookie::ignore_error`, no se registran. Tras un
  `DestroyNotify` no se envía nada: el servidor ya la quitó del save-set.
- Una retirada sintética de una ventana del escritorio activo (por ejemplo,
  porque el WM la mostró justo antes de recibirla) también se desmapea,
  para que no quede visible sin gestionar.
- **Salida normal** (`Mod4+Shift+Escape`):
  - el WM mapea él mismo las ventanas ocultas con `WM_STATE` =
    NormalState;
  - borra `_NET_ACTIVE_WINDOW`, `_NET_CURRENT_DESKTOP` y
    `_NET_NUMBER_OF_DESKTOPS`;
  - deja `_NET_WM_DESKTOP` en las ventanas, como recomienda EWMH para un
    WM posterior.

### Eventos de ventanas de escritorios inactivos

| Evento | Tratamiento |
|---|---|
| `DestroyNotify`, `ReparentNotify` (padre ≠ raíz) | Se olvida en su escritorio; no se reorganiza nada visible (se reorganiza al mostrarse) y el foco actual no cambia |
| `UnmapNotify` sintético | Retirada |
| `UnmapNotify` real | El del desmapeo del WM, o la carrera descrita arriba |
| `MapRequest` de una ventana gestionada oculta | Sigue oculta en su escritorio y se registra: ni cambio de escritorio ni robo de foco |
| `ConfigureRequest` | Como en el escritorio activo |
| `FocusIn` | Solo cuenta si la ventana es del escritorio activo |

### ICCCM y EWMH parciales

- `WM_STATE`: NormalState si la ventana está visible, IconicState si está
  oculta en otro escritorio, sin ventana de icono.
- En la raíz, `_NET_NUMBER_OF_DESKTOPS` = 8 y `_NET_CURRENT_DESKTOP`; en
  cada ventana gestionada, `_NET_WM_DESKTOP` (todos de tipo `CARDINAL`).
- Es información, no soporte declarado: no hay `_NET_SUPPORTED` ni
  `_NET_SUPPORTING_WM_CHECK` (la capa 3 del ADR 0003 no cambia), y no se
  atienden los `ClientMessage` de cambio de escritorio de los pagers.
  Herramientas como `xdotool get_desktop`, que exigen `_NET_SUPPORTED`, no
  lo leen. *Resuelto en F6 (ADR 0008): soporte declarado y peticiones
  atendidas.*

### Salvaguardas y procesos

- Las capas del ADR 0003 no cambian. Los átomos nuevos (`WM_STATE` y los
  tres `_NET_*`) se registran después de la capa 4, como los anteriores.
- El lanzador (ADR 0006) no sabe nada de escritorios. Los procesos nunca
  reciben señales del WM, así que cambiar o mover ventanas no les afecta.
  La ventana de un programa aparece en el escritorio activo **al
  mapearse**, no en el de la pulsación.

## Verificación

### Pruebas unitarias nuevas o cambiadas

- **U12** (`workspaces`, 8 pruebas, y `clients`, 1):
  - las ventanas nuevas van al activo, sin duplicarse en otro escritorio;
  - cambiar a otro escritorio, al mismo o fuera de rango; volver, que
    alterna, y sin escritorio anterior;
  - mosaicos, historiales, Zoom y navegación independientes;
  - mover la enfocada (en mosaico y flotante) y casos sin efecto;
  - retirar desde un escritorio inactivo sin tocar el activo;
  - 5 000 operaciones pseudoaleatorias con semilla fija, comprobando los
    invariantes tras cada una;
  - `take` y `adopt`.
- **U13** (`keys`, 3 pruebas nuevas y 2 cambiadas):
  - los 17 atajos con códigos 10–17 y 23, también con Bloq Mayús y Bloq
    Num;
  - Shift sobre la misma tecla;
  - ni `Alt+Tab` ni `Mod4+Shift+Tab`;
  - nombres, keysyms y carácter obligatorio de los 17;
  - detección de la repetición automática.

  La tabla de reconocimiento incluye los atajos nuevos, y el código de
  tecla sin atajo pasa del 10 al 18.
- **U14** (`wm`, 6 pruebas de `OwnUnmaps`):
  - coincidencia exacta de ventana y número de secuencia;
  - evento repetido;
  - retirada del cliente antes del desmapeo propio;
  - un sintético con la misma ventana y número no consume el par;
  - varios pendientes de la misma ventana;
  - olvidar una ventana;
  - números de más de 16 bits.

### Cliente de prueba

Las herramientas instaladas no pueden enviar el `UnmapNotify` sintético
con el que un cliente retira una ventana (xdotool no tiene `SendEvent` y no
hay python-xlib). `examples/test-client.rs` usa x11rb, sin dependencias
nuevas:
- `withdraw VENTANA`: `UnmapWindow` y el `UnmapNotify` sintético a la raíz
  con `SubstructureRedirect|SubstructureNotify`;
- `synthetic-unmap VENTANA`: solo el `UnmapNotify` sintético.

Rechaza cualquier `DISPLAY` que no sea `:N` con N ≠ 0 antes de conectar
(comprobado con `:0` y sin `DISPLAY`). Se compila con
`cargo build --examples`; no se instala ni se ejecuta fuera de los arneses.

### Pruebas de aceptación (`tests/acceptance/f5/`)

Como en F4: cada caso se ejecuta en un Xvfb nuevo; el WM corre con
`env -i` y el entorno mínimo, con su `PATH` reducido a copias de
`f4/fake.sh`; y cada caso registra la evidencia de aislamiento, que solo
se lanzaron programas falsos y que no hubo errores X. Las ventanas de prueba
son de `xev`.

| Id | Comprobación |
|---|---|
| A41 | `_NET_NUMBER_OF_DESKTOPS` = 8. Una ventana por escritorio; al recorrerlos (también de vuelta) solo es visible la del activo, que tiene el foco, con `_NET_CURRENT_DESKTOP` correcto. `_NET_WM_DESKTOP` y `WM_STATE` de las ocho; los ocho clientes vivos; sin `_NET_SUPPORTED` ni `_NET_SUPPORTING_WM_CHECK`; ninguna «retirada» |
| A42 | Escritorio 1 con A, B y C (foco en B) y 2 con D: al volver, el mismo mosaico, el mismo foco y la misma navegación |
| A43 | Mover B al 3: se oculta, el 1 se reorganiza y el foco pasa a C; propiedades; en el 3, B ocupa la pantalla y tiene el foco; mover al mismo escritorio y sin ventana enfocada no hace nada |
| A44 | El historial se conserva tras ir y volver; cerrar la enfocada de un escritorio oculto («destruida») no cambia el activo y, al volver, el foco va a la siguiente del historial y el mosaico se reorganiza |
| A45 | Zoom y `Mod4+j`/`Mod4+k` solo afectan al escritorio activo |
| A46 | Un diálogo se oculta y se restaura con la misma geometría y encima del mosaico; movido a otro escritorio, conserva su geometría |
| A47 | Cerrar ventanas en el escritorio activo y en uno oculto; `MapRequest` de una ventana oculta (`xdotool windowmap`): sigue oculta, sin cambio de escritorio ni de foco |
| A48 | 40 cambios sin pausa y 15 `Mod4+Tab` seguidos: estado final correcto y ninguna «retirada»; retirada real en el activo; retirada sintética de una oculta, que pierde sus propiedades y no vuelve; `MapRequest` de una oculta; **carreras**: 20 veces, el cliente retira su ventana (`test-client withdraw`, con 0–19 ms de retardo) a la vez que `Mod4+2` la oculta; la ventana siempre queda retirada y la de control sigue gestionada |
| A49 | `Mod4+Tab` sin anterior y `Mod4+1` en el 1 no hacen nada; alternancia; `Mod4+Tab` y `Mod4+Shift+2` mantenidos 1,5 s actúan una sola vez |
| A50 | En el escritorio 4: mosaico, navegación, Zoom y lanzamiento (programa falso); el hijo sobrevive a cinco cambios y un movimiento sin recibir señales |
| A51 | Tres instancias del WM en el mismo Xvfb. Ventanas ocultas en varios escritorios y ventanas retiradas (sintética y real): tras `Mod4+Shift+Escape`, SIGTERM y SIGKILL al proceso del WM lanzado por el arnés, las ocultas vuelven a verse y las retiradas no. Tras la salida normal, la raíz sin las propiedades y `WM_STATE` = NormalState |
| A52 | Como A9 y A40: sin pánicos ni residuos, sin programas falsos en marcha ni aplicaciones nuevas, y bspwm y sxhkd con el mismo PID |

### Comprobación de la capacidad de detección

Para confirmar que las pruebas distinguen un mecanismo roto, se compilaron
fuera del repositorio tres variantes de `wm.rs`:

| Mutación | Resultado |
|---|---|
| Ningún desmapeo se reconoce como propio | A41 falla: las ventanas desaparecen al cambiar de escritorio |
| Sin `ChangeSaveSet(Insert)` | A51 falla: tras SIGTERM, las ventanas ocultas siguen sin verse |
| Sin `ChangeSaveSet(Delete)` | A51 falla: las ventanas retiradas reaparecen al salir |

El código definitivo pasa esas mismas pruebas.

## Resultados de las pruebas de aceptación de F5

Ejecutadas el 2026-10-13 con `tests/acceptance/f5/run.sh` sobre el binario
de depuración (Xvfb 2:21.1.16-1.3+deb13u4, x11-utils 7.7+7,
x11-xserver-utils 7.7+11, xdotool 1:3.20160805.1-5.1, procps 2:4.0.4-9,
diffutils 1:3.10-4).

La ejecución definitiva dio **141 comprobaciones OK, 0 FALLA y 1 OMITIDA**
(`A52-xprop-:0`), con código de salida 0 y todos los `xvfb-run` con 0. Los
resultados detallados se guardaron fuera del repositorio.

| Prueba | Resultado |
|---|---|
| A41–A51 | Superadas, sin errores X; el único proceso lanzado fue un programa falso (A50) |
| A52 | **Parcial**: sin pánicos, residuos, programas falsos en marcha ni aplicaciones nuevas; bspwm y sxhkd con el mismo PID. La raíz de `:0` no se consultó |

**Carreras (A48).** Las 20 retiradas acabaron siempre retiradas y la
ventana de control, siempre gestionada. Por orden en el servidor:
- ejecución definitiva: 6 «desmapeada por el cliente» y 14 «`UnmapNotify`
  sintético»;
- dos ejecuciones de desarrollo con el mismo retardo: 5 y 6 «desmapeada
  por el cliente», 1 y 1 «desmapeada por el cliente; desmapeo del WM
  pendiente» (la carrera más estrecha) y 14 y 13 «`UnmapNotify`
  sintético».

Con un retardo de 0 a 57 ms solo se observaron «desmapeada por el cliente» y «`UnmapNotify` sintético», y
sin retardo, solo el del cliente.

Durante el desarrollo:
- una primera ejecución se bloqueó en A48 por un `wait` sin argumentos del
  arnés, que esperaba también al cliente pasivo `xprop -spy`; se corrigió;
- esa ejecución registró un fallo en `A52-sin-aplicaciones-nuevas`: un
  proceso `kitty` ajeno a las pruebas, iniciado en la sesión real mientras
  A48 estaba bloqueado. No figuraba en ningún registro del WM y su línea de
  órdenes no contenía el directorio de la prueba. No se repitió.

Regresiones, sobre el mismo binario: F1 46 OK y 1 OMITIDA (A9); F2 47 OK y
1 OMITIDA (A21); F3 61 OK y 1 OMITIDA (A32); F4 79 OK y 1 OMITIDA (A40).
Las cuatro terminaron con código 0 y no hubo que cambiar ninguna
comprobación. Como siempre, el `xvfb-run` de A8 termina con 1 (ADR 0003).

## Limitaciones conocidas

- **Cliente no conforme oculto:** si un cliente retira una ventana oculta
  solo con un desmapeo real, sin el `UnmapNotify` sintético de ICCCM
  §4.1.4, el desmapeo no tiene efecto (ya estaba desmapeada) y el WM no se
  entera. La ventana vuelve a mostrarse con su escritorio.
- **Override-redirect cambiado después:** si un cliente activa
  override-redirect en una ventana ya gestionada y oculta, podría mapearla
  sin el WM; no se trata.
- **Carrera más estrecha:** la de un cliente que desmapea entre el envío y
  el procesamiento del desmapeo del WM se observó en las pruebas, pero su
  momento no se puede forzar con las herramientas disponibles: el orden
  exacto en el servidor solo se prueba en U14.
- **Diálogos de ventanas ocultas:** van al escritorio activo, y al mover
  una ventana sus diálogos no la siguen (`WM_TRANSIENT_FOR` solo se usa
  para clasificar).
- **Sin sticky:** paneles y avisos (`DOCK`, `NOTIFICATION`) pertenecen a un
  escritorio y se ocultan con él; un panel futuro lo necesitará.
- **Ventanas sin foco:** no pueden moverse de escritorio, porque solo se
  mueve la enfocada.
- **SIGTERM/SIGKILL:** las ventanas ocultas vuelven a verse, pero conservan
  `WM_STATE` = IconicState y la raíz conserva `_NET_CURRENT_DESKTOP` y
  `_NET_NUMBER_OF_DESKTOPS`: no hay limpieza sin manejadores de señales.
- **Atajos globales:** `Mod4+1`…`8` y `Mod4+Tab` se capturan siempre. En un
  Xephyr dentro de la sesión real, sxhkd probablemente tendría ya esas
  combinaciones; en Xvfb no afecta.
- **EWMH:** sin `_NET_SUPPORTED`, los pagers y `xdotool get_desktop` no lo
  leen, y los mensajes de cambio de escritorio se ignoran. *Resuelto en F6
  (ADR 0008).*
- **Un monitor**, como en F2.

## Alternativas consideradas

- **Campo de escritorio en `Client`:** cada método de `Clients` tendría
  que filtrar y el historial global, partirse; descartado.
- **Mover las ventanas fuera de la pantalla** (como dwm): no genera
  eventos, pero las ventanas siguen visibles para el servidor, pueden
  recibir foco y contradice ICCCM, que asocia lo oculto con desmapeado e
  IconicState.
- **Contador de desmapeos pendientes por ventana:** ver «Desmapeos
  propios».
- **Mostrar el escritorio de una ventana oculta que pide mapearse:**
  descartado; robaría el foco.
- **Seguir a la ventana movida:** descartado por decisión del autor.
- **Diálogos al escritorio de su ventana principal:** aplazado; requiere
  leer el valor de `WM_TRANSIENT_FOR`.

## Consecuencias

- El ADR 0004 dejaba pendiente distinguir los desmapeos propios; queda
  resuelto aquí.
- `Clients` pasa a ser el estado de un escritorio, y `wm.rs` siempre opera
  sobre el activo, salvo para olvidar ventanas y responder a
  `ConfigureRequest`, que buscan en todos.
- El registro de cada retirada incluye el escritorio y el motivo; las
  pruebas anteriores buscaban «ventana X retirada» y «gestionada (tipo)»,
  que se conservan como prefijo.
- Los arneses necesitan `target/debug/examples/test-client`
  (`cargo build --examples`).
