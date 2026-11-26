//! OBS オーバーレイの編集画面。
//!
//! OBS プラグイン (`devhub-obs`) と同じレンダラでプレビューを描き、
//! 見た目 (テーマ・色・倍率…) と**キー/ボタン単位の表示**を `obs_overlay.json` に保存する。
//! プラグインは 1 秒ごとにファイルを見直すので、OBS を開いたまま即反映される。

use crate::app::Shell;
use crate::pages::keyboard::Cap;
use crate::ui::{self, Btn};
use crate::{overlay, theme};
use devhub_core::model::KeyboardCaps;
use devhub_core::*;
use devhub_obs::config::{Arrange, FillMode, MouseStyle, OverlayCfg, Theme as ObsTheme};
use devhub_obs::render::{self, Mode};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;
use std::sync::Arc;

const ACCENT_CHOICES: [u32; 8] = [
    0x3cbeff, 0x34d6a8, 0xa98bff, 0xff8a5c, 0xff5c8a, 0xf5c24c, 0xffffff, 0x78e678,
];

/// 編集対象のキーボード (接続中の caps、無ければ SU66 Max の既定レイアウト) とマウスのボタン名。
fn targets(this: &Shell) -> (String, KeyboardCaps, f32, String, Vec<String>) {
    let mut kb: Option<(String, KeyboardCaps)> = None;
    let mut mouse: Option<(String, Vec<String>)> = None;
    for d in &this.hub.devices {
        let Some(s) = this.hub.session(&d.id) else {
            continue;
        };
        let Some(c) = &s.caps else { continue };
        if kb.is_none() {
            if let Some(k) = &c.keyboard {
                if k.perf.as_ref().is_some_and(|p| p.live_travel) {
                    kb = Some((d.name.clone(), k.clone()));
                }
            }
        }
        if mouse.is_none() {
            if let Some(m) = &c.mouse {
                if !m.live_buttons.is_empty() {
                    let names = m
                        .live_buttons
                        .iter()
                        .map(|&i| {
                            m.buttons
                                .get(i)
                                .map(|b| b.name.clone())
                                .unwrap_or_else(|| format!("B{i}"))
                        })
                        .collect();
                    mouse = Some((d.name.clone(), names));
                }
            }
        }
    }
    let (kb_name, kc) = kb.unwrap_or_else(|| {
        let mut kc = devhub_core::demo::keyboard_caps()
            .keyboard
            .expect("demo kb");
        kc.layout = devhub_core::registry::su66max_layout();
        ("SU66 Max".into(), kc)
    });
    let travel = kc.perf.as_ref().map(|p| p.travel).unwrap_or(4.);
    let (m_name, names) =
        mouse.unwrap_or_else(|| ("マウス".into(), vec!["Left".into(), "Right".into()]));
    (kb_name, kc, travel, m_name, names)
}

impl Shell {
    fn obs_save(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = devhub_obs::config::save(&self.obs_cfg) {
            self.toast(format!("OBS 設定を保存できません: {e}"), true, cx);
        }
        cx.notify();
    }

