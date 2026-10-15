# ADR 0015: Motor de layouts extensible (F13)

- **Estado:** implementado en F13-B y auditado en F13-C para commit local;
  pendiente de integración y publicación. Diseño aprobado después de F13-A.
- **Fecha:** 2026-10-15.
- **Base:** `d7a4c40252e93d96fc5f5c352da89bda0403374c` (F12).
- **Rama:** `feat/f13-layout-engine`.

## Contexto y alcance

F12 tenía un único algoritmo: Master-stack con porcentaje global validado
al arrancar. `layout.rs` ya calculaba sin X11; `Clients::plan` seleccionaba
las ventanas y `Wm::place` aplicaba sus geometrías. F13 conserva esa
separación y añade exclusivamente Columns y selección por escritorio.

Monocle, Grid, Rows, inversión y cambios de eje quedan aplazados. Monocle
necesita además una política de visibilidad: el foco actual no eleva una
ventana normal de mosaico. No se implementan decoración, bordes, gaps,
ratón, multimonitor, recarga ni persistencia entre reinicios.

## Motor puro

- `LayoutKind`: `MasterStack` (predeterminado) y `Columns`; nombres
  estables `master-stack` y `columns`, y ciclo entre ambos.
- `LayoutArea`: rectángulo con dimensiones normalizadas a un mínimo de 1,
  conservando las coordenadas de F12.
- `LayoutParams`: porcentaje de maestra validado, de 10 a 90. Es global,
  inmutable durante la ejecución y se conserva en Columns.
- `layout::rectangles(kind, area, count, params)`: exactamente un `Rect`
  por posición elegible, en orden estable; no recibe identificadores de
  ventanas ni conexiones, no lee archivos ni realiza efectos secundarios.
- El despacho de Master-stack llama al cálculo histórico sin cambiarlo,
  incluidos sus casos extremos y redondeos.
- Columns divide el ancho W en N tramos. Para N > 0: `q = W / N`,
  `r = W % N`; el tramo i empieza en `i*q + min(i,r)` y mide
  `q + (i < r)`. Se reutiliza `split`, ya empleado por la pila histórica.

Se elige enum y funciones puras frente a traits: el conjunto de algoritmos
es pequeño, compilado y exhaustivo. No se añaden dependencias, features ni
cambios a `Cargo.lock`. La complejidad O(N) corresponde al cálculo y su
salida; el resto del núcleo conserva sus búsquedas existentes.

## Invariantes geométricas

- Cero elegibles: cero rectángulos del algoritmo; el plan aún puede añadir
  fullscreen.
- Una ventana: toda el área de trabajo.
- N ventanas: N rectángulos, tamaños positivos, sin bordes ni márgenes.
- Reparto entero determinista, resto para los primeros tramos.
- Si N > W en Columns, cada tramo mide 1: los sobrantes se sitúan en el
  último píxel. No es posible exigir separación con menos píxeles que
  ventanas. Ejemplo W=4, N=5: posiciones 0, 1, 2, 3, 3.
- Las coordenadas usan i16 y los tamaños u16, como X11; desplazamientos con
  saturación conservan la política F12. Los cálculos intermedios evitan
  overflow y underflow. No se llama a `split` con cero tramos.
- Contención en el área normalizada. Cobertura completa y ausencia de
  solapes se comprueban cuando los tramos y sus posiciones son
  representables. La saturación extrema puede generar huecos y solapes.
- Los struts siguen calculándose antes del layout, mediante workarea F10.

Oráculos U25, en formato `(x,y,ancho,alto)`:

| Entrada | Rectángulos esperados |
|---|---|
| Master-stack 1280×720, N=3, 50 % | (0,0,640,720); (640,0,640,360); (640,360,640,360) |
| Columns 1280×720, N=3 | (0,0,427,720); (427,0,427,720); (854,0,426,720) |
| Columns 1279×719, N=2 | (0,0,640,719); (640,0,639,719) |
| Columns área (0,40,1280,680), N=2 | (0,40,640,680); (640,40,640,680) |
| Columns área (-10,20,100,50), N=3 | (-10,20,34,50); (24,20,33,50); (57,20,33,50) |

## Escritorios, foco e integración

`Workspaces` mantiene ocho `Desktop`, cada uno con `Clients` y su
`LayoutKind`. Sus accesores de clientes conservan sus interfaces. Todos
empiezan con `[layout] default`; después la selección solo cambia mediante
`cycle_layout` en el activo. Cambiar de escritorio, volver al anterior,
vaciarlo o mover ventanas no reinicia ni transporta la selección.

`Clients::plan(work, screen, kind, params)` conserva el filtro de F12:
`Kind::Tiled` sin fullscreen. Asocia las posiciones a las ventanas en su
orden y añade fullscreen con `screen`. Flotantes y fijas no participan.
`Wm::place(desk)` consulta el layout del índice recibido, incluso oculto:
es necesario también en la restauración fullscreen de la salida normal.

