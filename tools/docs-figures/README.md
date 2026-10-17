# マニュアル図版の生成スクリプト

[MANUAL.md](../../MANUAL.md) と [docs/setup-guide.html](../../docs/setup-guide.html) で使う
図（本体LCDの再現図・スマホ操作の説明図・注釈入りスクリーンショット）を生成する。

この図版は元の C++ 版のマニュアル用に作成したものです。特に LCD の再現図は
[整理前チェックポイントの `src/main.cpp`](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/src/main.cpp)
の表示を基にしており、現在の Rust ソースを読み取って生成するものではありません。
現在の顔描画は [Rust 顔描画ライブラリ](../../rust/avatar-face/README.md)、画面制御は
[Rust ファームウェアの説明](../../rust/firmware/README.md) を参照してください。

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
| `gen_lcd.py` | 元の C++ 版の本体 128×128 LCD を再現する履歴図。整理前の描画座標を基にしたもの |
| `gen_device.py` | 本体正面／左側面のボタン位置の図（イメージ図） |
| `gen_phone.py` | スマホの Wi-Fi 設定・ブラウザ操作の図、2.4GHz の見分け方 |
| `gen_annot.py` | `docs/images/webui-*.jpg` に枠・番号・矢印・説明パネルを重ねる |
| `export.py` | `out/` を縮小・圧縮して `docs/images/` へ |
| `guide-body.html` | HTML版マニュアルの本文（画像は `__IMG_名前__` のプレースホルダ） |
| `build_html.py` | `guide-body.html` から `docs/setup-guide.html` を生成。あわせて画像を data URI で埋め込んだ単体版 `artifact.html` も出力する |

## 注意

- `gen_lcd.py` は上記チェックポイントの `startConfigPortal()` / `Wifi_setup()` /
  IP 表示部分を写したものです。旧 C++ ファイルを必要とする実行時の依存はありません。
  現在の Rust 版の画面を説明する図として使う場合は、実機表示との一致を確認して更新してください。
- `gen_annot.py` の枠の座標は元スクリーンショットの相対位置（0〜1）で書いてある。
  スクショを撮り直したら座標の調整が必要。
- `build_html.py` が出す `artifact.html` は画像を全部埋め込んだ約 950KB の単体ファイル。
  オフラインで渡したいときに使う（リポジトリには入れていない）。
