# ADR 0008: Compatibilidad ICCCM/EWMH (F6)

- **Estado:** aceptado. Implementado el 2026-10-13 (F6-B). Las 94 pruebas
  unitarias pasaron, y las de aceptación A53–A62 y el caso A64 pasaron en
  Xvfb; las comprobaciones de sesión de A64 quedaron parciales por el mismo
  motivo que A9 (ver «Resultados de las pruebas de aceptación de F6»).
- **Fecha:** 2026-10-13
- **Actualización F7 (2026-10-14):** `_NET_SUPPORTED` pasa a 16 átomos con
  `_NET_WM_STATE` y `_NET_WM_STATE_FULLSCREEN`, y se atienden las
  peticiones `_NET_WM_STATE` sobre FULLSCREEN (ADR 0009). El renderizador
  `ctypes` descrito abajo es ahora `tools/render-svg.py`. *La pantalla
  completa, citada como aplazada, está resuelta.*
- **Actualización F8 (2026-10-14):** `WM_TRANSIENT_FOR` se interpreta y se
  sigue con `PropertyNotify` (sin reclasificar la ventana), y se lee
  `WM_NORMAL_HINTS` para respetar `USPosition` y `PPosition` (ADR 0010).
  `_NET_SUPPORTED` no cambia. *El «`WM_TRANSIENT_FOR` avanzado» citado como
  aplazado está resuelto, salvo los grupos y `_NET_WM_STATE_MODAL`.*

![Diagrama de F6: arranque e identificación, mensajes ICCCM y EWMH entre las aplicaciones, el servidor X11 y Crimson WM, políticas de las peticiones externas, salida y funciones aplazadas](../images/crimson-wm-f6-icccm-ewmh.png)

*Diagrama técnico elaborado a partir del código de F6; no es una captura ni
demuestra una ejecución. Fuente editable:
`docs/images/crimson-wm-f6-icccm-ewmh.svg`.*

## Contexto

Hasta F5, Crimson WM publicaba algunas propiedades EWMH
(`_NET_ACTIVE_WINDOW` y las de escritorios) sin declarar soporte, así que
paneles, lanzadores y herramientas como xdotool las ignoraban. No se podían
cerrar ventanas, `WM_TAKE_FOCUS` llevaba `CurrentTime` cuando el foco no
venía de una pulsación, y los cambios de `WM_HINTS` y `WM_PROTOCOLS` se
ignoraban. Declarar EWMH exige `_NET_SUPPORTING_WM_CHECK`, precisamente la
propiedad que la capa 3 del ADR 0003 usa para detectar otro gestor de
ventanas. F6 implementa un subconjunto realista de ICCCM y EWMH sin
debilitar esa salvaguarda.

## Decisión

### Módulos

| Módulo | Cambio |
|---|---|
| `src/icccm.rs` (nuevo, sin E/S) | `input_hint` (trasladada desde `clients.rs`), `Protocols` (`WM_DELETE_WINDOW`, `WM_TAKE_FOCUS`), mensajes `WM_PROTOCOLS` y valor de `WM_STATE` |
| `src/ewmh.rs` (nuevo, sin E/S) | `SUPPORTED` (la lista exacta), `decode`, que valida las peticiones (`Request`, `Source`, `Invalid`), y el filtro de `_NET_CLIENT_LIST_STACKING` |
| `src/wm.rs` | Identificación (ventana de comprobación, `WM_S0`, `MANAGER`), hora del servidor, cierre, peticiones EWMH, `PropertyNotify`, `SelectionClear`/`SelectionRequest`, `Dirty`/`publish_dirty` y limpieza propia |
| `src/safeguards.rs` | Capa 3 ampliada: diagnóstico de `_NET_SUPPORTING_WM_CHECK` y propietario de `WM_S{pantalla}` |
| `src/clients.rs` | `Client` y `Class` ganan `delete_window`; `Client`, `order`; `get_mut`; `WindowInfo.protocols` |
| `src/workspaces.rs` | Orden de gestión, `client_list`, `get_mut` y `move_window`, que generaliza `move_focused` |
| `src/keys.rs` | `Action::Close` (`Mod4+w`) y `Action::Kill` (`Mod4+Shift+w`), obligatorios |
| `src/main.rs` | Pasa la pantalla a la capa 3; registra los módulos nuevos |
| `examples/test-client.rs` | Órdenes `message`, `owner`, `own`, `redirect` y `window` para las pruebas |

