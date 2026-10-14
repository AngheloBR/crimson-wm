# ADR 0012: Struts y área de trabajo EWMH (F10)

- **Estado:** aceptado. Implementado el 2026-10-14 (F10-B). Las 139 pruebas
  unitarias pasaron, y las de aceptación A104–A119 pasaron en Xvfb; A120
  quedó parcial por el mismo motivo que A9 (ver «Resultados de las pruebas
  de aceptación de F10»).
- **Fecha:** 2026-10-14

![Captura real en Xvfb: con F9, el panel superior tapa la parte de arriba del mosaico; con F10, el mosaico empieza debajo de los 40 px reservados; y, al pasar el panel abajo con su reserva, el mosaico se reajusta](../images/crimson-wm-f10-comparison.png)

*Capturas auténticas (A118 con el binario de F9, compilado a partir del
commit 2b2df85, y A119 con el de F10): volcados de `xwd -root` de Xvfb
aislados de 1280 × 720 con ventanas de `test-client` de color liso. La
composición solo añade rótulos, márgenes y bordes; cada panel es idéntico
píxel a píxel a su captura. Los rótulos de pila y área salen del servidor
en el momento de cada captura.*

![Diagrama de F10: lectura de los struts, las cuatro reservas y un tramo parcial, varios paneles, pantalla física frente a área de trabajo y los ocho escritorios, valores inválidos y extremos, y lo aplazado](../images/crimson-wm-f10-workarea.png)

*Diagrama técnico elaborado a partir del código de F10; no es una captura.
Fuente editable: `docs/images/crimson-wm-f10-workarea.svg`.*

## Contexto

Hasta F9, el WM usaba un único rectángulo, la pantalla física, para el
mosaico, la pantalla completa, el centrado de diálogos y el marco de
respaldo. Un panel (`DOCK`) era una ventana fija visible en todos los
escritorios (ADR 0011), pero no reservaba espacio: el mosaico quedaba debajo
de él (panel 1 de la captura).

EWMH define dos propiedades para reservar espacio en los bordes y una para
publicar el resultado. Las referencias aprobadas son las secciones 3 y 5 de
EWMH 1.5 (`specifications.freedesktop.org/wm/latest/ar01s03.html` y
`ar01s05.html`). No hay copia local y F10 se ha desarrollado sin red, con
las reglas aprobadas en F10-B como requisitos.

## Reglas de EWMH y decisiones de Crimson

| Regla | Origen |
|---|---|
| `_NET_WM_STRUT`: 4 `CARDINAL` de 32 bits (izquierda, derecha, arriba, abajo) | EWMH |
| `_NET_WM_STRUT_PARTIAL`: 12 `CARDINAL` de 32 bits: los 4 grosores y el tramo (inicio, fin) de cada borde | EWMH |
| Los tramos son coordenadas de la raíz | EWMH |
| `PARTIAL` tiene prioridad sobre `STRUT` cuando las dos existen | EWMH |
| `_NET_WORKAREA`: un rectángulo `[x, y, ancho, alto]` por escritorio | EWMH |
| Solo los `DOCK` gestionados reservan espacio | Crimson |
| `PARTIAL` presente pero mal formada: defecto, sin recurrir a `STRUT` | Crimson |
| Varios paneles: el máximo de cada borde, no la suma | Crimson |
| No se exige que el strut coincida con la geometría del panel | Crimson |
| El área de trabajo conserva al menos un píxel en cada eje | Crimson |
| Un tramo fuera de la raíz no reserva; uno que se sale cuenta | Crimson |
| La pantalla completa usa la pantalla física | Crimson (coherente con EWMH) |

## Decisión

### Lectura y validación (`ewmh::struts`)

- Se leen las dos propiedades con `GetProperty(AnyPropertyType)` acotado a
  12 y 4 valores. Así se conocen el tipo, el formato y si sobran datos
  (`bytes_after`) sin lecturas ilimitadas.
- **Tres estados** por propiedad: ausente (tipo `None`), presente y válida,
  y presente pero mal formada. Un error X al leer (la ventana ya no existe)
  no se confunde con «ausente»: no cambia nada y su UnmapNotify o
  DestroyNotify la retira.
