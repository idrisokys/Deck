"""Deck simgesi: kaynak görselden (Deck-icon-source.png) .icns ve 1024 px PNG üretir."""
import sys, io, struct
from PIL import Image, ImageFilter, ImageChops

src_path = sys.argv[1]
out_png = sys.argv[2]
out_icns = sys.argv[3]

im = Image.open(src_path).convert("RGB")

# 1) Siyah arka planı ayır: parlaklığa göre maske
lum = im.convert("L").point(lambda v: 255 if v > 14 else 0)
bbox = lum.getbbox()
print("şekil sınırları:", bbox)
crop = im.crop(bbox)
mask = lum.crop(bbox)

# kenardaki koyu halkayı kırp (2 px erozyon) + yumuşak kenar
mask = mask.filter(ImageFilter.MinFilter(5))
mask = mask.filter(ImageFilter.GaussianBlur(1.2))

# 2) macOS ikon ızgarası: 1024 tuval, 824 piksel gövde
BODY = 824
crop = crop.resize((BODY, BODY), Image.LANCZOS)
mask = mask.resize((BODY, BODY), Image.LANCZOS)

canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
off = (1024 - BODY) // 2

# gölge
shadow = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
sh_mask = Image.new("L", (1024, 1024), 0)
sh_mask.paste(mask, (off, off + 12))
sh_mask = sh_mask.filter(ImageFilter.GaussianBlur(14)).point(lambda v: int(v * 0.55))
shadow.putalpha(sh_mask)
canvas = Image.alpha_composite(canvas, shadow)

body = crop.convert("RGBA")
body.putalpha(mask)
canvas.alpha_composite(body, (off, off))
canvas.save(out_png)

# 3) .icns (PNG içerikli parçalar)
types = {"icp4": 16, "icp5": 32, "icp6": 64, "ic07": 128, "ic08": 256,
         "ic09": 512, "ic10": 1024, "ic11": 32, "ic12": 64, "ic13": 256, "ic14": 512}
chunks = b""
for code, px in types.items():
    buf = io.BytesIO()
    canvas.resize((px, px), Image.LANCZOS).save(buf, format="PNG")
    data = buf.getvalue()
    chunks += code.encode() + struct.pack(">I", len(data) + 8) + data
with open(out_icns, "wb") as f:
    f.write(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)
print("tamam")
