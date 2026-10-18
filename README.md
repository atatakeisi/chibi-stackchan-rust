# ちびスタックちゃん Rustファームウェア

ATOMS3R＋Atomic Echo Base向けの、Rustで実装したちびスタックちゃんです。
音声入出力、会話、顔、サーボ、Wi-Fi、Web設定、設定保存をRustへ移しました。
OS・Wi-Fi・TLSなどには、Rustの `esp-idf-svc` / HALを通じてESP-IDFのC SDKを利用します。

**実機で動くコードは [rust/firmware/src](rust/firmware/src) にあります。**
顔の描画は独立した [rust/avatar-face](rust/avatar-face/README.md) です。

## フォルダーの見方

```text
chibi-stackchan-rust-main/
├── rust/
│   ├── firmware/           # 本体のRustアプリケーション
│   │   ├── src/main.rs     # 起動
│   │   ├── src/device.rs   # 録音・会話・再生・本体の状態管理
│   │   ├── src/board.rs    # マイク・スピーカー・サーボの制御
│   │   ├── src/audio.rs    # 録音判定・音声解析・WAV
│   │   └── src/…           # 通信・画面・設定・音声命令など
│   └── avatar-face/        # 顔の形・表情・アニメーション
├── scripts/firmware.py    # Rust版のテスト・ビルド
├── .vscode/               # Rust用のプロジェクト設定・タスク
├── docs/                  # 移行記録・旧版の説明・図版
├── tools/docs-figures/    # 説明書用の図版生成ツール
├── MANUAL.md              # 元C++版から引き継いだ操作説明書
└── RUST_INTEGRATION.md    # 初期のC++＋Rust混在版の検証記録
```

ルートにあった `src/main.cpp`、PlatformIO設定、混在版専用の `rust/audio-core` は整理しました。
元のC++版と整理前の状態は、下記の移行記録から参照できます。

VS CodeにはRust用の推奨拡張と2つのCargoプロジェクトを設定しています。
「ターミナル → タスクの実行」からRustのテスト・ESP32-S3向けビルドを実行できます。
`target/`、`.embuild/`、ローカルツールはエクスプローラーで非表示にしています。

## ビルドとテスト

Python 3、Cargo、EspressifのXtensa Rustツールチェーン `esp` が必要です。
リポジトリのルートで実行します。

```sh
# 新しい環境での依存取得とローカルリンカー導入
cargo fetch --manifest-path rust/firmware/Cargo.toml
python3 scripts/firmware.py tools

# 顔ライブラリとファームウェアのホストテスト
python3 scripts/firmware.py test

# ESP32-S3向けreleaseビルド
python3 scripts/firmware.py build
```

ヘルパーは `rustup run esp cargo` を使います。シェルの環境変数や既定ツールチェーンの変更は不要です。
生成物は `rust/firmware/target/xtensa-esp32s3-espidf/release/stackchan-firmware` と、同名の `.bin` です。
上記のコマンドは実機への書き込みを行いません。
詳しい構成、必要なツール、書き込み時の注意は [RustファームウェアのREADME](rust/firmware/README.md) を参照してください。

## 操作と確認済みの動作

内蔵BMI270の加速度を使い、本体を横向き・逆さまにしたときに顔を90°刻みで自動回転します。
向きが約0.3秒安定してから切り替え、斜めの境界や大きな揺れでは向きを保持します。
画面を真上・真下へ向けた場合も直前の向きを保ちます。センサーの初期化・読み取りに失敗しても、本体の動作を継続します。
2026-10-18にユーザーが自動回転と再起動しないことを確認し、タイマー・天気・音量調節の動作も確認しています。

設定は本体のWeb画面で行います。既存のNVS設定を引き継ぎ、Rust版は `secrets.h` を読み込みません。
Wi-Fiに接続できない場合は設定APへ移行します。設定方法と各画面は [Rust版の説明](rust/firmware/README.md#設定とデータの引き継ぎ) を参照してください。

音量はWebと音声で同じ **0～255** の値を使います。
通常の呼びかけに続けて、次のように話します。

| 例 | 動作 |
|---|---|
| 「スタックちゃん、音量下げて」「音量上げて」 | 現在値から2ずつ変更 |
| 「おーい、音量20にして」 | 指定値を適用・保存 |
| 「ミュートして」 | 消音 |
| 「ミュート解除」 | 最後の消音前の音量へ復帰 |
| 「スタックちゃんタイマー1分」 | タイマー設定と音声通知 |

2026-10-17にユーザーが実機で確認した動作です。

- ミュート・解除、音量30・20・5への変更。5はとても小さな音量。
- 音量の上げ下げと、本体による実際の設定値の返答。
- 通常の会話応答、設定地点の天気、1分タイマーと通知。
- 元のAvatarに近い顔、自然な音声、iPhoneからの設定・操作。

ホストテストは **ファームウェア86件＋顔ライブラリ12件**。ESP32-S3向けreleaseビルドとアプリケーション書き込みも確認済みです。
サーボの実際の動きは未確認です。詳しい測定値と確認範囲は [実機確認記録](rust/firmware/README.md#書き込みと確認状況) に残しています。

## 何をどうRustへ移したか

- [Rustへの移行記録](docs/RUST_MIGRATION.md)：C++ファイルとRustモジュールの対応、移行段階、実機の問題と修正。
- [整理対象と実施記録](docs/RUST_CLEANUP_PLAN.md)：削除・保持したものと確認方法。
- [元README全文の保存版](docs/history/CPP_AND_MIXED_README.md)：C++・混在版の説明、配線、図版、クレジット。
- [C++＋Rust混在期の検証記録](RUST_INTEGRATION.md)：初期の音声解析連携と測定。
- [元の操作説明書](MANUAL.md)、[旧版セットアップガイド](docs/setup-guide.html)：移行前の画面や操作の記録。

[元のC++リポジトリ](https://github.com/atatakeisi/chibi-stackchan)は別に保持しています。
整理前のC++・混在コードとRust版を含む状態も、[固定コミット `ffb5c747`](https://github.com/atatakeisi/chibi-stackchan-rust/tree/ffb5c74706a106e09211d558a028f2fccbaac537) に保存しています。

## ライセンスと元プロジェクト

[LICENSE](LICENSE) と移植元の著作権表記を保持しています。
顔描画はM5Stack-Avatarを基にしており、MITの原文を [顔ライブラリのLICENSE](rust/avatar-face/LICENSE) に保存しています。
現在のMP3デコードはSymphoniaを使用します。
Stack-chan、M5Stack-Avatar、AI_StackChan2などへのクレジットと、旧版のライブラリ構成は [移行記録](docs/RUST_MIGRATION.md#ライセンスとクレジット) と [旧README](docs/history/CPP_AND_MIXED_README.md) に残しています。