La acción no cambia foco, historial, orden, escritorios de los clientes,
reglas ni fullscreen. `arrange` conserva las capas y familias existentes.
Un diálogo no se recentra y una flotante no cambia de marco por alternar
el layout. `zoom` mantiene su operación sobre el orden: en Columns coloca
la ventana enfocada primero, sin cambiarle el foco.

Fullscreen conserva su estado al moverse; la última gana en el destino.
Al salir, el mosaico se calcula con el layout vigente. Una flotante
restaura su marco real. Los escritorios ocultos mantienen sus geometrías
reales hasta que se colocan: ConfigureNotify no anuncia un plan pendiente.
No cambian los modelos de entrada, el tratamiento de errores X ni EWMH.

## TOML y teclado

```toml
[layout]
default = "master-stack" # también "columns"
master_percent = 50
```

Omitir `default` es equivalente a F12. El porcentaje no se renombra ni se
elimina y se valida también en Columns. Valores o tipos inválidos son
errores 15 antes de conectar con X. Se conservan claves estrictas,
precedencia de rutas, seguridad de archivos y `CRIMSON_RULES` de F12.
El informe histórico de Master-stack permanece idéntico; Columns añade
`layout columns` al resumen, también en `--check-config`.

```toml
[[keybindings]]
keys = "Mod4+space"
action = "cycle_layout"
```

Este fragmento es optativo y debe formar parte de una tabla completa con
salida: los atajos explícitos siguen sustituyendo la tabla entera. No se
añade ningún atajo predeterminado: siguen siendo 30. `cycle_layout` no
acepta parámetros y conserva la política de atajos de control obligatorios
(14 si no se capturan), alias, Lock, Num Lock y colisiones. Ignora la
autorepetición del servidor, sin imponer el intervalo de los lanzamientos.

## Pruebas y trazabilidad

- **U25:** oráculos exactos, cero/uno, área cero normalizada, dimensiones
  diminutas, coordenadas negativas y saturadas, 10 000 clientes, cobertura
  y determinismo, equivalencia del despacho con Master-stack histórico,
  parámetros válidos e inválidos y Columns independiente del porcentaje.
- **U26:** selección inicial de los ocho escritorios, independencia,
  persistencia vacíos, escritorio previo, orden, foco, movimiento,
  fullscreen y restauración con layout vigente.
- **U27:** valores anteriores equivalentes, formas TOML, nombres y tipos
  inválidos, porcentaje validado aunque no se use, acción sin parámetros y
  ausencia de atajos predeterminados nuevos.

| Caso Xvfb | Evidencia principal |
|---|---|
| A150 | Configuración ausente y ejemplo histórico F12; atajo nuevo sin capturar por defecto |
| A151 | Columns exacto para 1–4 clientes y comparación de capturas auténticas |
| A152 | Cambio de layout, foco, navegación, zoom y autorepetición |
| A153 | Persistencia por escritorio, anterior y escritorio vaciado |
| A154 | Movimiento teclado/EWMH y fullscreen que desplaza otra fullscreen |
| A155 | Struts dinámicos, workarea y escritorio oculto |
| A156 | Flotante con borde, foco y capas DESKTOP/DOCK/NOTIFICATION |
| A157 | Centrado inicial de diálogo, familia y retorno del foco |
| A158 | Fullscreen, cambio de layout, marco flotante y layout del escritorio oculto al salir |
| A159 | Reglas, nofocus, ausencia de reevaluación y precedencia CRIMSON_RULES |
| A160 | Check sin X, configuración inválida antes de conectar y servidor intacto |
| A161 | ConfigureNotify real/sintético y geometría real de ventana oculta |
| A162 | Adopción, retirada, destrucción y pérdida del Xvfb propio (13) |
| A163 | Xvfb impar 1279×719, ultrapanorámico 3840×1080 y diminuto 4×2 |
| A164 | Guardas, token, marca, validación sin conexión y namespace sin servidores visibles |
| A165 | Sesión observada solo por procesos, pánicos, recursos propios y resultados únicos |

Cada caso gráfico utiliza las guardas y entorno de F12, extraídos a
`f13/helpers.sh`: Xvfb propio autenticado, sin TCP ni `-ac`, configuración
temporal y solo programas falsos. No se consulta `:0`. El rechazo textual
de DISPLAY 0 se prueba en un namespace con sockets ocultos y red aislada.
El arnés comprueba códigos del script y de xvfb-run, resultados esperados
y ausencia de duplicados. En A162 se acepta xvfb-run 0 o 1 solo si el script
termina con 0 y el WM con 13: ese caso termina deliberadamente su servidor.

### Resultados de F13-B (2026-10-15)

Debian 13, Rust 1.85.1 del sistema, binario de depuración. Sin red,
instalaciones, sudo ni acceso a la sesión gráfica real. Las pruebas Xvfb y
la prueba unitaria con socket Unix se ejecutaron fuera del sandbox de la
herramienta, porque este impide observar los procesos del servidor y crear
ese socket. Se conservaron las guardas propias de los arneses.