`error.rs`, `display.rs`, `connect.rs`, `layout.rs` y `spawn.rs` no
cambian. No hay dependencias ni códigos de salida nuevos.

### Arranque seguro

1. **Capas 1 a 4** del ADR 0003. La capa 3 sigue rechazando (código 12)
   cualquier `_NET_SUPPORTING_WM_CHECK` en la raíz, también una obsoleta. El
   mensaje indica solo como diagnóstico si la ventana a la que apunta existe
   («existente» o «inexistente: obsoleta»).
2. **Nueva comprobación en la capa 3:** si la selección `WM_S{pantalla}`
   (ICCCM §2.8) tiene propietario, también se rechaza con 12. Así se
   detectan los gestores que no usan EWMH. El átomo se consulta sin crearlo:
   un servidor rechazado no queda modificado.
3. Captura de los atajos y registro de los átomos, como hasta ahora.
4. **Ventana de comprobación:** InputOnly, override-redirect y sin mapear,
   con `_NET_SUPPORTING_WM_CHECK` apuntándose a sí misma y `_NET_WM_NAME` =
   «Crimson WM» (`UTF8_STRING`).
5. **Hora del servidor:** `ChangeProperty` vacío (añadir cero bytes) en esa
   ventana. El `PropertyNotify` trae la marca de tiempo; se acepta el
   primero con un número de secuencia igual o posterior al de la petición.
6. **Con el servidor capturado** (`GrabServer`), para que ningún otro
   cliente se intercale entre la comprobación y la escritura:
   1. se vuelve a comprobar que `WM_S0` no tiene propietario y que la raíz
      no tiene `_NET_SUPPORTING_WM_CHECK`;
   2. `SetSelectionOwner` con la hora obtenida (nunca `CurrentTime`, ICCCM
      §2.1);
   3. `GetSelectionOwner` debe devolver la ventana propia;
   4. solo entonces se publican en la raíz `_NET_SUPPORTING_WM_CHECK` y
      `_NET_SUPPORTED`;
   5. `UngrabServer`.
7. **Anuncio `MANAGER`** a la raíz (ICCCM §2.8), con la hora, la selección y
   la ventana.

Si algo no cuadra en 4–6, el WM termina con 12 (otro gestor) o 14 y solo
destruye su propia ventana, lo que libera la selección si llegó a
adquirirla. En ese punto aún no ha escrito nada en la raíz. Nunca borra,
sustituye ni acepta propiedades de otro gestor, y no implementa el
reemplazo (`--replace`).

**Carreras.** Entre la capa 3 y la adquisición, otro cliente podría tomar
`WM_S0` o escribir la propiedad. Por eso se vuelven a comprobar ambas bajo
`GrabServer`. Mientras Crimson WM tiene `SubstructureRedirect`, ningún otro
gestor puede obtenerlo (`BadAccess`).

### Selección del gestor

- **`SelectionClear` de `WM_S0`** (otro cliente la ha tomado): el WM lo
  registra, limpia lo propio como en una salida normal y termina con 0, sin
  intentar recuperarla.
- **`SelectionRequest`:** el WM no ofrece ninguna conversión. Responde con
  un `SelectionNotify` con la propiedad None (ICCCM §2.2), para que el
  solicitante no quede esperando.

### Marcas de tiempo

- **`WM_TAKE_FOCUS`** lleva siempre una marca real: la de la pulsación, o
  la hora del servidor (paso 5) si el foco cambia al abrir o retirar una
  ventana, al arrancar o por una petición EWMH.
- **`SetInputFocus`** sigue usando `CurrentTime` cuando no hay pulsación. El
  WM decide el foco, y una marca anterior al último cambio haría que el
  servidor ignorara la petición, dejando el modelo y el servidor
  incoherentes. La marca que trae una petición `_NET_ACTIVE_WINDOW` no se
  usa por el mismo motivo: el WM aplica su propia política de origen.
