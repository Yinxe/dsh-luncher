#!/usr/bin/env python3
"""生成 DSH Starter 的 1024x1024 应用图标源图（深色圆角方块 + 终端提示符 + 波形）。"""
from PIL import Image, ImageDraw

SIZE = 1024
img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# 圆角方块背景
d.rounded_rectangle([32, 32, SIZE - 32, SIZE - 32], radius=200, fill=(13, 17, 28, 255))

# 渐变描边（简化：双层圆角矩形模拟）
d.rounded_rectangle([32, 32, SIZE - 32, SIZE - 32], radius=200, outline=(77, 107, 254, 90), width=14)

# 终端提示符 "❯"：两段折线
blue = (77, 107, 254, 255)
light = (126, 144, 255, 255)
# 折线 1：从 (250,300) 到 (470,512) 到 (250,724)
d.line([(250, 300), (470, 512)], fill=light, width=88)
d.line([(470, 512), (250, 724)], fill=blue, width=88)
# 圆角线帽
for (cx, cy) in [(250, 300), (250, 724)]:
    d.ellipse([cx - 44, cy - 44, cx + 44, cy + 44], fill=light if cy == 300 else blue)

# 光标下划线
d.rounded_rectangle([560, 680, 790, 768], radius=44, fill=(230, 234, 242, 255))

# 右上小圆点（表示"正在运行"）
d.ellipse([718, 218, 794, 294], fill=(52, 211, 153, 255))
d.ellipse([730, 230, 782, 282], fill=(13, 17, 28, 255))
d.ellipse([744, 244, 768, 268], fill=(52, 211, 153, 255))

img.save("src-tauri/icons/app-icon.png")
print("icon written:", img.size)
