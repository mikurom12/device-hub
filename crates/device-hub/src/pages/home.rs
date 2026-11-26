//! ホーム: 接続中のデバイスの一覧。

use crate::app::Shell;
use crate::ui::{self, Btn};
use crate::{glass, motion, theme};
use devhub_core::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::sync::Arc;


#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceArt {
    Mouse,
    Keyboard,
    Dac,
    Other,
}

/// 線画の種別は開いたセッションの caps だけで決める。
/// caps がまだ届いていない最初の一瞬だけ DeviceKind の粗い分類に退避する。
fn device_art_kind(this: &Shell, d: &DeviceInfo) -> DeviceArt {
    if let Some(caps) = this.hub.session(&d.id).and_then(|s| s.caps.as_ref()) {
        if caps.mouse.is_some() {
            DeviceArt::Mouse
        } else if caps.keyboard.is_some() {
            DeviceArt::Keyboard
        } else if caps.peq.is_some() {
            DeviceArt::Dac
        } else {
            DeviceArt::Other
        }
    } else {
        match d.kind {
            DeviceKind::Mouse => DeviceArt::Mouse,
            DeviceKind::Keyboard => DeviceArt::Keyboard,
            DeviceKind::Other => DeviceArt::Other,
        }
    }
}

fn rounded_rect_path<F>(
    pb: &mut PathBuilder,
    at: &F,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
) where
    F: Fn(f32, f32) -> Point<Pixels>,
{
    pb.move_to(at(x + r, y));
    pb.line_to(at(x + w - r, y));
    pb.curve_to(at(x + w, y + r), at(x + w, y));
    pb.line_to(at(x + w, y + h - r));
    pb.curve_to(at(x + w - r, y + h), at(x + w, y + h));
    pb.line_to(at(x + r, y + h));
    pb.curve_to(at(x, y + h - r), at(x, y + h));
    pb.line_to(at(x, y + r));
    pb.curve_to(at(x + r, y), at(x, y));
    pb.close();
}

fn circle_path<F>(pb: &mut PathBuilder, at: &F, cx: f32, cy: f32, r: f32)
where
    F: Fn(f32, f32) -> Point<Pixels>,
{
    let k = r * 0.552_284_8;
    pb.move_to(at(cx, cy - r));
    pb.cubic_bezier_to(at(cx + r, cy), at(cx + k, cy - r), at(cx + r, cy - k));
    pb.cubic_bezier_to(at(cx, cy + r), at(cx + r, cy + k), at(cx + k, cy + r));
    pb.cubic_bezier_to(at(cx - r, cy), at(cx - k, cy + r), at(cx - r, cy + k));
    pb.cubic_bezier_to(at(cx, cy - r), at(cx - r, cy - k), at(cx - k, cy - r));
    pb.close();
}

fn paint(window: &mut Window, pb: PathBuilder, color: Hsla) {
    if let Ok(path) = pb.build() {
        window.paint_path(path, color);
    }
}