- **`WM_DELETE_WINDOW`** lleva la marca de la pulsación (`Mod4+w`), la de la
  petición `_NET_CLOSE_WINDOW` si no es 0, o la hora del servidor.
- Mientras se espera la hora del servidor, los demás eventos se guardan en
  orden en una cola (`pending`) y se atienden después.

### Cierre de ventanas

| Origen | Cliente con `WM_DELETE_WINDOW` | Cliente sin `WM_DELETE_WINDOW` |
|---|---|---|
| `Mod4+w` (enfocada) | `ClientMessage` `WM_DELETE_WINDOW` (ICCCM §4.2.8.1) | Se registra; no se cierra |
| `_NET_CLOSE_WINDOW` (cualquier escritorio) | Igual | Se registra; **nunca** `KillClient` |
| `Mod4+Shift+w` (enfocada), solo por acción explícita del usuario | `KillClient` | `KillClient` |

`KillClient` cierra la conexión X11 del cliente: **todas** sus ventanas
desaparecen, no solo la enfocada (comprobado en A56 con dos ventanas de la
misma conexión). `Mod4+w` y `Mod4+Shift+w` son atajos de control
obligatorios (código 14) e ignoran la repetición automática, como mover de
escritorio.

### Cambios de propiedades

El WM selecciona `PropertyChange` en las ventanas gestionadas. Solo atiende
`WM_HINTS` y `WM_PROTOCOLS`: vuelve a leer ambas (lectura acotada; un valor
mal formado cuenta como ausente) y actualiza el modelo de entrada y el
soporte de `WM_DELETE_WINDOW`. El cambio se aplica en la siguiente decisión
de foco. Las propiedades que escribe el propio WM (`WM_STATE`,
`_NET_WM_DESKTOP`) también generan `PropertyNotify`, pero se ignoran, así
que no hay bucles. El tipo de ventana y `WM_TRANSIENT_FOR` se siguen
leyendo solo al gestionarla.

### Peticiones EWMH

`ewmh::decode` valida antes de actuar: formato 32, origen 0, 1 o 2 e
índices de 0 a 7. `wm.rs` comprueba además que la ventana esté gestionada.
Las rechazadas se registran y se ignoran.

| Petición | Comportamiento |
|---|---|
| `_NET_ACTIVE_WINDOW`, fuente 2 (paginador o usuario) | Cambia al escritorio de la ventana si hace falta y la enfoca |
| `_NET_ACTIVE_WINDOW`, fuente 1 (aplicación) o 0 | Solo si la ventana está en el escritorio activo; nunca cambia de escritorio |
| `_NET_CURRENT_DESKTOP` | Sobre la raíz y con un índice de 0 a 7: como `Mod4+n` |
| `_NET_WM_DESKTOP` | Mueve cualquier ventana gestionada sin cambiar de escritorio ni darle el foco. Si sale del activo, se oculta (y el foco pasa a la siguiente del historial); si entra en él, se muestra. Sticky (0xFFFFFFFF) y fuera de rango, rechazados |
| `_NET_CLOSE_WINDOW` | Cierre cooperativo (tabla anterior) |

### Lo que se declara

`_NET_SUPPORTED` contiene exactamente, en este orden:
`_NET_SUPPORTED`, `_NET_SUPPORTING_WM_CHECK`, `_NET_CLIENT_LIST`,
`_NET_CLIENT_LIST_STACKING`, `_NET_NUMBER_OF_DESKTOPS`,
`_NET_CURRENT_DESKTOP`, `_NET_ACTIVE_WINDOW`, `_NET_CLOSE_WINDOW`,
`_NET_WM_DESKTOP`, `_NET_WM_WINDOW_TYPE`, `_NET_WM_WINDOW_TYPE_NORMAL`,
`_NET_WM_WINDOW_TYPE_DIALOG`, `_NET_WM_WINDOW_TYPE_UTILITY` y
`_NET_WM_WINDOW_TYPE_TOOLBAR`.

