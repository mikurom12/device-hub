---
name: 新しいデバイスの対応を相談する
about: 未対応デバイスの解析情報を共有したり、driver 追加を相談したりする
title: "[device] <メーカー・機種名>"
labels: device-support
---

## デバイス

- メーカー / 機種名:
- 接続方式 (wired / 2.4GHz dongle / Bluetooth):
- VID / PID / usage page / usage (`cargo run -p devhub-hid --example enum` の出力):
- メーカーの設定ツール (WebHID / アプリ) と URL:

## 分かっていること

読み取れた設定、キャプチャ、protocol メモ、同じプロトコルらしい既存機種など。

## やりたいこと

- [ ] 自分で driver を書くので相談したい
- [ ] 解析情報を提供するので実装を手伝ってほしい
- [ ] 既存フォーク/ブランチの driver がある

## 安全確認

- [ ] ファームウェア更新 / DFU は求めていない
- [ ] まず読み取りと往復確認から進めることを理解した
