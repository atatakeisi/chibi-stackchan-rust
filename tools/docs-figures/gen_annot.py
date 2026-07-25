"""既存の Web UI スクリーンショットに、番号・枠・矢印・説明を重ねる"""
import os
from PIL import Image, ImageDraw
from gfx import jp, arrow, badge, rect, cross, RED, ORANGE, GREEN, INK

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(REPO, "docs", "images")
OUT = os.path.join(os.path.dirname(__file__), "out")
os.makedirs(OUT, exist_ok=True)
BG = (250, 250, 252)


def annotate(src, marks, panel_w=680, title=None, scale=1.0, cut=None):
    """marks = [(番号 or None, (fx0,fy0,fx1,fy1), 色, 見出し, 本文)]
    枠は元画像の相対座標 (0..1)。説明は右側パネルに、対象の高さに合わせて置く。
    cut=(f1,f2) を渡すと、その区間 (無地の余白) を詰めて縦を短くする。"""
    base = Image.open(os.path.join(SRC, src)).convert("RGB")
    if scale != 1.0:
        base = base.resize((int(base.width * scale), int(base.height * scale)), Image.LANCZOS)
    bw, bh = base.size
    if cut:
        y1, y2 = int(bh * cut[0]), int(bh * cut[1])
        spliced = Image.new("RGB", (bw, bh - (y2 - y1) + 18))
        spliced.paste(base.crop((0, 0, bw, y1)), (0, 0))
        spliced.paste(base.crop((0, y2, bw, bh)), (0, y1 + 18))
        base, bh_orig = spliced, bh
        bw, bh = base.size
    top = 74 if title else 24
    img = Image.new("RGB", (bw + panel_w + 48, bh + top + 24), BG)
    img.paste(base, (24, top))
    d = ImageDraw.Draw(img)
    if title:
        d.text((24, 26), title, font=jp(38, bold=True), fill=INK)

    if cut:                                # 切り詰めた位置に破線を引く
        yy = top + int(bh_orig * cut[0]) + 9
        for xx in range(24, 24 + bw, 26):
            d.line([(xx, yy), (xx + 14, yy)], fill=(190, 190, 198), width=4)

    def fy2y(f):                           # 元画像の相対 y -> 出力画像の y
        if cut and f > cut[1]:
            return top + int(bh_orig * f) - int(bh_orig * (cut[1] - cut[0])) + 18
        return top + int((bh_orig if cut else bh) * f)

    px = 24 + bw + 40                      # 説明パネルの左端
    cursor = top                           # 説明どうしが重ならないように下へ送る
    for n, (fx0, fy0, fx1, fy1), col, head, bodytext in marks:
        x0, y0 = 24 + fx0 * bw, fy2y(fy0)
        x1, y1 = 24 + fx1 * bw, fy2y(fy1)
        if col is None:                    # 枠なし (説明だけ)
            cy = (y0 + y1) / 2
        else:
            rect(d, (x0, y0, x1, y1), col, width=6, radius=12)
            cy = (y0 + y1) / 2
            if n:
                badge(d, (x0 - 20, cy), n, r=26, fill=col)
        # --- 説明 (重なったら下へずらす) ---
        lines = bodytext.split("\n")
        ty = max(cy - 34, cursor)
        if n:
            badge(d, (px + 24, ty + 22), n, r=24, fill=col or RED)
            tx = px + 60
        else:
            tx = px
        d.text((tx, ty), head, font=jp(31, bold=True), fill=col or INK)
        for i, line in enumerate(lines):
            d.text((px, ty + 46 + i * 38), line, font=jp(27), fill=(70, 70, 80))
        cursor = ty + 46 + len(lines) * 38 + 22
        if col is not None:
            arrow(d, (px - 18, ty + 22), (x1 + 10, cy), color=col, width=6, head=20)
    return img


