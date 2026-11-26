# Logitech G (HID++ 2.0) — PRO 2 LIGHTSPEED

G HUB 置換の第 1 段階。**実機 (PRO 2 LIGHTSPEED) での検証は未了**。この文書は
「何が分かっていて、何が未確認か」を分けて書く。

## 構成

```
crates/devhub-hidpp    HID++ 2.0 プロトコル (HID バックエンド非依存・単体テストあり)
  src/lib.rs           フレーム・Session (swid 突き合わせ・エラーフレーム・機能 index 解決)・デバイス番号探索
  src/features.rs      DPI / ポーリング / 電池 / FW / 名前 / オンボードモード
crates/devhub-hid/src/logitech.rs   DeviceDriver 実装 (UI との橋渡し・再接続時の再適用)
crates/devhub-hid/examples/hidppprobe.rs   読み取り専用の診断ツール
```

## 識別

| 接続 | VID:PID | 出典 |
|---|---|---|
| 有線 | 046d:c09a | Linux カーネル `hid-logitech-hidpp.c` の PRO 2 LIGHTSPEED 追加パッチ (2025-08) |
| LIGHTSPEED 受信機 | 046d:c543 | 同 `hid-logitech-dj.c` (`NANO_RECEIVER_LIGHTSPEED_1_2`) |

### 実機で確認できたこと (2026-11-26、受信機 046d:c543 経由、`hidppprobe` の出力)

- HID++ の IF は **usage page 0xFF00 / usage 0x0002** (long report 0x11)。usage 0x0001 / 0x0004 は書込みが
  `WriteFile` エラーになり HID++ ではない。FF43/0202 の行は不要なので受信機の分は削除した。
- デバイス番号は **0x01** (受信機のペア番号)、プロトコルは **4.2**、名前は `PRO 2 LIGHTSPEED`。
- ファームウェア: MPM 31.04.B0012 / BL1 70.00.B0007。バッテリー: UnifiedBattery (0x1004) で 99%。
- **DPI は AdjustableDpi (0x2201) ではなく ExtendedAdjustableDpi (0x2202)**。0x2201 は無い。
  → `features.rs` に 0x2202 を実装済み (func1 能力 / func2 リスト / func5 取得 / func6 設定)。
  実機での DPI 読み書きは**まだ試していない**。
- ポーリングは ExtendedReportRate (0x8061)。この接続 (受信機) では 125 / 250 / 500 / 1000 Hz の
  4 段階だけが報告された (8K は出なかった)。有線では未確認。
- OnboardProfiles (0x8100) あり。読んだ時点のモードは **Host** (G HUB が設定した状態)。
- 機能テーブルに ReprogControlsV4 (0x1B04) は**無い**。ボタン割り当ては別経路
  (おそらくオンボードプロファイル 0x8100) で、まだ解析していない。
- そのほか RgbEffects 0x8071 / 0x2250 / 0x2251 / 0x8090 / 0x80E0 / 0x8110 / 0x1D4B 等を持つ (未解析)。

有線 (046d:c09a) の usage は未確認。registry には FF00/0002 と FF43/0202 の両方を残してある。

## 実装したプロトコル (出典: Solaar `hidpp20*.py` / `settings_templates.py`、libratbag `hidpp20.c`)

フレーム: `[0x11, dev, feature_index, (function<<4)|swid, params…]` (20B)。応答は同じ
`feature_index` と `(function<<4)|swid` をエコーする。エラーは `feature_index=0xFF` で
`[0x11, dev, 0xFF, 失敗した feature_index, 失敗した func|swid, code]` (HID++ 1.0 は `0x8F`)。
`dev` は有線直結なら `0xFF`、レシーバー経由ならペア番号 1..6 (`find_device_index` が ping で探す)。
swid は `0x0D` (Solaar は `0x0B`)。

