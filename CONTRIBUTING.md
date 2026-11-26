# Contributing

ありがとうございます。特に歓迎しているのは **実機を持っている人による 1 機種 1 PR のデバイスドライバ** です。

## デバイス対応 PR の作り方

1. [docs/ADD_DEVICE.md](docs/ADD_DEVICE.md) と [docs/PROTOCOL_NOTES.md](docs/PROTOCOL_NOTES.md) を読み、読み取り解析から始める。
2. `crates/devhub-hid` に driver を追加し、`crates/devhub-core/src/registry.rs` に登録する。
3. UI は device name / VID / PID で分岐せず、`Capabilities` / `SettingSpec` / `ActionSpec` だけで出し分ける。
4. 契約 `devhub-core/src/model.rs` は**追加のみ**。既存 enum/struct の意味は変えない。demo backend も更新する。
5. 実機で確認した内容を PR 本文に書く: 接続方式 (wired / dongle / Bluetooth)、読み取り・書き込み・往復確認、未検証項目。
6. 解析に使った Web ツール・コード・配布物の出どころとライセンスを `THIRD_PARTY_NOTICES.md` に追記する。
7. ファームウェア更新・DFU・ブートローダー操作は対象外。

## マージ前チェック

```sh
cargo fmt --check
cargo clippy --workspace --locked -- -D warnings
cargo test --workspace --locked
```

- 1 PR は 1 機種または 1 つの独立した修正にしてください。
- 実機で確認していない書き込みは「未検証」と明記してください。
- 画像や README に個人情報、シリアル番号、ローカルパスを入れないでください。

小さな修正 (typo、リンク、テスト追加) も歓迎です。
