"""スマホ側の操作画面のイメージ図 (iPhone 風)

body(d) は「画面内の座標」で描き、注釈に使う矩形を dict で返す。
注釈 (赤枠・番号・吹き出し) はベゼル分オフセットして外側から重ねる。
"""
import os
from PIL import Image, ImageDraw
from gfx import (jp, mono, arrow, badge, label, ring, rect,
                 RED, ORANGE, GREEN, BLUE, INK, PAPER)

OUT = os.path.join(os.path.dirname(__file__), "out")
os.makedirs(OUT, exist_ok=True)

PW, PH = 660, 1180
BEZ = 22
BG = (250, 250, 252)
CARD = (28, 28, 30)
SEP = (58, 58, 60)
TXT = (255, 255, 255)
SUB = (152, 152, 157)
IOSBLUE = (10, 132, 255)
ROW = 82


# ---------------------------------------------------------------- アイコン描画
def ic_wifi(d, cx, cy, s=14, color=TXT):
    for i in (3, 2, 1):
        r = s * i / 3
        d.arc([cx - r, cy - r + s * 0.55, cx + r, cy + r + s * 0.55],
              215, 325, fill=color, width=max(2, int(s * 0.16)))
    d.ellipse([cx - s * 0.13, cy + s * 0.42, cx + s * 0.13, cy + s * 0.68], fill=color)


def ic_lock(d, cx, cy, s=15, color=TXT):
    d.arc([cx - s * 0.36, cy - s * 0.85, cx + s * 0.36, cy + s * 0.05],
          180, 360, fill=color, width=max(2, int(s * 0.16)))
    d.rounded_rectangle([cx - s * 0.5, cy - s * 0.1, cx + s * 0.5, cy + s * 0.7],
                        radius=3, fill=color)


def ic_check(d, cx, cy, s=14, color=IOSBLUE):
    d.line([(cx - s * 0.6, cy), (cx - s * 0.15, cy + s * 0.45)], fill=color, width=4)
    d.line([(cx - s * 0.15, cy + s * 0.45), (cx + s * 0.6, cy - s * 0.5)], fill=color, width=4)


def ic_info(d, cx, cy, s=15, color=IOSBLUE):
    d.ellipse([cx - s, cy - s, cx + s, cy + s], outline=color, width=3)
    d.ellipse([cx - 2, cy - s * 0.55, cx + 2, cy - s * 0.55 + 4], fill=color)
    d.line([(cx, cy - s * 0.15), (cx, cy + s * 0.55)], fill=color, width=3)


def statusbar(d):
    d.text((44, 34), "10:10", font=jp(26, bold=True), fill=TXT, anchor="lm")
    x = PW - 56
    d.rounded_rectangle([x, 24, x + 40, 44], radius=5, outline=TXT, width=3)
    d.rounded_rectangle([x + 4, 28, x + 30, 40], radius=2, fill=TXT)
    ic_wifi(d, x - 34, 26, 15)
    for i in range(4):
        h = 6 + i * 5
        d.rounded_rectangle([x - 90 + i * 11, 44 - h, x - 84 + i * 11, 44], radius=2, fill=TXT)


def navbar(d, title, back=None, right=None, y=72):
    if back:
        d.text((26, y + 26), "‹ " + back, font=jp(28), fill=IOSBLUE, anchor="lm")
    if right:
        d.text((PW - 26, y + 26), right, font=jp(29, bold=True), fill=IOSBLUE, anchor="rm")
    d.text((PW / 2, y + 26), title, font=jp(31, bold=True), fill=TXT, anchor="mm")
    return y + 66


def header(d, y, text):
    d.text((46, y), text, font=jp(23), fill=SUB)
    return y + 40