- **Válida:** tipo `CARDINAL`, formato 32, exactamente 4 o 12 valores y
  nada más. En `PARTIAL`, además, cada borde con grosor tiene `inicio ≤ fin`.
- **Prioridad:**
  - `PARTIAL` válida: se usa y `STRUT` se ignora, sea como sea;
  - `PARTIAL` ausente: se usa `STRUT` si es válida, con tramos de borde
    completo;
  - `PARTIAL` presente pero mal formada: se registra el defecto
    (`StrutError::Type`, `Length` o `Range`) y el panel no reserva nada; no
    se recurre a `STRUT` en silencio;
  - ninguna válida: el `DOCK` sigue visible sin reservar espacio.
- **Declarado frente a efectivo:** `Strut` guarda los valores declarados
  por el cliente, sin recortar (`Fixed::strut`). `Strut::effective` calcula
  la reserva efectiva con la geometría de la pantalla:
  - un tramo que no toca la raíz (`inicio` fuera de ella, como el de un
    monitor que no existe) no reserva y se registra;
  - un tramo que se sale de la raíz cuenta, porque basta con que la toque;
  - el grosor se limita a la dimensión perpendicular de la pantalla.

  Si la reserva efectiva difiere de la declarada, se registran las dos.

### Representación

- **`clients::FixedKind`** (`Dock`, `Notification` y `Desktop`) sustituye a
  `Layer` en `Disposition::Fixed`. `layer()` mantiene las capas de F9 y
  `reserves()` solo es cierto para `Dock`.
- **`Workspaces::fixed`** guarda, para cada fija, su `kind` y, desde F10,
  su `strut` declarado. `set_strut` solo lo admite en un panel. Un nuevo
  invariante lo comprueba: una fija que no es panel nunca tiene struts.
- **`Workspaces::insets`** agrega las reservas efectivas de los paneles con
  `layout::Insets::max`, borde a borde, con un resultado independiente del
  orden.
- **`layout::work_area(screen, insets)`** devuelve el área de trabajo y si
  ha hecho falta limitarla. **`layout::workarea_property`** da los 32
  valores de `_NET_WORKAREA`.
- **`Wm`** guarda `screen` (la pantalla física, fija desde el arranque) y
  `work`. `work` es una caché derivada de `insets` que solo sirve para
  saber cuándo cambia el área y no reorganizar sin motivo.

### Varios paneles

- **Máximo, no suma:** los struts se miden desde el borde de la pantalla.
  Dos paneles apilados arriba declaran 30 y 60; la reserva es 60, no 90
  (A107).
- **Bordes distintos:** cada uno restringe el suyo. Las reservas que se
  cruzan en una esquina no se cuentan dos veces, porque cada borde se trata
  por separado.
- **Al irse un panel,** la reserva se recalcula con los que quedan (A107,
  A111).

### Reservas extremas

No hay un límite porcentual. `work_area` solo garantiza un área válida y no
vacía:

- todo se calcula en `u32`, sin desbordamientos, y se convierte con
  `try_from`;
- cada eje conserva **al menos un píxel**; si las reservas de un eje no lo
  dejan, se limita primero la del borde final (derecha o abajo) y, si no
  basta, la del inicial. Es determinista;
- el resultado nunca tiene tamaño 0, que X11 rechaza con `BadValue`;
- cuando hay que limitar, se registra «reservas limitadas» con lo pedido.

El mínimo de un píxel es una regla de Crimson, no de EWMH, que no fija
ninguno; es el único necesario para que la geometría sea válida. Un panel
puede dejar un área inútil de un píxel (A110). Ningún valor provoca un
pánico ni un rectángulo imposible.

### Área de trabajo rectangular y tramos parciales

`_NET_WORKAREA` publica un único rectángulo por escritorio. Un panel
superior que ocupa solo 0–399 px reserva **toda** la franja superior (A108),
y el hueco libre a su derecha no se aprovecha. Un área no rectangular queda
aplazada.

### Mosaico, flotantes, pantalla completa y diálogos

- **Mosaico:** `Clients::plan(work, screen)` aplica master-stack sobre el
  área de trabajo, con la proporción del 50 %. No hay bordes ni márgenes
  que respetar.
