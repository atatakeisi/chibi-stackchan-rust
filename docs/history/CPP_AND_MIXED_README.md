# 整理前 README の保存：C++ 版・C++ / Rust 混在版

この文書は、旧ファイルを整理する前のチェックポイント `ffb5c74706a106e09211d558a028f2fccbaac537` にある README 全文の保存です。冒頭には当時追加した独立 Rust 版の案内があり、その後に元の C++ / 混在版の説明が続きます。

**以下の PlatformIO コマンド、ファイル配置、依存関係と検証状況は、その時点の記録です。現在の手順として実行するための文書ではありません。** 現在の構成は [プロジェクトの README](../../README.md) と [Rust 移行記録](../RUST_MIGRATION.md) を参照してください。

[チェックポイント上の元 README を開く](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/README.md)

当時の本文・クレジットを保ち、リポジトリ内へのリンクと画像はチェックポイントの固定 URL に置き換えました。別チェックアウトを指していた `guruguru-stackchan` は履歴上の名前として残しています。

---

# ちびスタックちゃん Rustファームウェア (Chibi StackChan Rust)

[C++版の元リポジトリ](https://github.com/atatakeisi/chibi-stackchan)を基に、
**アプリケーション全体を Rust で再構成したファームウェア**を [rust/firmware](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/rust/firmware/README.md) に追加しました。
音声入出力、画面、サーボ、会話、ネットワーク、設定保存を独立した Cargo プロジェクトで構成しています。

Arduino・M5Unified の C++ ランタイムを使わず、Rust の `esp-idf-svc` / HAL から
ESP-IDF の C SDK を利用します。ESP-IDF を含むバイナリ全体が Rust だけになる構成ではありません。

## Rustファームウェアのビルド

Python 3、Cargo、ESP32-S3 向けの Espressif Rust ツールチェーン `esp` が必要です。
この作業環境には `esp` が導入済みです。ビルドは `rustup run esp cargo` を明示的に呼び出します。
シェルの環境変数や既定のツールチェーンを変更する操作は不要です。

リポジトリのルートで実行します。

```sh
cargo fetch --manifest-path rust/firmware/Cargo.toml
python3 scripts/firmware.py tools   # ldproxy をプロジェクト内の .tools/ に導入
python3 scripts/firmware.py test    # ホスト上の単体テスト
python3 scripts/firmware.py build   # ESP32-S3 向け release ビルド
```

生成される ELF は `rust/firmware/target/xtensa-esp32s3-espidf/release/stackchan-firmware`、
アプリケーション用バイナリは同じ場所の `stackchan-firmware.bin` です。
Rust 版は `include/secrets.h` を読み込まず、既存 NVS 設定の移行と Web 設定ポータルを使います。
構成、操作、保存形式、ビルドの詳細は [Rustファームウェアの説明](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/rust/firmware/README.md) を参照してください。

## 実装と検証の範囲

- 録音・WAV・VAD、Gemini / Claude、Whisper、VOICEVOX、時刻・天気・タイマー・睡眠を Rust へ移しました。
- 「スタックちゃん、音量を下げて」「音量を10にして」「ミュート解除」などを音声認識後に本体で処理し、音量を変更・保存する機能を実装しました。マイク入力ではミュート・解除は動いた一方、数値指定・音量アップが通常会話へ流れたとの報告があり、解析と AI からの操作指示を修正し、実際の声で下げる・上げる操作を確認しました。数値 20 が「二重」になる認識違いも補正しました。[音声で音量を変える](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/rust/firmware/README.md#音声で音量を変える) に操作例をまとめています。
- LCD 2 種の判定、Atomic Echo Base の音声入出力、首振りサーボを Rust のドライバーで構成しました。
- 元の M5Stack-Avatar の白い丸目・四角い口・6 表情を [独立した顔描画ライブラリ](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/rust/avatar-face/README.md) へ移植しました。元の縮尺・配置で描画し、まばたき・視線・呼吸・口パク・傾きを制御します。[表情プレビュー](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/docs/images/rust-avatar-face-preview.png) で形を確認できます。
- Wi-Fi / 設定 AP / キャプティブ DNS / Web UI / NVS 移行 / SPIFFS ロール・音声読込を実装しました。
- **顔描画ライブラリのホストテスト 12 件、Rust ファームウェアの 86 件、既存音声解析ライブラリの 2 件、ESP32-S3 向け release ビルドが成功しました。**
- **接続した ATOMS3R の app0（`0x10000`）へ Rust 版を書き込み、書き込みハッシュを検証しました。**
- 元の Avatar を基にした顔描画修正版も実機へ書き込み、再生・マイク復帰・待機・Web 画面の応答を再確認しました。ユーザーが実機で「元の顔に戻った」と確認しました。
- 顔描画後に声がかすれる問題は、描画範囲の最適化と SPI 転送設定の修正で改善しました。実機で音声長 4,848 ms に対して再生処理 4,821 ms を確認し、ユーザーが「自然な声に戻った」と確認しました。保存音量 **10 / 255** と話者 **54** は保持しています。
- 実機で既存 Wi-Fi 設定の引き継ぎ・DHCP、ST7735S LCD の識別、VOICEVOX の HTTPS 音声取得と MP3 再生完了、マイク復帰・待機状態、Web の `/`・`/config`・`/role` の HTTP 200 を確認しました。
- ユーザーが音声出力とミュート時の無音を確認しました。過大音量の原因だったコーデックの +32 dB 設定を 0 dB に修正し、同じ音声の再生中に音量変更・ミュートが反映されることを実機ログで確認しました。音量修正後にユーザーが許容範囲と確認しました。前回の検証終了時は音量 **15 / 255**・話者 **54** でした。今回の書き込み前はユーザー設定の音量 **17 / 255** を引き継ぎ、検証後も17へ戻しています。
- 実機の文字入力から音声認識後と同じ処理へ「スタックちゃん音量下げて」「おーい、ミュートして」を渡し、保存値・再生音量・出力 PCM への反映を確認しました。同じ音声の出力 PCM は音量 15 で音量 5 のちょうど 3 倍となりました。初期版で取りこぼした音声命令は修正済みです。ユーザーが実際の声でミュート・解除、音量30・20・5への変更を確認し、5が小さな音量になることも確認しました。
- **2026-10-17：iPhone の Web 画面から設定変更・本体操作ができることをユーザーが確認しました。** この状態の Rust ソースとビルド設定を本プロジェクトに保存しています。書き込み済みバイナリの識別情報は [実機確認記録](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/rust/firmware/README.md#書き込みと確認状況) を参照してください。
- ユーザーが実機へ「スタックちゃんタイマー1分」と話して設定し、1 分後の音声による通知まで確認しました。前回の呼びかけ修正版でも同じ文を文字入力して、通知音声の再生完了とマイク復帰を確認しました。通常の会話応答、設定地点の天気、iPhone操作もユーザーが確認しました。サーボの実動作は未確認です。

書き込み前にフラッシュ全体 8 MiB を非公開の `.tools/backups/` 以下へバックアップし、実機の 6 パーティションと構成の一致を確認しました。
既存のブートローダー・パーティションテーブル・NVS・SPIFFS は保持しています。
上記のテスト・ビルドコマンドは実機への書き込みを行いません。

## 既存の PlatformIO 版

`src/` と PlatformIO の構成は参照・比較用として残しています。
`pio run` は従来どおり **Arduino C++＋Rust 音声解析ライブラリの混在版**をビルドします。

| PlatformIO 環境 | 用途 |
|---|---|
| `atoms3r-rust` | 既存の C++＋Rust 混在版（PlatformIO の既定） |
| `atoms3r-rust-check` | 既存の C++ / Rust 音声解析比較版 |
| `atoms3r` | 元の C++ 版 |

混在版の実機確認・音声解析測定は [RUST_INTEGRATION.md](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/RUST_INTEGRATION.md) に記録しています。
その測定値・実機確認は、新しい Rust ファームウェアには適用されません。
以下は保存している元プロジェクトの機能・配線・操作説明です。Arduino / PlatformIO 固有の手順を含みます。

---


**ATOMS3R AI Chatbotキット**（AtomS3R + Atomic Echo Base, 8MB PSRAM）で動く、
手のひらサイズのAIスタックちゃん。guruguru-stackchan（Core2版）からの派生です。

> 📖 **完成品を受け取った方へ**: 設定済みの機体の使い方は
> **[取扱説明書 (MANUAL.md)](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/MANUAL.md)** をご覧ください。以下は作る人向けの情報です。

- 🧠 **会話**: Google **Gemini**（`gemini-3.5-flash`・無料枠あり）または
  Anthropic **Claude**（`claude-haiku-4-5`・従量課金）— **Web UIで切り替え可**
- 👂 **音声認識(STT)**: OpenAI **Whisper**
- 🗣️ **音声合成(TTS)**: **VoiceVox**（Web API）
- 🙂 **表情**: m5stack-avatar（128×128 に縮小表示・**吹き出しなし**・口パクあり）
- 🔩 **サーボ**: SG90 × 1（Grove接続）。**左右首振りのみ・連続回転なし**。
  会話中は声の大きさに合わせてゆらゆら、待機中はときどきキョロッ
- 📣 **会話スタート**: 3モード切り替え — **呼びかけワード方式** /
  **なんでも返事**（聞き取れた発話すべてに応答）/ 無効（画面タッチのみ）
  ＋ 画面クリックでも即スタート
- ⏲️ **タイマー**: 「スタックちゃん、3分タイマー」で音声セット→時間が来たら喋って知らせる
- 😴 **おやすみ**: 3分間会話が無いと眠る（眠い顔・サーボ停止）。声をかけると
  「寝てしまった！」と飛び起きる（起床ボイスは本体にキャッシュされネット不要で即再生）
- ☀️ **おまけ**: 今日の天気（Open-Meteo・APIキー不要）と現在時刻（NTP）を会話に反映
- 🔌 **電源**: ATOMS3R の USB-C 給電
- 📶 **設定**: 全て NVS 保存。未設定/接続失敗/起動時画面長押しで **AP設定ポータル**
  （iPhoneのブラウザから入力）。**鍵情報クリアボタン**あり

---

## 必要なもの

| 区分 | 内容 |
|---|---|
| 本体 | M5Stack **ATOMS3R**（8MB PSRAM）+ **Atomic Echo Base**（= AI Chatbotキット） |
| サーボ | **SG90** など PWM ポジションサーボ ×1（180度型。連続回転型は不可） |
| 鍵 | Gemini または Claude APIキー（会話）/ OpenAI APIキー（Whisper用）/ VoiceVox APIキー |
| 電源 | USB-C（ATOMS3R 側に接続） |

## 配線（サーボ）

ATOMS3R 本体側面の **Grove ポート（G1 / G2 / 5V / GND）** に接続:

| SG90 | Grove |
|---|---|
| 信号（橙/黄） | **G2** |
| VCC（赤） | 5V |
| GND（茶/黒） | GND |

> 信号ピンを G1 に変えたい場合は `src/HeadServo.h` の `kServoPin` を `1` に。
> 可動範囲・振幅・速さも同ファイル冒頭の定数で調整できます。
> Echo Base は ATOMS3R の底面にそのまま合体（I2S/I2C は M5Unified が自動設定）。

## ビルド & 書き込み（PlatformIO）

```bash
cp include/secrets.h.example include/secrets.h   # （任意）初期値を書く場合
pio run                  # ビルド
pio run -t upload        # 書き込み
pio device monitor       # シリアルモニタ (115200)
```

- Core2 版と違い esp-sr を使わないため **`framework = arduino` 単独**でビルドできます
  （IRAM 問題・espidf ハイブリッド・setuptools 対策は不要）。
- `secrets.h` は **NVS が空のときの初期値**としてのみ使用。無くても
  （中身がプレースホルダのままでも）AP設定ポータルから全部入力できます。
- ⚠️ 書き込みはシリアルモニタを閉じてから。失敗する場合はポート明示
  （`pio run -t upload --upload-port /dev/cu.usbmodemXXXX`）や、本体側面の
  リセットボタン約2秒長押し（緑LED点灯=ダウンロードモード）を試してください。

## 初期設定（AP設定ポータル）

以下のいずれかで設定モード（AP）になります:

1. NVS に SSID が未保存（初回起動）
2. 起動時に Wi-Fi 接続へ 15 秒失敗
3. **起動時に画面（Aボタン）を押したまま** / Wi-Fi 接続試行中に画面長押し
4. 通常運用中に **画面を3秒長押し**

設定モードでは:

1. iPhone で Wi-Fi **`ChibiStackChan-Setup`**（パスワード **`stackchan`**）に接続
2. ブラウザで **http://192.168.4.1** を開く
3. SSID / パスワード / 各APIキー / **呼びかけワード** / 天気の場所 などを入力
   →「保存して再起動」

> ⚠️ ESP32 は **2.4GHz 専用**です（5GHz 非対応）。iPhone側は同じルーターなら5GHzでもOK。
> 💡 **空欄の項目は変更されません**。外出先では Wi-Fi の SSID/パスワードだけ入力すれば、
> APIキー等は引き継いだままその場のネットワークに切り替えられます。
> 🗑️ ページ下部の **「鍵情報クリア」** で Wi-Fi・APIキー等の NVS を全消去して
> 再起動→設定モードに戻ります（secrets.h の初期値へのフォールバックも無効化されます）。

## 使い方

| 操作 | 動作 |
|---|---|
| **「スタックちゃん」と呼びかける** | 録音→呼びかけワードを含めば会話開始。「スタックちゃん、今日の天気は？」のように続けて話すとそのまま質問扱い。呼びかけだけなら「なあに？」と聞き返してくる。**「なんでも返事」モードならワード無しの発話にもそのまま返事** |
| 画面クリック | 呼びかけ無しで即録音→会話 |
| 画面3秒長押し | 設定APモードへ |
| 「今何時？」「今日の天気は？」 | NTP時刻 / Open-Meteo の天気を参考情報として会話AI（Gemini / Claude）が答える |
| **「3分タイマーして」** | タイマーをセット。「タイマーやめて」で中止、「あと何分？」で残り確認。時間が来たら「ピピピッ！3分たったよ！」と喋る |
| 「アイピー教えて」 | 自分のIPアドレスを読み上げる（起動時にも画面に大きく数秒表示） |

### 画面の色 = いまの状態

| 背景色 | 状態 |
|---|---|
| ⚫ 黒 | 待機中（呼びかけ待ち） |
| 🟢 緑 | **録音中**（聞いている） |
| 🔵 紺 | **認識・考え中**（Whisper / 会話AI 処理中） |

緑にならない=声がしきい値に届いていない、緑→紺→黒で無反応=聞き取れたが
呼びかけワード無しで破棄（「呼びかけワードで起動」モード時）、という切り分けができます。
なんでも返事モードでは、**物音や無音は黙って無視**し、通信エラーで本当に聞き取れなかった
ときだけ「ごめん、うまく聞き取れなかったよ」と言います。おまけとして、音声認識が
音を言葉にできた場合（電子レンジの「チーン」→「【電子レンジの音】」等）は
**その物音に反応して喋ります**。

### 呼びかけ方式のしくみと注意

```
音量がしきい値を超える音が続く（0.5秒中0.16秒以上）
  → 🟢 録音開始（直前1秒のプリロール付き = 語頭が欠けない）
     喋り終わり（無音2秒）を検出して自動で録音終了（最大12秒）
  → 🔵 Whisper で文字起こし → 呼びかけワード照合
  → 含まれていれば会話AIへ / 無ければ無言で破棄
```

**呼びかけモード**（`/wifi` で切り替え）:

| モード | 動作 | 向き |
|---|---|---|
| 呼びかけワードで起動 | ワードを含む発話だけに応答 | テレビ・人の会話がある部屋 |
| なんでも返事 | 聞き取れた発話すべてに応答（ワード不要） | 静かな部屋・1対1。語頭が拾えなくても無視されない。誤反応とAPI利用は増える |
| 無効 | 画面タッチのみ | 誤反応を完全に防ぎたい時 |

- 呼びかけワードは Web UI で変更可。**カンマ（または読点）区切りで複数登録**できます。
- 照合は**長音・空白を無視**し、5文字以上のワードは**1文字までの聞き違いを許容**
  （「スタッフちゃん」「スタックちゃーん」でもOK）。**3文字以下の短いワードは
  誤爆しやすいので登録しない**のがおすすめ。
- 一瞬の衝撃音（ノック・食器音）ではトリガしません（継続音のみ）。
- **物音のたびに Whisper API が呼ばれる**ため、うるさい場所では設定画面の
  「音量トリガ感度」を大きく（鈍く）するか、呼びかけ起動を無効（画面クリックのみ）に。
- 無音時の Whisper 幻聴（「ご視聴ありがとうございました」等）は自動で弾きます。
- タイマーの定型句（「◯分タイマー」「◯分はかって」等）は会話AIを介さず
  本体だけで即処理されるため、APIの混雑時でも確実に動きます。

### 感度の調整（シリアルモニタを見ながら）

シリアル (115200) に毎秒 `[VAD] peak=823 (threshold=5000)` と出ます。

1. 静かな状態の peak（環境音）と、話しかけたときの peak をメモ
2. 「環境音 < しきい値 < 声」になるよう `/wifi` の音量トリガ感度を設定（保存で再起動）
3. 声の peak が 32000 付近に張り付く場合は**音割れ**。`/setting?mic=16` などで
   マイク感度を下げると Whisper の認識精度が上がります（即反映・再起動不要）

## 通常運用時の Web UI

Wi-Fi 接続後はポート80でWebサーバが起動。同じWi-FiのiPhoneから
**http://chibi-chan.local/**（mDNS/Bonjour。または起動時に画面表示されるIP）へ。

| URL | 内容 |
|---|---|
| `/` | メニュー（設定・ロール・しゃべらせる・文字で質問） |
| `/wifi` | **設定フォーム**（Wi-Fi / APIキー / **会話AI切り替え** / 呼びかけモード / 感度 / 音量 / 天気の場所 / 鍵クリア）→ 保存で再起動 |
| `/role` | ロール（キャラ設定＝システムプロンプト）。SPIFFS保存。感情タグのルールは自動追記 |
| `/role_get` | 現在のロール確認 |
| `/setting?volume=200&mic=48&speaker=3` | 音量・マイク感度・VoiceVox話者の即時変更（再起動不要） |
| `/speech?say=こんにちは` | しゃべらせる |
| `/chat?text=おはよう` | 文字で質問 |
| `/face?expression=1` | 表情変更（0:Neutral 1:Happy 2:Sleepy 3:Doubt 4:Sad 5:Angry） |

| メニュー (`/`) | 設定フォーム (`/wifi`) |
|---|---|
| ![メニュー](https://raw.githubusercontent.com/atatakeisi/chibi-stackchan-rust/ffb5c74706a106e09211d558a028f2fccbaac537/docs/images/webui-menu.jpg) | ![設定フォーム](https://raw.githubusercontent.com/atatakeisi/chibi-stackchan-rust/ffb5c74706a106e09211d558a028f2fccbaac537/docs/images/webui-settings.jpg) |

| ロール設定 (`/role`) | ロール確認 (`/role_get`) |
|---|---|
| ![ロール設定](https://raw.githubusercontent.com/atatakeisi/chibi-stackchan-rust/ffb5c74706a106e09211d558a028f2fccbaac537/docs/images/webui-role.jpg) | ![ロール確認](https://raw.githubusercontent.com/atatakeisi/chibi-stackchan-rust/ffb5c74706a106e09211d558a028f2fccbaac537/docs/images/webui-role-get.jpg) |

設定モード（APポータル `192.168.4.1`）は Wi-Fi 入力だけの専用画面になる。
全項目を触りたいときは末尾のリンク（`/wifi?full=1`）から従来のフォームを開く:

<img src="https://raw.githubusercontent.com/atatakeisi/chibi-stackchan-rust/ffb5c74706a106e09211d558a028f2fccbaac537/docs/images/webui-setup-mode.jpg" width="260" alt="設定モード画面">

### iPhoneのホーム画面に「アプリ化」する

1. Safari で http://chibi-chan.local/ を開く
2. **共有ボタン →「ホーム画面に追加」**
3. 以降はホーム画面のアイコンからワンタップで設定画面へ

> 💡 iPhone が同じルーターの **5GHz に接続していてもOK**（2.4GHz縛りは本体側だけ）。
> つながらない時は、ゲストWi-Fi・「プライバシーセパレータ/AP隔離」がONのSSIDでないか
> 確認を。mDNSが引けない場合は本体起動時に画面表示されるIPを直接入力してください。
> ⚠️ **2台以上を同じWi-Fiで動かすと `chibi-chan.local` が衝突**して、どちらに
> つながるか不定になります。複数台運用時はIP直打ちで（IPは起動時の画面表示か
> 「アイピー教えて」の読み上げで確認できます）。
>
> 💡 **スマホのテザリングで運用する場合**、iOS のインターネット共有は
> `172.20.10.0/28` 固定（iPhone自身が `.1`、クライアントは `.2`〜`.14`）なので、
> 本体はほぼ `http://172.20.10.2/`。テザリング元のiPhone自身のブラウザから開けます。
> iPhone の「インターネット共有」画面には接続台数しか出ず、IPは確認できません。
>
> 💡 IPの読み上げ（`ip.replace(".", "、てん、")`）は聞き取りづらいので、
> **側面ボタンを短く1回押して再起動 → 起動時の画面表示を読む**方が速い。

## 会話AIを Claude にする場合（任意）

Gemini の無料枠が混み合って不通になりがちな場合、会話だけ Claude に切り替えられます。

1. https://platform.claude.com/ にログイン（claude.ai と同じアカウントでOK。**Pro等の
   サブスクとAPIは別料金**）
2. Billing → **Buy credits** でクレジット購入（最低 $5・プリペイド式）。
   **Auto reload は OFF** にしておくと使い切っても勝手に課金されません
3. API Keys → **Create Key** → キーをコピー（一度しか表示されません）
4. 本体の `/wifi` で「Claude API Key」に貼り付け →「会話AI」を **Claude** に → 保存

モデルは `claude-haiku-4-5`（`main.cpp` の `CLAUDE_MODEL` で変更可）。
1会話あたり約0.3円、$5 で2,000回以上会話できます。

## APIの使用量・課金状況を確認する

| サービス | 確認場所 | 見るところ |
|---|---|---|
| Claude (Anthropic) | https://platform.claude.com/ | **Usage**: 日別・モデル別・**APIキー別**のトークン使用量 / **Cost**: 金額換算 / **Settings → Billing**: クレジット残高 |
| OpenAI (Whisper) | https://platform.openai.com/usage | 音声認識の使用量。**録音時間で課金**されるため、会話AIより先にこちらが増えがち |
| Gemini (Google) | https://aistudio.google.com/ | 無料枠の使用状況 |

- **機体ごとに専用のAPIキーを発行する**のがおすすめ。Usage はキー単位でフィルタ
  できるので機体別の使用量が分かり、使いすぎたらそのキーだけ無効化できます。
- コスト目安: 会話1回あたり Claude(haiku)で約0.3円 + Whisper で約0.5円。
  1日100回話しかけても月に数百円程度です。
- Anthropic は **Auto reload を OFF**、OpenAI は **Usage limits** を設定しておくと
  想定外の課金を防げます（プリペイド分を使い切ると止まるだけ）。

### 余談: 実は一番かかるのは「耳」

「AIとの会話」が主役に見えて、コストの内訳は **Whisper(音声認識) の方が高い**。
会話1往復あたり Claude(haiku) 約0.3円に対し、Whisper は約0.5円。

理由は**誤トリガも課金される**から。この設計は
「音量トリガ → 録音 → Whisperで文字起こし → *その後で* 物音かどうか判定」
という順番なので、生活音での誤反応もレンジの「チーン」も、文字起こし料金は発生済み。
**呼びかけワードモードにしても節約にはならない**（照合は文字起こしの後）。

節約したいときに効くのは **Web UI の「音量トリガ感度」を上げる**（録音回数が減る）
の一択。なお最大録音を12秒に延ばしてあるので、騒がしい部屋で誤トリガすると
最大12秒ぶん送られる（静かな部屋なら無音検出で2〜3秒で切れる）。

### OpenAI の支出制限は「アラート」と「上限」が別物

`https://platform.openai.com/settings/organization/limits` に2種類ある:

| 設定 | 動作 |
|---|---|
| **組織の支出制限**（上限） | この額に達すると **API が止まる**。本命の安全網 |
| **支出アラート** | 指定額でメールが来るだけ。**止まらない** |

上限に達すると Whisper が動かなくなり、本体は
**「ごめん、うまく聞き取れなかったよ」としか言わなくなる**（Wi-Fiも顔も正常なので
故障と見分けが付かない）。人に渡す機体では、上限を余裕のある額（月$5程度）に
しておくこと。アラートは上限に連動して自動調整されるので、80%で気づける。

## おまけ: 2台で会話させる

2台のちびスタックちゃんを向かい合わせると、勝手に会話を始めます。
喋っている間は自分のマイクがオフになる（スピーカーと同一チップ）ため、
自分の声には反応せず、相手の声にだけ反応する — という構造がそのまま使えます。

1. 両方とも呼びかけモードを「**なんでも返事**」に
2. **30〜50cm** で向かい合わせる（相手の声が音量トリガに届く距離）
3. `/role` でキャラを変える（例: ツッコミ役とボケ役）。
   「**返事は30文字以内で短く**」「**必ず相手に質問し返して**」を入れるのがコツ
   （長台詞は最大録音12秒からはみ出して会話が噛み合わなくなる。質問がないと
   「そうだね」「うんうん」で収束する）
4. VoiceVox話者番号も変える（`/setting?speaker=8` など。声の聞き分け用）
5. 片方の画面をクリックして「こんにちは、自己紹介して」で着火

止めるときはどちらかの画面を3秒長押し（設定モード落ち）か電源断。
1往復で両側のWhisper/会話AI/VoiceVoxを消費する（約1円/往復）ので、
放置しっぱなしにはご注意を。

## 構成

```
chibi-stackchan/
├── platformio.ini            # ATOMS3R (arduino単独・PSRAM有効)
├── MANUAL.md                 # 受け取った人向けの取扱説明書
├── docs/
│   ├── setup-guide.html      # 初回セットアップ手順だけを図解した単体HTML（スマホ向け）
│   └── images/               # Web UI スクショ + 説明用の作図（本体LCD再現・注釈付き）
├── include/
│   ├── secrets.h.example     # 鍵テンプレート
│   └── secrets.h             # 実際の鍵（git管理外・無くても可）
└── src/
    ├── main.cpp              # 本体（呼びかけ・タイマー・ポータル・Gemini/Claude・Web UI）
    ├── HeadServo.{h,cpp}     # ★SG90 左右首振り（新規）
    ├── WeatherClock.{h,cpp}  # ★NTP時刻 + Open-Meteo天気（新規）
    ├── Whisper / AudioWhisper        # STT（guruguruから移植・プリロール/自動打ち切り対応）
    ├── WebVoiceVoxTTS / AudioOutputM5Speaker / AudioFileSourceHTTPSStream  # TTS
    └── rootCA*.h             # 証明書（rootCAanthropic.h = Claude API用 GTS Root R4/R1）
```

## メモ / 既知の注意点（ATOMS3R固有のハマりどころ）

- **Atomic Echo Base（ES8311）対応は M5Unified 0.2.x 以降**が必要
  （`cfg.external_speaker.atomic_echo = true`）。本プロジェクトは 0.2.17 でビルド確認済み。
  マイクとスピーカーは同一コーデックのため、録音と再生を `M5.Mic`/`M5.Speaker` の
  begin/end で切り替えています（Core2版と同じ流儀）。
- **`Serial`(HWCDC) の出力がUSBに届かない**ため、アプリのログは `printf`
  （IDFコンソール）を使っています。自作ログを足すときも `printf` で。
- **`M5.Mic.isEnabled()` は動作状態を返しません**（ピン設定の有無だけ）。
  マイクの再始動検知は `isRunning()` を使うこと。
- **ES8311 はマイク再始動直後に約0.4秒のフルスケールノイズ**を出すため、
  VADは起動後0.8秒を捨てています（`vad_triggered()` のウォームアップ）。
- **ESP32-S3 の LEDC(PWM) は最大14ビット**。16ビットを指定すると初期化に失敗して
  サーボが動きません（`HeadServo.cpp` は14ビットに設定済み）。
- 顔の縮小は `avatar.setScale(0.4)` + `setPosition(-56, -96)`。ずれる場合はここを微調整。
- サーボはアイドル時 PWM を止めて脱力（省電力・ジー音防止）。
- NVS 名前空間: Wi-Fi=`wifi`、鍵・呼びかけ・天気=`chibi`、音量等=`setting`。
  guruguru（Core2版）の `guruchan` とは独立。

## 今後の課題: 渡した個体をどう更新するか

**未実装。将来やるときのための調査メモ。**

人に渡した個体のファームを更新する手段が無い。相手に VSCode + PlatformIO を
構築してもらうのは現実的でないため、以下の2案を検討した。

### 案A: 設定画面に「ファームウェア更新」ボタン（本命）

相手はすでに設定画面（`chibi-chan.local`）の開き方を知っているので、そこに
更新機能を足すのが一番負担が少ない。USB 接続すら不要になる。

- ESP32 の `Update.h` を使う。**パーティションは変更不要**
  （`default_8MB.csv` に `app1` = `ota_1` @ `0x340000` が既にある。
  現ファームは 1.4MB でスロット 3.1MB に収まる）
- 実装の形は2通り
  - Web UI から `.bin` をアップロードさせる（相手にファイルを渡す必要あり）
  - 本体が URL から自分で取ってくる（ボタン1つ、あるいは「アップデートして」と発話）
- **未解決の検討事項**: GitHub からファームを取る際の TLS 証明書。
  `raw.githubusercontent.com` は途中でリダイレクトして証明書が変わるため、
  既存の `rootCA*.h` 方式では素直に通らない。証明書検証を緩める（`setInsecure()`）と
  実装は楽だが、ファームウェアを検証なしで受け取ることになる。
  配布物のハッシュを別途検証する等、方式を決めてから着手すること
- **注意**: 一度 OTA を配ったあとにパーティションテーブルを変更すると、
  OTA では更新できなくなる（USB からの書き込みが必要になる）。今の表を維持すること

### 案B: ESP Web Tools（OTA が入っていない個体への初回書き込み）

Chrome の WebSerial でブラウザから直接書き込む方式（ESPHome や Adafruit が採用）。
案A を仕込む「最初の1回」はこれが要る。相手は Chrome でページを開いて USB を挿し、
ボタンを押すだけ。インストール作業は無い。GitHub Pages に置けば URL を渡すだけで済む
（public リポジトリなら Pages は無料）。

- **制約**: WebSerial は PC の Chrome / Edge 専用。Safari・Firefox・スマホでは動かない

### ⚠️ 書き込み時に壊してはいけないもの

どちらの案でも共通。**NVS（`0x9000`〜）を消すと Wi-Fi 設定と API キーが飛ぶ。**
API キーは渡した相手に教えていないため、消えると本人では復旧できない。

- **「全消去してから書き込む」は絶対に選ばせない**
- **1本にまとめた `.bin` を `0x0` から焼くのも不可**。
  `esptool --merge-bin` はパーティション間の隙間を `0xFF` で埋めるため、
  `0x9000` の NVS が消える
- **安全策: アプリ部分（`0x10000` の `firmware.bin`）だけを焼く。**
  ブートローダとパーティションテーブルが変わっていなければこれで足りる。
  NVS も SPIFFS（キャッシュ音声）も無傷で残る

現在のフラッシュ配置（`default_8MB.csv`）:

| 領域 | オフセット | 中身 | 焼いてよいか |
|---|---|---|---|
| bootloader | `0x0` | — | 通常は触らない |
| partitions | `0x8000` | — | 通常は触らない |
| **nvs** | **`0x9000`** | **Wi-Fi設定・APIキー** | **絶対に消さない** |
| otadata | `0xe000` | OTA の起動先 | OTA が管理 |
| app0 | `0x10000` | 現ファーム | ここだけ焼けばよい |
| app1 | `0x340000` | OTA の書き込み先 | OTA が使用 |
| **spiffs** | **`0x670000`** | **キャッシュ音声・ロール** | **消すと再取得が要る** |

## ライセンス / クレジット

本プロジェクト自体のコードは **MIT License** です（[LICENSE](https://github.com/atatakeisi/chibi-stackchan-rust/blob/ffb5c74706a106e09211d558a028f2fccbaac537/LICENSE)）。

このプロジェクトは、以下の素晴らしいオープンソースプロジェクトの上に成り立っています。
原作者の皆さまに心から感謝します。

| プロジェクト | 作者 | ライセンス | 本プロジェクトでの役割 |
|---|---|---|---|
| [Stack-chan](https://github.com/meganetaaan/stack-chan) | ししかわ (Shinya Ishikawa / meganetaaan) さん | Apache-2.0 | スタックちゃんの原作。すべてはここから |
| [M5Stack-Avatar](https://github.com/meganetaaan/m5stack-avatar) | 同上 | MIT | 顔・表情・口パク |
| [AI_StackChan2](https://github.com/robo8080/AI_StackChan2) | robo8080 さん | MIT | AIスタックちゃんの母体（STT/TTS/会話の流れはここから派生） |
| [M5Unified / M5GFX](https://github.com/m5stack/M5Unified) | M5Stack | MIT | ハードウェア制御（画面・マイク・スピーカー） |
| [ESP8266Audio](https://github.com/earlephilhower/ESP8266Audio) | Earle F. Philhower, III さん | **GPL-3.0** | MP3再生。⚠️ このライブラリをリンクしたビルド済みバイナリの配布には GPL-3.0 が適用されます（本リポジトリはソースのみ配布） |

そのほかのサービス:

- 天気: [Open-Meteo](https://open-meteo.com/)（無料・APIキー不要）
- 音声合成: [VOICEVOX](https://voicevox.hiroshiba.jp/)（生成音声の利用は各キャラクターの利用規約に従ってください）
- 音声認識: OpenAI Whisper API / 会話: Google Gemini API・Anthropic Claude API
