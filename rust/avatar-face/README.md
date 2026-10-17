# Avatar face in Rust

`stackchan-avatar-face` は LCD・I²C・Wi-Fi から独立した顔描画ライブラリです。
元の `M5Stack-Avatar` 0.10.0 の `Face.cpp`・`Eye.cpp`・`Mouth.cpp` を基に、
このプロジェクトの C++ 側で指定していた縮尺 `0.4` と `setPosition(-56, -96)` を再現します。
元ライブラリの MIT ライセンスと著作権表示は [LICENSE](LICENSE) に保持しています。

- 白い小さな丸目、四角い口、表情ごとの目の切り欠き。
- 口の幅 90→50、高さ 4→60、目の半径 8 は元の 320×240 座標で保持。
- 元の 1bit 描画と同様に白・背景の二色で描画し、縮小・回転は顔全体に適用。
- まばたき、視線、呼吸、口の開き、傾きは `FaceParams` で指定。`animation::Animator` が時刻に応じたまばたき・視線・呼吸と、左右の視線に応じた ±5 度の傾きを生成。
- 描画時のヒープ確保や一時 320×240 バッファは不要。外部 crate への依存なし。
- 標準の眉の高さは 0 なので眉を描かず、追加した頬・ハイライト・ラベルも描きません。

`render_rgb565_be` は 128×128 の RGB565 フレームへ直接書き込みます。
元の LovyanGFX のピクセル中心に合わせて逆変換し、背景色は RGB565 で指定します。
背景を塗った後、目・口を含む範囲だけを逆変換し、顔のない領域の計算を省きます。
`Doubt` は元の標準顔と同じ目の形です。
回転は浮動小数点で処理するため LovyanGFX の固定小数点処理と境界画素が異なる場合があります。
元の右目の視線にあった縦横値の取り違えは修正しています。顔の形状・配置を基にした移植で、動きまで画素単位で同一とはしていません。

ホスト確認:

```sh
cargo test --manifest-path rust/avatar-face/Cargo.toml --offline
cargo run --manifest-path rust/avatar-face/Cargo.toml --example preview --offline -- /private/tmp/stackchan-avatar-face.ppm
```

ホストテスト 12 件で、形状・表情・バッファ境界・アニメーションを検証しています。
描画範囲を絞った最適化は、512 通りの表情・回転などで元の全画面サンプリングと画素が一致することを確認しています。

![6 表情の描画プレビュー](../../docs/images/rust-avatar-face-preview.png)

プレビューは左から Neutral / Happy / Sleepy / Doubt / Sad / Angry、上段は口閉じ、下段は口開きです。