/// Frost Graphite のホームカードに置く、モノクロの大きな線画。
/// docs/mockups/frost-graphite.html の ART を GPUI path へ簡略化したもの。
fn graphite_device_art(kind: DeviceArt, color: Hsla) -> impl IntoElement {
    let (w, h) = match kind {
        DeviceArt::Mouse => (74., 118.),
        DeviceArt::Keyboard => (124., 53.),
        DeviceArt::Dac => (120., 80.),
        DeviceArt::Other => (90., 90.),
    };
    canvas(
        move |_, _, _| kind,
        move |bounds, kind, window, _| {
            let bw = f32::from(bounds.size.width);
            let bh = f32::from(bounds.size.height);
            let at = |x: f32, y: f32| bounds.origin + point(px(bw * x), px(bh * y));
            match kind {
                DeviceArt::Mouse => {
                    let m = |x: f32, y: f32| at(x / 74., y / 118.);
                    let mut outer = PathBuilder::stroke(px(1.3));
                    outer.move_to(m(37., 2.));
                    outer.cubic_bezier_to(m(3., 46.), m(14., 2.), m(3., 20.));
                    outer.line_to(m(3., 76.));
                    outer.cubic_bezier_to(m(37., 116.), m(3., 100.), m(17., 116.));
                    outer.cubic_bezier_to(m(71., 76.), m(57., 116.), m(71., 100.));
                    outer.line_to(m(71., 46.));
                    outer.cubic_bezier_to(m(37., 2.), m(71., 20.), m(60., 2.));
                    outer.close();
                    paint(window, outer, theme::alpha(color, 0.9));

                    let mut detail = PathBuilder::stroke(px(1.2));
                    detail.move_to(m(37., 2.));
                    detail.line_to(m(37., 42.));
                    detail.move_to(m(3., 42.));
                    detail.cubic_bezier_to(m(37., 48.), m(15., 47.), m(25., 48.));
                    detail.cubic_bezier_to(m(71., 42.), m(49., 48.), m(59., 47.));
                    detail.move_to(m(5., 62.));
                    detail.curve_to(m(10., 67.), m(10., 62.));
                    detail.line_to(m(10., 75.));
                    paint(window, detail, theme::alpha(color, 0.48));

                    let mut wheel = PathBuilder::stroke(px(1.2));
                    rounded_rect_path(&mut wheel, &m, 33.5, 14., 7., 16., 3.5);
                    paint(window, wheel, theme::alpha(color, 0.85));
                }
                DeviceArt::Keyboard => {
                    let k = |x: f32, y: f32| at(x / 160., y / 68.);
                    let mut outer = PathBuilder::stroke(px(1.3));
                    rounded_rect_path(&mut outer, &k, 1., 1., 158., 66., 9.);
                    paint(window, outer, theme::alpha(color, 0.9));

                    let mut keys = PathBuilder::stroke(px(1.));
                    for r in 0..4 {
                        for c in 0..11 {
                            let x = 8. + c as f32 * 13.;
                            let y = 8. + r as f32 * 13.;
                            if r == 3 && c > 2 && c < 8 {
                                if c == 3 {
                                    rounded_rect_path(&mut keys, &k, x, y, 62., 10., 2.);
                                }
                                continue;
                            }
                            rounded_rect_path(&mut keys, &k, x, y, 10., 10., 2.);
                        }
                    }
                    paint(window, keys, theme::alpha(color, 0.5));

                    let mut esc = PathBuilder::stroke(px(1.));
                    rounded_rect_path(&mut esc, &k, 8., 8., 10., 10., 2.);
                    paint(window, esc, theme::alpha(color, 0.9));
                }
                DeviceArt::Dac => {
                    let d = |x: f32, y: f32| at(x / 120., y / 80.);
                    let mut body = PathBuilder::stroke(px(1.3));
                    rounded_rect_path(&mut body, &d, 8., 22., 76., 40., 12.);
                    paint(window, body, theme::alpha(color, 0.9));

                    let mut knobs = PathBuilder::stroke(px(1.2));
                    circle_path(&mut knobs, &d, 34., 42., 9.);
                    circle_path(&mut knobs, &d, 34., 42., 3.);
                    circle_path(&mut knobs, &d, 64., 42., 2.5);
                    paint(window, knobs, theme::alpha(color, 0.75));

                    let mut cable = PathBuilder::stroke(px(1.2));
                    cable.move_to(d(84., 42.));
                    cable.line_to(d(94., 42.));
                    cable.cubic_bezier_to(d(108., 26.), d(104., 42.), d(108., 36.));
                    cable.line_to(d(108., 6.));
                    paint(window, cable, theme::alpha(color, 0.55));
                }
                DeviceArt::Other => {
                    let o = |x: f32, y: f32| at(x / 90., y / 90.);
                    let mut body = PathBuilder::stroke(px(1.3));
                    rounded_rect_path(&mut body, &o, 20., 24., 50., 44., 10.);
                    paint(window, body, theme::alpha(color, 0.85));
                    let mut detail = PathBuilder::stroke(px(1.2));
                    detail.move_to(o(45., 24.));
                    detail.line_to(o(45., 12.));
                    detail.move_to(o(34., 45.));
                    detail.line_to(o(56., 45.));
                    paint(window, detail, theme::alpha(color, 0.5));
                }
            }
        },
    )
    .w(px(w))
    .h(px(h))
}

