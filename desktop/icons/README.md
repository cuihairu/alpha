# Desktop Icons

This directory ships the assets required by the Tauri desktop bundle:

- `32x32.png`, `128x128.png`, `128x128@2x.png`: PNGs referenced by `tauri.conf.json`.
- `icon.png`: the tray icon that Tauri loads at runtime.
- `icon.icns`: the macOS bundle icon generated from the 512 px artwork.
- `icon.ico`: the Windows bundle icon that wraps the 256 px PNG payload.
- `source/`: gradient PNG masters that can be regenerated if the artwork changes.

## Regenerating the assets

1. Rebuild the source PNG set (16–1024 px) with Python's standard library:

   ```python
   # desktop/icons/source/generate.py
   import os, struct, zlib

   SIZES = [16, 32, 64, 128, 256, 512, 1024]
   BASE = os.path.dirname(__file__)

   def chunk(tag, data):
       return (struct.pack('>I', len(data)) + tag + data +
               struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF))

   def write_png(size):
       rows = []
       for y in range(size):
           row = bytearray([0])
           for x in range(size):
               mix = (x + y) / (2 * size)
               r = int(30 + mix * 180)
               g = int(110 + mix * 120)
               b = int(200 - mix * 80)
               row.extend([r & 0xFF, g & 0xFF, b & 0xFF, 255])
           rows.append(bytes(row))
       raw = b''.join(rows)
       ihdr = struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)
       data = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr) +
               chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))
       with open(os.path.join(BASE, f'alpha-{size}.png'), 'wb') as fh:
           fh.write(data)

   if __name__ == '__main__':
       os.makedirs(BASE, exist_ok=True)
       for size in SIZES:
           write_png(size)
   ```

2. Copy the 32/128/256 px outputs into the top-level files (`32x32.png`, `128x128*.png`, `icon.png`).
3. Generate `icon.icns` and `icon.ico`:

   ```bash
   sips -s format icns desktop/icons/source/alpha-512.png --out desktop/icons/icon.icns
   python3 - <<'PY'
   import pathlib, struct
   png = pathlib.Path('desktop/icons/source/alpha-256.png').read_bytes()
   header = struct.pack('<HHH', 0, 1, 1)
   entry = struct.pack('<BBBBHHII', 0, 0, 0, 0, 1, 32, len(png), 22)
   pathlib.Path('desktop/icons/icon.ico').write_bytes(header + entry + png)
   PY
   ```

Keeping the raw sources in `source/` keeps the desktop package tidy while retaining the gradient artwork for future tweaks.