No se declaran:
- **`_NET_WM_NAME`:** está en la ventana de comprobación, pero el WM no lee
  la de los clientes.
- **Los demás tipos de ventana** (paneles, escritorio, avisos, menús…):
  solo quedan flotantes, sin un tratamiento propio.
- **Nada de lo aplazado.**

### Listas de ventanas y publicación agrupada

- **`_NET_CLIENT_LIST`:** todas las ventanas gestionadas, también las de
  escritorios ocultos, en el orden en que empezaron a gestionarse
  (`Client::order`, asignado por `Workspaces`). No hay ninguna copia del
  estado.
- **`_NET_CLIENT_LIST_STACKING`:** se calcula con `QueryTree` de la raíz,
  que da el orden real de apilamiento de abajo arriba, filtrado por las
  ventanas gestionadas.
- **Publicación agrupada:** los manejadores no escriben las listas, solo
  marcan `Dirty { clients, stacking }`. Antes de enviar las peticiones de
  cada tanda de eventos, `publish_dirty` las escribe una sola vez, con a lo
  sumo un `QueryTree`.

| Suceso | Marca |
|---|---|
| Ventana gestionada o retirada | `clients` y `stacking` |
| `arrange` (sube las flotantes), cambio de escritorio, movimiento, `ConfigureRequest` de una flotante o no gestionada | `stacking` |

`_NET_ACTIVE_WINDOW`, `_NET_CURRENT_DESKTOP`, `_NET_WM_DESKTOP` y
`WM_STATE` se siguen escribiendo en el momento, como en F5.

### Salida y terminación abrupta

- **Salida normal o `SelectionClear`:**
  1. muestra las ventanas ocultas (con `WM_STATE` = NormalState);
  2. si `_NET_SUPPORTING_WM_CHECK` de la raíz sigue apuntando a su ventana,
     borra las siete propiedades del WM en la raíz; si no, se conservan,
     porque serían de otro gestor;
  3. destruye la ventana de comprobación, lo que libera `WM_S0` si aún es
     suya;
  4. devuelve el foco al puntero y espera la respuesta de `GetInputFocus`.
- **Error después de la identificación** (por ejemplo, `QueryTree` al
  adoptar): la misma limpieza antes de devolver el error, salvo si se ha
  perdido la conexión.
- **SIGTERM o SIGKILL** (sin manejadores de señales):
  - el servidor destruye la ventana, así que `WM_S0` queda libre;
  - el save-set vuelve a mapear las ventanas ocultas;
  - `_NET_SUPPORTING_WM_CHECK` queda obsoleta en la raíz, y el siguiente
    arranque en ese servidor se rechaza con 12 (**fail-closed**) hasta
    borrarla a mano, por ejemplo con
    `xprop -root -remove _NET_SUPPORTING_WM_CHECK` en el servidor de
    prueba.

  El WM nunca la elimina por su cuenta: no puede saber si la dejó él u otro
  gestor caído.

## Verificación

### Pruebas unitarias nuevas o cambiadas (94 en total, 14 más que en F5)

- **U15** (`icccm`, 4 pruebas; `keys`, 1):
  - campo `input` de `WM_HINTS` (trasladada desde `clients`);
  - `Protocols`;
  - mensajes `WM_DELETE_WINDOW` y `WM_TAKE_FOCUS` byte a byte (sustituye a
    la prueba de `wm`);
  - valor de `WM_STATE`;
  - atajos de cierre, obligatorios.
- **U16** (`ewmh`, 7; `safeguards`, 2; `workspaces`, 2):
  - lista exacta de `_NET_SUPPORTED`, sin nada aplazado;
  - decodificación y rechazo de cada petición: origen, índice, sticky, raíz
    y formato; tipos ignorados;
  - filtro de apilamiento;
  - nombre de la selección y valor de la comprobación;
  - `move_window`;
  - orden de `_NET_CLIENT_LIST`.

  Los invariantes de `Workspaces` comprueban además que la lista esté
  completa y sin repeticiones tras cada una de las 5 000 operaciones
  pseudoaleatorias, que ahora incluyen `move_window`.

