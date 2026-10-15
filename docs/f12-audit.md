# Auditoría independiente F12-C

Fecha: 2026-10-15. Base: `19bc9692714beccb4e87dd3c877dd37b0e19b522`.
Rama: `feat/f12-toml-config`. Alcance: trabajo pendiente de F12-B y
correcciones mínimas, sin red, instalación de paquetes ni cambios de fase.

## Hallazgos y correcciones

| Defecto demostrado | Corrección | Regresión |
|---|---|---|
| `[a.b.c]` o `[[a.b.c]]`, seguido de `[a]` y `b.d = 2`, era TOML válido rechazado como sintaxis inválida | Permitir ampliar una tabla implícita con puntos y marcarla como definida por puntos; siguen prohibidas su redefinición y la ampliación de tablas explícitas, inline y arrays estáticos | `claves_con_punto_amplian_tablas_implicitas`; `tabla_implicita_valida_se_rechaza_por_el_esquema` verifica además clasificación y posición del error del esquema |
| Sustituir un archivo regular por FIFO entre `metadata` y `open` bloqueaba el arranque | Apertura con `O_NONBLOCK`, `O_NOCTTY` y `O_CLOEXEC`; se conservan las comprobaciones de tipo, dispositivo/inodo, permisos, tamaño y UTF-8 | `sustitucion_entre_metadata_y_open` sustituye exactamente en ese punto por otro archivo y por una FIFO sin escritor |
| Los errores de teclas y modificadores desconocidos podían contener controles de terminal literales | Escapar los controles al presentar nombres, conservando el límite de 32 caracteres | `errores_escapan_caracteres_de_control`, con ESC, NUL y DEL |
| Con Num Lock en Mod1, una salida `Alt+Shift+q` se capturaba pero nunca se reconocía: el estado pierde Mod1 al ignorar Num Lock | Rechazar el atajo obligatorio con código 14; desactivar el opcional con aviso antes de capturarlo | `A137-numlock`, con el mapa real de un Xvfb aislado |

Las tres primeras regresiones de implementación fallaron antes de la
corrección; la de FIFO agotó el límite externo de tres segundos. A137
confirmó que la configuración de salida incompatible arrancaba antes de
la corrección. No se sustituyó el parser ni se rediseñaron componentes.

`rustix` 1.1.5 se declara también como dependencia directa para usar su API
segura de apertura. Ya estaba fijada y compilada por x11rb con las mismas
features. No cambian paquetes, versiones, licencias ni MSRV por esa
declaración. Se verificaron localmente los checksums de `toml_parser`,
`winnow` y `rustix` contra los archivos `.crate` de la caché; el parser
sigue sin las features `unsafe`, `simd` y `debug`.

## Arquitectura y compatibilidad

La secuencia sigue siendo CLI → validación de DISPLAY → token → resolución,
lectura, documento TOML y esquema completos → `CRIMSON_RULES` → conexión
Unix autenticada → marca de sandbox → rechazo de otro WM →
`SubstructureRedirect` → núcleo. `--check-config` sale por una rama sin
conexión X ni lanzamiento de procesos. Una configuración inválida no se
aplica parcialmente; los errores de lectura y esquema son código 15 antes
de conectar. Los conflictos dependientes del mapa X solo se pueden
detectar al capturar atajos y dan 14.

Se cotejaron los 30 atajos y seis programas predeterminados con F11, tanto
en el código histórico como en las pruebas de equivalencia. Las reglas
conservan el evaluador y las validaciones de F11: `CRIMSON_RULES` definida
sustituye el archivo, e inválida deja cero reglas. Se mantienen la
supervisión de procesos, el argv literal sin shell, el entorno saneado,
los ocho escritorios, las políticas ICCCM/EWMH, diálogos, fullscreen,
capas y struts. El porcentaje predeterminado permanece en 50.

La normalización de alias y modificadores detecta duplicados antes de X.
Las colisiones de keycode se detectan al capturar: gana el primer atajo;
un segundo atajo obligatorio detiene el arranque. Caps Lock y Num Lock
siguen ignorándose; el mapa habitual con Num Lock en Mod2 no cambia.

Se compararon 12.744 documentos de tablas, claves con punto y arrays de
tablas con `tomllib` de Python, sin discrepancias después de la corrección
(dos en los 2.744 casos iniciales). Se distinguió aceptación sintáctica de
rechazo por esquema. Esta comparación cubre el subconjunto común de TOML
1.0/1.1 y no certifica toda la especificación. Las pruebas existentes
cubren también Unicode, escapes, cadenas multilínea, números, duplicados,
inline, posiciones, tipos y límites.

## Validación

Toolchain de Debian: rustc y Cargo 1.85.1. Rustfmt y Clippy se ejecutaron
con los binarios del sistema; un `CARGO_HOME` temporal con enlace a la
caché del registro evitó los proxies de rustup sin toolchain configurada,
sin leer credenciales ni instalar nada.

