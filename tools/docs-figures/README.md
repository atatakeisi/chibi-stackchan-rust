# マニュアル図版の生成スクリプト

[MANUAL.md](../../MANUAL.md) と [docs/setup-guide.html](../../docs/setup-guide.html) で使う
図（本体LCDの再現図・スマホ操作の説明図・注釈入りスクリーンショット）を生成する。

写真が用意できなかったため作図で代用している。**実機写真が撮れたら
`docs/images/device-buttons.png` などを差し替えてよい**（スクリプトは不要になる）。

## 使い方

```bash
python3 -m venv venv && ./venv/bin/pip install Pillow

cd tools/docs-figures
../../tools/docs-figures/venv/bin/python gen_lcd.py     # 本体LCDの再現図 -> out/
./venv/bin/python gen_device.py                          # 本体ボタンの説明図
./venv/bin/python gen_phone.py                           # スマホ操作の説明図
./venv/bin/python gen_annot.py                           # 既存スクショに注釈を重ねる
./venv/bin/python export.py                              # out/ を docs/images/ へ書き出し
./venv/bin/python build_html.py                          # docs/setup-guide.html を組み立て
```

macOS 前提（日本語フォントを `/System/Library/` から探す）。

## ファイル

| ファイル | 役割 |
|---|---|
| `gfx.py` | 共通ヘルパ。フォント探索、矢印・番号バッジ・吹き出しの描画 |
| `gen_lcd.py` | 本体 128×128 LCD の再現図。**`src/main.cpp` の描画座標と 1:1 で対応**させてある |
| `gen_device.py` | 本体正面／左側面のボタン位置の図（イメージ図） |
| `gen_phone.py` | スマホの Wi-Fi 設定・ブラウザ操作の図、2.4GHz の見分け方 |
| `gen_annot.py` | `docs/images/webui-*.jpg` に枠・番号・矢印・説明パネルを重ねる |
| `export.py` | `out/` を縮小・圧縮して `docs/images/` へ |
| `guide-body.html` | HTML版マニュアルの本文（画像は `__IMG_名前__` のプレースホルダ） |
| `build_html.py` | `guide-body.html` から `docs/setup-guide.html` を生成。あわせて画像を data URI で埋め込んだ単体版 `artifact.html` も出力する |

## 注意

- `gen_lcd.py` は `src/main.cpp` の `startConfigPortal()` / `Wifi_setup()` /
  IP表示部分の描画コードを写したもの。**本体の表示を変えたら、この図も直すこと。**
- `gen_annot.py` の枠の座標は元スクリーンショットの相対位置（0〜1）で書いてある。
  スクショを撮り直したら座標の調整が必要。
- `build_html.py` が出す `artifact.html` は画像を全部埋め込んだ約 950KB の単体ファイル。
  オフラインで渡したいときに使う（リポジトリには入れていない）。
