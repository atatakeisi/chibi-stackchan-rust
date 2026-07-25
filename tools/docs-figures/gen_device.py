"""本体のボタン位置の説明図 (イメージ図)"""
import os
from PIL import Image, ImageDraw
from gfx import jp, arrow, badge, label, RED, ORANGE, INK, PAPER

OUT = os.path.join(os.path.dirname(__file__), "out")
os.makedirs(OUT, exist_ok=True)

BG = (250, 250, 252)
BODY = (54, 54, 60)
BODY_D = (34, 34, 38)
EDGE = (120, 120, 128)


def face_on(d, box, bg=(0, 0, 0)):
    x0, y0, x1, y1 = box
    d.rectangle(box, fill=bg)
    w, h = x1 - x0, y1 - y0
    ey = y0 + h * 0.40
    r = w * 0.105
    for fx in (0.31, 0.69):
        ex = x0 + w * fx
        d.ellipse([ex - r, ey - r, ex + r, ey + r], fill=PAPER)
    d.arc([x0 + w * 0.36, y0 + h * 0.58, x0 + w * 0.64, y0 + h * 0.80],
          20, 160, fill=PAPER, width=int(w * 0.035))


def legend(d, y, n, title, body, x=90, w=1220):
    """番号バッジ + 見出し + 本文 の 1 行"""
    badge(d, (x + 30, y + 30), n, r=30)
    d.text((x + 82, y + 8), title, font=jp(38, bold=True), fill=INK)
    for i, line in enumerate(body.split("\n")):
        d.text((x + 82, y + 62 + i * 44), line, font=jp(31), fill=(70, 70, 80))
    return y + 62 + len(body.split("\n")) * 44 + 26


def build():
    W, H = 1400, 1320
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)

    d.text((W / 2, 54), "本体のボタンは 2 つだけ", font=jp(54, bold=True),
           fill=INK, anchor="mm")
    d.text((W / 2, 108), "※ 形はイメージ図です。ボタンの「位置」を見てください",
           font=jp(28), fill=(130, 130, 140), anchor="mm")

    # ---------------- 正面 ----------------
    fx0, fy0, fw = 230, 220, 400
    d.text((fx0 + fw / 2, fy0 - 36), "正面", font=jp(36, bold=True), fill=INK, anchor="mm")
    d.rounded_rectangle([fx0, fy0, fx0 + fw, fy0 + fw + 96], radius=34, fill=BODY_D)
    d.rounded_rectangle([fx0, fy0, fx0 + fw, fy0 + fw], radius=34, fill=BODY)
    sc = (fx0 + 56, fy0 + 56, fx0 + fw - 56, fy0 + fw - 56)
    d.rounded_rectangle([sc[0] - 6, sc[1] - 6, sc[2] + 6, sc[3] + 6], radius=10, fill=(10, 10, 12))
    face_on(d, sc)
    for i in range(5):
        y = fy0 + fw + 26 + i * 12
        d.rounded_rectangle([fx0 + 86, y, fx0 + fw - 86, y + 5], radius=3, fill=(72, 72, 78))

    # 「面ぜんぶが押せる」ことを示す破線枠 + 奥へ押す矢印
    d.rounded_rectangle([fx0 + 14, fy0 + 14, fx0 + fw - 14, fy0 + fw - 14],
                        radius=24, outline=ORANGE, width=6)
    badge(d, (fx0 + 14, fy0 + 14), "1", r=32)
    d.text((fx0 + fw / 2, fy0 + fw + 122), "この面ぜんぶがボタン",
           font=jp(32, bold=True), fill=ORANGE, anchor="mm")

    # ---------------- 左側面 ----------------
    sx0, sy0, sh = 900, 220, 400
    d.text((sx0 + 75, sy0 - 36), "左側面", font=jp(36, bold=True), fill=INK, anchor="mm")
    d.rounded_rectangle([sx0, sy0, sx0 + 150, sy0 + sh + 96], radius=28, fill=BODY_D)
    d.rounded_rectangle([sx0, sy0, sx0 + 150, sy0 + sh], radius=28, fill=BODY)
    bx, by = sx0 + 75, sy0 + 130
    d.ellipse([bx - 22, by - 22, bx + 22, by + 22], fill=(205, 205, 212), outline=EDGE, width=3)
    arrow(d, (bx + 210, by - 96), (bx + 34, by - 14), width=7, head=22)
    badge(d, (bx + 236, by - 110), "2", r=30)

    ux, uy = sx0 + 75, sy0 + sh + 52
    d.rounded_rectangle([ux - 44, uy - 13, ux + 44, uy + 13], radius=13,
                        fill=(16, 16, 18), outline=EDGE, width=3)
    arrow(d, (ux + 210, uy + 96), (ux + 52, uy + 10), width=7, head=22)
    badge(d, (ux + 236, uy + 110), "3", r=30)

    # ---------------- 凡例 ----------------
    d.line([(90, 800), (W - 90, 800)], fill=(215, 215, 222), width=3)
    y = 840
    y = legend(d, y, "1", "画面ぜんたいが「押しボタン」",
               "スマホのように なでても・軽く触れても反応しません。\n"
               "本体ごと奥へ「カチッ」と手ごたえがあるまで押し込みます。")
    y = legend(d, y, "2", "小さな丸ボタン ＝ リセット（再起動）",
               "固まったときだけ、短く 1 回ポチッと押します。\n"
               "長く押し続けると画面が真っ暗になります（もう一度短く押せば戻ります）。")
    y = legend(d, y, "3", "USB-C ＝ 電源",
               "スマホの充電器でOK。電源スイッチはありません。挿せば起動します。")
    return img


if __name__ == "__main__":
    build().save(os.path.join(OUT, "device-buttons.png"))
    print("wrote device-buttons")
