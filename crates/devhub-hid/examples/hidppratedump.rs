//! ExtendedReportRate (0x8061) の生データ診断。ポーリングレートが書き込めない原因調査用。
//!
//! ```sh
//! cargo run -p devhub-hid --example hidppratedump             # 読むだけ
//! cargo run -p devhub-hid --example hidppratedump -- --set 2  # index 2 (=500Hz) を書いて読み戻す
//! ```
//! index: 0=125Hz 1=250Hz 2=500Hz 3=1000Hz (4=2000 5=4000 6=8000)。
//! `--set` は ReportRate だけを書く (DPI などには触れない)。元に戻すには `--set 3`。
//!
//! ```sh
//! cargo run -p devhub-hid --example hidppratedump -- --set 2 --host
//! ```
//! `--host` を付けると、書込み前にホストモード(0x8100 mode=2)へ切り替え、読み戻したあと
//! 元のモードに必ず戻す。オンボードプロファイルがレートを上書きしている疑いの検証用。
//! ホストモードの間 (数秒) ボタンやライトの挙動が変わることがある。

use devhub_hid::logitech::HidIo;
use devhub_hidpp::features::{onboard_mode, set_onboard_mode, OnboardMode, EXTENDED_REPORT_RATE as F};
use devhub_hidpp::{find_device_index, Session};
use hidapi::HidApi;
use std::time::Duration;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut set: Option<u8> = None;
    let mut host = false;
    while let Some(a) = args.next() {
        if a == "--set" {
            set = args.next().and_then(|v| v.parse().ok());
        } else if a == "--host" {
            host = true;
        }
    }
    let api = HidApi::new().expect("hidapi init");
    for d in api
        .device_list()
        .filter(|d| d.vendor_id() == 0x046D && d.usage_page() == 0xFF00 && d.usage() == 0x0002)
    {
        println!("== {:04x}:{:04x} up=ff00 u=0002 ==", d.vendor_id(), d.product_id());
        let Ok(dev) = api.open_path(d.path()) else {
            println!("open 失敗");
            continue;
        };
        let (io, idx) = find_device_index(HidIo::new(dev));
        let Ok(idx) = idx else {
            println!("HID++ 応答なし");
            continue;
        };
        let mut s = Session::new(io, idx);
        match s.feature(F) {
            Ok(Some(f)) => println!("feature 0x8061: index={} version={}", f.index, f.version),
            other => {
                println!("0x8061 なし: {other:?}");
                continue;
            }
        }
        let show = |s: &mut Session<HidIo>, label: &str, func: u8, params: &[u8]| {
            match s.call(F, func, params) {
                Ok(p) => println!("{label:<28} -> {}", hex(&p)),
                Err(e) => println!("{label:<28} -> ERR {e}"),
            }
        };
        show(&mut s, "f0 capabilities", 0, &[]);
        for c in 0..4u8 {
            show(&mut s, &format!("f1 rate list [conn={c}]"), 1, &[c]);
        }
        show(&mut s, "f1 rate list []", 1, &[]);
        show(&mut s, "f2 current []", 2, &[]);
        let orig_mode = onboard_mode(&mut s);
        println!("onboard mode (現在)          -> {orig_mode:?}");
        if let Some(i) = set {
            if host {
                println!("\n-- ホストモードへ切替 --");
                println!("set_onboard_mode(Host)       -> {:?}", set_onboard_mode(&mut s, OnboardMode::Host));
                std::thread::sleep(Duration::from_millis(300));
                println!("onboard mode (切替後)        -> {:?}", onboard_mode(&mut s));
                show(&mut s, "f2 current (host直後)", 2, &[]);
            }
            println!("\n-- 書込み: f3 [{i}] --");
            show(&mut s, &format!("f3 set [{i}]"), 3, &[i]);
            for ms in [50u64, 200, 600] {
                std::thread::sleep(Duration::from_millis(ms));
                show(&mut s, &format!("f2 current (+{ms}ms)"), 2, &[]);
            }
            if host {
                // 元のモードへ必ず戻す (戻した後にレートがどうなるかも見る)
                let back = orig_mode.as_ref().ok().copied().unwrap_or(OnboardMode::Onboard);
                println!("\n-- 元のモードへ復帰 --");
                println!("set_onboard_mode({back:?})   -> {:?}", set_onboard_mode(&mut s, back));
                std::thread::sleep(Duration::from_millis(300));
                println!("onboard mode (復帰後)        -> {:?}", onboard_mode(&mut s));
                show(&mut s, "f2 current (復帰後)", 2, &[]);
            }
        }
    }
}
