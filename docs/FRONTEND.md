# フロントエンドの構造 (crates/device-hub)

```
src/
  main.rs        窓を開く (TitleBar::window_options, 1320x840, 背面時 250ms 間引き)
  app.rs         Shell: 全状態・ルート (Home / Device / Settings)・タブ・サイドバー・キー入力・ドラッグ
  store.rs       Hub / Session: デバイスごとのワーカースレッド、楽観的反映 (apply_local)、futures channel で結果受信
  overlay.rs     ポップアップメニュー・モーダル (確認 / ボタン割り当て / キー取り込み / 色 / テキスト)・トースト
  ui.rs          部品: card / row / toggle / slider(_curve) / segmented / select / stepper / tab_bar / button / chip
  glass.rs       ガラス面 (Panel / Clear / Frost / Sheet)。Liquid Glass の泡レンズと Frost Graphite の粒子
  motion.rs      実時間補間 (bezier `at` / 厳密解ばね `at_spring` / `presence` / hover `feel`)・要素矩形の記録。動いている間だけ次フレーム要求
  theme.rs       テーマ系統 (Liquid Glass / Frost Graphite)、配色 (ダーク/ライト)・フォント
  wallpaper.rs   オーロラ / Frost Graphite のモノクロ壁紙の生成 / ユーザー画像
  prefs.rs       %APPDATA%\DeviceHub\prefs.json
  keycode.rs     GPUI キー名 → HID usage
  palette.rs     コマンドパレット (Ctrl+K)
  presets.rs     PC に保存するプリセット (%APPDATA%\DeviceHub\presets\<model>\*.json)
  pages/
    home.rs       デバイス一覧
    device.rs     ヘッダ (名前・接続・プロファイル・同期) + タブ + 読込中/失敗/未対応
    generic.rs    SettingSpec / ActionSpec を section ごとのカードに自動配置
    mouse.rs      DPI ステージ・ボタン割り当て
    keyboard.rs   キーボード図 (共通)・キー割り当て・トリガー (磁気スイッチ / ストロークテスト)
    advanced.rs   高度キー (DKS/MPT/MT/TGL/END/SOCD/RS)
    lighting.rs   照明 (領域・エフェクト・色・キーごとの塗り)
    macros.rs     マクロ (記録・挿入・遅延編集)
    system.rs     デバイス情報・プロファイル・書き出し/読み込み・メンテナンス
    settings.rs   アプリ設定
    test.rs       テスト (クリック/ホイール/チャタリング、キー入力・同時押し)
    calibrate.rs  磁軸キャリブレーション・ウィザード
```

## タブの出し分け (`app::tabs_for`)

| caps | タブ |
|---|---|
| mouse | DPI, ボタン (buttons があれば), 性能 (performance/sensor/click/magnetic/power の設定があれば) |
| keyboard | キー割り当て, トリガー (perf), 高度キー (advanced) |
| lighting / lighting section | ライティング |
| macros | マクロ |
| 常に | 一般 (system / connection / keyboard / 未知の section、情報・プロファイル・バックアップ) |

## データの流れ

UI 操作 → `Shell::apply(Change)` → `Hub::apply` が UI の状態を即書き換え (apply_local) → ワーカーへ送信 →
ワーカーが同じ対象の古い変更を捨てて `DeviceDriver::apply` → 結果 (`Msg::Applied`) → 失敗ならトースト + `read` し直し。
ヘッダの点: 黄 = 書き込み待ちあり / 緑 = 同期済み。

## テーマ系統

`theme.rs` は系統 (Liquid Glass / Frost Graphite) と明暗を独立して持ち、`prefs.json` の `theme_family` に保存する
(既存ファイルでは既定 `liquid`)。Frost Graphite は `docs/mockups/frost-graphite.html` の TOKENS を `Pal` に写した白黒テーマで、
選択値・主ボタンだけを `inv` / `inv_fg` に反転し、セグメント選択は霜チップ、タブは下線、壁紙はクローム球のモノクロになる。
Liquid Glass 側の色と部品の既定値は変えない。切替はアプリ設定の「デザイン」またはコマンドパレット。
Graphite のホームカードには caps (mouse / keyboard / peq) から選んだモノクロ線画を出し、DPI タブでは ←/→ で編集ステージを移動できる
(テキスト入力・モーダル・コマンドパレットが前面のときは従来どおりそちらへ渡す)。

Graphite のガラスは `glass.rs` で面ごとに分ける。外側の `Panel` / 浮いた `Clear` は外周だけ薄く屈折し、
画素ごとのリム光 (左上を主光、右下を弱い補助光) と連続リムを乗せる。カード内の操作部は `Frost` に解決し、
再屈折させずに弱い曇り・細い反射・下端の落ち影だけにする (`NEST_HOSTS` は親カードより後に描かれる子が
二度曲げないようにフレームごとへ張る矩形リスト)。RGB 分散は白黒テーマなので 0.018 に抑え、
粒子は通常 alpha 合成で汚れない強さに焼き込む。Liquid Glass の既存パラメータは変えない。

## モーション

