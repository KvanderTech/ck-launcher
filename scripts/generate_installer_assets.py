from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
LOGO = Image.open(ROOT / "app/src/assets/logo-transparent.png").convert("RGBA")
OUT = ROOT / "app/src-tauri/installer"
OUT.mkdir(parents=True, exist_ok=True)


def gradient(size: tuple[int, int]) -> Image.Image:
    image = Image.new("RGB", size)
    pixels = image.load()
    for y in range(size[1]):
        for x in range(size[0]):
            t = (x / max(size[0] - 1, 1)) * .65 + (1 - y / max(size[1] - 1, 1)) * .35
            pixels[x, y] = (int(5 + 3 * t), int(16 + 65 * t), int(34 + 116 * t))
    return image


def logo_mark(size: int) -> Image.Image:
    mark = LOGO.resize((size, size), Image.Resampling.LANCZOS)
    return mark


sidebar = gradient((164, 314))
sidebar.paste(logo_mark(112), (26, 42), logo_mark(112))
draw = ImageDraw.Draw(sidebar)
font = ImageFont.truetype("arialbd.ttf", 18)
small = ImageFont.truetype("arial.ttf", 10)
draw.text((22, 178), "ЦК Лаунчер", font=font, fill=(246, 250, 255))
draw.text((22, 207), "Твой мир — твои правила", font=small, fill=(176, 201, 227))
draw.rounded_rectangle((22, 250, 142, 282), radius=8, fill=(19, 145, 238))
draw.text((48, 259), "УСТАНОВКА", font=small, fill=(255, 255, 255))
sidebar.save(OUT / "sidebar.bmp", format="BMP")

header = gradient((150, 57))
header.paste(logo_mark(48), (8, 4), logo_mark(48))
draw = ImageDraw.Draw(header)
draw.text((60, 14), "ЦК Лаунчер", font=ImageFont.truetype("arialbd.ttf", 13), fill=(255, 255, 255))
draw.text((60, 33), "Установка", font=ImageFont.truetype("arial.ttf", 9), fill=(171, 205, 235))
header.save(OUT / "header.bmp", format="BMP")
