"""Render the Brisa symbol as a Windows multi-resolution icon."""
import math
import struct
import zlib
from pathlib import Path


def clear_arc(x, y, start, end, level):
    if start <= x <= end:
        center = level + 0.072 * math.sin((x - start) * math.tau / (end - start))
        return abs(y - center) <= 0.031
    return False


def icon_pixel(x, y):
    dx = max(abs(x - 0.5) - 0.272, 0)
    dy = max(abs(y - 0.5) - 0.272, 0)
    if dx * dx + dy * dy > 0.173**2:
        return (0, 0, 0, 0)
    if clear_arc(x, y, 0.22, 0.80, 0.40):
        return (233, 255, 247, 255)
    if clear_arc(x, y, 0.28, 0.76, 0.58):
        return (153, 238, 210, 255)
    if clear_arc(x, y, 0.36, 0.66, 0.74):
        return (100, 213, 184, 255)
    mix = (x + y) / 2
    return (int(23 + 4 * mix), int(76 + 48 * mix), int(76 + 26 * mix), 255)


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def png(size):
    rows = bytearray()
    samples = 4
    for y in range(size):
        rows.append(0)
        for x in range(size):
            colors = [
                icon_pixel((x + (sx + 0.5) / samples) / size, (y + (sy + 0.5) / samples) / size)
                for sy in range(samples) for sx in range(samples)
            ]
            alpha = sum(color[3] for color in colors)
            if alpha:
                rows.extend(round(sum(color[i] * color[3] for color in colors) / alpha) for i in range(3))
            else:
                rows.extend((0, 0, 0))
            rows.append(round(alpha / len(colors)))
    header = struct.pack(">2I5B", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


sizes = (16, 24, 32, 48, 64, 128, 256)
offset = 6 + 16 * len(sizes)
directory = bytearray(struct.pack("<3H", 0, 1, len(sizes)))
images = []
for size in sizes:
    image = png(size)
    directory.extend(struct.pack("<4B2H2I", size % 256, size % 256, 0, 0, 1, 32, len(image), offset))
    images.append(image)
    offset += len(image)
Path(__file__).with_name("brisa.ico").write_bytes(directory + b"".join(images))
Path(__file__).resolve().parents[1].joinpath("docs", "brisa.png").write_bytes(images[sizes.index(128)])
