#!/usr/bin/env python3
# Capturas para a Microsoft Store (1920x1080, mínimo 1366x768) a partir de docs/prints.
# Uso: python3 scripts/loja-capturas.py docs/prints <saída> 01-inicio.png 08-tecnico-sessao.png ...
import sys
from PIL import Image, ImageDraw, ImageFilter
src_dir, out_dir = sys.argv[1], sys.argv[2]
W, H = 1920, 1080
for i, name in enumerate(sys.argv[3:], 1):
    bg = Image.new("RGB", (W, H))
    d = ImageDraw.Draw(bg)
    for y in range(H):  # azul do ícone, em degradê
        t = y / H
        d.line([(0, y), (W, y)], fill=(int(36 + 20 * t), int(84 - 30 * t), int(255 - 70 * t)))
    shot = Image.open(f"{src_dir}/{name}").convert("RGB")
    h = 960
    w = round(shot.width * h / shot.height)
    shot = shot.resize((w, h), Image.LANCZOS)
    x, y = (W - w) // 2, (H - h) // 2
    shadow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle([x + 6, y + 14, x + w + 6, y + h + 14], 14, fill=(0, 0, 0, 110))
    bg.paste(shadow.filter(ImageFilter.GaussianBlur(18)), (0, 0), shadow.filter(ImageFilter.GaussianBlur(18)))
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, w, h], 12, fill=255)
    bg.paste(shot, (x, y), mask)
    bg.save(f"{out_dir}/{i:02d}-{name}", optimize=True)
    print(f"{i:02d}-{name}")