### Cliente de prueba

`examples/test-client.rs` sigue siendo solo infraestructura de pruebas. No
lo usa el WM ni se instala, y rechaza cualquier `DISPLAY` que no sea `:N`
con N ≠ 0 antes de conectar. Órdenes nuevas:

| Orden | Uso |
|---|---|
| `message FORMATO VENTANA TIPO D0…D4` | Envía `ClientMessage` a la raíz; también mal formados |
| `owner SELECCIÓN` | Escribe el propietario de la selección |
| `own SELECCIÓN SEGUNDOS` | Toma la selección (A54) |
| `redirect SEGUNDOS` | Toma solo `SubstructureRedirect` (capa 4, A54) |
| `window NOMBRE [--no-delete] [--take-focus] [--pair]` | Ventana que registra cada `WM_TAKE_FOCUS` y `WM_DELETE_WINDOW` con su marca de tiempo |

### Pruebas de aceptación (`tests/acceptance/f6/`)

Mismo aislamiento que en F4 y F5: un Xvfb nuevo por caso, el WM con
`env -i` y el `PATH` reducido a programas falsos, y evidencia de
aislamiento por caso.

| Id | Comprobación |
|---|---|
| A53 | La raíz y la ventana apuntan a la ventana de comprobación (InputOnly, sin mapear, override-redirect, «Crimson WM»). `_NET_SUPPORTED` es exacta, `WM_S0` es suya y el anuncio `MANAGER` llega a un cliente de la raíz. Están las siete propiedades. Tras la salida: ninguna propiedad, la ventana destruida, `WM_S0` libre y un nuevo arranque limpio |
| A54 | `_NET_SUPPORTING_WM_CHECK` ajena viva u obsoleta: 12, la propiedad intacta y nada publicado. `WM_S0` con propietario: 12, sin tocarla. La capa 4 (`BadAccess`) con un cliente que solo toma la redirección. SIGKILL: ocultas visibles, propiedad obsoleta, `WM_S0` libre; el reinicio da 12 sin tocar la propiedad, y tras borrarla el arnés, arranca. `SelectionClear`: sale con 0, no recupera `WM_S0`, borra lo suyo y deja visibles las ventanas |
| A55 | `Mod4+w`: `WM_DELETE_WINDOW` con marca > 0, la ventana se cierra y el mosaico se reorganiza; también con xev; mantener `Mod4+w` cierra una sola ventana |
| A56 | Sin `WM_DELETE_WINDOW`: `Mod4+w` no cierra y lo registra; `Mod4+Shift+w` termina al cliente; con dos ventanas en una conexión, `KillClient` cierra ambas |
| A57 | `_NET_CLOSE_WINDOW` en el escritorio activo (marca 0, se pone la hora del servidor) y en uno oculto (se conserva la marca 12345); sin `WM_DELETE_WINDOW`, el cliente sigue vivo; formato 8 y fuente 9, rechazados |
| A58 | `_NET_CLIENT_LIST`: vacía, con ventanas en varios escritorios, tras cerrar una oculta, mover, retirar y abrir |
| A59 | `_NET_CLIENT_LIST_STACKING` igual que `QueryTree`: flotante arriba, subida denegada en mosaico, ocultas incluidas, tras mover |
| A60 | `_NET_ACTIVE_WINDOW`: aplicación en el mismo escritorio sí; aplicación o fuente 0 hacia otro escritorio, no; paginador sí, cambiando de escritorio; `xdotool windowactivate`; raíz y fuente 5, rechazadas |
| A61 | `xdotool get_desktop`, `set_desktop` y `set_desktop_for_window` (entre ocultos y hacia el activo); escritorio 8, sticky, no-raíz y 9, rechazados |
| A62 | `WM_TAKE_FOCUS` con la hora del servidor al abrir y con la marca de la pulsación después. `PropertyNotify`: quitar `WM_PROTOCOLS`, solo `WM_TAKE_FOCUS` (LocallyActive, xev recibe el mensaje) y solo `WM_DELETE_WINDOW` (`Mod4+w` cierra). Solo esos tres cambios en el registro: sin bucles |
| A64 | SIGTERM: ocultas visibles, propiedad obsoleta, `WM_S0` libre y reinicio rechazado sin tocarla. En la sesión: como A9 |