`motion.rs` は Valorant tracker と同じ考え方で、実時間だけを使って補間する。`at` は cubic-bezier
(ホバー 140ms、メニュー 180/120ms、ダイアログ 220/150ms、インジケータ 200ms) で、
タブ下線・セグメントの選択チップ・トグル位置・presence・ホバー/押下はこの行き過ぎない曲線に統一している。
ページ/デバイス/タブの内容は切替時に fade せず即時に差し替え、点滅やガラスの明滅を避ける。選択行の霜チップと
Graphite のトグル色や性能タイルの選択色も補間せず即時にし、動きはタブ下線・セグメントチップのスライドとつまみの移動だけに残す。
壁紙はクロスフェード。
`at_spring` はフレームレートに依らない厳密解ばねで、DPI 数字・スライダー・live depth など連続値にだけ使う。
ばねは臨界減衰を使い、逆向きの速度は retarget 時に切り、目標を跨ぐステップでは target に貼り付け、距離 0.002 未満で静止する
(overshoot と末尾の sub-pixel の這いを防ぐ)。`prefs.json` の `motion=false` では全補間が即時値になり、GPUI vendor の
`smooth_wheel` も切る。スクロール領域は `.overflow_*_scroll().smooth_wheel()` を付けたものだけバネ補間される。
`end_frame` は補間中だけ `request_animation_frame` するので、アイドル時は従来どおり再描画しない。

## 新しい画面が必要になったら

構造を持つ新機能が契約に追加されたとき (`docs/FRONTEND_TODO.md`) だけ。`pages/` にファイルを足し、
`Tab` と `tabs_for` に 1 行。スカラー設定・操作は generic.rs が自動で描くので画面追加は不要。

## 動作確認

```bash
DEVHUB_DEMO=1 cargo run -p device-hub
```
デモは HTS Plus Pro (無線) / HTX Ultra (有線・磁気ボタン) / SU66 Max / ATK F1 (未対応) の 4 台。

## Web 版より便利にしている機能 (フロントだけで実現)

| 機能 | 場所 | 中身 |
|---|---|---|
| 元に戻す / やり直し | ヘッダの ↶↷、Ctrl+Z / Ctrl+Y | 変更前の `DeviceState` を履歴に積み、差分 (`store::diff_changes`) を書き戻す。0.8 秒以内の連続操作は 1 回分 |
| プリセット | 一般タブ・ヘッダのドロップダウン | 現在の設定を PC に名前付きで保存し、ワンクリックで適用 (差分だけ書く)。オンボードの枠数に縛られない |
| コマンドパレット | Ctrl+K / サイドバーの検索 | 画面・設定・操作・デバイスを名前で探して移動・実行 |
| テスト | テストタブ | クリック回数・間隔・チャタリング疑い、ホイール、移動イベント数、キー入力・同時押し数・二重入力 |
| キャリブレーション・ウィザード | 一般タブの「磁軸キャリブレーション」 | 押下深さを見ながら、底まで押したキーを緑で表示 |
| トリガーのプリセット | トリガータブ | 最速 RT / 競技 RT / 標準 / タイピングを選択キーに一括適用 |
| キー検索 | キー割り当て | 全グループから名前で検索 |
| レイヤーのコピー | キー割り当て | 表示中のレイヤーを別レイヤーへ複製 |
| 進捗表示 | ヘッダ | `HubEvent::Progress` を受けて進捗バー |
| バッテリー低下の通知 | トースト | 15% 以下で 1 回 |
| 設定プロファイル | 自動化 (サイドバー) | デバイス画面での変更を `Change` 列として録画し、任意タイミングで再生 (prefs.json の `profiles`)。「今の状態」でスナップショット記録 (設定/DPI/性能/ライティング/EQ — キー割り当て等の構造系は除外)、「名前」でリネーム (ルール参照も追従) |
| プロセス連動 | 自動化 | `proc_rules` の exe 名がフォアグラウンドの間 `on_fg` プロファイルを適用、外れたら `on_leave`。`procmon.rs` が ~1 秒おきに前面窓を見る (最小化中も動作・実機検証済)。プロセスは「実行中のアプリ」ドロップダウン (可視窓を持つ exe 一覧+タイトル) から選択可、ルールは ON/OFF トグル・編集・実行中チップつき |
| 常時レート計測 | 性能/テストタブ | Raw Input `RIDEV_INPUTSINK` で実測 Hz を更新 (~400ms おき、デバイス画面を開いている間)。**Windows 11 (Moment 3以降) は背面プロセスへの raw input 配信を制限する**ため、窓が背面の間は値が落ちて信用できない — 表示側は `last_window_active` で出し分け、背面中は「OS が入力を制限中」と薄字表示にする (MouseTester と同じ OS 制限)。計測スレッドは MMCSS "Pro Audio" + ABOVE_NORMAL、間隔の中央値→Hz 逆算と p95 ばらつき警告も併記 |

### ポーリング真値の検証 (アプリ外)

WM_INPUT は「OS が届けた分」を数える層なので、デバイスが実際にワイヤに出したレポート数
(真の 8K 検証) を取りたいときは **USBPcap + Wireshark** を使う:

1. USBPcap をインストール (カーネルフィルタドライバ — 管理者権限が要る)。
2. Wireshark で対象ホストコントローラを選択してキャプチャ開始。
3. マウスを動かす → `usb.bInterfaceClass == 3` / interrupt-IN URB の
   `usb.dst` (host) 方向のフレーム間隔を読む。8K なら ~0.125ms 間隔が並ぶ。
4. ドングル経由の場合も同じ — レシーバーがバスに出す分が見える。

ベンダー I/F (ff02 等) の直接 read は代替にならない — 自発レポートはイベント通知のみ
(実測 ~10Hz) で、モーションデータはシステム TLC 専用 (Windows はそちらを読ませない)。

