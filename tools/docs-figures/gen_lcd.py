"""元の C++ 版マニュアル用の本体 LCD (128x128) 再現図。

整理前の描画座標を記録した履歴図で、現在の Rust ソースは読み取らない。
参照元:
https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/src/main.cpp
現在の顔描画については rust/avatar-face/README.md を参照する。

- setTextSize(1) = 6x8px セル / setTextSize(2) = 12x16px セル
- 文字は 1 文字ずつセル中央に描いてレイアウトを実機と一致させる
"""
import os
from PIL import Image, ImageDraw
from gfx import mono, jp, arrow, badge, label, RED, PAPER

S = 6                      # 拡大率 128 -> 768
W = 128 * S
OUT = os.path.join(os.path.dirname(__file__), "out")
os.makedirs(OUT, exist_ok=True)

BLACK = (0, 0, 0)
WHITE = (255, 255, 255)
YELLOW = (255, 255, 0)
CYAN = (0, 255, 255)
GRAY = (150, 150, 150)
LINE = (70, 70, 70)
BAR = (220, 110, 0)


def new_lcd():
    img = Image.new("RGB", (W, W), BLACK)
    return img, ImageDraw.Draw(img)


def put(d, x, y, text, color, size=1, bg=None):
    """M5.Lcd.setCursor(x,y); print(text) と同じ位置に描く"""
    cw, ch = 6 * size, 8 * size
    f = mono(int(ch * S * 0.98))
    for i, ch_ in enumerate(text):
        cx, cy = (x + i * cw) * S, y * S
        if bg is not None:
            d.rectangle([cx, cy, cx + cw * S - 1, cy + ch * S - 1], fill=bg)
        d.text((cx + cw * S / 2, cy + ch * S / 2), ch_, font=f, fill=color, anchor="mm")


def bezel(lcd, caption=None, cap_color=(60, 60, 66)):
    """LCD を本体風のベゼルで囲む"""
    pad = 34
    cap_h = 74 if caption else 0
    img = Image.new("RGB", (W + pad * 2, W + pad * 2 + cap_h), (250, 250, 252))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, W + pad * 2 - 1, W + pad * 2 - 1], radius=40,
                        fill=(38, 38, 42))
    img.paste(lcd, (pad, pad))
    d.rectangle([pad - 2, pad - 2, pad + W + 1, pad + W + 1], outline=(90, 90, 96), width=2)
    if caption:
        d.text(((W + pad * 2) / 2, W + pad * 2 + cap_h / 2 - 4), caption,
               font=jp(34, bold=True), fill=cap_color, anchor="mm")
    return img


# ---------------------------------------------------------------- 1) 接続中
def connecting(dots=8, ssid="aterm-a1b2c3-g"):
    img, d = new_lcd()
    put(d, 2, 4, "Connecting WiFi", WHITE)
    put(d, 2, 16, ssid[:20], CYAN)
    put(d, 2, 34, "." * dots, WHITE)
    d.rectangle([0, 60 * S, W, 60 * S + S - 1], fill=LINE)
    put(d, 2, 68, "Press screen 1s", YELLOW)
    put(d, 2, 78, "-> WiFi SETUP", YELLOW)
    put(d, 2, 100, "or wait 15s for", GRAY)
    put(d, 2, 110, "auto setup mode", GRAY)
    return img


# ------------------------------------------------------------- 2) SETUP MODE
def setup_mode():
    img, d = new_lcd()
    d.rectangle([0, 0, W, 20 * S - 1], fill=BAR)
    put(d, 3, 2, "SETUP MODE", WHITE, size=2, bg=BAR)
    put(d, 2, 26, "1) Connect WiFi", YELLOW)
    put(d, 8, 38, "ChibiStackChan", WHITE)
    put(d, 8, 47, "-Setup", WHITE)
    put(d, 8, 56, "pass: stackchan", WHITE)
    put(d, 2, 72, "2) Open browser", YELLOW)
    put(d, 8, 84, "http://192.168.4.1", CYAN)
    d.rectangle([0, 100 * S, W, 100 * S + S - 1], fill=LINE)
    put(d, 2, 106, "Save -> auto reboot", GRAY)
    put(d, 2, 116, "Quit = unplug USB", GRAY)
    return img


# ---------------------------------------------------------------- 3) IP 表示
def ip_screen(ip="192.168.0.5"):
    img, d = new_lcd()
    dot2 = ip.index(".", ip.index(".") + 1)
    put(d, 0, 16, "IP:", WHITE, size=2)
    put(d, 0, 32, ip[:dot2 + 1], WHITE, size=2)
    put(d, 0, 48, ip[dot2 + 1:], WHITE, size=2)
    put(d, 0, 72, " chibi-chan.local", WHITE)
    return img


# ------------------------------------------------------- 4) 顔 (待機中の画面)
def face(bg=BLACK, caption=None):
    img, d = new_lcd()
    d.rectangle([0, 0, W, W], fill=bg)
    eye_y = 52 * S
    for ex in (40 * S, 88 * S):
        d.ellipse([ex - 13 * S, eye_y - 13 * S, ex + 13 * S, eye_y + 13 * S], fill=WHITE)
    d.arc([48 * S, 78 * S, 80 * S, 100 * S], 20, 160, fill=WHITE, width=int(3.2 * S))
    return img


if __name__ == "__main__":
    for name, im, cap in [
        ("lcd-connecting", connecting(), "① 起動直後：Wi-Fi をさがしています"),
        ("lcd-setup-mode", setup_mode(), "② SETUP MODE（はじめの設定）"),
        ("lcd-ip", ip_screen(), "③ つながった：この番号が住所です"),
        ("lcd-face", face(), "④ 顔が出たら準備完了"),
    ]:
        bezel(im, cap).save(os.path.join(OUT, name + ".png"))
        print("wrote", name)