`xprop -f PROP 32a -set` guarda un solo átomo: una lista separada por comas
sería un átomo con ese nombre. Por eso A62 cambia `WM_PROTOCOLS` de un
protocolo en otro.

### Cambios en los arneses anteriores

- **F1 A6:** la segunda instancia la rechaza ahora la capa 3 (patrón
  `_NET_SUPPORTING_WM_CHECK (0x…, existente)`), tras esperar a que la
  primera se identifique. La capa 4 se prueba en A54.
- **F5:**
  - `A41-ewmh-parcial` pasa a ser `A41-ewmh-declarado`;
  - A51 comprueba el rechazo tras SIGTERM (`A51-tras-sigterm`) y borra la
    propiedad antes de reiniciar;
  - `run.sh` compara cada binario solo con sus fuentes. Antes también
    comparaba el WM con `examples/`, y una edición de `test-client` bastaba
    para que se negara a ejecutarse.

### Comprobación de la capacidad de detección

Se compilaron fuera del repositorio tres variantes de `wm.rs`:

| Mutación | Resultado |
|---|---|
| `WM_TAKE_FOCUS` con `CurrentTime` (comportamiento de F5) | A62 falla: marca 0 |
| Una aplicación puede cambiar de escritorio con `_NET_ACTIVE_WINDOW` | A60 falla |
| `_NET_CLOSE_WINDOW` usa `KillClient` sin `WM_DELETE_WINDOW` | A57 falla: C3 destruida |

## Resultados de las pruebas de aceptación de F6

Ejecutadas el 2026-10-13 con `tests/acceptance/f6/run.sh` sobre el binario
de depuración (Xvfb 2:21.1.16-1.3+deb13u4, x11-utils 7.7+7,
x11-xserver-utils 7.7+11, xdotool 1:3.20160805.1-5.1, procps 2:4.0.4-9,
diffutils 1:3.10-4).

La ejecución definitiva dio **122 comprobaciones OK, 0 FALLA y 1 OMITIDA**
(`A64-xprop-:0`), con código de salida 0 y todos los `xvfb-run` con 0. La
primera ejecución dio 97 OK y 27 FALLA, todos fallos del arnés y no del
WM, que se corrigieron:
- A53 suponía que el identificador de la ventana cambiaría al reiniciar;
- A54 no leía la propiedad ajena puesta como `CARDINAL`;
- en A58, la lista vacía;
- en A62, la sintaxis de `xprop` descrita arriba;
- A56 no admitía el `BadWindow` benigno de `KillClient`.

Ese `BadWindow` aparece al reorganizar tras retirar una ventana de la
conexión cerrada: la otra ventana ya no existe (ADR 0003). La comprobación
de A56 solo lo admite sobre esas dos ventanas.

Regresiones sobre el mismo binario:
- **F1:** 46 OK y 1 OMITIDA. Como siempre, el `xvfb-run` de A8 termina con
  1 (ADR 0003).
- **F2:** 47 OK y 1 OMITIDA. En una primera ejecución, A17 falló en la
  preparación común del Xvfb («servidor sin reinicios»), antes de arrancar
  el WM, que no llegó a crear su registro. La repetición pasó.
- **F3:** 61 OK y 1 OMITIDA.
- **F4:** 79 OK y 1 OMITIDA.
- **F5:** 142 OK y 1 OMITIDA, con las adaptaciones descritas.

Todas terminaron con código 0.

## Documentación visual

- `docs/images/crimson-wm-f6-icccm-ewmh.svg` (fuente editable) y
  `docs/images/crimson-wm-f6-icccm-ewmh.png` (2800 × 3080, escala 2). El
  PNG se renderizó con las bibliotecas del sistema librsvg 2.60.0 y cairo
  1.18.4, llamadas desde Python con `ctypes`, sin instalar nada. El
  renderizador interno de ImageMagick no es fiel: no dibuja los trazos. Se
  revisó visualmente a escala 1 y ampliado: sin cortes ni superposiciones.
