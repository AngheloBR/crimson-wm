# ADR 0013: Reglas de ventanas (F11)

- **Estado:** aceptado. Implementado el 2026-10-14 (F11-B) en la rama
  `feat/f11-window-rules`. Ver «Resultados de las pruebas de aceptación de
  F11».
- **Fecha:** 2026-10-14

![Captura real en Xvfb: con F10, cuatro ventanas en mosaico; con F11 y dos reglas, Viewer queda flotante y Chat va al escritorio 2, donde aparece sola](../images/crimson-wm-f11-comparison.png)

*Capturas auténticas (A133 con el binario de F10, compilado a partir del
commit f90dfa2, y A134 con el de F11), con las mismas reglas en el entorno
de los dos WM: `class=Viewer; floating` y `class=Chat; desktop=2`. El de
F10 las ignora. Volcados de `xwd -root` de Xvfb aislados de 1280 × 720; la
composición solo añade rótulos, márgenes y bordes, y cada panel es idéntico
píxel a píxel a su captura.*

![Diagrama de F11: dónde se aplican las reglas, sintaxis provisional, evaluación determinista, prioridades protegidas, entradas inválidas y lo aplazado](../images/crimson-wm-f11-rules.png)

*Diagrama técnico elaborado a partir del código de F11; no es una captura.
Fuente editable: `docs/images/crimson-wm-f11-rules.svg`.*

## Contexto

Hasta F10, el comportamiento inicial de una ventana dependía solo de sus
propiedades ICCCM y EWMH. F11 añade reglas por aplicación (`WM_CLASS`),
tipo y transitoriedad, sin introducir todavía la configuración TOML
prevista. El WM no leía `WM_CLASS` de los clientes.

## Decisión

### Fuente provisional: `CRIMSON_RULES`

- Variable de entorno leída **una vez**, en `main.rs`, después de validar
  `DISPLAY` (capa 1) y el token del sandbox (capa 2), y **antes** de
  conectar con X. Los cambios posteriores no se leen.
- Como mucho 16384 bytes (`rules::MAX_SOURCE`) y 64 reglas
  (`rules::MAX_RULES`). El tamaño se comprueba antes que la codificación,
  que debe ser UTF-8, y el número de reglas (líneas que no están vacías ni
  son comentarios) antes que su sintaxis, así que el error indica el
  número real.
- **Una fuente inválida se descarta entera:** el WM registra «CRIMSON_RULES
  inválida (motivo): se continúa sin reglas» y arranca sin ninguna. Nunca se
  acepta una parte, y las salvaguardas no cambian (A132).
- **No pasa a los programas lanzados:** `spawn` la elimina del entorno del
  hijo, como el token (ADR 0006).
- **La sintaxis es provisional e inestable.** La sustituirá una
  configuración TOML. Cada clave se corresponde con un campo de una futura
  tabla `[[rule]]`, así que solo cambiará el analizador.

### Sintaxis

```text
# una regla por línea; cláusulas separadas por ';'
class=Firefox; desktop=2
instance=pavucontrol; floating; nofocus
type=dialog; transient=no; fullscreen
```

- **Formato general:**
  - las líneas vacías y las que empiezan por `#` se ignoran;
  - en cada cláusula se recortan los espacios de los extremos de la clave y
    del valor;
  - no hay escape: un valor no puede contener `;`.
- **Condiciones**, que se combinan con Y lógico:
  - `instance=…` y `class=…`: exactas y sensibles a mayúsculas, comparadas
    con los bytes de `WM_CLASS`;
  - `type=normal|dialog|utility|toolbar|splash|menu|dropdown_menu|popup_menu|tooltip|combo|dnd`:
    el **tipo efectivo**, es decir, el primero reconocido de
    `_NET_WM_WINDOW_TYPE` o, sin ninguno, `normal` (o `dialog` si la
    ventana es transitoria), como define EWMH. `dock`, `desktop` y
    `notification` son un error, porque las ventanas fijas no admiten
    reglas;
  - `transient=yes|no`: la ventana tiene (o no) `WM_TRANSIENT_FOR`, con
    cualquier valor.
- **Acciones:**
  - `desktop=1…8`;
  - `floating`, `nofocus` y `fullscreen`, sin valor.
- **Se rechaza toda la fuente si:**
  - hay una clave desconocida (las claves son en minúsculas);
  - una clave se repite en la misma regla; con eso no puede haber
    cláusulas contradictorias;
  - una cláusula está vacía;
  - falta un valor o sobra en una acción sin valor;
  - un tipo es desconocido o de ventana fija;
  - `transient` no es `yes` ni `no`;
  - el escritorio está fuera de 1–8;
  - una regla no tiene condiciones o no tiene acciones.

