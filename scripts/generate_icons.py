#!/usr/bin/env python3
"""Generate the application icons (two monitors with a transfer arrow).

    pip install pillow && python3 scripts/generate_icons.py

Writes src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png and icon.ico.
"""
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
BG = (15, 108, 189, 255)      # restrained blue
ACCENT = (20, 184, 166, 255)  # teal
WHITE = (255, 255, 255, 255)


def draw(size: int) -> Image.Image:
    s = size * 4  # supersample for smooth edges
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    r = s // 5
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=r, fill=BG)

    def monitor(x0, y0, w, h, fill):
        d.rounded_rectangle([x0, y0, x0 + w, y0 + h], radius=s // 40, outline=WHITE, width=max(2, s // 32), fill=fill)
        d.rectangle([x0 + w // 2 - s // 40, y0 + h, x0 + w // 2 + s // 40, y0 + h + s // 16], fill=WHITE)
        d.rectangle([x0 + w // 2 - s // 10, y0 + h + s // 16, x0 + w // 2 + s // 10, y0 + h + s // 16 + s // 40], fill=WHITE)

    w, h = int(s * 0.34), int(s * 0.26)
    monitor(int(s * 0.10), int(s * 0.22), w, h, (52, 136, 210, 255))
    monitor(int(s * 0.56), int(s * 0.22), w, h, ACCENT)
    # Arrow from left to right monitor.
    y = int(s * 0.72)
    d.line([int(s * 0.27), y, int(s * 0.70), y], fill=WHITE, width=max(3, s // 22))
    d.polygon([(int(s * 0.78), y), (int(s * 0.66), y - s // 14), (int(s * 0.66), y + s // 14)], fill=WHITE)
    return img.resize((size, size), Image.LANCZOS)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    draw(32).save(OUT / "32x32.png")
    draw(128).save(OUT / "128x128.png")
    draw(256).save(OUT / "128x128@2x.png")
    draw(512).save(OUT / "icon.png")
    draw(256).save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print(f"Icons written to {OUT}")


if __name__ == "__main__":
    main()
