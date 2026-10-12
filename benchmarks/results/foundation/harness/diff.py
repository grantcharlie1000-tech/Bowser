"""Pixel-compares two render.mjs runs: python diff.py <dir> <labelA> <labelB>.

Prints, per view, the number of differing pixels and the largest channel difference, and writes
<labelB>-vs-<labelA>-<view>.png (changed pixels in magenta) for any view that changed."""
import sys
from pathlib import Path

from PIL import Image, ImageChops

root, a, b = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
total = 0
for left in sorted(root.glob(f"{a}-*.png")):
    view = left.name[len(a) + 1:-4]
    right = root / f"{b}-{view}.png"
    if not right.exists():
        print(f"{view:28} missing in {b}")
        continue
    x, y = Image.open(left).convert("RGB"), Image.open(right).convert("RGB")
    if x.size != y.size:
        print(f"{view:28} size {x.size} vs {y.size}")
        continue
    delta = ImageChops.difference(x, y)
    mask = delta.convert("L").point(lambda v: 255 if v > 0 else 0)
    changed = sum(1 for v in mask.getdata() if v)
    peak = max(max(band) for band in (c.getextrema() for c in delta.split()))
    total += changed
    print(f"{view:28} changed={changed:6d} peak={peak}")
    if changed:
        marked = y.copy()
        marked.paste((255, 0, 255), mask=mask)
        marked.save(root / f"{b}-vs-{a}-{view}.png")
print(f"total changed pixels: {total}")