# ------------------------------------------------------- SETUP MODE の入力画面
def setup_mode():
    return annotate("webui-setup-mode.jpg", [
        (None, (0.03, 0.026, 0.97, 0.080), ORANGE, "オレンジ色の帯が出ていればOK",
         "この帯があるページが「はじめの設定」です。"),
        ("1", (0.070, 0.288, 0.925, 0.344), RED, "おうちの Wi-Fi の名前 (SSID)",
         "ルーター本体のシールや、スマホの Wi-Fi 一覧\nに出ている名前をそのまま入力します。"),
        ("2", (0.070, 0.380, 0.925, 0.436), RED, "おうちの Wi-Fi のパスワード",
         "ルーターのシールでは「暗号化キー」\n「KEY」「PSK-AES」などと書かれています。"),
        ("3", (0.045, 0.452, 0.955, 0.512), ORANGE, "2.4GHz のみ対応",
         "末尾が -a / -A / 5G の名前では つながりません。\n-g / -G / 2G の方を入力してください。"),
        ("4", (0.040, 0.566, 0.960, 0.630), GREEN, "「保存してつなぐ」を押す",
         "10 秒ほどで再起動し、「こんにちは」と\n挨拶したら成功です。"),
    ], cut=(0.80, 0.885), title="はじめの設定（SETUP MODE）で入力するのは 2 つだけ")


# ------------------------------------------------------------------- 設定画面
def settings():
    return annotate("webui-settings.jpg", [
        (None, (0.030, 0.010, 0.970, 0.037), GREEN, "緑の帯 ＝ ふつうの設定画面",
         "オレンジの帯なら、まだ Wi-Fi 未接続です。"),
        ("×", (0.030, 0.234, 0.970, 0.349), RED, "APIキー ＝ さわらない",
         "この子が聞いたり喋ったりするための鍵です。\n空欄のまま保存すれば変わりません。"),
        ("1", (0.030, 0.506, 0.970, 0.532), GREEN, "音量トリガ感度",
         "生活音に反応しすぎる → 数値を上げる\n呼んでも気づかない → 数値を下げる"),
        ("2", (0.030, 0.596, 0.970, 0.628), GREEN, "VoiceVox 話者（声）",
         "一覧から選ぶだけで声が変わります。"),
        ("3", (0.030, 0.639, 0.970, 0.670), GREEN, "スピーカー音量",
         "0〜255。うるさければ 80、静かなら 150 など。"),
        ("4", (0.030, 0.914, 0.970, 0.949), GREEN, "「保存して再起動」で反映",
         "空欄のままの項目は変更されません。"),
        ("×", (0.030, 0.957, 0.970, 0.992), RED, "鍵情報クリア ＝ 押さない",
         "押すと全部消えて動かなくなります。"),
    ], panel_w=760, scale=1.6, cut=(0.700, 0.905),
        title="設定画面：さわってよい所（緑）と、さわらない所（赤）")


# ------------------------------------------------------------------ メニュー
def menu():
    return annotate("webui-menu.jpg", [
        ("1", (0.12, 0.140, 0.94, 0.174), GREEN, "設定",
         "音量・声・感度・Wi-Fi の変更。"),
        ("2", (0.12, 0.184, 0.94, 0.218), GREEN, "ロール（キャラ設定）",
         "性格を自由な文章で変えられます。"),
        ("3", (0.12, 0.228, 0.94, 0.262), GREEN, "現在のロール確認",
         "いまの性格設定を表示します。"),
        ("4", (0.05, 0.286, 0.70, 0.330), GREEN, "しゃべらせる",
         "入力した文をそのまま喋ります。"),
        ("5", (0.05, 0.334, 0.70, 0.378), GREEN, "文字で質問する",
         "声の代わりに文字で会話できます。"),
    ], cut=(0.40, 0.885), title="設定画面のメニュー（つながったあとに開けます）")


if __name__ == "__main__":
    for name, im in [("webui-setup-mode-annot", setup_mode()),
                     ("webui-settings-annot", settings()),
                     ("webui-menu-annot", menu())]:
        im.save(os.path.join(OUT, name + ".png"))
        print("wrote", name, im.size)
