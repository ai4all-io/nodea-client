#!/usr/bin/env python3
"""Generate the Nodea app icon (matches frontend/src/components/BrandMark.tsx)."""
from PIL import Image, ImageDraw, ImageFont

SIZE = 1024
BG = (130, 113, 239, 255)  # HSL(248, 80%, 69%) == frontend --primary
FG = (255, 255, 255, 255)
RADIUS = 180

img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
draw = ImageDraw.Draw(img)
draw.rounded_rectangle([0, 0, SIZE - 1, SIZE - 1], radius=RADIUS, fill=BG)

font = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 560)
text = "N"
bbox = draw.textbbox((0, 0), text, font=font)
w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
draw.text(((SIZE - w) / 2 - bbox[0], (SIZE - h) / 2 - bbox[1]), text, font=font, fill=FG)

img.save("app-icon.png")
print("Created app-icon.png")