- **Cuando cambia el área:** el escritorio activo se reorganiza al momento;
  un escritorio oculto, al mostrarse (A113).
- **Flotantes:** nunca se mueven al cambiar el área (A114).
- **Pantalla completa:** ocupa la pantalla física (`plan` y el
  `ConfigureNotify` sintético usan `screen`). Enfocada, queda por encima del
  panel (capas de F9). Al salir, una ventana en mosaico vuelve a su hueco
  del área; una flotante, a su marco guardado (A115).
- **Marco de respaldo** (flotante adoptada a pantalla completa sin marco
  conocido): 2/3 del área de trabajo, centrado.
- **Diálogos:** se centran sobre su propietario, limitados al área de
  trabajo, así que no nacen debajo del panel (A114). Las reservas inválidas
  ya se han rechazado antes, así que no alteran la restauración de F7 ni de
  F8.

### Eventos y ciclo de vida

| Evento | Acción |
|---|---|
| `MapRequest` de un `DOCK` | `manage_fixed` selecciona `PropertyChange` y después lee los struts; si el área cambia, `arrange` **antes** de mapearlo |
| `PropertyNotify` de `_NET_WM_STRUT` o `_NET_WM_STRUT_PARTIAL` de un `DOCK` gestionado | releer ambas; recalcular solo si sus struts han cambiado; `arrange` solo si cambia el área |
| Esas propiedades en otra ventana | se ignoran (A117) |
| `ConfigureRequest` de un `DOCK` | como en F9: geometría sí, apilamiento no; la reserva no cambia (A111) |
| `UnmapNotify` / `DestroyNotify` | la reserva desaparece con la ventana; recálculo y `arrange` si cambia el área |
| Cambio de escritorio | el área es la misma en todos; `show` reorganiza con ella |
| Adopción | se leen los struts de los paneles adoptados y un único `arrange` al final (A116) |
| Salida | `release` borra `_NET_WORKAREA` con las demás propiedades de la raíz, si la comprobación sigue siendo del WM (A116, A53) |
| Reinicio | la nueva instancia vuelve a leerlos al adoptar (A116) |

- `_NET_WORKAREA` se publica en `publish_dirty`: la primera vez después de
  identificarse y luego solo cuando cambia el área.
- **Sin bucles:** la raíz no selecciona `PropertyChange` (solo
  `SubstructureRedirect|SubstructureNotify`), así que escribir
  `_NET_WORKAREA` no genera eventos para el WM, y el WM nunca escribe los
  struts.
- **Un panel que cambie su strut sin parar** provoca, por evento, dos
  lecturas acotadas y, como mucho, la reconfiguración de las ventanas cuya
  geometría cambia.

### `_NET_SUPPORTED`

Se añadieron `_NET_WM_STRUT`, `_NET_WM_STRUT_PARTIAL` y `_NET_WORKAREA`, al
final de la lista (24 átomos), **después** de que la batería de F10 pasara
sin ellos. La comprueban la prueba unitaria y A53 (F6), A100 (F9) y A104
(F10).

### Seguridad y límites de confianza

- **Lecturas acotadas:** 12 y 4 valores. El tipo, el formato y la longitud
  se validan antes de usar nada.
- **Valores hostiles** (`u32::MAX`, tramos invertidos, tipos falsos): se
  rechazan o se limitan sin pánicos (A110). Un panel puede reducir el área
  a un píxel por eje; como desde F9, cualquier cliente X comparte el mismo
  límite de confianza y podría tapar la pantalla con override-redirect.
  No obtiene ningún privilegio nuevo.
- **Solo los `DOCK`:** una aplicación normal no puede encoger el área de
  trabajo de las demás (A117).
- Las salvaguardas de F1–F9 no cambian.

## Alternativas descartadas

- **Conservar al menos el 50 % de la pantalla** (propuesta de F10-A):
  descartada en la aprobación. Es una política arbitraria que EWMH no pide;
  se sustituyó por el mínimo de un píxel.
- **Recurrir a `STRUT` si `PARTIAL` es defectuosa:** ocultaría el defecto y
  aplicaría una reserva que el cliente ya ha sustituido.
- **Sumar las reservas de un mismo borde:** contaría dos veces los paneles
  apilados.
