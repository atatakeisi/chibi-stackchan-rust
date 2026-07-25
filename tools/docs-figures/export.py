"""out/ に描いた図を docs/images/ へ書き出す

  - 幅は 1100px を上限に縮小 (マニュアルの表示にはこれで十分)
  - *-annot は元がJPEGのスクショなので JPEG、それ以外の作図は減色PNG
    (どちらも「読めるまま容量を1/3にする」ためのもの)
"""
import glob
import os

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(HERE, "out")
DST = os.path.join(REPO, "docs", "images")
MAXW = 1100


def main():
    files = sorted(glob.glob(os.path.join(OUT, "*.png")))
    if not files:
        raise SystemExit(f"{OUT} に画像がありません。先に gen_*.py を実行してください。")
    total = 0
    for f in files:
        name = os.path.basename(f)[:-4]
        im = Image.open(f).convert("RGB")
        if im.width > MAXW:
            im = im.resize((MAXW, round(im.height * MAXW / im.width)), Image.LANCZOS)
        if name.endswith("-annot"):
            out = os.path.join(DST, name + ".jpg")
            im.save(out, quality=86, optimize=True, progressive=True)
        else:
            out = os.path.join(DST, name + ".png")
            im.quantize(colors=128, method=Image.MEDIANCUT,
                        dither=Image.NONE).save(out, optimize=True)
        n = os.path.getsize(out)
        total += n
        print(f"{os.path.basename(out):32} {im.size}  {n / 1024:6.1f} KB")
    print(f"合計 {total / 1024:.0f} KB")


if __name__ == "__main__":
    main()
