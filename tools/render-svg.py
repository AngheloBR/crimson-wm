#!/usr/bin/env python3
"""Renderiza un SVG a PNG con librsvg y cairo del sistema (ADR 0009).

Uso: tools/render-svg.py ENTRADA.svg SALIDA.png [--scale N]

Llama directamente, con ctypes, a las bibliotecas compartidas que ya instala
Debian 13 (librsvg2-2 y libcairo2): no necesita módulos de Python adicionales,
no instala nada ni accede a la red. Es el mismo renderizador que usa
`rsvg-convert`; el interno de ImageMagick (MSVG) no es fiel (no dibuja los
trazos).

Códigos de salida: 0 si se escribió el PNG; 1 si el SVG no se pudo cargar o
renderizar o el PNG no se pudo escribir; 2 si faltan bibliotecas o los
argumentos no son válidos.
"""

import argparse
import ctypes
import ctypes.util
import sys

CAIRO_FORMAT_ARGB32 = 0


def load(names):
    """Carga la primera biblioteca disponible de `names` o termina con 2."""
    for name in names:
        try:
            return ctypes.CDLL(name)
        except OSError:
            continue
    sys.exit(f"render-svg: falta la biblioteca {names[0]} (instala el paquete correspondiente)")


class RsvgRectangle(ctypes.Structure):
    _fields_ = [
        ("x", ctypes.c_double),
        ("y", ctypes.c_double),
        ("width", ctypes.c_double),
        ("height", ctypes.c_double),
    ]


def main():
    parser = argparse.ArgumentParser(description="Renderiza un SVG a PNG con librsvg y cairo.")
    parser.add_argument("source", help="SVG de entrada")
    parser.add_argument("target", help="PNG de salida")
    parser.add_argument("--scale", type=float, default=1.0, help="factor de escala (por defecto, 1)")
    args = parser.parse_args()
    if not 0 < args.scale <= 8:
        parser.error("la escala debe estar entre 0 y 8")

    rsvg = load(["librsvg-2.so.2", ctypes.util.find_library("rsvg-2") or "librsvg-2.so"])
    cairo = load(["libcairo.so.2", ctypes.util.find_library("cairo") or "libcairo.so"])
    gobject = load(["libgobject-2.0.so.0", ctypes.util.find_library("gobject-2.0") or "libgobject-2.0.so"])

    rsvg.rsvg_handle_new_from_file.restype = ctypes.c_void_p
    rsvg.rsvg_handle_new_from_file.argtypes = [ctypes.c_char_p, ctypes.c_void_p]
    rsvg.rsvg_handle_get_intrinsic_size_in_pixels.restype = ctypes.c_int
    rsvg.rsvg_handle_get_intrinsic_size_in_pixels.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_double),
        ctypes.POINTER(ctypes.c_double),
    ]
    rsvg.rsvg_handle_render_document.restype = ctypes.c_int
    rsvg.rsvg_handle_render_document.argtypes = [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.POINTER(RsvgRectangle),
        ctypes.c_void_p,
    ]
    cairo.cairo_image_surface_create.restype = ctypes.c_void_p
    cairo.cairo_image_surface_create.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_int]
    cairo.cairo_create.restype = ctypes.c_void_p
    cairo.cairo_create.argtypes = [ctypes.c_void_p]
    cairo.cairo_scale.argtypes = [ctypes.c_void_p, ctypes.c_double, ctypes.c_double]
    cairo.cairo_surface_write_to_png.restype = ctypes.c_int
    cairo.cairo_surface_write_to_png.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
    cairo.cairo_destroy.argtypes = [ctypes.c_void_p]
    cairo.cairo_surface_destroy.argtypes = [ctypes.c_void_p]
    gobject.g_object_unref.argtypes = [ctypes.c_void_p]

    handle = rsvg.rsvg_handle_new_from_file(args.source.encode(), None)
    if not handle:
        print(f"render-svg: no se pudo cargar {args.source}", file=sys.stderr)
        return 1
    width, height = ctypes.c_double(), ctypes.c_double()
    if not rsvg.rsvg_handle_get_intrinsic_size_in_pixels(handle, ctypes.byref(width), ctypes.byref(height)):
        print("render-svg: el SVG no tiene un tamaño intrínseco en píxeles", file=sys.stderr)
        gobject.g_object_unref(handle)
        return 1
    pixels = (round(width.value * args.scale), round(height.value * args.scale))
    surface = cairo.cairo_image_surface_create(CAIRO_FORMAT_ARGB32, *pixels)
    context = cairo.cairo_create(surface)
    cairo.cairo_scale(context, args.scale, args.scale)
    viewport = RsvgRectangle(0, 0, width.value, height.value)
    ok = rsvg.rsvg_handle_render_document(handle, context, ctypes.byref(viewport), None)
    written = ok and cairo.cairo_surface_write_to_png(surface, args.target.encode()) == 0
    cairo.cairo_destroy(context)
    cairo.cairo_surface_destroy(surface)
    gobject.g_object_unref(handle)
    if not ok:
        print("render-svg: librsvg no pudo renderizar el documento", file=sys.stderr)
        return 1
    if not written:
        print(f"render-svg: no se pudo escribir {args.target}", file=sys.stderr)
        return 1
    print(f"{args.target}: {pixels[0]}x{pixels[1]} (escala {args.scale:g})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
