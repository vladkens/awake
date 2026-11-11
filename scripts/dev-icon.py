#!/usr/bin/env python3
"""Awake Dev's icon: Awake's in orange, so the two apps tell apart at a glance.

  scripts/dev-icon.py <icon.png> <out.png>

Each colour takes the hue HUE and keeps its saturation and brightness: the gradient's light and
dark stay, the cream cup too. No image library: a palette PNG has only its PLTE chunk changed; an
8-bit RGB or RGBA one is unfiltered, tinted pixel by pixel and written again unfiltered.
"""

import colorsys
import struct
import sys
import zlib
from pathlib import Path

SIGNATURE = b"\x89PNG\r\n\x1a\n"
# Orange.
HUE = 0.07
# Bytes per pixel of an 8-bit PNG's colour types that store pixels: RGB, RGBA.
BPP = {2: 3, 6: 4}


def orange(px: bytes, bpp: int) -> bytes:
    """The RGB of each pixel `bpp` bytes long tinted, the rest (alpha) kept; one colour once."""
    out, seen = bytearray(px), {}
    for i in range(0, len(px), bpp):
        rgb = px[i : i + 3]
        if rgb not in seen:
            _, s, v = colorsys.rgb_to_hsv(*(c / 255 for c in rgb))
            seen[rgb] = bytes(round(c * 255) for c in colorsys.hsv_to_rgb(HUE, s, v))

        out[i : i + 3] = seen[rgb]

    return bytes(out)


def unfilter(data: bytes, width: int, bpp: int) -> bytes:
    """The pixels of decompressed IDAT data: each row's filter byte undone (the PNG spec's)."""
    stride, out, prev = width * bpp, bytearray(), bytearray(width * bpp)
    for at in range(0, len(data), stride + 1):
        kind, row = data[at], bytearray(data[at + 1 : at + 1 + stride])
        for i in range(stride):
            a = row[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if kind == 1:
                row[i] = (row[i] + a) & 0xFF
            elif kind == 2:
                row[i] = (row[i] + b) & 0xFF
            elif kind == 3:
                row[i] = (row[i] + (a + b) // 2) & 0xFF
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                near = a if pa <= pb and pa <= pc else b if pb <= pc else c
                row[i] = (row[i] + near) & 0xFF

        out += row
        prev = row

    return bytes(out)


def chunk(kind: bytes, body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))


def main() -> int:
    src, dst = Path(sys.argv[1]), Path(sys.argv[2])
    data = src.read_bytes()
    if not data.startswith(SIGNATURE):
        sys.exit(f"{src}: not a PNG")

    chunks, at = [], len(SIGNATURE)
    while at < len(data):
        (size,) = struct.unpack(">I", data[at : at + 4])
        chunks.append((data[at + 4 : at + 8], data[at + 8 : at + 8 + size]))
        at += 12 + size

    width, _, depth, kind, _, _, interlace = struct.unpack(">IIBBBBB", chunks[0][1])
    if depth != 8 or interlace or kind not in (3, *BPP):
        sys.exit(f"{src}: only 8-bit palette, RGB or RGBA PNGs without interlacing")

    out = bytearray(SIGNATURE)
    if kind == 3:
        for name, body in chunks:
            out += chunk(name, orange(body, 3) if name == b"PLTE" else body)
    else:
        bpp, stride = BPP[kind], width * BPP[kind]
        idat = zlib.decompress(b"".join(body for name, body in chunks if name == b"IDAT"))
        px = orange(unfilter(idat, width, bpp), bpp)
        rows = b"".join(b"\0" + px[i : i + stride] for i in range(0, len(px), stride))
        idat = False
        for name, body in chunks:
            if name != b"IDAT":
                out += chunk(name, body)
            elif not idat:
                out += chunk(name, zlib.compress(rows, 9))
                idat = True

    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
