#!/usr/bin/env python3
# Imagens do pacote MSIX (packaging/windows/msix/Assets) a partir do ícone.
# Uso: python3 scripts/msix-imagens.py packaging/icons/elo-1024.png packaging/windows/msix/Assets
import sys
from PIL import Image
src = Image.open(sys.argv[1]).convert("RGBA")
out = sys.argv[2]
def icon(n):
    return src.resize((n, n), Image.LANCZOS)
def canvas(w, h, frac):
    c = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    n = int(min(w, h) * frac)
    c.alpha_composite(icon(n), ((w - n) // 2, (h - n) // 2))
    return c
jobs = {}
for s, f in ((1, ""), (2, ".scale-200")):
    jobs[f"Square44x44Logo{f}.png"] = icon(44 * s)
    jobs[f"Square150x150Logo{f}.png"] = canvas(150 * s, 150 * s, 0.66)
    jobs[f"Wide310x150Logo{f}.png"] = canvas(310 * s, 150 * s, 0.66)
    jobs[f"StoreLogo{f}.png"] = icon(50 * s)
for t in (16, 24, 32, 48, 256):
    jobs[f"Square44x44Logo.targetsize-{t}.png"] = icon(t)
    jobs[f"Square44x44Logo.targetsize-{t}_altform-unplated.png"] = icon(t)
for name, im in jobs.items():
    im.save(f"{out}/{name}", optimize=True)
print(len(jobs), "imagens")
