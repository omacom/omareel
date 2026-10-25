#!/usr/bin/env python3
"""Side-by-side of two frames (Qt left, Slint right), each scaled to `width`.

compare.py <qt.png> <slint.png> <out.png> [width]"""
import sys
from PIL import Image

a, b = Image.open(sys.argv[1]).convert('RGB'), Image.open(sys.argv[2]).convert('RGB')
w = int(sys.argv[4]) if len(sys.argv) > 4 else 960
h = round(a.height * w / a.width)
out = Image.new('RGB', (2 * w + 8, h), (255, 255, 255))
out.paste(a.resize((w, h), Image.LANCZOS), (0, 0))
out.paste(b.resize((w, h), Image.LANCZOS), (w + 8, 0))
out.save(sys.argv[3])