- `cargo fmt -- --check`: aprobado.
- `cargo clippy --locked --offline --all-targets -- -D warnings`: aprobado.
- `cargo test --locked --offline`: 204 aprobadas, 0 fallidas, 0 ignoradas.
  La salida detallada detectó que el sandbox impedía crear el socket Unix
  de una comprobación de archivos. La ejecución definitiva se repitió
  fuera del sandbox, sin DISPLAY ni XAUTHORITY, y sin ninguna comprobación
  interna omitida.
- `bash -n` y `shellcheck -x`, desde el directorio de cada script: 32
  scripts de aceptación, 64 comprobaciones aprobadas.
- `git diff --check`: aprobado.
- Enlaces locales de documentación, SVG y PNG: comprobados; el diagrama se
  regeneró con librsvg/cairo y se inspeccionó visualmente. La comparación
  PNG de F12-B coincide píxel a píxel con la regenerada en A147/A148 de
  esta auditoría (0 píxeles diferentes).

La primera ejecución de aceptación dentro del sandbox no pudo identificar
el Xvfb hijo y abortó sus casos gráficos con la guarda 90. Se repitió fuera
del sandbox con las guardas intactas. La ejecución previa a la corrección
de Num Lock dio 186 OK, 1 FALLA (la regresión nueva) y 1 OMITIDA.

Resultados definitivos: todas las baterías terminaron con código 0.

| Batería | Aprobadas | Fallidas | Omitidas |
|---|---:|---:|---:|
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

Cada omisión corresponde exclusivamente a la consulta prohibida de `:0`.
F9, F10 y F11 incluyen las comparaciones con los binarios históricos de
F8 (`195938d`), F9 (`2b2df85`) y F10 (`f90dfa2`), preparados mediante
`git archive` y compilados sin red en directorios temporales propios.

Los registros permanecen fuera del repositorio, en
`/tmp/crimson-f12c.EzRhMJ/` y en los directorios temporales que indica cada
batería. Solo se conectan clientes a los Xvfb creados y autenticados por el
arnés. Los valores de DISPLAY 0 usados como entradas de rechazo se prueban
con sockets ocultos y red aislada; nunca se consulta ni conecta a `:0`.

## Límites y riesgos restantes

- No se comprueba el propietario ni los permisos de los directorios
  padre. Los enlaces simbólicos se siguen. La configuración debe estar en
  directorios controlados por el usuario; no es una frontera contra otro
  escritor con acceso a esos directorios o al mismo inodo.
- `O_NONBLOCK` evita el bloqueo por FIFO y `O_NOCTTY` evita adquirir un tty,
  pero no garantizan ausencia de efectos al abrir cualquier dispositivo
  sustituido por un atacante. Tampoco garantizan un plazo de lectura en
  sistemas de archivos remotos ni una instantánea atómica del contenido.
- No se hizo una auditoría completa de los bloques `unsafe` de las
  dependencias ni una consulta de avisos de vulnerabilidades: se mantuvo
  la restricción sin red. El código propio sigue prohibiendo `unsafe`.
- El mapa de teclado se obtiene al arrancar; no se incorpora seguimiento
  de cambios de mapa. La validación sin X no puede predecir keycodes ni
  conflictos con capturas de otros clientes.
- No hay recarga en caliente. La configuración se valida una vez.

## Revisión del contenido

El índice estaba vacío al iniciar la auditoría. Se revisaron los archivos
rastreados modificados y los nuevos, modos ejecutables, lockfile,
licencias y fuentes locales de dependencias. Una búsqueda local de
patrones de claves privadas y tokens no encontró coincidencias; no es una
garantía absoluta de ausencia de secretos. No se incluyen registros,
binarios, cookies, configuraciones privadas ni resultados temporales.

La identidad autorizada para el único commit local es
`AngheloBR <182365321+AngheloBR@users.noreply.github.com>`, como autor y
committer, sin coautorías. No se modifican `CLAUDE.md`, `AGENTS.md`, ramas
históricas ni referencias remotas; tampoco se publica F12.

## Archivos previstos para el commit

Solo los 33 archivos revisados de F12, incluidos los heredados de F12-B:

```text
Cargo.lock
Cargo.toml
README.md
docs/README-technical-es.md
docs/adr/0003-salvaguardas-desarrollo.md
docs/adr/0006-lanzamiento-de-aplicaciones.md
docs/adr/0013-reglas-de-ventanas.md
docs/adr/0014-configuracion-toml.md
docs/configuration.md
docs/dependencies.md
docs/examples/wm.toml
docs/f12-audit.md
docs/images/crimson-wm-f12-comparison.png
docs/images/crimson-wm-f12-config.png
docs/images/crimson-wm-f12-config.svg
src/cli.rs
src/clients.rs
src/config.rs
src/config/document.rs
src/config/schema.rs
src/config/tests.rs
src/error.rs
src/keynames.rs
src/keys.rs
src/layout.rs
src/main.rs
src/rules.rs
src/spawn.rs
src/wm.rs
tests/acceptance/f1/lib.sh
tests/acceptance/f12/isolated.sh
tests/acceptance/f12/run.sh
tests/acceptance/f12/xvfb-case.sh
```
