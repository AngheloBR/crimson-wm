//! Geometría del diseño master-stack (ADR 0004). Sin E/S: solo calcula
//! rectángulos.
//!
//! Los tipos coinciden con los del protocolo X11 (coordenadas `INT16` y
//! tamaños `CARD16`), así que ningún resultado queda fuera de su rango, y
//! ningún tamaño es 0, que X11 rechaza con `BadValue`.

/// Porcentaje del ancho que ocupa la columna maestra (fijo en F2).
pub const MASTER_PERCENT: u32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// Geometría de `count` ventanas en mosaico, en el orden recibido: la
/// primera ocupa la columna maestra, a la izquierda, y el resto se reparte
/// la altura de la columna derecha (la pila).
///
/// Sin bordes ni márgenes: los rectángulos cubren el área sin huecos ni
/// solapes. Solo en casos degenerados (un área de menos de 2 píxeles de ancho
/// o más ventanas en la pila que píxeles de alto) los rectángulos miden al
/// menos 1 y se solapan.
pub fn master_stack(area: Rect, count: usize) -> Vec<Rect> {
    let area = Rect {
        width: area.width.max(1),
        height: area.height.max(1),
        ..area
    };
    match count {
        0 => Vec::new(),
        1 => vec![area],
        _ => {
            let (master_width, stack_offset) = split_columns(area.width);
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
fn split_columns(width: u16) -> (u16, u16) {
    if width < 2 {
        return (width, 0);
    }
    // MASTER_PERCENT <= 100, así que el resultado cabe en u16.
    let master = u32::from(width) * MASTER_PERCENT / 100;
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
        assert!(master_stack(SCREEN, 0).is_empty());
    }

    #[test]
    fn una_ventana_ocupa_el_area() {
        assert_eq!(master_stack(SCREEN, 1), vec![SCREEN]);
    }

    #[test]
    fn dos_y_tres_ventanas() {
        assert_eq!(
            master_stack(SCREEN, 2),
            vec![rect(0, 0, 640, 720), rect(640, 0, 640, 720)]
        );
        assert_eq!(
            master_stack(SCREEN, 3),
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
            master_stack(rect(0, 0, 1279, 719), 3),
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
            master_stack(rect(-10, 20, 100, 50), 3),
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
        assert_eq!(master_stack(rect(0, 0, 0, 0), 1), vec![rect(0, 0, 1, 1)]);
        // Un píxel de ancho: las columnas se solapan.
        assert_eq!(
            master_stack(rect(0, 0, 1, 10), 2),
            vec![rect(0, 0, 1, 10), rect(0, 0, 1, 10)]
        );
        // Más ventanas en la pila que píxeles de alto.
        assert_eq!(
            master_stack(rect(0, 0, 4, 2), 5),
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
        let rects = master_stack(rect(i16::MAX - 1, i16::MAX - 1, u16::MAX, u16::MAX), 4);
        assert_eq!(rects.len(), 4);
        for r in &rects {
            assert!(r.width >= 1 && r.height >= 1, "{r:?}");
        }
        // Las posiciones se saturan en el máximo de INT16.
        assert_eq!(rects[1].x, i16::MAX);
        assert_eq!(rects[3].y, i16::MAX);

        let rects = master_stack(rect(i16::MIN, i16::MIN, u16::MAX, u16::MAX), 3);
        assert_eq!(rects[1].x, -1);
        assert_eq!(rects[2].y, 0);
    }

    #[test]
    fn muchas_ventanas() {
        let rects = master_stack(SCREEN, 10_000);
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
    /// no se solapan y la cubren por completo.
    #[test]
    fn cubren_el_area_sin_solapes() {
        for width in 2..=24u16 {
            for height in 1..=24u16 {
                for count in 1..=usize::from(height) + 1 {
                    let area = rect(3, 5, width, height);
                    let rects = master_stack(area, count);
                    assert_eq!(rects.len(), count);
                    let mut covered = 0u32;
                    for (i, r) in rects.iter().enumerate() {
                        assert!(r.width >= 1 && r.height >= 1);
                        assert!(r.x >= area.x && r.y >= area.y);
                        assert!(i32::from(r.x) + i32::from(r.width) <= 3 + i32::from(width));
                        assert!(i32::from(r.y) + i32::from(r.height) <= 5 + i32::from(height));
                        for other in &rects[i + 1..] {
                            assert!(!overlaps(r, other), "{area:?} {count}: {r:?} {other:?}");
                        }
                        covered += u32::from(r.width) * u32::from(r.height);
                    }
                    assert_eq!(covered, u32::from(width) * u32::from(height));
                }
            }
        }
    }
}