### Modelo (`src/rules.rs`, puro)

- `Matcher` (las condiciones), `Actions`, `Rule` (con su número de línea)
  y `Decision`, que guarda cada acción con la línea que la fija, las
  reglas que coinciden y los escritorios ignorados.
- `rules::load` y `rules::parse` analizan la fuente; `rules::evaluate`
  aplica las reglas a un `Subject` (`WM_CLASS`, tipo efectivo y
  transitoriedad).
- Sin E/S, sin expresiones regulares, sin comodines, sin órdenes externas
  y sin dependencias nuevas.

### Semántica

- **Coincidencia:** se cumplen todas las condiciones de la regla. Una
  condición sobre `WM_CLASS` nunca se cumple si la propiedad falta o es
  defectuosa.
- **Orden y conflictos:** se evalúan todas las reglas, en orden. **Cada
  acción la fija la primera regla que coincide y la define.** Un escritorio
  distinto de una regla posterior se ignora y se registra (A128); las
  acciones sin valor no pueden contradecirse.
- **Momento:** una sola vez, en el `MapRequest` de una ventana que no está
  gestionada, después de `clients::classify`.
  - Los cambios posteriores de `WM_CLASS`, del título o de
    `_NET_WM_WINDOW_TYPE` no la reevalúan, como en F9. Al retirarla y
    volver a mapearla se evalúa de nuevo (A131).
  - Las ventanas adoptadas al arrancar no usan reglas.

### Aplicación (`on_map_request`)

1. **`floating`:** si la clasificación es mosaico, pasa a flotante. Nunca
   al revés. Las demás capacidades de la clase no cambian: no se centra,
   y el foco y la pantalla completa siguen las de su tipo.
2. `manage` en el escritorio activo, como siempre.
3. **Escritorio:**
   - un diálogo con propietario efectivo va al escritorio de su principal
     (`place_with_owner`, ADR 0010) y el de la regla se ignora y se
     registra (A129);
   - en los demás casos, `place_on` deja la ventana oculta en el escritorio
     de la regla, en IconicState y con su `_NET_WM_DESKTOP`, sin cambiar el
     escritorio activo ni el foco. `place_on` es la generalización de
     `place_with_owner`, el mismo mecanismo de F8.
4. Centrado de diálogos, como en F8.
5. **`fullscreen`:** como un `_NET_WM_STATE_FULLSCREEN` inicial. Solo si el
   tipo la admite (`Class::fullscreen`, ADR 0011); si no, se rechaza y se
   registra (A127).
6. `arrange`, mapeo y foco. **`nofocus`** solo evita el foco inicial: la
   ventana sigue admitiéndolo y el usuario puede enfocarla después (A126).
   Una ventana oculta en otro escritorio no se mapea ni recibe el foco; al
   mostrar ese escritorio, recibe el foco como cualquier otra, salvo con
   `nofocus`.

### `WM_CLASS` (`icccm::wm_class`)

- Se lee junto con las demás propiedades de `window_info`, con
  `AnyPropertyType` y como mucho 512 bytes (`icccm::WM_CLASS_LEN`).
- **Válida:** `STRING`, formato 8, leída entera y con exactamente dos
  cadenas terminadas en NUL; se admite que falte el NUL final. Las cadenas
  vacías son válidas, pero nunca coinciden con una regla.
- **Bytes:** se conservan los originales (STRING es Latin-1) y no se
  convierten a UTF-8.
- **Defectuosa** (`Type`, `TooLong` o `Structure`): se registra y la
  ventana queda sin clase. Se gestiona igual, y las demás siguen con sus
  reglas (A124).

### Lo que una regla nunca cambia

- Override-redirect, `DOCK`, `DESKTOP` y `NOTIFICATION`: fuera de las
  reglas (A130).
- La familia de un diálogo (ADR 0010).
- La pantalla completa de los tipos que no la admiten (ADR 0011).
- El escritorio activo y el foco: ninguna regla roba el foco.
- Las capas de apilamiento (ADR 0011) y el área de trabajo (ADR 0012).
- Las salvaguardas: `:0`, el token y la marca se comprueban igual con
  reglas válidas o inválidas (A132).

## Seguridad

- **Lecturas acotadas:** fuente de 16 KiB y 64 reglas; `WM_CLASS` de
  512 bytes.
