#!/usr/bin/env python3
"""Generate the filer app icon: a dark rounded square with a folder + a
downward arrow funneling into sorted slots. Renders a 1024x1024 PNG that
`tauri icon` turns into all platform sizes.

Run: python3 scripts/make_icon.py
"""
from PIL import Image, ImageDraw

SIZE = 1024
BG_TOP = (51, 65, 85)      # slate-700
BG_BOTTOM = (15, 23, 42)   # slate-900
FOLDER = (241, 245, 249)   # slate-100
SLOT = (51, 65, 85)        # slate-700 (slots cut into the folder)
ARROW = (245, 158, 11)     # amber-500

RADIUS = 220  # rounded-rect corner radius for the background


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def vertical_gradient(size, top, bottom):
    img = Image.new("RGB", (size, size), top)
    px = img.load()
    for y in range(size):
        c = lerp(top, bottom, y / (size - 1))
        for x in range(size):
            px[x, y] = c
    return img


def rounded_rect_mask(size, radius):
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    d.rounded_rectangle((0, 0, size - 1, size - 1), radius=radius, fill=255)
    return mask


def draw_icon():
    bg = vertical_gradient(SIZE, BG_TOP, BG_BOTTOM)
    out = Image.new("RGB", (SIZE, SIZE), (0, 0, 0))
    out.paste(bg, (0, 0), rounded_rect_mask(SIZE, RADIUS))
    d = ImageDraw.Draw(out)

    # Folder body (rounded rect) centered, lower half.
    fx0, fy0, fx1, fy1 = 232, 470, 792, 820
    fr = 48
    # Folder tab on top-left (plain rect — Pillow's rounded_rectangle has no
    # per-corner radius).
    tab_w, tab_h = 150, 70
    d.rectangle((fx0, fy0 - tab_h, fx0 + tab_w, fy0), fill=FOLDER)
    d.rounded_rectangle((fx0, fy0, fx1, fy1), radius=fr, fill=FOLDER)

    # Three horizontal "sorted slot" lines cut into the folder.
    slot_y = [fy0 + 110, fy0 + 200, fy0 + 290]
    for y in slot_y:
        d.rounded_rectangle((fx0 + 60, y, fx1 - 60, y + 26), radius=13, fill=SLOT)

    # Downward arrow above the folder, pointing into it.
    cx = (fx0 + fx1) // 2
    top_y = 150
    # shaft
    d.rounded_rectangle((cx - 52, top_y, cx + 52, 430), radius=26, fill=ARROW)
    # arrowhead (triangle)
    d.polygon([(cx - 170, 360), (cx + 170, 360), (cx, 540)], fill=ARROW)

    return out


if __name__ == "__main__":
    img = draw_icon()
    img.save("scripts/icon-source-1024.png", "PNG")
    print("wrote scripts/icon-source-1024.png (1024x1024)")
    print("next: ./ui/node_modules/.bin/tauri icon scripts/icon-source-1024.png")