def rows(d, y, items, fill=CARD):
    """items = [(name, icons)] icons: 'lock','wifi','check','info' の並び"""
    h = ROW * len(items)
    d.rounded_rectangle([22, y, PW - 22, y + h], radius=18, fill=fill)
    boxes = []
    for i, (name, icons) in enumerate(items):
        ry = y + ROW * i
        if i:
            d.line([(58, ry), (PW - 22, ry)], fill=SEP, width=2)
        d.text((58, ry + ROW / 2), name, font=jp(29), fill=TXT, anchor="lm")
        ix = PW - 56
        for ic in reversed(icons):
            {"lock": ic_lock, "wifi": ic_wifi, "check": ic_check, "info": ic_info}[ic](
                d, ix, ry + ROW / 2)
            ix -= 46
        boxes.append((22, ry, PW - 22, ry + ROW))
    return y + h + 14, boxes


def render(body, caption, annotate=None, ph=None):
    global PH
    PH = ph or PH
    sc = Image.new("RGB", (PW, PH), (0, 0, 0))
    d = ImageDraw.Draw(sc)
    marks = body(d) or {}
    cap_h = 74 if caption else 0
    img = Image.new("RGB", (PW + BEZ * 2, PH + BEZ * 2 + cap_h), BG)
    dd = ImageDraw.Draw(img)
    dd.rounded_rectangle([0, 0, PW + BEZ * 2 - 1, PH + BEZ * 2 - 1], radius=54, fill=(30, 30, 34))
    img.paste(sc, (BEZ, BEZ))
    if caption:
        dd.text(((PW + BEZ * 2) / 2, PH + BEZ * 2 + cap_h / 2 - 6), caption,
                font=jp(30, bold=True), fill=(70, 70, 80), anchor="mm")
    if annotate:
        annotate(dd, {k: tuple(v[i] + BEZ for i in range(4)) for k, v in marks.items()})
    return img


def note(d, y, text, color=RED, fsize=27):
    """画面下の空きスペースに置く吹き出し (画面幅いっぱい)"""
    return label(d, (BEZ + PW / 2, y), text, fsize=fsize, bg=color,
                 anchor="ct", pad=(22, 14))


# ============================================================ ① Wi-Fi 一覧
def wifi_list():
    def body(d):
        statusbar(d)
        y = navbar(d, "Wi-Fi", back="設定")
        y += 10
        d.rounded_rectangle([22, y, PW - 22, y + ROW], radius=18, fill=CARD)
        d.text((58, y + ROW / 2), "Wi-Fi", font=jp(29), fill=TXT, anchor="lm")
        d.rounded_rectangle([PW - 132, y + 18, PW - 46, y + ROW - 18], radius=23,
                            fill=(52, 199, 89))
        d.ellipse([PW - 90, y + 20, PW - 48, y + ROW - 20], fill=PAPER)
        y += ROW + 30
        y = header(d, y, "ネットワーク")
        y, bx = rows(d, y, [
            ("ChibiStackChan-Setup", ["wifi"]),
            ("aterm-a1b2c3-g", ["lock", "wifi"]),
            ("aterm-a1b2c3-a", ["lock", "wifi"]),
            ("Buffalo-2G-1A2B", ["lock", "wifi"]),
        ])
        return {"target": bx[0], "home": bx[1]}

    def ann(d, m):
        x0, y0, x1, y1 = m["target"]
        rect(d, (x0 - 5, y0 - 5, x1 + 5, y1 + 5), RED, width=6, radius=20)
        badge(d, (x0 - 2, (y0 + y1) / 2), "1", r=27)
        arrow(d, (BEZ + PW / 2, y1 + 250), (BEZ + PW / 2, y1 + 16), width=7, head=22)
        note(d, y1 + 262,
             "「ChibiStackChan-Setup」を選びます。\n"
             "出てこないときは、Wi-Fi のスイッチを\n"
             "一度 OFF → すぐ ON に戻すと出ます。")

    return render(body, "① スマホの「設定 › Wi-Fi」を開く", ann, ph=900)