    pub(crate) fn obs_edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut OverlayCfg)) {
        f(&mut self.obs_cfg);
        self.obs_save(cx);
    }

    /// プレビュー画像を描き直す (前の画像は GPU から解放)。
    fn obs_preview(&mut self, window: &mut Window) -> Option<(Arc<RenderImage>, u32, u32)> {
        let (kb_name, kc, travel, m_name, names) = targets(self);
        let live = render::demo_live(
            Some((kb_name, kc.layout.clone(), travel)),
            Some((m_name, names)),
            self.obs_t,
        );
        let vis = render::Vis::default().with_cfg(&self.obs_cfg);
        let font = self
            .obs_font
            .get_or_insert_with(|| Rc::new(render::font()))
            .clone();
        let pix = render::draw(&live, (*font).as_ref(), self.obs_mode, &vis);
        let (w, h) = (pix.w, pix.h);
        let mut buf = pix.buf;
        // RenderImage は BGRA。
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let img = image::RgbaImage::from_raw(w, h, buf)?;
        let ri = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            img
        )]));
        if let Some(old) = self.obs_img.replace(ri.clone()) {
            let _ = window.drop_image(old);
        }
        Some((ri, w, h))
    }

    pub fn page_obs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::pal();
        let cfg = self.obs_cfg.clone();
        let (_, kc, _, _, names) = targets(self);

        // ---------------- ヘッダー
        let header = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                div()
                    .size(px(48.))
                    .rounded(px(14.))
                    .bg(p.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::icon("cast", 22., p.accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(ui::title("OBS オーバーレイ"))
                    .child(ui::sub("配信画面にキー/ボタンの押し込みをライブ表示。ここでの変更は OBS に 1 秒以内で反映されます")),
            );

        // ---------------- プレビュー
        let modes = ["キーボード", "マウス", "両方"].map(String::from);
        let cur_mode = match self.obs_mode {
            Mode::Kb => 0,
            Mode::Mouse => 1,
            Mode::Both => 2,
        };
        let mode_sw = div().w(px(300.)).child(ui::segmented(
            "obs-mode",
            &modes,
            cur_mode,
            cx,
            |this, i, cx| {
                this.obs_mode = [Mode::Kb, Mode::Mouse, Mode::Both][i];
                cx.notify();
            },
        ));
        let src_name = match self.obs_mode {
            Mode::Kb => "Device HUB キーボード",
            Mode::Mouse => "Device HUB マウス",
            Mode::Both => "Device HUB Live",
        };
        let mut stage = div()
            .w_full()
            .min_h(px(200.))
            .p(px(18.))
            .rounded(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            // 透過が分かるよう、配信画面っぽい暗いグラデーションの上に置く
            .bg(linear_gradient(
                135.,
                linear_color_stop(theme::hex(0x1b2a4a, 1.), 0.),
                linear_color_stop(theme::hex(0x3a2140, 1.), 1.),
            ));
        if let Some((img_, w, h)) = self.obs_preview(window) {
            let max_w = 880f32;
            let k = (max_w / w as f32).min(1.);
            stage = stage.child(
                img(img_)
                    .w(px(w as f32 * k))
                    .h(px(h as f32 * k))
                    .object_fit(ObjectFit::Contain),
            );
        }
        let preview = ui::card_titled("プレビュー", "monitor-play", Some(mode_sw.into_any_element())).child(
            ui::body()
                .pt(px(8.))
                .gap(px(8.))
                .child(stage)
                .child(ui::faint(format!(
                    "OBS の「ソース追加」→「{src_name}」で追加 (デモの動きです。実際の表示は接続中のデバイスの押し込み)"
                ))),
        );

        // ---------------- デザイン
        let themes: Vec<String> = ObsTheme::ALL
            .iter()
            .map(|t| t.label().to_string())
            .collect();
        let theme_i = ObsTheme::ALL
            .iter()
            .position(|t| *t == cfg.theme)
            .unwrap_or(0);
        let mstyles: Vec<String> = MouseStyle::ALL
            .iter()
            .map(|t| t.label().to_string())
            .collect();
        let ms_i = MouseStyle::ALL
            .iter()
            .position(|t| *t == cfg.mouse_style)
            .unwrap_or(0);
        let scales = ["1x", "1.5x", "2x"].map(String::from);
        let scale_i = [1.0f32, 1.5, 2.0]
            .iter()
            .position(|s| (cfg.scale - s).abs() < 0.01)
            .unwrap_or(0);
        let cur_acc = Rgb::new(cfg.accent[0], cfg.accent[1], cfg.accent[2]);
        let mut acc_row = div().flex().items_center().gap(px(6.));
        for hex in ACCENT_CHOICES {
            let c = Rgb::hex(hex);
            acc_row = acc_row.child(
                div()
                    .id(ElementId::Name(format!("obs-acc-{hex:06x}").into()))
                    .cursor_pointer()
                    .child(ui::swatch(theme::rgb_of(c), 20., c == cur_acc))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.obs_edit(cx, |c2| c2.accent = [c.r, c.g, c.b]);
                    })),
            );
        }
        let custom = !ACCENT_CHOICES.iter().any(|h| Rgb::hex(*h) == cur_acc);
        acc_row = acc_row.child(
            div()
                .id("obs-acc-custom")
                .cursor_pointer()
                .child(if custom {
                    ui::swatch(theme::rgb_of(cur_acc), 20., true)
                } else {
                    div()
                        .size(px(20.))
                        .rounded_full()
                        .border_1()
                        .border_color(p.hair)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::icon("plus", 12., p.sub))
                })
                .on_click(cx.listener(move |this, _, w, cx| {
                    overlay::pick_color(
                        this,
                        "アクセント色",
                        cur_acc,
                        w,
                        cx,
                        |this, c, _, cx| {
                            this.obs_edit(cx, |c2| c2.accent = [c.r, c.g, c.b]);
                        },
                    );
                })),
        );
        // 押し込み色のスウォッチ (自動 = アクセントに追従)。
        let cur_pre = cfg.pressed.map(|c| Rgb::new(c[0], c[1], c[2]));
        let mut pre_row = div().flex().items_center().gap(px(6.));
        pre_row = pre_row.child(
            div()
                .id("obs-pre-auto")
                .cursor_pointer()
                .px(px(8.))
                .h(px(20.))
                .rounded(px(10.))
                .border_1()
                .border_color(if cur_pre.is_none() { p.accent } else { p.hair })
                .bg(if cur_pre.is_none() {
                    p.accent_soft
                } else {
                    gpui::transparent_black()
                })
                .flex()
                .items_center()
                .text_size(px(11.))
                .text_color(if cur_pre.is_none() { p.accent } else { p.sub })
                .child("自動")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.obs_edit(cx, |c| c.pressed = None);
                })),
        );
        for hex in ACCENT_CHOICES {
            let c = Rgb::hex(hex);
            let sel = cur_pre == Some(c);
            pre_row = pre_row.child(
                div()
                    .id(ElementId::Name(format!("obs-pre-{hex:06x}").into()))
                    .cursor_pointer()
                    .child(ui::swatch(theme::rgb_of(c), 20., sel))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.obs_edit(cx, |c2| c2.pressed = Some([c.r, c.g, c.b]));
                    })),
            );
        }
        {
            let pcustom =
                cur_pre.is_some_and(|c| !ACCENT_CHOICES.iter().any(|h| Rgb::hex(*h) == c));
            pre_row = pre_row.child(
                div()
                    .id("obs-pre-custom")
                    .cursor_pointer()
                    .child(if let (true, Some(c)) = (pcustom, cur_pre) {
                        ui::swatch(theme::rgb_of(c), 20., true).into_any_element()
                    } else {
                        div()
                            .size(px(20.))
                            .rounded_full()
                            .border_1()
                            .border_color(p.hair)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon("plus", 12., p.sub))
                            .into_any_element()
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        let base = cur_pre.unwrap_or(Rgb::new(255, 150, 60));
                        overlay::pick_color(
                            this,
                            "押し込み色",
                            base,
                            w,
                            cx,
                            |this, c, _, cx| {
                                this.obs_edit(cx, |c2| c2.pressed = Some([c.r, c.g, c.b]));
                            },
                        );
                    })),
            );
        }
        let fills: Vec<String> = FillMode::ALL
            .iter()
            .map(|f| f.label().to_string())
            .collect();
        let fill_i = FillMode::ALL
            .iter()
            .position(|f| *f == cfg.fill_mode)
            .unwrap_or(0);
        let title_in = self.input(
            "obs_title",
            &cfg.title_text,
            "空欄 = デバイス名",
            window,
            cx,
        );
        let design = ui::card_titled("デザイン", "palette", None).child(
            ui::body()
                .child(ui::row_first(
                    "テーマ",
                    Some("「背景なし」はキーだけを配信画面に重ねます"),
                    div().w(px(360.)).child(ui::segmented(
                        "obs-theme",
                        &themes,
                        theme_i,
                        cx,
                        |this, i, cx| {
                            this.obs_edit(cx, |c| c.theme = ObsTheme::ALL[i]);
                        },
                    )),
                ))
                .child(ui::row(
                    "アクセント色",
                    Some("枠・タイトル・Hz の色。OBS 側のソース設定で色を選ぶとそちらが優先"),
                    acc_row,
                ))
                .child(ui::row(
                    "押し込み色",
                    Some("フィル・発光・押下時の輪郭。「自動」はアクセントと同じ"),
                    pre_row,
                ))
                .child(ui::row(
                    "フィルの方向",
                    Some("押し込み量の塗り方"),
                    div().w(px(300.)).child(ui::segmented(
                        "obs-fill",
                        &fills,
                        fill_i,
                        cx,
                        |this, i, cx| {
                            this.obs_edit(cx, |c| c.fill_mode = FillMode::ALL[i]);
                        },
                    )),
                ))
                .child(ui::row(
                    "背景の不透明度",
                    Some("パネルの透け具合。0% は「背景なし」と同じ"),
                    ui::slider_value(
                        "obs-alpha",
                        cfg.panel_alpha as f32,
                        0.,
                        100.,
                        5.,
                        "%",
                        cx,
                        |this, v, cx| {
                            this.obs_edit(cx, |c| c.panel_alpha = v as u8);
                        },
                    ),
                ))
                .child(ui::row(
                    "発光の強さ",
                    Some("押したときの外側の光。0 で消灯"),
                    ui::slider_value(
                        "obs-glow",
                        cfg.glow as f32,
                        0.,
                        200.,
                        10.,
                        "%",
                        cx,
                        |this, v, cx| {
                            this.obs_edit(cx, |c| c.glow = v as u8);
                        },
                    ),
                ))
                .child(ui::row(
                    "影",
                    Some("キーとパネルの落ち影"),
                    ui::toggle("obs-shadow", cfg.shadow, cx, |this, v, cx| {
                        this.obs_edit(cx, |c| c.shadow = v)
                    }),
                ))
                .child(ui::row(
                    "解像度",
                    Some("OBS で拡大するなら 1.5x / 2x にするとにじみません"),
                    div().w(px(220.)).child(ui::segmented(
                        "obs-scale",
                        &scales,
                        scale_i,
                        cx,
                        |this, i, cx| {
                            this.obs_edit(cx, |c| c.scale = [1.0, 1.5, 2.0][i]);
                        },
                    )),
                ))
                .child(ui::row(
                    "マウスの表示",
                    None,
                    div().w(px(300.)).child(ui::segmented(
                        "obs-mstyle",
                        &mstyles,
                        ms_i,
                        cx,
                        |this, i, cx| {
                            this.obs_edit(cx, |c| c.mouse_style = MouseStyle::ALL[i]);
                        },
                    )),
                )),
        );

        // ---------------- レイアウト
        let arranges: Vec<String> = Arrange::ALL.iter().map(|a| a.label().to_string()).collect();
        let arr_i = Arrange::ALL
            .iter()
            .position(|a| *a == cfg.arrange)
            .unwrap_or(0);
        let layout_card = ui::card_titled("レイアウト", "layout-dashboard", None).child(
            ui::body()
                .child(ui::row_first(
                    "キーの大きさ",
                    Some("キー1個のサイズ。解像度はそのままに密度だけ変わります"),
                    ui::slider_value(
                        "obs-ksize",
                        cfg.key_size,
                        0.7,
                        1.4,
                        0.05,
                        "x",
                        cx,
                        |this, v, cx| {
                            this.obs_edit(cx, |c| c.key_size = v);
                        },
                    ),
                ))
                .child(ui::row(
                    "角丸",
                    Some("キーとパネルの丸み。0 で四角"),
                    ui::slider_value(
                        "obs-radius",
                        cfg.radius,
                        0.,
                        2.,
                        0.1,
                        "x",
                        cx,
                        |this, v, cx| {
                            this.obs_edit(cx, |c| c.radius = v);
                        },
                    ),
                ))
                .child(ui::row(
                    "余白",
                    Some("パネル内側の空き"),
                    ui::slider_value("obs-pad", cfg.pad, 0.5, 2., 0.1, "x", cx, |this, v, cx| {
                        this.obs_edit(cx, |c| c.pad = v);
                    }),
                ))
                .child(ui::row(
                    "配置",
                    Some("「キー + マウス」統合ソースでの並び"),
                    div().w(px(300.)).child(ui::segmented(
                        "obs-arr",
                        &arranges,
                        arr_i,
                        cx,
                        |this, i, cx| {
                            this.obs_edit(cx, |c| c.arrange = Arrange::ALL[i]);
                        },
                    )),
                )),
        );

        // ---------------- 表示するもの
        let show_card = ui::card_titled("表示するもの", "eye", None).child(
            ui::body()
                .child(ui::row_first(
                    "タイトル (デバイス名)",
                    Some("右の入力欄で文字を変えられます (空欄 = デバイス名)"),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::toggle("obs-title", cfg.title, cx, |this, v, cx| {
                            this.obs_edit(cx, |c| c.title = v)
                        }))
                        .child(
                            div()
                                .w(px(200.))
                                .child(gpui_kit::component::input::Input::new(&title_in)),
                        ),
                ))
                .child(ui::row(
                    "キー名・ボタン名",
                    None,
                    ui::toggle("obs-labels", cfg.labels, cx, |this, v, cx| {
                        this.obs_edit(cx, |c| c.labels = v)
                    }),
                ))
                .child(ui::row(
                    "設定バッジ (AP/R)",
                    Some("キー内の作動点・リセットの値 (RT時は押/放の2値)"),
                    ui::toggle("obs-badges", cfg.badges, cx, |this, v, cx| {
                        this.obs_edit(cx, |c| c.badges = v)
                    }),
                ))
                .child(ui::row(
                    "作動点ライン",
                    Some("キー内の横線。この深さで ON になる位置"),
                    ui::toggle("obs-tline", cfg.trigger_line, cx, |this, v, cx| {
                        this.obs_edit(cx, |c| c.trigger_line = v)
                    }),
                ))
                .child(ui::row(
                    "押し込み量の数字",
                    Some("押している間だけキーの下に深さを表示"),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::toggle("obs-num", cfg.numbers, cx, |this, v, cx| {
                            this.obs_edit(cx, |c| c.numbers = v)
                        }))
                        .child(ui::segmented(
                            "obs-numfmt",
                            &["mm", "%"].map(String::from),
                            cfg.num_pct as usize,
                            cx,
                            |this, i, cx| this.obs_edit(cx, |c| c.num_pct = i == 1),
                        )),
                )),
        );

        // ---------------- 表示するキー
        // show_keys が空 = すべて表示。クリックで表示集合を選ぶ。
        let restricted = !cfg.show_keys.is_empty();
        let caps_v: Vec<Cap> = kc
            .layout
            .iter()
            .map(|k| {
                let shown = !cfg.hide_keys.contains(&k.id)
                    && (!restricted || cfg.show_keys.contains(&k.id));
                Cap {
                    top: k.label.clone(),
                    dim: !shown,
                    tint: (restricted && cfg.show_keys.contains(&k.id))
                        .then(|| theme::alpha(theme::rgb_of(cur_acc), 0.28)),
                    badge: (!shown).then(|| "非表示".into()),
                    ..Default::default()
                }
            })
            .collect();
        let ids: Vec<u16> = kc.layout.iter().map(|k| k.id).collect();
        let ids2 = ids.clone();
        let view = self.keyboard_view(
            "obs-keys",
            &kc,
            caps_v,
            cx,
            Rc::new(move |this, i, _, cx| {
                let id = ids2[i];
                this.obs_edit(cx, |c| {
                    if c.show_keys.is_empty() {
                        // すべて表示 → このキーだけを表示
                        c.show_keys = vec![id];
                    } else if let Some(pos) = c.show_keys.iter().position(|x| *x == id) {
                        c.show_keys.remove(pos);
                    } else {
                        c.show_keys.push(id);
                    }
                });
            }),
        );
        let only = |labels: &'static [&'static str]| {
            let layout = kc.layout.clone();
            move |this: &mut Shell, _: &mut Window, cx: &mut Context<Shell>| {
                let show: Vec<u16> = layout
                    .iter()
                    .filter(|k| labels.iter().any(|l| l.eq_ignore_ascii_case(&k.label)))
                    .map(|k| k.id)
                    .collect();
                this.obs_edit(cx, |c| c.show_keys = show);
            }
        };
        let left_half = {
            let layout = kc.layout.clone();
            move |this: &mut Shell, _: &mut Window, cx: &mut Context<Shell>| {
                // 左手側 (G 列まで + 左の修飾キー)。
                let show: Vec<u16> = layout
                    .iter()
                    .filter(|k| k.x + k.w / 2. <= 6.6)
                    .map(|k| k.id)
                    .collect();
                this.obs_edit(cx, |c| c.show_keys = show);
            }
        };
        let n_shown = ids
            .iter()
            .filter(|id| !cfg.hide_keys.contains(id) && (!restricted || cfg.show_keys.contains(id)))
            .count();
        let key_tools = div()
            .flex()
            .gap(px(6.))
            .child(ui::button(
                "obs-k-all",
                "すべて表示",
                Some("eye"),
                Btn::Ghost,
                cx,
                |this, _, cx| {
                    this.obs_edit(cx, |c| {
                        c.show_keys.clear();
                        c.hide_keys.clear();
                    });
                },
            ))
            .child(ui::button(
                "obs-k-wasd",
                "WASD + Space",
                None,
                Btn::Ghost,
                cx,
                only(&["W", "A", "S", "D", "Space"]),
            ))
            .child(ui::button(
                "obs-k-fps",
                "FPS",
                None,
                Btn::Ghost,
                cx,
                only(&[
                    "Esc", "1", "2", "3", "4", "5", "Tab", "Q", "W", "E", "R", "T", "Caps", "A",
                    "S", "D", "F", "G", "LShift", "Shift", "Z", "X", "C", "V", "LCtrl", "Ctrl",
                    "LAlt", "Space",
                ]),
            ))
            .child(ui::button(
                "obs-k-left",
                "左半分",
                None,
                Btn::Ghost,
                cx,
                left_half,
            ))
            .child({
                let ids3 = ids.clone();
                let cur_show = cfg.show_keys.clone();
                ui::button(
                    "obs-k-inv",
                    "反転",
                    None,
                    Btn::Ghost,
                    cx,
                    move |this, _, cx| {
                        let ids3 = ids3.clone();
                        let cur_show = cur_show.clone();
                        this.obs_edit(cx, |c| {
                            // 現在の実効表示集合の補集合を選ぶ (空 = すべて表示 → 反転は全キー解除で空にしない)。
                            let cur: std::collections::HashSet<u16> = if cur_show.is_empty() {
                                ids3.iter().copied().collect()
                            } else {
                                cur_show.iter().copied().collect()
                            };
                            c.show_keys = ids3
                                .iter()
                                .copied()
                                .filter(|id| !cur.contains(id))
                                .collect();
                            if c.show_keys.len() == ids3.len() {
                                c.show_keys.clear(); // 全選択は「すべて表示」と同じ
                            }
                        });
                    },
                )
            });
        let keys_card = ui::card_titled("表示するキー", "keyboard", Some(key_tools.into_any_element())).child(
            ui::body()
                .pt(px(10.))
                .gap(px(8.))
                .child(view)
                .child(ui::faint(format!(
                    "クリックで表示するキーを選択/解除 ({n_shown}/{} キー表示中。未選択 = すべて表示)。表示するキーだけにオーバーレイは切り詰められます",
                    ids.len()
                ))),
        );

        // ---------------- 表示するマウスボタン (show_buttons 空 = すべて表示)
        let btn_restricted = !cfg.show_buttons.is_empty();
        let mut chips = div().flex().flex_wrap().gap(px(8.));
        for (i, name) in names.iter().enumerate() {
            let off = cfg.hide_buttons.contains(name)
                || (btn_restricted && !cfg.show_buttons.contains(name));
            let n2 = name.clone();
            chips = chips.child(
                div()
                    .id(ElementId::Name(format!("obs-btn-{i}").into()))
                    .px(px(12.))
                    .py(px(7.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(if off {
                        theme::alpha(p.faint, 0.06)
                    } else {
                        p.accent_soft
                    })
                    .border_1()
                    .border_color(if off { p.hair } else { p.accent })
                    .child(ui::icon(
                        if off { "eye-off" } else { "eye" },
                        14.,
                        if off { p.faint } else { p.accent },
                    ))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(if off { p.faint } else { p.fg })
                            .child(name.clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let n2 = n2.clone();
                        this.obs_edit(cx, |c| {
                            if c.show_buttons.is_empty() {
                                // すべて表示 → このボタンだけを表示
                                c.show_buttons = vec![n2];
                            } else if let Some(pos) = c.show_buttons.iter().position(|x| *x == n2) {
                                c.show_buttons.remove(pos);
                            } else {
                                c.show_buttons.push(n2);
                            }
                        });
                    })),
            );
        }
        let btn_card = ui::card_titled("表示するマウスボタン", "mouse", None).child(
            ui::body()
                .pt(px(8.))
                .gap(px(8.))
                .child(chips)
                .child(ui::faint(
                    "押し込み深さを測れるボタン (磁気スイッチ) だけが並びます。クリックで切り替え",
                )),
        );

        // ---------------- 導入方法
        let path = devhub_obs::config::path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let step = |n: &str, t: &str| {
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(
                    div()
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .bg(p.accent_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .text_color(p.accent)
                        .child(n.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.5))
                        .text_color(p.sub)
                        .child(t.to_string()),
                )
        };
        let howto = ui::card_titled("OBS への追加", "badge-info", None).child(
            ui::body()
                .pt(px(8.))
                .gap(px(10.))
                .child(step("1", "devhub_obs.dll を C:\\ProgramData\\obs-studio\\plugins\\devhub_obs\\bin\\64bit\\ に置いて OBS を起動"))
                .child(step("2", "ソース追加 →「Device HUB キーボード」「Device HUB マウス」を別々に追加 (両方を 1 枚にするなら「Device HUB Live」)"))
                .child(step("3", "見た目と表示キーはこの画面で変更 — 全ソースに即反映。ソースのプロパティ側でも表示するキー/ボタンを選べます"))
                .child(step("4", "Device HUB 本体は閉じていても OK (OBS プラグインが直接デバイスを読みます)"))
                .child(div().min_w_0().child(ui::faint(format!("設定ファイル: {path}")))),
        );

        div()
            .id("obs-scroll")
            .size_full()
            .overflow_y_scroll().smooth_wheel()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::PAGE_GAP))
                    .p(px(theme::PAGE_PAD))
                    .pt(px(theme::PAGE_TOP))
                    .pb(px(theme::PAGE_BOT))
                    .child(header)
                    .child(preview)
                    .child(design)
                    .child(layout_card)
                    .child(show_card)
                    .child(keys_card)
                    .child(ui::grid2(vec![
                        btn_card.into_any_element(),
                        howto.into_any_element(),
                    ])),
            )
    }
}
