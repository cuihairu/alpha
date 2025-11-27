import os
import struct
import zlib

SIZES = [16, 32, 64, 128, 256, 512, 1024]
BASE = os.path.dirname(__file__)


def chunk(tag, data):
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def write_png(size: int) -> None:
    rows = []
    for y in range(size):
        row = bytearray([0])  # filter type 0
        for x in range(size):
            mix = (x + y) / (2 * size)
            r = int(30 + mix * 180)
            g = int(110 + mix * 120)
            b = int(200 - mix * 80)
            row.extend([r & 0xFF, g & 0xFF, b & 0xFF, 255])
        rows.append(bytes(row))

    raw = b"".join(rows)
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )

    os.makedirs(BASE, exist_ok=True)
    with open(os.path.join(BASE, f"alpha-{size}.png"), "wb") as fh:
        fh.write(png)


if __name__ == "__main__":
    for size in SIZES:
        write_png(size)
