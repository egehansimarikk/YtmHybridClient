#!/usr/bin/env python3
"""
Create an .icns file from a PNG image for macOS application bundling.
Requires PIL/Pillow: pip install Pillow
"""

import sys
import io
from PIL import Image


def create_icns(png_path, icns_path):
    """Create an ICNS file from a PNG image."""

    # ICNS requires specific sizes (the format uses internal type codes)
    # Size mapping: (png_size, icns_type)
    sizes = [
        (1024, b'ic15'),  # 1024x1024 @2x
        (512, b'ic12'),   # 512x512
        (512, b'ic05'),   # 512x512 @2x (fallback)
        (256, b'ic13'),   # 256x256
        (256, b'ic14'),   # 256x256 @2x
        (128, b'ic06'),   # 128x128
        (64, b'ic07'),    # 64x64
        (32, b'ic08'),    # 32x32
        (16, b'h16 '),    # 16x16
    ]

    # Open and prepare the source image
    img = Image.open(png_path)

    # Convert RGBA to RGB with white background
    if img.mode == 'RGBA':
        background = Image.new('RGB', img.size, (255, 255, 255))
        background.paste(img, mask=img.split()[3])
        img = background
    elif img.mode != 'RGB':
        img = img.convert('RGB')

    # Build ICNS file
    icns_data = io.BytesIO()
    icns_data.write(b'ICNS')  # Header

    pos = 8  # Start after header (4 bytes type + 4 bytes size)

    for size, icon_type in sizes:
        # Create resized version
        resized = img.copy()
        resized.thumbnail((size, size), Image.Resampling.LANCZOS)

        # Save to buffer
        temp_buf = io.BytesIO()
        resized.save(temp_buf, format='PNG')
        icon_data = temp_buf.getvalue()

        # Write ICNS entry: type (4 bytes) + size (4 bytes) + data
        entry_size = 8 + len(icon_data)
        icns_data.write(icon_type)
        icns_data.write(entry_size.to_bytes(4, 'big'))
        icns_data.write(icon_data)
        pos += entry_size

    # Write final file
    with open(icns_path, 'wb') as f:
        f.write(icns_data.getvalue())

    print(f"Created {icns_path} ({pos} bytes)")


if __name__ == '__main__':
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <input.png> <output.icns>")
        sys.exit(1)

    create_icns(sys.argv[1], sys.argv[2])