- `docs/images/crimson-wm-overview.svg`, actualizado a F6:
  - capa 3 ampliada;
  - atajos de cierre;
  - paso de identificación;
  - eventos ICCCM/EWMH;
  - en la lista de pendientes sale `_NET_SUPPORTING_WM_CHECK` y entra la
    pantalla completa.
- **Sin captura de Xvfb:** F6 no cambia nada visible en pantalla. Sus
  efectos son propiedades, selecciones y mensajes entre clientes, y una
  captura mostraría el mismo mosaico que las de F2 y F5. La evidencia está
  en los resultados de las pruebas (`xprop`, `test-client` y los registros
  del WM).

## Limitaciones conocidas

- **Recuperación manual:** tras SIGTERM o SIGKILL hay que borrar
  `_NET_SUPPORTING_WM_CHECK` a mano para volver a arrancar en el mismo
  servidor. Es el precio de no relajar la capa 3.
- **Sin manejadores de señales:** SIGTERM no limpia, igual que en F5.
- **Clientes que no responden:** sin `_NET_WM_PING`, el WM no detecta que
  un cliente ignora `WM_DELETE_WINDOW`. Queda `Mod4+Shift+w`.
- **`KillClient`** cierra todas las ventanas de la conexión, por ejemplo
  todas las de una misma instancia de una aplicación.
- **Cambios de `WM_HINTS`** (campo `input`): solo se prueban en las pruebas
  unitarias. Comparten el camino de `WM_PROTOCOLS`, probado en A62, pero
  `xprop` no puede escribir una propiedad de tipo `WM_HINTS`.
- **Activación sin subir la ventana:** `_NET_ACTIVE_WINDOW` enfoca, pero no
  sube una flotante tapada.
- **Diagnóstico de la capa 3:** acepta cualquier tipo de 32 bits. Es solo
  informativo: el rechazo no depende de él.
- **Aplazado** (no declarado):
  - para F7: `_NET_WM_STATE` y la pantalla completa;
  - `_NET_FRAME_EXTENTS`;
  - `_NET_WORKAREA` y los struts;
  - ventanas sticky, `_NET_WM_PING`, la urgencia y `_NET_WM_ALLOWED_ACTIONS`;
  - `WM_TRANSIENT_FOR` avanzado y las reglas por `WM_CLASS`;
  - varios monitores, IPC y configuración.

## Alternativas consideradas

- **Aceptar una `_NET_SUPPORTING_WM_CHECK` obsoleta si su ventana no
  existe:** descartado; debilitaría la capa 3, porque un gestor caído
  también la deja.
- **Borrar automáticamente las propiedades obsoletas:** descartado por la
  misma razón.
- **`SetInputFocus` con la marca de la petición externa:** descartado; una
  marca antigua haría que el servidor ignorara el foco con el modelo ya
  cambiado.
- **Mantener un modelo propio de la pila para
  `_NET_CLIENT_LIST_STACKING`:** descartado; las flotantes se reordenan
  solas y el modelo se desfasaría. `QueryTree` da el orden real.
- **`KillClient` en `_NET_CLOSE_WINDOW` sin `WM_DELETE_WINDOW`:**
  descartado; una petición externa no debe terminar un cliente.
- **Declarar todos los tipos de ventana:** descartado; los paneles y el
  escritorio no tienen un tratamiento propio.

## Consecuencias

- Paneles, lanzadores y xdotool reconocen el WM y sus escritorios.
- La segunda instancia de Crimson WM sobre un servidor se detiene en la
  capa 3 y no en la 4. La capa 4 sigue probada.
- `wm.rs` crece y concentra toda la E/S. La lógica sin E/S está en
  `icccm.rs`, `ewmh.rs`, `clients.rs` y `workspaces.rs`.
- Los arneses de F5 y F6 necesitan `target/debug/examples/test-client`
  (`cargo build --examples`).