- **Analizador estricto y fuente atómica:** todo o nada.
- **Sin pánicos:** las propiedades defectuosas se registran y se ignoran.
- **Solo comparación de bytes:** sin expresiones regulares (sin riesgo de
  retroceso exponencial) ni ejecución de órdenes.
- **Límite de confianza:** las reglas solo cambian dónde y cómo aparece una
  ventana, nunca qué puede hacer un cliente. Un cliente puede fingir el
  `WM_CLASS` de otra aplicación y recibir sus reglas, porque X11 no
  autentica a los clientes. Las salvaguardas no cambian.

## Alternativas descartadas

- **Leer las reglas de un archivo** (`--rules ARCHIVO`): sería ya una
  configuración; queda para el TOML.
- **Una tabla compilada en el binario:** no se podría probar en Xvfb sin
  recompilar.
- **Aceptar las líneas válidas de una fuente con errores:** el resultado
  dependería de qué líneas fallan; se descarta en favor de una fuente
  atómica.
- **Que gane la última regla:** con la primera, añadir reglas generales al
  final no altera las específicas.
- **Comparar ignorando mayúsculas o con comodines:** ICCCM distingue
  mayúsculas; los comodines se aplazan.
- **Aplicar las reglas en la adopción:** reorganizaría los escritorios al
  reiniciar el WM.

## Consecuencias y limitaciones

- Sin `CRIMSON_RULES`, el comportamiento es el de F10 (A121 y regresiones
  F1–F10).
- **Pendiente** (no se presenta como implementado):
  - configuración TOML y recarga;
  - geometría inicial;
  - coincidencia por título;
  - expresiones regulares y comodines;
  - reevaluación dinámica;
  - cambiar al escritorio de destino;
  - forzar mosaico o foco;
  - sticky;
  - reglas en la adopción y en ventanas fijas;
  - valores con `;`.
- La variable de entorno no es cómoda fuera de los arneses: es una fuente
  provisional para probar el motor.

## Verificación

### Pruebas unitarias (145 en total, 6 más que en F10)

**U23:**
- `rules`:
  - reglas válidas, comentarios y todos los tipos;
  - 22 defectos de línea y el número de la línea que falla;
  - límites de tamaño y número (el número real de reglas, comprobado antes
    que la sintaxis), variable ausente y no UTF-8;
  - coincidencias exactas, mayúsculas, Y lógico, `WM_CLASS` ausente,
    bytes Latin-1 y cadenas vacías;
  - prioridad y conflictos;
  - decisión vacía sin reglas.
- `icccm`: `WM_CLASS` válida, sin NUL final, vacía, Latin-1, ausente, mal
  estructurada, de otro tipo o formato, demasiado larga, y de exactamente
  512 bytes leídos enteros, que es válida.
- `spawn`: `CRIMSON_RULES` no pasa a los hijos.

### Pruebas de aceptación (`tests/acceptance/f11/`)

| Caso | Qué comprueba |
|---|---|
| A121 | Sin `CRIMSON_RULES`: mosaico, foco y ningún registro de reglas |
| A122 | `class` exacta → escritorio 3: oculta, IconicState, `_NET_WM_DESKTOP` 2, sin cambiar el activo ni el foco; visible y enfocada al mostrar el 3 |
| A123 | Sin coincidencia por mayúsculas o por una condición del Y lógico; con las dos condiciones, sí |
| A124 | `WM_CLASS` de una cadena, de formato 32 o de 600 bytes: sin clase, en mosaico, WM en marcha; otra ventana sigue con su regla |
| A125 | `floating`: geometría pedida, encima del mosaico, en la navegación con `Mod4+j` |
| A126 | `nofocus`: sin foco inicial; `Mod4+j` la enfoca después |
| A127 | `fullscreen`: admitida en una normal; rechazada en un `SPLASH` |
| A128 | Conflicto: escritorio de la primera regla, el de la segunda ignorado y registrado; acciones de reglas distintas combinadas |
| A129 | Diálogo con propietario: el escritorio de la regla se ignora; sigue centrado sobre su principal; `transient=yes` con `nofocus` |
| A130 | `DOCK`, `NOTIFICATION` y `DESKTOP` con reglas que coinciden: sin efecto |
| A131 | `xdotool set_window --class` con la ventana mapeada: sin reevaluar; al retirarla y volver a mapearla, la regla se aplica |
| A132 | Sintaxis inválida, 65 reglas, más de 16 KiB y no UTF-8: sin reglas y registrado; sin marca no arranca; en `unshare -rmn`, `:0` y la falta de token se rechazan con reglas, sin conectar |
| A133 | Panel 1 de la comparación con el binario de F10 |
| A134 | Paneles 2 y 3 con F11 y composición |
| A135 | Estado posterior, pánicos, residuos y procesos (como A9) |

