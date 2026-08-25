from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parent
OUTPUT = ROOT / "app_icon.ico"
SIZE = 256

image = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
draw = ImageDraw.Draw(image)
draw.rounded_rectangle((7, 7, 249, 249), radius=54, fill="#101828")
draw.rounded_rectangle((48, 48, 208, 208), radius=34, fill="#155EEF")

font_path = Path(r"C:\Windows\Fonts\arialbd.ttf")
font = ImageFont.truetype(str(font_path), 118) if font_path.exists() else ImageFont.load_default()
box = draw.textbbox((0, 0), "O", font=font)
text_width = box[2] - box[0]
text_height = box[3] - box[1]
draw.text(
    ((SIZE - text_width) / 2, (SIZE - text_height) / 2 - box[1] - 2),
    "O", font=font, fill="#FFFFFF",
)
image.save(OUTPUT, format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48),
                                         (64, 64), (128, 128), (256, 256)])
print(OUTPUT)
