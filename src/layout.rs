//! Geometría del diseño master-stack (ADR 0004) y del área de trabajo
//! (ADR 0012). Sin E/S: solo calcula rectángulos.
//!
//! Los tipos coinciden con los del protocolo X11 (coordenadas `INT16` y
//! tamaños `CARD16`), así que ningún resultado queda fuera de su rango, y
//! ningún tamaño es 0, que X11 rechaza con `BadValue`.

/// Porcentaje del ancho que ocupa la columna maestra si la configuración no
/// lo cambia (el valor fijo de F2 a F11).
pub const DEFAULT_MASTER_PERCENT: u8 = 50;
/// Límites de `[layout] master_percent` (ADR 0014): ninguna columna queda
/// por debajo del 10 % del ancho.
pub const MIN_MASTER_PERCENT: u8 = 10;
pub const MAX_MASTER_PERCENT: u8 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// Grosor reservado en cada borde de la pantalla (ADR 0012): el máximo de
/// las reservas efectivas de los paneles en ese borde.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl Insets {
    /// El máximo de cada borde entre las dos: los struts se miden desde el
    /// borde de la pantalla, así que dos paneles apilados en el mismo borde
    /// no se suman.
    pub fn max(self, other: Insets) -> Insets {
        Insets {
            left: self.left.max(other.left),
            right: self.right.max(other.right),
            top: self.top.max(other.top),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

/// Área de trabajo: la pantalla menos las reservas, y si alguna reserva se
/// ha tenido que limitar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkArea {
    pub rect: Rect,
    pub limited: bool,
}

/// Área de trabajo de `screen` con las reservas `insets` (ADR 0012). Nunca
/// es vacía: en cada eje queda al menos un píxel. Si las reservas de un eje
/// no dejan ninguno, se limitan de forma determinista: primero la del borde
/// final (derecha o abajo) y, si no basta, la del inicial. Es la única
/// restricción de Crimson sobre los valores (EWMH no fija ningún mínimo).
/// Todo se calcula en `u32` sin desbordamientos.
pub fn work_area(screen: Rect, insets: Insets) -> WorkArea {
    // (inicio, tamaño) de un eje y si se ha limitado.
    let axis = |origin: i16, size: u16, before: u32, after: u32| {
        let size32 = u32::from(size.max(1));
        let before_eff = before.min(size32 - 1);
        let after_eff = after.min(size32 - 1 - before_eff);
        let limited = before_eff != before || after_eff != after;
        let length = size32 - before_eff - after_eff;
        // before_eff < size32 <= u16::MAX y length <= size32: caben.
        let offset = u16::try_from(before_eff).unwrap_or(0);
        (
            origin.saturating_add_unsigned(offset),
            u16::try_from(length).unwrap_or(1),
            limited,
        )
    };
    let (x, width, lx) = axis(screen.x, screen.width, insets.left, insets.right);
    let (y, height, ly) = axis(screen.y, screen.height, insets.top, insets.bottom);
    WorkArea {
        rect: Rect {
            x,
            y,
            width,
            height,
        },
        limited: lx || ly,
    }
}

/// Valor de `_NET_WORKAREA`: `[x, y, ancho, alto]` del área de trabajo,
/// repetido para cada uno de los `desktops` escritorios (ADR 0012). Las
/// coordenadas negativas, imposibles con la raíz en el origen, se escriben
/// como 0.
pub fn workarea_property(work: Rect, desktops: usize) -> Vec<u32> {
    let one = [
        u32::try_from(work.x).unwrap_or(0),
        u32::try_from(work.y).unwrap_or(0),
        u32::from(work.width),
        u32::from(work.height),
    ];
    one.iter().copied().cycle().take(4 * desktops).collect()
}

/// Posición (x, y) de una ventana de `width` × `height` con borde `border`
/// centrada sobre `over` y dentro de `area` (ADR 0010). Si no cabe en el
/// área, queda en su esquina superior izquierda. Se calcula en `i32`, sin
/// desbordamientos, y el resultado cabe en `i16`.
pub fn center(width: u16, height: u16, border: u16, over: Rect, area: Rect) -> (i16, i16) {
    let axis = |start: i16, size: u16, over_start: i16, over_size: u16, outer: u16| {
        let outer = i32::from(outer) + 2 * i32::from(border);
        let wanted = i32::from(over_start) + (i32::from(over_size) - outer) / 2;
        let min = i32::from(start);
        let max = (min + i32::from(size) - outer).max(min);
        let value = wanted.clamp(min, max);
        i16::try_from(value).unwrap_or(start)
    };
    (
        axis(area.x, area.width, over.x, over.width, width),
        axis(area.y, area.height, over.y, over.height, height),
    )
}

/// Geometría de `count` ventanas en mosaico, en el orden recibido: la
/// primera ocupa la columna maestra, a la izquierda, y el resto se reparte
/// la altura de la columna derecha (la pila).
///
/// Sin bordes ni márgenes: los rectángulos cubren el área sin huecos ni
/// solapes. Solo en casos degenerados (un área de menos de 2 píxeles de ancho
/// o más ventanas en la pila que píxeles de alto) los rectángulos miden al
/// menos 1 y se solapan.
///
/// `master_percent` es el porcentaje del ancho de la columna maestra; la
/// configuración lo limita a 10–90, pero cualquier valor da rectángulos
/// válidos.
pub fn master_stack(area: Rect, count: usize, master_percent: u8) -> Vec<Rect> {
    let area = Rect {
        width: area.width.max(1),
        height: area.height.max(1),
        ..area
    };
    match count {
        0 => Vec::new(),
        1 => vec![area],
        _ => {
            let (master_width, stack_offset) = split_columns(area.width, master_percent);
            let stack_x = area.x.saturating_add_unsigned(stack_offset);
            let stack_width = area.width - stack_offset;

            let mut rects = Vec::with_capacity(count);
            rects.push(Rect {
                width: master_width,
                ..area
            });
            rects.extend(split(area.height, count - 1).map(|(offset, height)| Rect {
                x: stack_x,
                y: area.y.saturating_add_unsigned(offset),
                width: stack_width,
                height,
            }));
            rects
        }
    }
}

/// Ancho de la columna maestra y desplazamiento de la pila. Con menos de 2
/// píxeles ambas columnas ocupan el área completa.
fn split_columns(width: u16, master_percent: u8) -> (u16, u16) {
    if width < 2 {
        return (width, 0);
    }
    // Con un porcentaje de hasta 100 el resultado cabe en u16; uno mayor se
    // recorta con el `clamp`.
    let master = u32::from(width) * u32::from(master_percent) / 100;
    let master = u16::try_from(master).unwrap_or(width).clamp(1, width - 1);
    (master, master)
}

/// Divide `total` píxeles en `parts` tramos `(desplazamiento, tamaño)`. Los
/// primeros `total % parts` tramos miden un píxel más, de modo que la suma
/// es exactamente `total`. Si hay más tramos que píxeles, cada uno mide 1 y
/// los sobrantes se colocan en el último píxel.
fn split(total: u16, parts: usize) -> impl Iterator<Item = (u16, u16)> {
    let total_px = usize::from(total);
    let base = total_px / parts;
    let remainder = total_px % parts;
    (0..parts).map(move |i| {
        if base == 0 {
            // Desplazamiento < total, que cabe en u16.
            let offset = i.min(total_px - 1);
            (to_u16(offset), 1)
        } else {
            let offset = i * base + i.min(remainder);
            let size = base + usize::from(i < remainder);
            (to_u16(offset), to_u16(size))
        }
    })
}

/// Los valores de `split` nunca superan `total`, que ya es un `u16`.
fn to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // U20: centrado de diálogos.

    #[test]
    fn centra_sobre_la_ventana() {
        let over = Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 720,
        };
        assert_eq!(center(300, 200, 0, over, SCREEN), (170, 260));
        // El borde cuenta en el tamaño exterior.
        assert_eq!(center(300, 200, 5, over, SCREEN), (165, 255));
        // Sobre la pila de la derecha.
        let stack = Rect {
            x: 640,
            y: 360,
            width: 640,
            height: 360,
        };
        assert_eq!(center(320, 240, 0, stack, SCREEN), (800, 420));
    }

    #[test]
    fn centrado_dentro_del_area() {
        // Una ventana mayor que su principal no sale del área.
        let small = Rect {
            x: 1200,
            y: 680,
            width: 80,
            height: 40,
        };
        assert_eq!(center(400, 300, 0, small, SCREEN), (880, 420));
        // Mayor que el área: esquina superior izquierda.
        assert_eq!(center(2000, 1000, 0, small, SCREEN), (0, 0));
        // Valores extremos, sin desbordamientos.
        let far = Rect {
            x: i16::MAX,
            y: i16::MIN,
            width: u16::MAX,
            height: u16::MAX,
        };
        assert_eq!(center(u16::MAX, u16::MAX, u16::MAX, far, SCREEN), (0, 0));
        assert_eq!(center(10, 10, 0, far, SCREEN), (1270, 0));
    }

    fn rect(x: i16, y: i16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    };

    #[test]
    fn sin_ventanas() {
        assert!(master_stack(SCREEN, 0, DEFAULT_MASTER_PERCENT).is_empty());
    }

    #[test]
    fn una_ventana_ocupa_el_area() {
        assert_eq!(
            master_stack(SCREEN, 1, DEFAULT_MASTER_PERCENT),
            vec![SCREEN]
        );
    }

    #[test]
    fn dos_y_tres_ventanas() {
        assert_eq!(
            master_stack(SCREEN, 2, DEFAULT_MASTER_PERCENT),
            vec![rect(0, 0, 640, 720), rect(640, 0, 640, 720)]
        );
        assert_eq!(
            master_stack(SCREEN, 3, DEFAULT_MASTER_PERCENT),
            vec![
                rect(0, 0, 640, 720),
                rect(640, 0, 640, 360),
                rect(640, 360, 640, 360)
            ]
        );
    }

    #[test]
    fn tamanos_impares() {
        assert_eq!(
            master_stack(rect(0, 0, 1279, 719), 3, DEFAULT_MASTER_PERCENT),
            vec![
                rect(0, 0, 639, 719),
                rect(639, 0, 640, 360),
                rect(639, 360, 640, 359)
            ]
        );
    }

    #[test]
    fn el_resto_va_a_los_primeros_tramos() {
        assert_eq!(
            split(10, 3).collect::<Vec<_>>(),
            vec![(0, 4), (4, 3), (7, 3)]
        );
        assert_eq!(
            split(9, 3).collect::<Vec<_>>(),
            vec![(0, 3), (3, 3), (6, 3)]
        );
    }

    #[test]
    fn area_desplazada() {
        assert_eq!(
            master_stack(rect(-10, 20, 100, 50), 3, DEFAULT_MASTER_PERCENT),
            vec![
                rect(-10, 20, 50, 50),
                rect(40, 20, 50, 25),
                rect(40, 45, 50, 25)
            ]
        );
    }

    #[test]
    fn casos_degenerados_sin_tamanos_nulos() {
        // Área vacía: se trata como 1×1.
        assert_eq!(
            master_stack(rect(0, 0, 0, 0), 1, DEFAULT_MASTER_PERCENT),
            vec![rect(0, 0, 1, 1)]
        );
        // Un píxel de ancho: las columnas se solapan.
        assert_eq!(
            master_stack(rect(0, 0, 1, 10), 2, DEFAULT_MASTER_PERCENT),
            vec![rect(0, 0, 1, 10), rect(0, 0, 1, 10)]
        );
        // Más ventanas en la pila que píxeles de alto.
        assert_eq!(
            master_stack(rect(0, 0, 4, 2), 5, DEFAULT_MASTER_PERCENT),
            vec![
                rect(0, 0, 2, 2),
                rect(2, 0, 2, 1),
                rect(2, 1, 2, 1),
                rect(2, 1, 2, 1),
                rect(2, 1, 2, 1)
            ]
        );
    }

    #[test]
    fn coordenadas_extremas_sin_desbordamiento() {
        let rects = master_stack(
            rect(i16::MAX - 1, i16::MAX - 1, u16::MAX, u16::MAX),
            4,
            DEFAULT_MASTER_PERCENT,
        );
        assert_eq!(rects.len(), 4);
        for r in &rects {
            assert!(r.width >= 1 && r.height >= 1, "{r:?}");
        }
        // Las posiciones se saturan en el máximo de INT16.
        assert_eq!(rects[1].x, i16::MAX);
        assert_eq!(rects[3].y, i16::MAX);

        let rects = master_stack(
            rect(i16::MIN, i16::MIN, u16::MAX, u16::MAX),
            3,
            DEFAULT_MASTER_PERCENT,
        );
        assert_eq!(rects[1].x, -1);
        assert_eq!(rects[2].y, 0);
    }

    #[test]
    fn muchas_ventanas() {
        let rects = master_stack(SCREEN, 10_000, DEFAULT_MASTER_PERCENT);
        assert_eq!(rects.len(), 10_000);
        assert!(rects.iter().all(|r| r.width >= 1 && r.height >= 1));
        assert!(rects.iter().all(|r| r.y >= 0 && r.y < 720));
    }

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        let (ax, ay, bx, by) = (
            i32::from(a.x),
            i32::from(a.y),
            i32::from(b.x),
            i32::from(b.y),
        );
        ax < bx + i32::from(b.width)
            && bx < ax + i32::from(a.width)
            && ay < by + i32::from(b.height)
            && by < ay + i32::from(a.height)
    }

    /// Para tamaños no degenerados, los rectángulos quedan dentro del área,
    /// no se solapan y la cubren por completo, con cualquier proporción
    /// admitida.
    #[test]
    fn cubren_el_area_sin_solapes() {
        for percent in [
            MIN_MASTER_PERCENT,
            33,
            DEFAULT_MASTER_PERCENT,
            60,
            MAX_MASTER_PERCENT,
        ] {
            for width in 2..=24u16 {
                for height in 1..=24u16 {
                    for count in 1..=usize::from(height) + 1 {
                        check_cover(rect(3, 5, width, height), count, percent);
                    }
                }
            }
        }
    }

    fn check_cover(area: Rect, count: usize, percent: u8) {
        let rects = master_stack(area, count, percent);
        assert_eq!(rects.len(), count);
        let mut covered = 0u32;
        for (i, r) in rects.iter().enumerate() {
            assert!(r.width >= 1 && r.height >= 1);
            assert!(r.x >= area.x && r.y >= area.y);
            assert!(
                i32::from(r.x) + i32::from(r.width) <= i32::from(area.x) + i32::from(area.width)
            );
            assert!(
                i32::from(r.y) + i32::from(r.height) <= i32::from(area.y) + i32::from(area.height)
            );
            for other in &rects[i + 1..] {
                assert!(
                    !overlaps(r, other),
                    "{area:?} {count} {percent}: {r:?} {other:?}"
                );
            }
            covered += u32::from(r.width) * u32::from(r.height);
        }
        assert_eq!(covered, u32::from(area.width) * u32::from(area.height));
    }

    // U24: proporción configurable (ADR 0014).

    #[test]
    fn proporcion_de_la_maestra() {
        // 60 %: 768 + 512 en 1280.
        assert_eq!(
            master_stack(SCREEN, 2, 60),
            vec![rect(0, 0, 768, 720), rect(768, 0, 512, 720)]
        );
        assert_eq!(
            master_stack(SCREEN, 3, 60),
            vec![
                rect(0, 0, 768, 720),
                rect(768, 0, 512, 360),
                rect(768, 360, 512, 360)
            ]
        );
        // Límites exactos de la configuración: 10 % y 90 %.
        assert_eq!(
            master_stack(SCREEN, 2, MIN_MASTER_PERCENT),
            vec![rect(0, 0, 128, 720), rect(128, 0, 1152, 720)]
        );
        assert_eq!(
            master_stack(SCREEN, 2, MAX_MASTER_PERCENT),
            vec![rect(0, 0, 1152, 720), rect(1152, 0, 128, 720)]
        );
        // Una sola ventana ocupa el área con cualquier proporción.
        assert_eq!(master_stack(SCREEN, 1, 60), vec![SCREEN]);
        // Redondeo hacia abajo: 1279 × 60 % = 767,4.
        assert_eq!(master_stack(rect(0, 0, 1279, 10), 2, 60)[0].width, 767);
    }

    #[test]
    fn proporciones_fuera_de_rango_no_dan_tamanos_nulos() {
        // La configuración no las admite, pero la geometría las tolera.
        for percent in [0, 1, 99, 100, u8::MAX] {
            let rects = master_stack(SCREEN, 2, percent);
            assert!(rects.iter().all(|r| r.width >= 1), "{percent}: {rects:?}");
            assert_eq!(
                u32::from(rects[0].width) + u32::from(rects[1].width),
                1280,
                "{percent}"
            );
        }
    }

    #[test]
    fn la_proporcion_predeterminada_es_la_de_f11() {
        assert_eq!(DEFAULT_MASTER_PERCENT, 50);
        assert!((MIN_MASTER_PERCENT..=MAX_MASTER_PERCENT).contains(&DEFAULT_MASTER_PERCENT));
    }

    // U22: área de trabajo (ADR 0012).

    fn insets(left: u32, right: u32, top: u32, bottom: u32) -> Insets {
        Insets {
            left,
            right,
            top,
            bottom,
        }
    }

    fn work(left: u32, right: u32, top: u32, bottom: u32) -> (Rect, bool) {
        let w = work_area(SCREEN, insets(left, right, top, bottom));
        (w.rect, w.limited)
    }

    #[test]
    fn reservas_en_los_cuatro_bordes() {
        assert_eq!(work(0, 0, 0, 0), (SCREEN, false));
        assert_eq!(work(0, 0, 40, 0), (rect(0, 40, 1280, 680), false));
        assert_eq!(work(0, 0, 0, 30), (rect(0, 0, 1280, 690), false));
        assert_eq!(work(200, 0, 0, 0), (rect(200, 0, 1080, 720), false));
        assert_eq!(work(0, 150, 0, 0), (rect(0, 0, 1130, 720), false));
        assert_eq!(work(10, 20, 30, 40), (rect(10, 30, 1250, 650), false));
    }

    #[test]
    fn maximo_por_borde_no_suma() {
        let a = insets(0, 0, 30, 0);
        let b = insets(0, 0, 60, 20);
        let c = insets(100, 0, 0, 0);
        assert_eq!(a.max(b).max(c), insets(100, 0, 60, 20));
        assert_eq!(a.max(a), a);
        assert_eq!(Insets::default().max(a), a);
    }

    #[test]
    fn reservas_extremas_dejan_un_pixel() {
        // Un borde que pide toda la pantalla, o más: queda un píxel.
        assert_eq!(work(0, 0, 720, 0), (rect(0, 719, 1280, 1), true));
        assert_eq!(work(0, 0, u32::MAX, 0), (rect(0, 719, 1280, 1), true));
        assert_eq!(
            work(u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            (rect(1279, 719, 1, 1), true)
        );
        // Dos bordes opuestos que no caben: se limita primero el final.
        assert_eq!(work(0, 0, 400, 400), (rect(0, 400, 1280, 1), true));
        assert_eq!(work(700, 700, 0, 0), (rect(700, 0, 1, 720), true));
        // Justo en el límite: sin limitar.
        assert_eq!(work(0, 0, 360, 359), (rect(0, 360, 1280, 1), false));
        assert_eq!(work(1279, 0, 0, 0), (rect(1279, 0, 1, 720), false));
    }

    #[test]
    fn pantallas_minimas_y_desplazadas() {
        let tiny = rect(0, 0, 1, 1);
        assert_eq!(work_area(tiny, insets(5, 5, 5, 5)).rect, tiny);
        let shifted = rect(-10, 20, 100, 50);
        assert_eq!(
            work_area(shifted, insets(10, 0, 5, 0)).rect,
            rect(0, 25, 90, 45)
        );
        // El área nunca tiene un tamaño 0, que X11 rechazaría.
        for (l, r, t, b) in [(0, 0, 0, 0), (u32::MAX, 0, 0, u32::MAX), (1, 1, 1, 1)] {
            let w = work_area(tiny, insets(l, r, t, b)).rect;
            assert!(w.width >= 1 && w.height >= 1);
        }
    }

    #[test]
    fn propiedad_workarea() {
        let v = workarea_property(rect(0, 40, 1280, 680), 8);
        assert_eq!(v.len(), 32);
        for chunk in v.chunks(4) {
            assert_eq!(chunk, [0, 40, 1280, 680]);
        }
        assert_eq!(workarea_property(rect(-5, 3, 1, 1), 1), vec![0, 3, 1, 1]);
        assert_eq!(workarea_property(SCREEN, 0), Vec::<u32>::new());
    }
}
