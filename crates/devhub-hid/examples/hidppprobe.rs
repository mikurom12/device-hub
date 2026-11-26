//! Logitech HID++ 2.0 診断 (読み取り専用)。PRO 2 LIGHTSPEED の実機でプロトコルを確定するための道具。
//!
//! ```sh
//! cargo run -p devhub-hid --example hidppprobe            # VID 046d の全 HID IF を試す
//! cargo run -p devhub-hid --example hidppprobe -- --vid 046d --pid c09a
//! ```
//!
//! やること: 各 vendor 定義 IF (usage page >= 0xFF00) を開き、HID++ の ping を打ち、
//! 応答した IF で機能テーブル (feature id → index)・名前・FW・電池・DPI リスト/現在値・
//! ポーリングレート・オンボードモードを**読むだけ**。設定の書き込みは一切しない。
//! 出力をそのまま貼れば、registry の usage page/usage と機能の有無を確定できる。

use devhub_hid::logitech::HidIo;
use devhub_hidpp::features as f;
use devhub_hidpp::{find_device_index, Session};
use hidapi::HidApi;

fn arg(name: &str) -> Option<u16> {
    let mut it = std::env::args();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok());
        }
    }
    None
}

fn main() {
    let vid = arg("--vid").unwrap_or(0x046D);
    let pid = arg("--pid");
    let api = HidApi::new().expect("hidapi init");

    println!("== HID インターフェース (vid {vid:04x}) ==");
    let mut targets = Vec::new();
    for d in api.device_list().filter(|d| d.vendor_id() == vid) {
        if pid.is_some_and(|p| p != d.product_id()) {
            continue;
        }
        println!(
            "{:04x}:{:04x} if={} up={:04x} u={:04x} {:?}",
            d.vendor_id(),
            d.product_id(),
            d.interface_number(),
            d.usage_page(),
            d.usage(),
            d.product_string()
        );
        if d.usage_page() >= 0xFF00 {
            targets.push((d.product_id(), d.usage_page(), d.usage(), d.path().to_owned()));
        }
    }
    if targets.is_empty() {
        println!("\nvendor 定義 IF が見つかりません。マウスを有線接続 / レシーバー接続して再実行してください。");
        return;
    }

    for (pid, up, u, path) in targets {
        println!("\n== {vid:04x}:{pid:04x} up={up:04x} u={u:04x} を HID++ で試す ==");
        let dev = match api.open_path(&path) {
            Ok(d) => d,
            Err(e) => {
                println!("open 失敗: {e}");
                continue;
            }
        };
        let (io, idx) = find_device_index(HidIo::new(dev));
        let idx = match idx {
            Ok(i) => i,
            Err(e) => {
                println!("HID++ 2.0 の応答なし: {e}  (この IF は HID++ ではない / マウスがスリープ)");
                continue;
            }
        };
        println!("HID++ 2.0 応答あり: device index = {idx:#04x}  ← この (usage_page, usage) を registry に残す");
        let mut s = Session::new(io, idx);
        if let Ok((maj, min)) = s.ping() {
            println!("protocol {maj}.{min}");
        }
        match s.enumerate() {
            Ok(t) => {
                println!("-- 機能テーブル --");
                for (i, id, ty, ver) in t {
                    println!("  [{i:02}] {id:04x} v{ver} type={ty:02x}  {}", f::feature_name(id));
                }
            }
            Err(e) => println!("機能テーブル取得失敗: {e}"),
        }
        println!("name      : {:?}", f::device_name(&mut s));
        println!("firmware  : {:?}", f::firmware(&mut s));
        println!("battery   : {:?}", f::battery(&mut s));
        println!("dpi list  : {:?}", f::dpi_list(&mut s, 0));
        println!("dpi now   : {:?}", f::get_dpi(&mut s, 0));
        println!("rates     : {:?}", f::report_rates(&mut s));
        println!("rate now  : {:?}", f::report_rate(&mut s));
        println!("onboard   : {:?}", f::onboard_mode(&mut s));
    }
}