# ======================================================== ② パスワード入力
def wifi_pass():
    def body(d):
        statusbar(d)
        y = navbar(d, "パスワードを入力", back="キャンセル", right="接続")
        d.text((PW / 2, y + 40), "ChibiStackChan-Setup", font=jp(27, bold=True),
               fill=SUB, anchor="mm")
        y += 92
        d.rounded_rectangle([22, y, PW - 22, y + ROW], radius=18, fill=CARD)
        d.text((58, y + ROW / 2), "パスワード", font=jp(27), fill=SUB, anchor="lm")
        d.text((PW - 46, y + ROW / 2), "stackchan", font=mono(31), fill=TXT, anchor="rm")
        pw_box = (22, y, PW - 22, y + ROW)
        ky = PH - 400
        d.rectangle([0, ky, PW, PH], fill=(38, 38, 40))
        for r, keys in enumerate(["qwertyuiop", "asdfghjkl", "zxcvbnm"]):
            kw = 58
            ox = (PW - kw * len(keys)) / 2
            for i, k in enumerate(keys):
                kx = ox + i * kw
                d.rounded_rectangle([kx + 4, ky + 26 + r * 86, kx + kw - 4, ky + 98 + r * 86],
                                    radius=10, fill=(90, 90, 94))
                d.text((kx + kw / 2, ky + 62 + r * 86), k, font=jp(29), fill=TXT, anchor="mm")
        return {"pw": pw_box, "go": (PW - 130, 72, PW - 16, 72 + 52)}

    def ann(d, m):
        x0, y0, x1, y1 = m["pw"]
        rect(d, (x0 - 5, y0 - 5, x1 + 5, y1 + 5), RED, width=6, radius=20)
        badge(d, (x0 - 2, (y0 + y1) / 2), "2", r=27)
        ring(d, m["go"], RED, width=6)
        arrow(d, (BEZ + PW / 2, y1 + 250), (BEZ + PW / 2, y1 + 16), width=7, head=22)
        note(d, y1 + 262,
             "stackchan と半角小文字で入力し、\n"
             "右上の「接続」を押します。")

    return render(body, "② パスワードは stackchan", ann, ph=1180)


# =========================================== ③「インターネット未接続」は正常
def wifi_no_internet():
    def body(d):
        statusbar(d)
        y = navbar(d, "Wi-Fi", back="設定")
        y += 10
        y = header(d, y, "接続済み")
        y, bx = rows(d, y, [("ChibiStackChan-Setup", ["check", "info"])])
        warn_y = y
        d.text((46, y), "インターネットに接続していません", font=jp(23), fill=SUB)
        y += 46
        y = header(d, y, "ネットワーク")
        y, _ = rows(d, y, [("aterm-a1b2c3-g", ["lock", "wifi"]),
                           ("aterm-a1b2c3-a", ["lock", "wifi"])])
        return {"conn": bx[0], "warn": (40, warn_y - 6, PW - 120, warn_y + 34)}

    def ann(d, m):
        rect(d, m["warn"], ORANGE, width=5, radius=12)
        x0, y0, x1, y1 = m["conn"]
        badge(d, (x0 - 2, (y0 + y1) / 2), "3", r=27)
        yy = m["warn"][3] + 300
        arrow(d, (BEZ + PW / 2, yy - 12), (BEZ + PW / 2, m["warn"][3] + 8), width=7, head=22)
        note(d, yy,
             "この警告が出て正常です。\n"
             "設定が終わるまで、スマホを\n"
             "家の Wi-Fi に戻さないでください。", color=(200, 110, 0))

    return render(body, "③「インターネット未接続」で正常", ann, ph=900)


