# Rust専用構成への整理記録

2026-10-17。削除対象一覧についてユーザーの明示的な承認を得た後、以下の整理を実施しました。

## 整理前の保存先

- [動作確認済みRust版と旧混在ソースのチェックポイント](https://github.com/atatakeisi/chibi-stackchan-rust/tree/ffb5c74706a106e09211d558a028f2fccbaac537)
- [別リポジトリとして残る元のC++版](https://github.com/atatakeisi/chibi-stackchan/tree/10460b3bdd97fd59e05dcc9f69395839ea51a8fb)

元C++版の別のローカルチェックアウトも、変更のない状態であることを確認しました。
削除対象のソースはチェックポイントから復元できます。ビルド生成物は元ソースから再生成できます。

## 削除したもの

| 対象 | 理由 |
|---|---|
| `src/` 全19ファイル | 旧Arduino/C++版とC ABIブリッジ。現行Rust版から参照しない |
| `include/` 内の `secrets.h.example` と空になるフォルダー | 旧C++向け設定例。Rust版はNVSとWeb設定を使用 |
| `platformio.ini` | 旧C++・混在版だけのビルド設定 |
| `scripts/build_rust.py` | 旧C++へRustの音声解析ライブラリをリンクする補助処理 |
| `rust/audio-core/`（旧ソース3ファイルと生成物） | 移行途中の音声解析ライブラリ。現在のCargo依存に含まれない |
| `.pio/` | PlatformIOのビルド・依存ライブラリ生成物 |
| `.vscode/c_cpp_properties.json`、`.vscode/launch.json` | PlatformIO用の生成済みエディター設定 |
| `tools/docs-figures/__pycache__/` | Pythonの再生成可能なキャッシュ |

監査時点で旧ビルド生成物は合計約281MiBです。削除時はこのプロジェクト配下の列挙した対象だけを扱い、シンボリックリンクの先はたどりません。

## 保持・更新するもの

- `rust/firmware/`：実機で動作するRustアプリケーション。
- `rust/avatar-face/`：独立したRust顔描画ライブラリとMIT表記。
- `scripts/firmware.py`：現行Rust版のテスト・ビルド。
- `README.md`、`MANUAL.md`、`RUST_INTEGRATION.md`、既存の全Markdown文書と図版：説明を現在用／履歴用に整理して保持。
- [Rust移行記録](RUST_MIGRATION.md)：各C++機能とRustファイルの対応、修正経緯、実機確認。
- `tools/docs-figures/`：マニュアルの図版を作る道具。旧版用であることを明記して保持。
- `.vscode/extensions.json`：Rust用の推奨拡張へ変更。RustのCargoプロジェクトを指定する設定・タスクを追加。
- `.tools/`：必要なリンカーと実機バックアップを保持し、Git対象から除外。
- Rust側の `target/`・`.embuild/`：現行ESP-IDFのSDKとビルド生成物。VS Codeでは非表示にするが削除対象にはしない。

## 削除後の確認項目

1. 現行Rustソースがチェックポイントと同じであることを確認。
2. 顔ライブラリとRustファームウェアのテスト、およびESP32-S3向けビルドを実行。
3. 現行文書・ビルド設定に削除したC++コードへの動作上の依存がないことを確認。
4. 整理内容を別のコミットとして通常pushし、mainと一致を確認。

承認した一覧に含まれる1,528ファイルを削除しました。その直後にPlatformIO拡張が空のプロジェクト雛形を再生成したため、ユーザーがこのワークスペースのPlatformIOを無効化し、追加の4ファイル（platformio.ini、include/README、lib/README、test/README）と空のフォルダーの削除も明示的に承認しました。再生成分も削除済みです。追跡対象の旧ソース26ファイルは削除前にチェックポイントとの一致を確認しています。README・プロジェクトのMarkdown・図版・実機バックアップは保持しました。

整理後の確認結果：顔ライブラリ12件とファームウェア86件のテストが成功し、ESP32-S3向けreleaseビルドも成功しました。生成バイナリのSHA-256は書き込み済み版と同じ `a7496e3b9d8c1f8bf6f117634906836837c0664919314dbe33dcf9ea690df5a7` です。現行Rustコードは整理前チェックポイントから変更していません。