impl Shell {
    pub fn page_home(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::pal();
        let mut devices = self.hub.devices.clone();
        devices.sort_by_key(|d| (!d.supported, d.kind != DeviceKind::Mouse, d.name.clone()));
        let n = devices.len();

        let mut grid = div().flex().flex_wrap().gap(px(theme::GAP));
        for d in devices {
            grid = grid.child(self.device_tile(&d, cx));
        }

        let hero = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(ui::title("デバイス"))
            .child(ui::sub(if n == 0 {
                if self.hub.scanned {
                    "対応デバイスが見つかりません。USB / レシーバーを接続してください。".to_string()
                } else {
                    "デバイスを探しています…".to_string()
                }
            } else {
                format!("{n} 台を検出しました。ブラウザを閉じたまま設定できます。")
            }));

        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(theme::PAGE_GAP))
            .p(px(theme::PAGE_PAD))
            .pt(px(theme::PAGE_TOP))
            .pb(px(theme::PAGE_BOT))
            .child(hero);
        if n == 0 && self.hub.scanned {
            col = col.child(
                ui::card().child(
                    div()
                        .relative()
                        .p(px(20.))
                        .child(ui::empty_state(
                            "usb",
                            "デバイスが接続されていません",
                            "Web 版 (mouse.fit / hub.xsyd.top) をブラウザで開いていると HID を掴まれて設定できないことがあります。ブラウザのタブを閉じてから再接続してください。",
                        ))
                        .child(
                            div()
                                .flex()
                                .justify_center()
                                .gap(px(8.))
                                .child(ui::button("rescan", "再検索", Some("refresh-cw"), Btn::Glass, cx, |this, _, cx| {
                                    this.hub.scan();
                                    cx.notify();
                                }))
                                .child(ui::button("demo", "デモで試す", Some("sparkles"), Btn::Primary, cx, |this, _, cx| {
                                    this.hub.switch_backend(Arc::new(devhub_core::demo::DemoHub::new()));
                                    cx.notify();
                                })),
                        ),
                ),
            );
        } else {
            col = col.child(grid);
        }
        col = col.child(self.home_tips());
        div()
            .id("home-scroll")
            .size_full()
            .overflow_y_scroll().smooth_wheel()
            .child(col)
            .text_color(p.fg)
    }

    fn device_tile(&mut self, d: &DeviceInfo, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::pal();
        let id = d.id.clone();
        let key = format!("tile:{}", d.id);
        let hover = motion::ease(&format!("tileh:{}", d.id), motion::hovered(&key));
        let name = self
            .prefs
            .nicknames
            .get(&d.id.0)
            .cloned()
            .unwrap_or_else(|| d.name.clone());
        let icon = match d.kind {
            DeviceKind::Mouse => "mouse",
            DeviceKind::Keyboard => "keyboard",
            DeviceKind::Other if d.driver == DriverKind::Walkplay => "headphones",
            DeviceKind::Other => "usb",
        };
        let status: (String, Hsla) = if !d.supported {
            ("検出済み · ドライバ未実装".into(), p.warn)
        } else if d.asleep {
            ("スリープ中 — 動かすと起動します".into(), p.warn)
        } else {
            (
                match d.transport {
                    Transport::Wired => "有線で接続中",
                    Transport::Wireless24G => "2.4GHz で接続中",
                    Transport::Bluetooth => "Bluetooth で接続中",
                    Transport::Unknown => "接続中",
                }
                .into(),
                p.ok,
            )
        };

        let graphite = theme::is_graphite();
        let content = if graphite {
            let art_kind = device_art_kind(self, d);
            let kind_label = match art_kind {
                DeviceArt::Mouse => "マウス",
                DeviceArt::Keyboard => "キーボード",
                DeviceArt::Dac => "USB DAC",
                DeviceArt::Other => "デバイス",
            };
            let meta = d
                .firmware
                .as_ref()
                .map(|fw| format!("FW {fw}"))
                .unwrap_or_else(|| d.vendor.clone());
            let power = match d.battery.and_then(|b| b.percent.map(|x| (x, b.charging))) {
                Some((x, c)) => ui::battery_pill(x, c).into_any_element(),
                None => ui::faint("USB 給電").into_any_element(),
            };
            div()
                .relative()
                .size_full()
                .p(px(18.))
                .flex()
                .gap(px(18.))
                .child(
                    div()
                        .w(px(150.))
                        .h_full()
                        .flex_none()
                        .rounded(px(12.))
                        .border_1()
                        .border_color(p.hair)
                        .bg(p.ctrl)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(graphite_device_art(art_kind, p.fg)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(p.faint)
                                .child(kind_label),
                        )
                        .child(
                            div()
                                .pt(px(4.))
                                .text_size(px(22.))
                                .font_weight(FontWeight::LIGHT)
                                .child(name),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .pt(px(4.))
                                .child(div().size(px(6.)).rounded(px(3.)).bg(status.1))
                                .child(ui::faint(status.0)),
                        )
                        .child(ui::faint(meta).pt(px(2.)))
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .pt(px(10.))
                                .border_t_1()
                                .border_color(p.hair)
                                .child(power)
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(5.))
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.fg)
                                        .child("開く")
                                        .child(ui::icon("arrow-right", 13., p.fg)),
                                ),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .relative()
                .size_full()
                .p(px(18.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_start()
                        .child(
                            div()
                                .size(px(52.))
                                .rounded(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(if !d.supported {
                                    theme::alpha(p.warn, 0.18).into()
                                } else {
                                    theme::accent_grad()
                                })
                                .child(ui::icon(
                                    icon,
                                    26.,
                                    if !d.supported { p.warn } else { p.on_accent },
                                )),
                        )
                        .child(div().flex_1())
                        .when_some(
                            d.battery.and_then(|b| b.percent.map(|x| (x, b.charging))),
                            |dv, (x, c)| {
                                dv.child(if d.asleep && !c {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_size(px(11.))
                                        .text_color(p.faint)
                                        .child(ui::icon("battery", 14., p.faint))
                                        .child(format!("{x}% · スリープ中"))
                                } else {
                                    ui::battery_pill(x, c)
                                })
                            },
                        ),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight::BOLD)
                        .child(name),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pt(px(3.))
                        .child(div().size(px(6.)).rounded(px(3.)).bg(status.1))
                        .child(ui::faint(status.0)),
                )
                .child(
                    ui::faint(format!("{} · {:04X}:{:04X}", d.vendor, d.vid, d.pid)).pt(px(2.)),
                )
                .into_any_element()
        };

        div()
            .id(SharedString::from(key.clone()))
            .relative()
            .w(px(if graphite { 500. } else { 300. }))
            .h(px(176.))
            .mt(px(-hover * 3.))
            .rounded(px(theme::r_card()))
            .cursor_pointer()
            .shadow(vec![BoxShadow {
                color: theme::alpha(p.shadow, p.shadow.a * (0.8 + hover * 0.4)),
                offset: point(px(0.), px(10. + hover * 8.)),
                blur_radius: px(32. + hover * 10.),
                spread_radius: px(-12.),
                inset: false,
            }])
            .child(motion::track(key.clone()))
            .child(glass::surface_hover(glass::Kind::Panel, theme::r_card(), key))
            .child(content)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_device(id.clone());
                cx.notify();
            }))
    }

    fn home_tips(&self) -> impl IntoElement {
        let p = theme::pal();
        let tip = |icon: &str, t: &str, b: &str| {
            div()
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(32.))
                        .flex_none()
                        .rounded(px(10.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(p.accent_soft)
                        .child(ui::icon(
                            icon,
                            15.,
                            if theme::is_graphite() { p.sub } else { p.accent },
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t.to_string()),
                        )
                        .child(ui::faint(b.to_string())),
                )
        };
        ui::card_titled("ヒント", "info", None).child(
            ui::body().gap(px(14.)).pt(px(8.)).pb(px(18.)).child(tip(
                "zap",
                "変更はすぐにデバイスへ書き込まれます",
                "Web 版と同じ即時反映です。右上の点が黄色の間は書き込み中、緑になれば完了です。",
            ))
            .child(tip(
                "feather",
                "ゲーム中も軽い",
                "アニメーションが止まると描画も止まります。背面にあるときは CPU / GPU をほぼ使いません。",
            ))
            .child(tip(
                "globe",
                "Web 版と同時に使わない",
                "ブラウザの configurator が開いていると HID を取り合います。どちらか一方を使ってください。",
            )),
        )
    }
}