# ========================================================== ④ ブラウザで開く
def safari_url():
    def body(d):
        statusbar(d)
        d.rounded_rectangle([22, 90, PW - 22, 168], radius=20, fill=(58, 58, 60))
        d.text((46, 129), "192.168.4.1", font=mono(32), fill=TXT, anchor="lm")
        d.text((PW - 46, 129), "開く", font=jp(28, bold=True), fill=IOSBLUE, anchor="rm")
        y = 206
        y = header(d, y, "このページを開く")
        y, bx = rows(d, y, [("192.168.4.1", [])])
        y += 16
        y = header(d, y, "Google 検索")
        d.rounded_rectangle([22, y, PW - 22, y + ROW], radius=18, fill=(20, 20, 22))
        d.text((58, y + ROW / 2), "192.168.4.1 とは", font=jp(28), fill=SUB, anchor="lm")
        search = (22, y, PW - 22, y + ROW)
        return {"open": bx[0], "search": search, "go": (PW - 116, 96, PW - 20, 162)}

    def ann(d, m):
        x0, y0, x1, y1 = m["open"]
        rect(d, (x0 - 5, y0 - 5, x1 + 5, y1 + 5), RED, width=6, radius=20)
        badge(d, (x0 - 2, (y0 + y1) / 2), "4", r=27)
        ring(d, m["go"], RED, width=5)
        sx0, sy0, sx1, sy1 = m["search"]
        yy = sy1 + 300
        arrow(d, (BEZ + PW / 2, yy - 12), (BEZ + PW / 2, y1 + 12), width=7, head=22)
        note(d, yy,
             "検索候補ではなく、この行\n"
             "（または右上の「開く」）をタップ。\n"
             "数字と「.」はすべて半角で。")

    return render(body, "④ ブラウザに 192.168.4.1 と入れる", ann, ph=1020)


# ===================================================== ⑤ 2.4GHz の見分け方
def ssid_24g():
    W, H = 1400, 760
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    d.text((W / 2, 58), "入力するのは 2.4GHz の Wi-Fi です",
           font=jp(50, bold=True), fill=INK, anchor="mm")
    d.text((W / 2, 116), "Wi-Fi 一覧に、末尾だけ違う似た名前が 2 つ並んでいたら要注意",
           font=jp(28), fill=(120, 120, 130), anchor="mm")
    items = [
        ("aterm-a1b2c3-g", True, "末尾が -g / -G → これを入力"),
        ("aterm-a1b2c3-a", False, "末尾が -a / -A → つながりません"),
        ("Buffalo-2G-1A2B", True, "2G / 2.4 付き → これを入力"),
        ("Buffalo-5G-1A2B", False, "5G 付き → つながりません"),
    ]
    y = 186
    for name, ok, note_ in items:
        col = (233, 246, 237) if ok else (253, 234, 232)
        line = GREEN if ok else RED
        d.rounded_rectangle([90, y, W - 90, y + 108], radius=18, fill=col, outline=line, width=4)
        mx, my, mr = 158, y + 54, 24
        if ok:
            d.ellipse([mx - mr, my - mr, mx + mr, my + mr], outline=line, width=6)
        else:
            d.line([(mx - mr, my - mr), (mx + mr, my + mr)], fill=line, width=7)
            d.line([(mx - mr, my + mr), (mx + mr, my - mr)], fill=line, width=7)
        d.text((212, y + 54), name, font=mono(36, bold=True), fill=INK, anchor="lm")
        d.text((W - 132, y + 54), note_, font=jp(29, bold=True), fill=line, anchor="rm")
        y += 128
    d.text((W / 2, H - 48), "※ 似た名前が 1 つしか無ければ、それがそのまま 2.4GHz です",
           font=jp(28), fill=(120, 120, 130), anchor="mm")
    return img


if __name__ == "__main__":
    for name, im in [
        ("phone-wifi-list", wifi_list()),
        ("phone-wifi-pass", wifi_pass()),
        ("phone-no-internet", wifi_no_internet()),
        ("phone-safari-url", safari_url()),
        ("ssid-24ghz", ssid_24g()),
    ]:
        im.save(os.path.join(OUT, name + ".png"))
        print("wrote", name)
