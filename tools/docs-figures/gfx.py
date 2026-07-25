"""共通ヘルパ: フォント・矢印・番号バッジ・吹き出しラベル"""
import glob
import math
from PIL import Image, ImageDraw, ImageFont

def _find(*patterns):
    """フォントを探す。macOS の追加フォントはハッシュ付きパスに入るので glob で拾う。"""
    for pat in patterns:
        hits = sorted(glob.glob(pat, recursive=True))
        if hits:
            return hits[0]
    raise SystemExit("フォントが見つかりません: " + " / ".join(patterns))


_ASSETS = "/System/Library/AssetsV2/**"
JP_M = _find(f"{_ASSETS}/YuGothic-Medium.otf",
             "/System/Library/Fonts/Hiragino Sans*.ttc",
             "/System/Library/Fonts/Supplemental/Arial Unicode.ttf")
JP_B = _find(f"{_ASSETS}/YuGothic-Bold.otf",
             "/System/Library/Fonts/Hiragino Sans*.ttc",
             "/System/Library/Fonts/Supplemental/Arial Unicode.ttf")
MONO = _find("/System/Library/Fonts/Menlo.ttc",
             "/System/Library/Fonts/Monaco.ttf")

_cache = {}


def font(path, size, index=0):
    k = (path, size, index)
    if k not in _cache:
        _cache[k] = ImageFont.truetype(path, size, index=index)
    return _cache[k]


def jp(size, bold=False):
    return font(JP_B if bold else JP_M, size)


def mono(size, bold=False):
    return font(MONO, size, index=1 if bold else 0)


RED = (232, 48, 32)
ORANGE = (240, 140, 0)
GREEN = (34, 170, 90)
BLUE = (45, 108, 255)
INK = (24, 24, 28)
PAPER = (255, 255, 255)


def arrow(d, start, end, color=RED, width=7, head=26):
    """start から end へ矢印。end 側に矢じり。"""
    d.line([start, end], fill=color, width=width)
    ang = math.atan2(end[1] - start[1], end[0] - start[0])
    for s in (+1, -1):
        a = ang + s * math.radians(28) + math.pi
        d.line([end, (end[0] + head * math.cos(a), end[1] + head * math.sin(a))],
               fill=color, width=width)


def badge(d, center, text, r=26, fill=RED, fg=PAPER, fsize=None):
    """番号バッジ (丸に数字)"""
    x, y = center
    d.ellipse([x - r, y - r, x + r, y + r], fill=fill, outline=PAPER, width=3)
    f = jp(fsize or int(r * 1.25), bold=True)
    d.text((x, y - r * 0.06), str(text), font=f, fill=fg, anchor="mm")


def ring(d, box, color=RED, width=7):
    """強調用の楕円"""
    d.ellipse(box, outline=color, width=width)


def rect(d, box, color=RED, width=7, radius=14):
    d.rounded_rectangle(box, radius=radius, outline=color, width=width)


def cross(d, box, color=RED, width=9):
    """「触らない」の×印"""
    x0, y0, x1, y1 = box
    d.line([(x0, y0), (x1, y1)], fill=color, width=width)
    d.line([(x0, y1), (x1, y0)], fill=color, width=width)


def label(d, xy, text, fsize=30, color=PAPER, bg=RED, pad=(16, 10), anchor="lt",
          bold=True, radius=12, maxw=None):
    """背景付きテキスト。改行対応。戻り値: (x0,y0,x1,y1)"""
    f = jp(fsize, bold=bold)
    lines = text.split("\n")
    lh = int(fsize * 1.45)
    w = max(d.textlength(s, font=f) for s in lines)
    h = lh * len(lines)
    x, y = xy
    if anchor[0] == "r":
        x -= w + pad[0] * 2
    elif anchor[0] == "c":
        x -= (w + pad[0] * 2) / 2
    if anchor[1] == "b":
        y -= h + pad[1] * 2
    elif anchor[1] == "m":
        y -= (h + pad[1] * 2) / 2
    box = (x, y, x + w + pad[0] * 2, y + h + pad[1] * 2)
    if bg is not None:
        d.rounded_rectangle(box, radius=radius, fill=bg)
    for i, s in enumerate(lines):
        d.text((x + pad[0], y + pad[1] + i * lh + lh * 0.5), s,
               font=f, fill=color, anchor="lm")
    return box


def extend(img, right=0, left=0, top=0, bottom=0, bg=(255, 255, 255)):
    """注釈スペースを外側に足す。戻り値: (新画像, 元画像の左上オフセット)"""
    w, h = img.size
    out = Image.new("RGB", (w + left + right, h + top + bottom), bg)
    out.paste(img, (left, top))
    return out, (left, top)