- **`test-client`** gana `--class INSTANCIA CLASE`. Los `WM_CLASS`
  defectuosos se escriben con `xprop`, y el cambio dinámico con `xdotool
  set_window --class`, que escribe las dos cadenas de ICCCM.
- `CRIMSON_RULES` solo llega al WM en el arnés de F11 (`start11`). Los
  `run.sh` de F1–F11 la eliminan del entorno heredado.
- **Binario de F10** (`CRIMSON_F10_BIN`, opcional): `git archive f90dfa2`
  compilado en un directorio temporal, sin worktree ni red.

## Resultados de las pruebas de aceptación de F11

Ejecuciones del 2026-10-14 en Xvfb con el binario de depuración de F11 y
la toolchain de Debian (`env PATH=/usr/bin:/bin`). Los arneses pidieron
los binarios a Cargo, que los dio por buenos («al día»).

- **F11:** 132 OK, 0 FALLA y 1 OMITIDA (`A135-xprop-:0`, la consulta de la
  raíz de `:0`, que se omite deliberadamente, como A9). A133 se ejecutó
  con el binario de F10 (`CRIMSON_F10_BIN`).
- **Regresiones**, todas con 0 FALLA y 1 OMITIDA por el mismo motivo:
  F1, 46 OK; F2, 47 OK; F3, 61 OK; F4, 79 OK; F5, 142 OK; F6, 122 OK;
  F7, 123 OK; F8, 106 OK; F9, 166 OK; F10, 140 OK.
- **Defectos del arnés corregidos durante F11-B**, en la primera ejecución
  de F11, que dio 8 FALLA, ninguna del WM:
  - la comprobación del entorno del WM separaba `/proc/PID/environ` por
    líneas, y una `CRIMSON_RULES` de varias líneas producía nombres de
    variable falsos; ahora lee las entradas separadas por NUL;
  - en `isolated.sh`, `env -u` iba detrás de las asignaciones, y GNU `env`
    lo tomaba por la orden.

  Después, F11 pasó dos veces seguidas: la segunda, dentro de la batería
  completa, con 132 OK.

- **Corrección de F11-C:** con más de 64 reglas, el analizador se
  detenía en la 65.ª y registraba «65 reglas» aunque hubiera más. Ahora
  cuenta las reglas antes de analizarlas (prueba unitaria con 100). Tras
  la corrección, F11 se repitió: 132 OK, 0 FALLA y 1 OMITIDA, con el
  binario recompilado por Cargo. F1–F10 no se repitieron, porque no reciben
  reglas ni ejecutan el analizador. La comparación documentada procede de
  la ejecución anterior; la corrección no afecta a esa escena.

Son los resultados de esas ejecuciones concretas, no una garantía para
cambios posteriores.

## Documentación visual

### Captura real (A133 y A134)

- **Escena:** A (rojo, `8B0A1F`) y B (negro, `1F2328`), sin `WM_CLASS`; V
  (`viewer`, `Viewer`; gris claro, `D0D7DE`; pide 700, 120, 460 × 300) y D
  (`chat`, `Chat`; blanco, `F6F8FA`). Las dos instancias del WM reciben las
  mismas reglas: `class=Viewer; floating` y `class=Chat; desktop=2`.
- **Panel 1 (F10):** las cuatro en mosaico: A 0 0 640 720, B 640 0 640 240,
  V 640 240 640 240 y D 640 480 640 240; ningún registro de reglas; foco en
  D.
- **Panel 2 (F11, escritorio 1):** A y B en mosaico (mitades); V flotante
  en 700, 120, 460 × 300, encima; D oculta en el escritorio 2; foco en V.
- **Panel 3 (F11, escritorio 2):** D sola, con el área completa, y
  enfocada.
- **Comprobaciones:** colores en puntos conocidos de cada captura; tres
  capturas de 1280 × 720; cada panel idéntico a su captura (0 píxeles
  distintos). Composición de 4012 × 1024. SHA-256 de
  `crimson-wm-f11-comparison.png`:
  `bb6937ca4dc39121de2fb6cff3e18454aed69bed9320a7558620d42d495f5377`.

### Diagrama

`crimson-wm-f11-rules.svg`, renderizado con `tools/render-svg.py …
--scale 2` a 2800 × 2920.
