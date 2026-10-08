#!/usr/bin/env python3
"""Generate the desktop app icons and the sign-in page marks on macOS. Requires Node, Pillow and CairoSVG."""
import io
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import cairosvg
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / 'src-tauri' / 'icons'
ICONUTIL = shutil.which('iconutil')
if ICONUTIL is None:
    raise SystemExit('Run on macOS with iconutil to generate the desktop icons.')
# Physical density does not change the geometry selected for a logical icon size.
PNG_SIZES = {
    '32x32.png': (32, 32), '64x64.png': (64, 64),
    '128x128.png': (128, 128), '128x128@2x.png': (256, 128),
    'icon.png': (512, 512), 'StoreLogo.png': (50, 50),
    **{f'Square{size}x{size}Logo.png': (size, size)
       for size in (30, 44, 71, 89, 107, 142, 150, 284, 310)},
}
# The pocket-fold symbol and the sign-in marks come from the shapes the app draws.
script = """
import { FOLD_BODY, FOLD_EAR, signInMarkSvg, signedInMarkSvg } from './src/lib/pocket-fold.js';
console.log(JSON.stringify({ body: FOLD_BODY, ear: FOLD_EAR, still: signInMarkSvg(), signedIn: signedInMarkSvg() }));
"""
FOLD = json.loads(subprocess.check_output(['node', '--input-type=module', '-e', script], cwd=ROOT, text=True))
FOLD_BODY, FOLD_EAR = FOLD['body'], FOLD['ear']
FOLD_WIDTH, FOLD_HEIGHT = 526, 459
(ICONS / 'muniment-mark-auth.svg').write_text(FOLD['still'] + '\n')
(ICONS / 'muniment-mark-signed-in.svg').write_text(FOLD['signedIn'] + '\n')


def png(pixels, logical):
    # The symbol, reversed on the dark card as in the brand avatar, spans 62% of
    # the card width and sits centered on it.
    scale = 1024 * .617 / FOLD_WIDTH
    left, top = (1024 - FOLD_WIDTH * scale) / 2, (1024 - FOLD_HEIGHT * scale) / 2
    source = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{pixels}" height="{pixels}" viewBox="0 0 1024 1024">'
              f'<rect width="1024" height="1024" fill="#141716"/>'
              f'<g transform="translate({left:.2f} {top:.2f}) scale({scale:.5f})">'
              f'<path fill="#FFFFFF" d="{FOLD_BODY}"/><path fill="#2A7264" d="{FOLD_EAR}"/></g></svg>')
    raw = cairosvg.svg2png(bytestring=source.encode(), output_width=pixels * 4, output_height=pixels * 4)
    image = Image.open(io.BytesIO(raw)).convert('RGBA').resize((pixels, pixels), Image.Resampling.LANCZOS)
    output = io.BytesIO()
    image.save(output, format='PNG')
    return output.getvalue()


for name, (pixels, logical) in PNG_SIZES.items():
    (ICONS / name).write_bytes(png(pixels, logical))

# ICO holds separate reductions, not resizes of the largest graph.
ico_sizes = (16, 24, 32, 48, 64, 128, 256)
payloads = [png(size, size) for size in ico_sizes]
offset = 6 + 16 * len(payloads)
entries = []
for size, data in zip(ico_sizes, payloads):
    entries.append(struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(data), offset))
    offset += len(data)
(ICONS / 'icon.ico').write_bytes(struct.pack('<HHH', 0, 1, len(payloads)) + b''.join(entries) + b''.join(payloads))

# Let Apple's compiler encode the small representations for native icon readers.
# Each 1x and 2x representation still selects geometry by logical size.
with tempfile.TemporaryDirectory() as temporary:
    iconset = Path(temporary) / 'muniment.iconset'
    iconset.mkdir()
    for logical in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            suffix = '@2x' if scale == 2 else ''
            name = f'icon_{logical}x{logical}{suffix}.png'
            (iconset / name).write_bytes(png(logical * scale, logical))
    subprocess.run([ICONUTIL, '--convert', 'icns', str(iconset),
                    '--output', str(ICONS / 'icon.icns')], check=True)
print('Generated desktop PNG, ICO, ICNS, and sign-in SVG assets.')