| Batería | OK | FALLA | OMITIDA |
|---|---:|---:|---:|
| Unitarias, U1–U27 y regresiones F12-C | 213 | 0 | 0 |
| F13, A150–A165 | 160 | 0 | 1 |
| Regresión F1 | 46 | 0 | 1 |
| Regresión F2 | 47 | 0 | 1 |
| Regresión F3 | 61 | 0 | 1 |
| Regresión F4 | 79 | 0 | 1 |
| Regresión F5 | 142 | 0 | 1 |
| Regresión F6 | 122 | 0 | 1 |
| Regresión F7 | 123 | 0 | 1 |
| Regresión F8 | 106 | 0 | 1 |
| Regresión F9 | 166 | 0 | 1 |
| Regresión F10 (repetición completa) | 140 | 0 | 1 |
| Regresión F11 | 132 | 0 | 1 |
| Regresión F12 | 187 | 0 | 1 |

La única omisión de cada arnés es la consulta de `:0`: sus comprobaciones
de sesión quedan parciales por diseño. Las comparaciones históricas de
F9–F11 sí se ejecutaron: binarios locales de F8 `195938d`, F9 `2b2df85` y
F10 `f90dfa2`, extraídos con `git archive` y compilados offline en `/tmp`,
sin cambiar referencias ni ramas.

Incidencias conservadas para revisión:

- Las dos ejecuciones preliminares de F13 detectaron defectos del arnés
  nuevo: dato de átomo fullscreen, formato del observador ConfigureNotify,
  saltos de línea en TSV y código del envoltorio en la pérdida deliberada
  del servidor. Se corrigieron antes de la ejecución final de 160 OK.
- La primera regresión F10 dio 132 OK, 9 FALLA y 1 OMITIDA: A115 abortó en
  la preparación común, porque `_CRIMSON_CONTROL` no persistió; las ocho
  comprobaciones posteriores quedaron ausentes. Crimson WM todavía no
  había arrancado en ese caso. La batería completa se repitió sin cambiar
  código ni guardas y dio 140 OK, 0 FALLA y 1 OMITIDA. No se ha determinado
  la causa del fallo transitorio de preparación de Xvfb.

También pasaron `cargo fmt -- --check`,
`cargo clippy --locked --offline --all-targets -- -D warnings`,
`cargo test --locked --offline` (con `--nocapture`), `bash -n` y
`shellcheck -x` en los 36 scripts de aceptación, y `git diff --check`.
Se usó `PATH=/usr/bin:/bin` y un CARGO_HOME temporal con enlace a la caché
local de paquetes para evitar los proxies rustup instalados; no se
descargaron paquetes ni se cambió la configuración de la toolchain.

Se comprobaron los 104 enlaces locales en 20 documentos, el XML y la
ausencia de recursos externos del SVG, las dimensiones y CRC de los PNG y
la fidelidad AE=0 de ambos paneles de A151. Los PNG se inspeccionaron
visualmente. No se verificaron enlaces externos, por la restricción de red.
La revisión de archivos no detectó credenciales conocidas, archivos
temporales ni cambios en dependencias o imágenes históricas.

Registros y configuraciones temporales fuera del repositorio:
`/tmp/crimson-f13b.nJenWS` (unitarias, validaciones y regresiones),
`/tmp/crimson-f13.RNOH4Y` (F13 final),
`/tmp/crimson-f10.GUqmfW` (F10 inicial) y
`/tmp/crimson-f10.xLlZ0t` (F10 repetida).

### Auditoría F13-C

La [auditoría posterior](../f13-audit.md) añade un oráculo independiente
por turnos de píxeles y corrige observaciones incompletas de A150, A161 y
la atribución de recursos de A165. Explicita además la restauración de
bordes, visibilidad y estado fullscreen al salir en A158. El cálculo de
producción no necesitó correcciones. Resultado: 214 unitarias aprobadas,
166 OK / 0 FALLA / 1 OMITIDA en F13 y regresiones F1–F12 aprobadas. La
omisión sigue siendo exclusivamente la consulta prohibida de `:0`.

La dependencia de un cliente persistente se verificó en un Xvfb propio.
La causa del fallo intermitente de preparación de F10 sigue sin confirmarse;
compartir la precondición de F3 no demuestra una causa común con el issue #1.

## Documentación visual

El [diagrama técnico](../images/crimson-wm-f13-layouts.svg) y su
[PNG renderizado](../images/crimson-wm-f13-layouts.png) son esquemas
conceptuales del código, no capturas. La [comparación](../images/crimson-wm-f13-comparison.png)
procede de A151: mismo binario, mismas tres ventanas de colores, Master-stack
al 50 % a la izquierda y Columns a la derecha. Solo se añaden rótulos; los
paneles se contrastan píxel a píxel con sus volcados originales de `xwd`.
No se modifican imágenes históricas.

## Límites y trabajo aplazado

No hay persistencia, recarga, opciones por algoritmo no implementadas ni
configuración inicial distinta por escritorio. La selección en ejecución
sí es independiente. Se mantienen los límites de F12 sobre propietario y
directorios de configuración, mapa de teclado leído al arrancar y auditoría
incompleta de dependencias. Columns puede producir ventanas estrechas;
no se incorpora soporte nuevo de WM_NORMAL_HINTS. Ningún resultado de
pruebas garantiza entradas o cambios posteriores.