- **Exigir que el strut coincida con la geometría del panel:** EWMH no lo
  pide, y los paneles que se ocultan solos lo incumplen.
- **Mover las flotantes al cambiar el área:** movimientos inesperados.
- **Pantalla completa limitada al área de trabajo:** contradice el sentido
  de la pantalla completa y las capas de F9.

## Consecuencias y limitaciones

- Los `DOCK` reservan su borde; el mosaico, los diálogos nuevos y el marco
  de respaldo usan el área de trabajo, y la pantalla completa, la física.
- **Pendiente** (no se presenta como implementado):
  - varios monitores (RandR o Xinerama): una sola pantalla, fija desde el
    arranque;
  - áreas no rectangulares;
  - reservas distintas por escritorio;
  - márgenes y bordes del mosaico;
  - `_NET_WM_STATE_ABOVE`/`BELOW` y paneles que se ocultan solos;
  - Crimson Panel, IPC y configuración.
- **Reservas extremas legítimas pero absurdas** (un panel que pide toda la
  pantalla) dejan un área de un píxel. Se registran, pero no se corrigen.
- **Propiedad obsoleta tras una terminación abrupta:** si el WM muere con
  `SIGKILL`, `_NET_WORKAREA` queda en la raíz con las demás propiedades,
  como desde F6 (ADR 0008).

## Verificación

### Pruebas unitarias (139 en total, 13 más que en F9)

**U22:**
- **`ewmh`:**
  - `PARTIAL` válida y orden de los 12 valores;
  - prioridad y los tres estados;
  - propiedades mal formadas (corta, larga, `bytes_after`, tipo, formato,
    tramo invertido);
  - reserva efectiva de tramos parciales, fuera de la raíz, que se salen y
    con grosores excesivos;
  - lista de `_NET_SUPPORTED`.
- **`layout`:**
  - los cuatro bordes;
  - máximo frente a suma;
  - reservas extremas con un píxel y orden de limitación;
  - pantallas mínimas y desplazadas;
  - los 32 valores de `_NET_WORKAREA`.
- **`clients`:**
  - plan con área de trabajo y pantalla física;
  - marco de respaldo en el área;
  - `FixedKind`.
- **`workspaces`:**
  - varios paneles;
  - solo los paneles reservan;
  - prueba pseudoaleatoria con struts arbitrarios que comprueba siempre
    un área válida.

### Pruebas de aceptación (`tests/acceptance/f10/`)

| Caso | Qué comprueba |
|---|---|
| A104 | `DOCK` superior; registro; foco; `_NET_SUPPORTED` exacta |
| A105 | `DOCK` inferior |
| A106 | Izquierda con `STRUT` (sin `PARTIAL`) y derecha con `PARTIAL` |
| A107 | Máximo por borde, dos bordes, esquina y retirada del panel más grueso |
| A108 | Tramo parcial (toda la franja) y tramo fuera de la pantalla |
| A109 | `PARTIAL` gana a `STRUT`; `STRUT` sin `PARTIAL`; `PARTIAL` mal formada sin recurrir a `STRUT` |
| A110 | `u32::MAX`, bordes opuestos que no caben, `INTEGER`, formato 8 y tramo invertido; declarada y efectiva registradas |
| A111 | Aparición, movimiento, retirada, vuelta y destrucción |
| A112 | Cambios dinámicos de `PARTIAL` y `STRUT`, también su borrado |
| A113 | 32 valores; un escritorio oculto se recoloca al mostrarse |
| A114 | Flotante sin mover; diálogos centrados dentro del área |
| A115 | Pantalla completa sobre la pantalla física y por encima del panel; vuelta al área |
| A116 | Adopción, salida limpia sin `_NET_WORKAREA` y reinicio |
| A117 | Struts de `NOTIFICATION`, `DESKTOP` y una ventana normal ignorados |
| A118 | Panel 1 de la comparación con el binario de F9 |
| A119 | Paneles 2 y 3 con F10 y composición |
| A120 | Estado posterior, pánicos, residuos y procesos (como A9) |

- Cada comprobación de área contrasta la propiedad publicada **y** la
  geometría real de las ventanas (`xwininfo`), es decir, el efecto, no solo
  lo escrito.
