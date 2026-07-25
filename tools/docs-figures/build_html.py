"""guide-body.html から 2 種類の HTML を組み立てる

  1) docs/setup-guide.html … リポジトリ用。images/ を相対参照する単体ページ
  2) artifact.html         … 公開用。画像を data URI で埋め込んだ本文だけの断片
"""
import base64, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
IMGDIR = os.path.join(REPO, "docs", "images")

body = open(os.path.join(HERE, "guide-body.html"), encoding="utf-8").read()
names = sorted(set(re.findall(r"__IMG_([a-z0-9\-]+)__", body)))


def resolve(name):
    for ext in (".png", ".jpg"):
        p = os.path.join(IMGDIR, name + ext)
        if os.path.exists(p):
            return p, ext
    raise SystemExit(f"画像が見つかりません: {name}")


# ---- 1) リポジトリ用 (相対パス) ----
rel = body
for n in names:
    _, ext = resolve(n)
    rel = rel.replace(f"__IMG_{n}__", f"images/{n}{ext}")

doc = f"""<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light dark">
<title>ちびスタックちゃん はじめの設定</title>
</head>
<body>
{rel}
</body>
</html>
"""
out1 = os.path.join(REPO, "docs", "setup-guide.html")
open(out1, "w", encoding="utf-8").write(doc)

# ---- 2) 公開用 (data URI 埋め込み) ----
inline = body
total = 0
for n in names:
    p, ext = resolve(n)
    raw = open(p, "rb").read()
    total += len(raw)
    mime = "image/png" if ext == ".png" else "image/jpeg"
    inline = inline.replace(f"__IMG_{n}__",
                            f"data:{mime};base64,{base64.b64encode(raw).decode()}")
out2 = os.path.join(HERE, "artifact.html")
open(out2, "w", encoding="utf-8").write(inline)

print(f"images: {len(names)}  ({total/1024:.0f} KB)")
print(f"{out1}  {os.path.getsize(out1)/1024:.0f} KB")
print(f"{out2}  {os.path.getsize(out2)/1024:.0f} KB")
assert "__IMG_" not in rel and "__IMG_" not in inline, "未置換のプレースホルダあり"