| 機能 | ID | 使う関数 |
|---|---|---|
| Root | 0x0000 | f0 GetFeature(id)→index / f1 ping |
| FeatureSet | 0x0001 | f0 count / f1 index→id |
| DeviceFwVersion | 0x0003 | f0 count / f1 entry |
| DeviceName | 0x0005 | f0 len / f1 chunk |
| UnifiedBattery / BatteryVoltage / BatteryStatus | 0x1004 / 0x1001 / 0x1000 | 順に試す |
| AdjustableDpi | 0x2201 | f1 list / f2 get / f3 set (リストは `0xE000\|step` + 上限の範囲表現あり)。PRO 2 には無い |
| ExtendedAdjustableDpi | 0x2202 | f1 caps / f2 list `[0,dir,chunk]` / f5 get / f6 set `[0,X,Y,LOD]` — **PRO 2 はこちら** |
| ReportRate | 0x8060 | f0 list(ms bitmask) / f1 get / f2 set |
| ExtendedReportRate | 0x8061 | f1 list(bit i = 125Hz<<i) / f2 get / f3 set — **8K はこちら** |
| OnboardProfiles | 0x8100 | f1 set mode / f2 get mode (1=Onboard, 2=Host) |

## 現状の機能と制限

できる: DPI (単一値)・ポーリングレート (8K 含む)・オンボード/ホスト切替・電池・FW 表示・
再接続時に直近の DPI/レートを再適用 (`%APPDATA%\DeviceHub\logitech\046d_c09a.json`)。
書込み後は必ず読み戻し、反映されなければ UI にエラーを出す (オンボードモードで無視される機器対策)。

できない (実機のトレースが必要):

- **DPI ステージ複数 / ボタン割り当て / マクロ**: オンボードプロファイル領域
  (0x8100 の sector 読み書き・CRC-CCITT) の実装と、書込みが安全であることの実機確認が要る。
  壊すと G HUB で復旧が必要になるため、トレース無しでは入れていない。
- **ボタンのダイバート** (ReprogControlsV4 0x1B04): ホストモードでボタン機能を
  ソフトで処理する場合に必要。入力の再注入 (SendInput) も要る。
- **ライティング** (0x8071 RGB_EFFECTS / 0x8070)。
- **ファームウェア更新**: 非対応。G HUB が唯一の経路。

注意: ホストモードではマウス本体の DPI ボタン等の挙動がアプリ依存になる。既定は変更しない
(UI の「設定の保持先」で切替)。

## 実機での検証手順

1. G HUB は起動したままでよい (HID++ は swid で多重化される)。ただし G HUB が同じ項目を
   書き換えると競合するので、変更の検証は G HUB を止めてから行う。
2. `cargo run -p devhub-hid --example hidppprobe` — 出力を保存 (これが最重要の一次資料)。
   `HID++ 2.0 応答あり` が出た IF の (usage page, usage) が registry の正解。
3. `cargo run --release -p device-hub` でマウスを選び、DPI とポーリングレートを変える。
   反映は `hidppprobe` の `dpi now` / `rate now`、または polling 計測 (テスト画面) で確認。
4. 問題なければ registry の `verified: true` と usage の確定、この文書の「未確認」を更新。
5. その後 `scripts/switch-from-ghub.ps1` (既定はドライラン)。

## G HUB の通信を取ってプロトコルを広げる

USBPcap で G HUB の操作 (DPI ステージ変更・ボタン割り当て) を記録し、`0x11` で始まる
OUT/IN をデコードする (`docs/capture.ps1`, `docs/USBPCAP_NATIVE_HANDOFF.md` 参照)。
1 項目だけ変えて差分を見る方法は `docs/ADD_DEVICE.md` §2.3 と同じ。

## ポーリングレート (0x8061) の実機知見 (PRO 2 + 2.4GHz 受信機 c543, 2026-11-27)

- f1 (対応一覧) は引数に関わらず `00 0f` = 125/250/500/1000Hz。受信機経由では 2000Hz 以上は出ない。
- 書込みは `f3 [idx]` (接続種別なし) でよく、**反映されるのは f2 の conn=1 の枠だけ**。
  f2 を引数なし / conn=0 で読むと常に 1000Hz のまま、conn=2,3 はエラー。
- そのため `report_rate()` は f2 `[1]` を優先して読み、失敗時のみ引数なしにフォールバックする。
- オンボードモードは既に Host で、ホスト/オンボード切替は結果に影響しなかった。
- 有線 (c09a) で枠が 0 になるのか 1 のままか、8000Hz が出るかは未確認。