- **`test-client`** gana `--strut L R T B` y `--strut-partial V1 … V12`. Los
  valores mal formados y los cambios dinámicos se escriben con `xprop`.
- **Binario de F9** (`CRIMSON_F9_BIN`, opcional): `git archive 2b2df85`
  extraído en un directorio temporal y compilado allí con
  `cargo build --locked --offline`, sin worktree, sin tocar `main` y sin
  red. Solo lo usa A118; sin él, A118 y la composición de A119 quedan
  OMITIDAS.
- **Cambios en los arneses anteriores:**
  - `_NET_SUPPORTED` esperada en F6 (A53) y F9 (A100), con los tres átomos
    nuevos;
  - `ROOT_PROPS` de F6 incluye `_NET_WORKAREA`, así que A53 comprueba que
    se publica y que se borra al salir.

## Resultados de las pruebas de aceptación de F10

Ejecuciones del 2026-10-14 en Xvfb con el binario de depuración de F10
(`--bins --examples` compilados antes de cada batería):

- **F10:** 140 OK, 0 FALLA y 1 OMITIDA (`A120-xprop-:0`, la consulta de la
  raíz de `:0`, que se omite deliberadamente, como A9). A118 se ejecutó con
  el binario de F9 (`CRIMSON_F9_BIN`).
- **Regresiones**, todas con 0 FALLA y 1 OMITIDA por el mismo motivo:
  F1, 46 OK; F2, 47 OK; F3, 61 OK; F4, 79 OK; F5, 142 OK; F6, 122 OK (con
  `_NET_WORKAREA` en `ROOT_PROPS`); F7, 123 OK; F8, 106 OK; F9, 166 OK.
- **Orden de verificación:** la batería de F10 pasó primero sin los átomos
  nuevos en `_NET_SUPPORTED` (139 OK, 1 OMITIDA). Después se añadieron, con
  su comprobación (A104), y se repitieron F1–F10 completas.
- **Escena de la comparación:** la primera versión usaba un panel de
  1280 × 40, y los paneles 1 y 2 salían casi iguales, porque la diferencia
  quedaba debajo del panel. Se sustituyó por un panel de 800 × 40 con tramo
  0–799, que muestra la franja reservada entera, y se repitió solo la
  batería de F10 (140 OK, 1 OMITIDA en las dos ejecuciones posteriores). El
  código no cambió entre esas ejecuciones.

Son los resultados de esas ejecuciones concretas, no una garantía para
cambios posteriores.

## Documentación visual

### Captura real (A118 y A119)

- **Escena:** A (rojo, `8B0A1F`) y B (negro, `1F2328`) en mosaico; un
  `DOCK` K (blanco, `F6F8FA`) de 800 × 40 arriba, con
  `_NET_WM_STRUT_PARTIAL` de 40 px y tramo 0–799.
- **Panel 1 (F9):** A 0 0 640 720 y B 640 0 640 720; sin `_NET_WORKAREA`.
  El panel tapa la parte superior de A, y B llega hasta arriba a la derecha
  del panel. Puntos medidos: (100, 20) blanco, (1000, 20) negro de B,
  (100, 300) rojo.
- **Panel 2 (F10):** `_NET_WORKAREA` 0 40 1280 680; A 0 40 640 680 y B
  640 40 640 680. A la derecha del panel queda el fondo de la raíz de Xvfb
  (`000000`), porque se reserva la franja entera (área rectangular).
- **Panel 3 (F10):** el panel pasa a 0 680 con una reserva inferior de
  40 px y tramo 0–799; `_NET_WORKAREA` 0 0 1280 680, y A y B empiezan en
  y = 0. Puntos medidos: (100, 20) rojo, (100, 700) blanco, (1000, 700)
  `000000`, (900, 675) negro.
- **Comprobaciones:** tres capturas de 1280 × 720; colores en puntos
  conocidos; cada panel, idéntico a su captura (0 píxeles distintos).
  Composición de 4012 × 1024. SHA-256 de `crimson-wm-f10-comparison.png`:
  `10132d452f687c67f3205f80a7f16d63976b50e8e6cea88f2e36c72dc8cef1e0`.

### Diagrama

`crimson-wm-f10-workarea.svg`, renderizado con
`tools/render-svg.py … --scale 2` a 2800 × 3080.
