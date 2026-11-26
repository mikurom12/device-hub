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

/// f2 を 引数なし と conn=0..3 の全部で読んで1行に並べる。
fn f2_all(s: &mut Session<HidIo>, tag: &str) {
    let mut out = String::new();
    let one = |s: &mut Session<HidIo>, p: &[u8]| match s.call(F, 2, p) {
        Ok(r) => format!("{:02x}", r[0]),
        Err(_) => "ERR".into(),
    };
    out += &format!("[]={} ", one(s, &[]));
    for c in 0..4u8 {
        out += &format!("[{c}]={} ", one(s, &[c]));
    }
    println!("f2 全枠 {tag:<14} -> {out}");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut set: Option<u8> = None;
    let mut host = false;
    let mut try_all = false;
    while let Some(a) = args.next() {
        if a == "--set" {
            set = args.next().and_then(|v| v.parse().ok());
        } else if a == "--host" {
            host = true;
        } else if a == "--try" {
            try_all = true;
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
        f2_all(&mut s, "(開始時)");
        if try_all {
            // 引数の形を変えながら f3 を書き、f2 が変わるかを見る (500Hz=2 を狙う。変われば止まる)
            let read = |s: &mut Session<HidIo>, p: &[u8]| s.call(F, 2, p).map(|r| r[0]).ok();
            let base = read(&mut s, &[]);
            println!("baseline f2 = {base:?}");
            let shapes: Vec<(&str, Vec<u8>)> = vec![
                ("[idx]", vec![2]),
                ("[0,idx]", vec![0, 2]),
                ("[1,idx]", vec![1, 2]),
                ("[2,idx]", vec![2, 2]),
                ("[3,idx]", vec![3, 2]),
                ("[idx,0]", vec![2, 0]),
                ("[idx,1]", vec![2, 1]),
                ("[0,0,idx]", vec![0, 0, 2]),
            ];
            let mut hit = None;
            for (name, p) in &shapes {
                let r = s.call(F, 3, p);
                std::thread::sleep(Duration::from_millis(400));
                let now = read(&mut s, &[]);
                let now0 = read(&mut s, &[0]);
                let now1 = read(&mut s, &[1]);
                println!("f3 {name:<10} reply={:<6} -> f2={now:?} f2[0]={now0:?} f2[1]={now1:?}",
                    if r.is_ok() { "ok" } else { "ERR" });
                if now != base || now0 != base || now1 != base {
                    println!("★ 変化あり: {name}");
                    hit = Some(p.clone());
                    break;
                }
            }
            if let Some(mut p) = hit {
                // 同じ形で 1000Hz(=3) に戻す
                if let Some(x) = p.iter_mut().find(|x| **x == 2) { *x = 3; }
                let _ = s.call(F, 3, &p);
                std::thread::sleep(Duration::from_millis(400));
                println!("復元後 f2 = {:?}", read(&mut s, &[]));
            } else {
                println!("どの形でも変化なし (現在値は元のまま)");
            }
            continue;
        }
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
                f2_all(&mut s, &format!("(+{ms}ms)"));
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
