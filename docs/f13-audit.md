# Auditoría F13-C: motor de layouts extensible

Fecha: 2026-10-15. Base F12:
`d7a4c40252e93d96fc5f5c352da89bda0403374c`.
Rama: `feat/f13-layout-engine`. Alcance: Master-stack, Columns y selección
por escritorio del [ADR 0015](adr/0015-motor-de-layouts.md).

## Estado inicial y método

Repositorio, rama y HEAD coincidían con lo autorizado; `main` y la
referencia local `origin/main` seguían en la base. Índice vacío, doce
archivos modificados y ocho nuevos, todos de F13. Se conservaron hashes
iniciales de esos archivos y de los trece commits anteriores en un
directorio temporal de la auditoría. No se restauró ni descartó trabajo.

Se revisaron el diff completo, consumidores del motor, estado de clientes,
salida, foco, configuración y los observadores de A150–A165. Los resultados
de F13-B se trataron como antecedentes; se repitieron las validaciones.
No se encontró un defecto funcional confirmado del WM ni un problema
estructural que requiriera rediseño. Las correcciones afectan al arnés y
a su evidencia; el cálculo de producción no cambió en F13-C.

## Geometría y compatibilidad

- `master_stack`, `split_columns`, `split` y `to_u16` coinciden literalmente
  con F12. El despacho nuevo normaliza dimensiones igual que la función
  histórica. No se modifica su redondeo ni su saturación.
- Columns conserva el orden y devuelve cero resultados para N=0. Para
  N>0 reparte el ancho con diferencia máxima de un píxel, dando el resto
  a las primeras ventanas. Con N>W, los sobrantes ocupan el último píxel.
- El nuevo test `u25_oraculo_por_reparto_de_pixeles` calcula las anchuras
  mediante turnos de un píxel, sin cocientes ni fórmula de offsets de
  producción. Contrasta 2.160 combinaciones: doce anchuras, diez cantidades,
  seis orígenes y tres alturas, incluidos cero y límites i16/u16.
- Los otros U25 comprueban los cinco oráculos exactos aprobados, workarea,
  contigüidad y cobertura donde son representables, determinismo y 10.000
  clientes. La prueba nueva complementa esas propiedades; no certifica
  todos los posibles valores de entrada.
- `split` solo recibe total y número de tramos positivos. Si el cociente
  es positivo, N≤W≤65535 y los productos de desplazamiento no superan W;
  si es cero, se acota el índice al último píxel. La multiplicación del
  porcentaje usa u32. Los desplazamientos de coordenadas saturan como en
  F12: en extremos pueden quedar huecos y solapes, pero no desbordamientos.

`Clients::plan` sigue filtrando Tiled sin fullscreen y añade la fullscreen
con la pantalla física. Flotantes, diálogos flotantes y fijas no entran en
el cálculo. Struts/workarea y la aplicación de geometrías conservan sus
contratos. ConfigureNotify usa geometrías reales, también para ocultas.

Los ocho escritorios inicializan su selección desde Config. Solo
`cycle_layout` cambia la del activo. Movimiento, vaciado, navegación y
escritorio previo no copian ni reinician layouts. La acción no escribe en
la lista ni el historial de clientes, no enfoca, no lanza procesos y no
reevalúa reglas. Usa el filtro de autorepetición existente. Zoom conserva
su reordenamiento lógico. `place(desk)` lee el layout de ese índice antes
de obtener sus clientes; la salida restaura también fullscreen ocultas
con su propio layout, y remapea las ventanas ocultas.

La tabla de treinta atajos predeterminados coincide literalmente con F12.
`cycle_layout` usa el parser, captura y política obligatoria de controles
existentes, incluidos alias, Lock, Num Lock, colisiones y código 14.
La extensión TOML solo acepta `master-stack` y `columns`; omitir `default`
conserva F12. `master_percent` sigue validándose globalmente, también en
Columns. CLI → DISPLAY/token → configuración completa → conexión no
cambia. U27, A160/A164 y la regresión F12 verifican errores 15 y
`--check-config` sin servidor. El ejemplo sigue equivaliendo a los defaults.

## Defectos de pruebas confirmados y correcciones

| Defecto de F13-B | Corrección y regresión |
|---|---|
| A150-30 solo verificaba código 0; también habría aprobado un informe con 31 atajos | Comprueba código y contenido real `30 atajos`. A150-space-libre exige un KeyPress de Mod4+space recibido por xev: no basta con que la geometría no cambie |
| A161-oculto solo consultaba geometría; no observaba la notificación que debía comunicarla. Las búsquedas de eventos leían el registro entero | Las tres observaciones de ConfigureNotify exigen eventos posteriores al estímulo. A161-oculto-notify exige la geometría real en un nuevo evento sintético |
| F13 no registraba la salida de cada xvfb-run en runs.txt. El atribuidor común veía 17 Xvfb de 0 ejecuciones y no podía acreditar que su lista estuviera completa | Registra cada envoltorio; A165-atribucion exige OWN_COMPLETE=1. Resultado: 17 servidores propios de 17 ejecuciones, manteniendo las guardas y el rechazo de residuos |

También se explicitan observaciones de salida en A158: borde flotante
restaurado, ventana oculta remapeada y propiedad fullscreen vacía. Se
añadieron subidentificadores dentro de A150–A165, sin reutilizar casos
históricos.

Un ensayo local sin X demuestra las diferencias: el registro de F13-B da
OWN_COMPLETE=0 y el registro completo da 1; un resumen con 31 atajos pasa
el criterio anterior y falla el nuevo; un ConfigureNotify antiguo no
satisface el observador corregido y un evento nuevo sí. La aceptación
completa confirma después las propiedades sobre el servidor real.

La revisión del índice detectó además una línea en blanco adicional al
final de helpers.sh, invisible al diff de archivos rastreados antes de
prepararlo. Se retiró y se repitió diff --cached --check sobre el índice
completo; no cambia el comportamiento del arnés.

## Incidencia de infraestructura Xvfb

Evidencia F13-B: `/tmp/crimson-f10.GUqmfW`. A115 pasó las guardas de
DISPLAY, servidor hijo, cookie y ausencia de TCP. Falló después, en
`holding`, dentro de `f1/xvfb-env.sh`, antes de despachar el caso o arrancar
el WM: no existe A115.log y ambos scripts registraron salida 1. Resultado
132 OK, 9 FALLA y 1 OMITIDA: una precondición fallida más ocho resultados
ausentes. La repetición completa dio 140 OK, 0 FALLA y 1 OMITIDA.

El cliente persistente es `xprop -root -spy`. `holding` exige que su PID
exista, escribe `_CRIMSON_CONTROL` y vuelve a leerlo. Un PID vivo no prueba
por sí solo una conexión X establecida; además, la salida de ese cliente y
los errores de cada intento se descartan en el arnés histórico. El registro
disponible no permite distinguir su salida prematura, pérdida de conexión,
reinicio o error de una lectura. No demuestra una carrera concreta.

Un experimento F13-C en otro Xvfb autenticado, usando las guardas comunes
intactas y sin arrancar ningún WM, confirmó el mecanismo documentado en el
ADR 0003: con cliente persistente el control permanece; tras desconectar
el último cliente desaparece y tampoco persiste una escritura aislada;
con un nuevo cliente persistente vuelve a conservarse. Esto confirma la
dependencia del cliente, no la causa del fallo intermitente de A115.

La incidencia F3/A24 asociada por el usuario al issue #1 comparte helper,
precondición y síntoma, según el README técnico. **No se ha confirmado una
causa común.** No se consultó GitHub ni se modificó el issue; la incidencia
permanece abierta en esta documentación. La nueva ejecución completa de
F10 pasó al primer intento. No se añadieron reintentos de batería ni se
debilitaron guardas para convertir errores de infraestructura en éxitos.

## Validación F13-C

Debian 13, Rust 1.85.1 del sistema, debug. Cargo siempre locked/offline,
sin instalaciones. CARGO_HOME temporal enlazado a la caché local evita
proxies rustup. Las unitarias con socket y los arneses se ejecutaron fuera
del sandbox de la herramienta, conservando su aislamiento propio.

| Batería | OK | FALLA | OMITIDA |
|---|---:|---:|---:|
| Unitarias U1–U27 y regresiones | 214 | 0 | 0 |
| F13 A150–A165 | 166 | 0 | 1 |
| F1 | 46 | 0 | 1 |
| F2 | 47 | 0 | 1 |
| F3 | 61 | 0 | 1 |
| F4 | 79 | 0 | 1 |
| F5 | 142 | 0 | 1 |
| F6 | 122 | 0 | 1 |
| F7 | 123 | 0 | 1 |
| F8 | 106 | 0 | 1 |
| F9 | 166 | 0 | 1 |
| F10 | 140 | 0 | 1 |
| F11 | 132 | 0 | 1 |
| F12 | 187 | 0 | 1 |

Cada omisión de aceptación es exclusivamente la consulta de `:0`, que no
se ejecuta. Las comprobaciones restantes de sesión y recursos sí se hacen.
Se usaron Xvfb propios autenticados, sin TCP, HOME/XDG temporales, clientes
auxiliares de prueba y programas falsos para todos los lanzamientos del WM.
F9–F11 incluyeron los tres binarios históricos locales de F13-B.

También pasaron formato, Clippy all-targets con warnings como errores,
bash -n y ShellCheck en los 36 scripts, diff --check, estructura Markdown
y enlaces locales, XML SVG y PNG. Los enlaces externos quedaron OMITIDOS
por la restricción de red.

El SVG renderizado de nuevo coincide con su PNG (AE=0). La comparación
auténtica de A151 de F13-C coincide con la imagen incorporada en F13-B
(AE=0); cada panel coincide con su captura xwd original. Se revisaron
visualmente los PNG. No se alteraron imágenes históricas.

Se revisaron permisos, archivos no rastreados y patrones conocidos de
credenciales sin hallazgos; no se incorporan registros, cookies, xwd,
archivos temporales ni compilaciones. Cargo.toml/Cargo.lock y las
dependencias no cambian. Los arneses F1–F12 y CLAUDE.md permanecen intactos.

Registros locales: `/tmp/crimson-f13c.mt89m7` (unitarias, regresiones,
ensayo del arnés y ciclo de vida) y `/tmp/crimson-f13.yLuVNq` (F13 final).
Los archivos temporales no forman parte del commit.

## Límites restantes

El fallo intermitente de preparación de Xvfb no tiene causa confirmada.
La cobertura de propiedades es amplia pero finita. Columns puede dar
ventanas estrechas y, en áreas diminutas, solapadas conforme a la política
aprobada; no añade WM_NORMAL_HINTS. Persisten los límites documentados de
F12 sobre archivos, mapas de teclado y auditoría de dependencias.
No se añade persistencia, recarga, nuevos algoritmos ni otras funciones.

F13-C autoriza un único commit local después de revisar el índice. La
integración y publicación requieren autorización F13-D; no se realizó
ninguna operación remota ni se modificaron los trece commits anteriores.
