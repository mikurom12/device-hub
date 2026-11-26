//! マウスの「性能」タブ — 作り直し版 (LAMZU / mouse.fit / ATK 共通、caps だけで描く)。
//!
//! レイアウト:
//! 1. ヒーロー 3 枚: ポーリングレート (+ 実測) / LOD / デバウンス — id の慣習
//!    (`polling*`, `lod`, `debounce`) で拾う。無い機種はその枠を出さない。
//! 2. セクションごとのカード: トグルは「タイル」(アイコン + 名前 + 説明 + スイッチ)、
//!    選択肢はセグメント (長い名前は折り返すチップ)、スライダーは大きめの行。
//!    `angle_tune` には傾くマウスの絵を添える。
//! 3. 速度連動 DPI (`MouseCaps::adaptive_dpi`) のカーブ編集カード。
//!
//! `SettingSpec::enabled_when` を満たさない設定は薄く表示し、理由を添えて操作させない
//! (例: 20K FPS は競技モード ON のときだけ)。

use crate::app::{CbXY, Shell, Tab};
use crate::pages::generic::{section_meta, visible, SECTIONS};
use crate::ui::{self, Btn};
use crate::{motion, theme};
use devhub_core::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;

/// `enabled_when` を満たすか。満たさないときは理由。
pub fn enabled(sp: &SettingSpec, st: &DeviceState) -> Result<(), String> {
    match &sp.enabled_when {
        None => Ok(()),
        Some((id, v, note)) => {
            let ok = st.settings.get(id).is_some_and(|cur| match (cur, v) {
                (SettingValue::Bool(a), SettingValue::Bool(b)) => a == b,
                (a, b) => a.as_choice() == b.as_choice(),
            });
            if ok {
                Ok(())
            } else {
                Err(note.clone())
            }
        }
    }
}

/// トグルのタイルに付けるアイコン (id の慣習。未知は汎用)。
fn tile_icon(id: &str) -> &'static str {
    match id {
        "motion_sync" => "activity",
        "angle_snap" => "ruler",
        "ripple" => "audio-waveform",
        "glass_mode" => "gem",
        "hyper_mode" => "zap",
        "tracking_mode" => "flame",
        "rapid_left" | "rapid_right" => "mouse-pointer-click",
        "dpi_xy" => "move",
        _ if id.contains("sleep") => "moon",
        _ => "sliders-horizontal",
    }
}

fn is_hero(id: &str) -> bool {
    id.starts_with("polling") || id == "lod" || id == "debounce"
}

impl Shell {
    /// 設定 1 件を送る (確認ダイアログと enabled_when を守る)。
    pub fn send_setting(&mut self, sp: &SettingSpec, v: SettingValue, cx: &mut Context<Self>) {
        if let Some(st) = self.cur().and_then(|s| s.state.clone()) {
            if let Err(why) = enabled(sp, &st) {
                self.toast(why, true, cx);
                return;
            }
        }
        let change = Change::Setting {
            id: sp.id.clone(),
            value: v,
        };
        if let Some(msg) = sp.confirm.clone() {
            let c2 = change.clone();
            self.confirm(
                "確認",
                &msg,
                "変更する",
                false,
                Rc::new(move |this, _, cx| this.apply(c2.clone(), cx)),
                cx,
            );
        } else {
            self.apply(change, cx);
        }
    }

    pub fn page_performance(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(s) = self.cur() else {
            return div().into_any_element();
        };
        let (Some(caps), Some(state)) = (s.caps.clone(), s.state.clone()) else {
            return div().into_any_element();
        };
        // マウス以外 (キーボード等) は従来の自動カード。
        if caps.keyboard.is_some() || caps.mouse.is_none() {
            return self.page_performance_legacy(window, cx).into_any_element();
        }
        let specs: Vec<SettingSpec> = caps
            .settings
            .iter()
            .filter(|sp| {
                crate::app::tab_of_section(&sp.section, caps.keyboard.is_some()) == Tab::Performance
                    && visible(sp, &state)
            })
            .cloned()
            .collect();

        // ---- 1. ヒーロー
        let mut heroes: Vec<AnyElement> = vec![];
        if let Some(sp) = specs.iter().find(|s| s.id.starts_with("polling")) {
            heroes.push(self.hero_polling(sp, &state, cx));
        }
        if let Some(sp) = specs.iter().find(|s| s.id == "lod") {
            heroes.push(self.hero_choice(sp, &state, "scan-line", cx));
        }
        if let Some(sp) = specs.iter().find(|s| s.id == "debounce") {
            heroes.push(self.hero_stepper(sp, &state, cx));
        }

        // ---- 2. セクションカード
        let mut cards: Vec<AnyElement> = vec![];
        let mut order: Vec<String> = SECTIONS.iter().map(|s| s.0.to_string()).collect();
        for sp in &specs {
            if !order.contains(&sp.section) {
                order.push(sp.section.clone());
            }
        }
        for sec in order {
            let list: Vec<SettingSpec> = specs
                .iter()
                .filter(|s| s.section == sec && !is_hero(&s.id))
                .cloned()
                .collect();
            let actions: Vec<ActionSpec> = caps
                .actions
                .iter()
                .filter(|a| {
                    a.section == sec
                        && crate::app::tab_of_section(&a.section, false) == Tab::Performance
                        && !a.id.ends_with("_finish")
                })
                .cloned()
                .collect();
            if !list.is_empty() || !actions.is_empty() {
                cards.push(self.perf_section(&sec, &list, &actions, &state, window, cx));
            }
            // 速度連動 DPI はセンサーの直後。
            if sec == section::SENSOR {
                if let Some(c) = self.adaptive_card(&caps, &state, cx) {
                    cards.push(c);
                }
            }
        }

        let mut col = div().flex().flex_col().gap(px(theme::GAP));
        if !heroes.is_empty() {
            col = col.child(div().flex().gap(px(theme::GAP)).children(heroes));
        }
        col.child(ui::grid2(cards)).into_any_element()
    }

    // ------------------------------------------------------------ ヒーロー

    fn hero_shell(title: &str, icon: &str, body: impl IntoElement) -> Div {
        let p = theme::pal();
        ui::card()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(26.))
                                    .rounded(px(8.))
                                    .bg(p.accent_soft)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(ui::icon(icon, 14., p.accent)),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.sub)
                                    .child(title.to_string()),
                            ),
                    )
                    .child(body),
            )
    }

    fn hero_polling(&mut self, sp: &SettingSpec, st: &DeviceState, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::pal();
        let SettingKind::Choice { options } = &sp.kind else {
            return div().into_any_element();
        };
        let cur = st.settings.get(&sp.id).map(|v| v.as_choice());
        let big = cur
            .map(|c| {
                if c >= 1000 {
                    format!("{}K", c / 1000)
                } else {
                    format!("{c}")
                }
            })
            .unwrap_or_else(|| "—".into());
        let labels: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
        let sel = cur.and_then(|c| options.iter().position(|o| o.value == c));
        // 実測 (Raw Input)。USB 計測中は同じスナップショットを計測元つきで併記する。
        let live = self.cur().and_then(|s| s.poll.as_ref()).map(|st| {
            let v = st.peak_hz.or(st.seg_hz).unwrap_or(st.hz);
            if !self.last_window_active {
                format!("背面の受信 {:.0} 件/秒 (参考値)", st.hz)
            } else if v < 1. {
                "実測: マウスを動かすと表示".to_string()
            } else {
                format!("実測ピーク {v:.0} Hz")
            }
        });
        let live = match self.cur().and_then(|s| {
            self.usb_cap
                .as_ref()
                .filter(|(cid, _)| cid == &s.id)
                .map(|(_, c)| self.usb_snap.unwrap_or_else(|| c.snapshot()))
        }) {
            Some(snap) => {
                let u = match snap.status {
                    devhub_polling::Status::Waiting => "USB 計測: 待機中".to_string(),
                    devhub_polling::Status::Live => format!("USB 実測 {} Hz", snap.hz),
                    _ => "USB 計測: 停止".to_string(),
                };
                Some(match live {
                    Some(l) => format!("{l} · {u}"),
                    None => u,
                })
            }
            None => live,
        };
        let selector = if theme::is_graphite() {
            // Frost Graphite: ポーリングレートは低→高の階段。選択だけ反転する。
            let mut bars = div().flex().items_end().gap(px(8.)).w_full();
            let n = options.len().max(1) as f32;
            for (i, o) in options.iter().enumerate() {
                let on = Some(i) == sel;
                let before = sel.is_some_and(|s| i < s);
                let h = 24. + if n > 1. { i as f32 / (n - 1.) } else { 0. } * 56.;
                let sp2 = sp.clone();
                let v = o.value;
                bars = bars.child(
                    div()
                        .id(SharedString::from(format!("poll-{}-{}", sp.id, o.value)))
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .child(
                            div()
                                .w_full()
                                .h(px(h))
                                .rounded(px(8.))
                                .bg(if on {
                                    p.inv
                                } else if before {
                                    p.fill
                                } else {
                                    p.ctrl
                                })
                                .border_1()
                                .border_color(if on { theme::alpha(p.edge, 0.) } else { p.hair }),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .font_weight(if on {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if on { p.fg } else { p.sub })
                                .child(o.label.clone()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !on {
                                this.send_setting(&sp2, SettingValue::Choice(v), cx);
                            }
                        })),
                );
            }
            bars
        } else {
            let (sp2, opts) = (sp.clone(), options.clone());
            ui::segmented(
                &format!("hero-{}", sp.id),
                &labels,
                sel.unwrap_or(usize::MAX),
                cx,
                move |this, i, cx| {
                    if Some(i) != sel {
                        this.send_setting(&sp2, SettingValue::Choice(opts[i].value), cx);
                    }
                },
            )
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(6.))
                    .child(ui::big_number(big, 34.))
                    .child(ui::faint("Hz"))
                    .child(div().flex_1())
                    .when_some(live, |d, l| d.child(ui::faint(l))),
            )
            .child(selector)
            .child(div().text_size(px(11.)).text_color(p.faint).child(
                sp.help.clone().unwrap_or_default(),
            ));
        Self::hero_shell(&sp.label, "gauge", body).into_any_element()
    }

    fn hero_choice(&mut self, sp: &SettingSpec, st: &DeviceState, icon: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::pal();
        let SettingKind::Choice { options } = &sp.kind else {
            return div().into_any_element();
        };
        let cur = st.settings.get(&sp.id).map(|v| v.as_choice());
        let label_now = cur
            .and_then(|c| options.iter().find(|o| o.value == c))
            .map(|o| o.label.clone())
            .unwrap_or_else(|| "—".into());
        let dis = enabled(sp, st).err();
        let mut chips = div().flex().flex_wrap().gap(px(6.));
        for o in options {
            let on = Some(o.value) == cur;
            let (sp2, v) = (sp.clone(), o.value);
            chips = chips.child(
                div()
                    .id(ElementId::Name(format!("hc-{}-{}", sp.id, o.value).into()))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(9.))
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .bg(if on { p.accent_soft } else { theme::alpha(p.faint, 0.07) })
                    .border_1()
                    .border_color(if on { p.accent } else { p.hair })
                    .text_color(if on { p.fg } else { p.sub })
                    .hover(|s| s.bg(theme::alpha(p.faint, 0.14)))
                    .child(o.label.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !on {
                            this.send_setting(&sp2, SettingValue::Choice(v), cx);
                        }
                    })),
            );
        }
        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(6.))
                    .child(ui::big_number(label_now, 34.)),
            )
            .child(chips.when(dis.is_some(), |d| d.opacity(0.4)))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(if dis.is_some() { p.warn } else { p.faint })
                    .child(dis.unwrap_or_else(|| sp.help.clone().unwrap_or_default())),
            );
        Self::hero_shell(&sp.label, icon, body).into_any_element()
    }

    fn hero_stepper(&mut self, sp: &SettingSpec, st: &DeviceState, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::pal();
        let SettingKind::Slider { min, max, step, unit } = &sp.kind else {
            return div().into_any_element();
        };
        let cur = st.settings.get(&sp.id).map(|v| v.as_num());
        let (min, max, step) = (*min, *max, *step);
        let btn = |id: String, icon: &'static str, delta: f32, cx: &mut Context<Self>| {
            let sp2 = sp.clone();
            div()
                .id(ElementId::Name(id.into()))
                .size(px(36.))
                .rounded(px(10.))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::alpha(p.faint, 0.08))
                .border_1()
                .border_color(p.hair)
                .hover(|s| s.bg(theme::alpha(p.faint, 0.16)))
                .child(ui::icon(icon, 16., p.fg))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(c) = cur {
                        let nv = ui::quantize(c + delta, min, max, step);
                        if (nv - c).abs() > step * 0.5 {
                            this.send_setting(&sp2, SettingValue::Num(nv), cx);
                        }
                    }
                }))
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(btn(format!("hs-{}-m", sp.id), "minus", -step, cx))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(4.))
                            .child(ui::big_number(
                                cur.map(|c| ui::fmt_num(c, step, "")).unwrap_or_else(|| "—".into()),
                                34.,
                            ))
                            .child(ui::faint(unit.clone())),
                    )
                    .child(btn(format!("hs-{}-p", sp.id), "plus", step, cx)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.faint)
                    .child(sp.help.clone().unwrap_or_default()),
            );
        Self::hero_shell(&sp.label, "timer", body).into_any_element()
    }

    // ------------------------------------------------------------ セクション

    fn perf_section(
        &mut self,
        sec: &str,
        list: &[SettingSpec],
        actions: &[ActionSpec],
        st: &DeviceState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (title, icon) = section_meta(sec);
        let toggles: Vec<&SettingSpec> = list.iter().filter(|s| matches!(s.kind, SettingKind::Toggle)).collect();
        let others: Vec<&SettingSpec> = list.iter().filter(|s| !matches!(s.kind, SettingKind::Toggle)).collect();
        let mut body = ui::body().pt(px(6.)).gap(px(4.));
        if !toggles.is_empty() {
            let mut grid = div().flex().flex_wrap().gap(px(8.)).pb(px(6.));
            for sp in toggles {
                grid = grid.child(self.tile(sp, st, cx));
            }
            body = body.child(grid);
        }
        for sp in others.iter() {
            body = body.child(self.perf_row(sp, st, window, cx));
        }
        for (i, a) in actions.iter().enumerate() {
            body = body.child(self.action_row(a, i == 0 && others.is_empty(), cx));
        }
        ui::card_titled(title, icon, None).child(body).into_any_element()
    }

    /// トグルのタイル。
    fn tile(&mut self, sp: &SettingSpec, st: &DeviceState, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::pal();
        let on = st.settings.get(&sp.id).map(|v| v.as_bool());
        let dis = enabled(sp, st).err();
        let sp2 = sp.clone();
        let icon = tile_icon(&sp.id);
        let k = if theme::is_graphite() {
            if on == Some(true) { 1. } else { 0. }
        } else {
            motion::ease(&format!("tile:{}", sp.id), on == Some(true))
        };
        div()
            .id(ElementId::Name(format!("tile-{}", sp.id).into()))
            .flex_1()
            .min_w(px(180.))
            .p(px(12.))
            .rounded(px(14.))
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap(px(6.))
            .bg(theme::mix(theme::alpha(p.faint, 0.06), p.accent_soft, k))
            .border_1()
            .border_color(theme::mix(p.hair, theme::alpha(p.accent, 0.7), k))
            .hover(|s| s.bg(theme::alpha(p.faint, 0.12)))
            .when(dis.is_some(), |d| d.opacity(0.45))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon(icon, 15., if on == Some(true) { p.accent } else { p.sub }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.fg)
                            .child(sp.label.clone()),
                    )
                    .child(div().flex_none().child(match on {
                        Some(v) => {
                            let sp3 = sp.clone();
                            ui::toggle(format!("tg-{}", sp.id), v, cx, move |this, nv, cx| {
                                this.send_setting(&sp3, SettingValue::Bool(nv), cx)
                            })
                            .into_any_element()
                        }
                        None => ui::faint("—").into_any_element(),
                    })),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(15.))
                    .text_color(if dis.is_some() { p.warn } else { p.faint })
                    .child(dis.clone().unwrap_or_else(|| sp.help.clone().unwrap_or_default())),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(v) = on {
                    this.send_setting(&sp2, SettingValue::Bool(!v), cx);
                }
            }))
            .into_any_element()
    }

    /// 選択肢・スライダーの行 (ラベル上・操作下の 2 段で、狭いカードでもはみ出さない)。
    fn perf_row(&mut self, sp: &SettingSpec, st: &DeviceState, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::pal();
        let dis = enabled(sp, st).err();
        let v = st.settings.get(&sp.id).cloned();
        let head = div()
            .flex()
            .items_baseline()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.fg)
                    .child(sp.label.clone()),
            )
            .child(div().flex_1())
            .when_some(v.as_ref().and_then(|v| match (&sp.kind, v) {
                (SettingKind::Slider { step, unit, .. }, SettingValue::Num(n)) => {
                    Some(ui::fmt_num(*n, *step, unit))
                }
                _ => None,
            }), |d, t| {
                d.child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.accent)
                        .child(t),
                )
            });
        let control: AnyElement = match (&sp.kind, v) {
            (_, None) => ui::faint("読み取れません").into_any_element(),
            (SettingKind::Choice { options }, Some(v)) => {
                let cur = v.as_choice();
                let mut chips = div().flex().flex_wrap().gap(px(6.));
                for o in options {
                    let on = o.value == cur;
                    let (sp2, val) = (sp.clone(), o.value);
                    chips = chips.child(
                        div()
                            .id(ElementId::Name(format!("ch-{}-{}", sp.id, o.value).into()))
                            .px(px(12.))
                            .py(px(6.))
                            .rounded(px(9.))
                            .cursor_pointer()
                            .text_size(px(12.5))
                            .bg(if on { p.accent_soft } else { theme::alpha(p.faint, 0.07) })
                            .border_1()
                            .border_color(if on { p.accent } else { p.hair })
                            .text_color(if on { p.fg } else { p.sub })
                            .hover(|s| s.bg(theme::alpha(p.faint, 0.14)))
                            .child(o.label.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !on {
                                    this.send_setting(&sp2, SettingValue::Choice(val), cx);
                                }
                            })),
                    );
                }
                chips.into_any_element()
            }
            (SettingKind::Slider { min, max, step, unit }, Some(v)) => {
                let (min, max, step) = (*min, *max, *step);
                let cur = v.as_num();
                let fid = format!("perf-{}", sp.id);
                let (sp2, sp3) = (sp.clone(), sp.clone());
                let inp = self.num_field(
                    &fid,
                    cur,
                    min,
                    max,
                    step,
                    unit,
                    window,
                    cx,
                    Rc::new(move |this, nv, cx| this.send_setting(&sp2, SettingValue::Num(nv), cx)),
                );
                let mut r = div().flex().items_center().gap(px(12.));
                if sp.id == "angle_tune" {
                    // 傾くマウスの絵 (公式と同じく角度をそのまま回転で見せる)。
                    let rad = cur.to_radians();
                    r = r.child(
                        div()
                            .size(px(40.))
                            .rounded(px(10.))
                            .bg(theme::alpha(p.faint, 0.07))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                ui::icon("mouse", 24., p.accent)
                                    .with_transformation(Transformation::rotate(radians(rad))),
                            ),
                    );
                }
                r.child(div().flex_1().min_w_0().child(ui::slider(
                    &fid,
                    cur,
                    min,
                    max,
                    step,
                    None,
                    cx,
                    move |this, nv, cx| {
                        if (nv - cur).abs() >= step * 0.5 {
                            this.send_setting(&sp3, SettingValue::Num(nv), cx);
                        }
                    },
                )))
                .child(div().w(px(76.)).flex_none().child(gpui_kit::component::input::Input::new(&inp)))
                .into_any_element()
            }
            (SettingKind::Toggle, Some(v)) => {
                let sp2 = sp.clone();
                ui::toggle(format!("pr-{}", sp.id), v.as_bool(), cx, move |this, nv, cx| {
                    this.send_setting(&sp2, SettingValue::Bool(nv), cx)
                })
                .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .py(px(12.))
            .border_t_1()
            .border_color(p.hair)
            .child(head)
            .child(div().when(dis.is_some(), |d| d.opacity(0.4)).child(control))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(if dis.is_some() { p.warn } else { p.faint })
                    .child(dis.unwrap_or_else(|| sp.help.clone().unwrap_or_default())),
            )
            .into_any_element()
    }

    // ------------------------------------------------------------ 速度連動 DPI

    fn adaptive_card(&mut self, caps: &Capabilities, st: &DeviceState, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ac = caps.mouse.as_ref()?.adaptive_dpi.clone()?;
        let curve = st.mouse.as_ref()?.adaptive.clone()?;
        let p = theme::pal();
        let send = |this: &mut Shell, c: AdaptiveCurve, cx: &mut Context<Shell>| {
            this.apply(Change::Adaptive(c), cx);
        };
        // 有効/無効
        let c_on = curve.clone();
        let toggle = ui::toggle("ada-on", curve.enabled, cx, move |this, v, cx| {
            let mut c = c_on.clone();
            c.enabled = v;
            send(this, c, cx);
        });
        // プリセット
        let cur_preset = ac.presets.iter().position(|(_, pts)| *pts == curve.points);
        let mut presets = div().flex().flex_wrap().gap(px(6.));
        for (i, (name, pts)) in ac.presets.iter().enumerate() {
            let on = cur_preset == Some(i);
            let (pts, en) = (pts.clone(), curve.enabled);
            presets = presets.child(
                div()
                    .id(ElementId::Name(format!("ada-pre-{i}").into()))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(9.))
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .bg(if on { p.accent_soft } else { theme::alpha(p.faint, 0.07) })
                    .border_1()
                    .border_color(if on { p.accent } else { p.hair })
                    .text_color(if on { p.fg } else { p.sub })
                    .child(name.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        send(this, AdaptiveCurve { enabled: en, points: pts.clone() }, cx);
                    })),
            );
        }
        presets = presets.child(
            div()
                .px(px(12.))
                .py(px(6.))
                .rounded(px(9.))
                .text_size(px(12.5))
                .border_1()
                .border_color(if cur_preset.is_none() { p.accent } else { p.hair })
                .bg(if cur_preset.is_none() { p.accent_soft } else { theme::alpha(p.faint, 0.0) })
                .text_color(if cur_preset.is_none() { p.fg } else { p.faint })
                .child("カスタム"),
        );
        // 点の追加/削除
        let n = curve.points.len();
        let c_add = curve.clone();
        let c_del = curve.clone();
        let smax = ac.speed_max;
        let pts_tools = div()
            .flex()
            .gap(px(6.))
            .child(ui::button("ada-add", "点を追加", Some("plus"), Btn::Ghost, cx, move |this, _, cx| {
                let mut c = c_add.clone();
                if c.points.len() >= ac.max_points {
                    this.toast(format!("点は最大 {} 個です", ac.max_points), true, cx);
                    return;
                }
                let last = c.points.last().copied().unwrap_or(AdaptivePoint { speed: 0, ratio: 1. });
                let speed = (last.speed + 15).min(smax);
                if speed <= last.speed {
                    this.toast("右端に空きがありません", true, cx);
                    return;
                }
                c.points.push(AdaptivePoint { speed, ratio: last.ratio });
                send(this, c, cx);
            }))
            .child(ui::button("ada-del", "点を削除", Some("minus"), Btn::Ghost, cx, move |this, _, cx| {
                let mut c = c_del.clone();
                if c.points.len() <= ac.min_points {
                    this.toast(format!("点は最低 {} 個必要です", ac.min_points), true, cx);
                    return;
                }
                c.points.pop();
                send(this, c, cx);
            }));
        let graph = self.adaptive_graph(&ac, &curve, cx);
        let pts_txt = curve
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| format!("P{} {}→×{:.2}", i + 1, p.speed, p.ratio))
            .collect::<Vec<_>>()
            .join("  ·  ");
        let body = ui::body()
            .pt(px(6.))
            .gap(px(10.))
            .child(ui::row_first(
                "有効",
                Some("マウスの速度に応じて DPI を自動で変えます (倍率 1.0 = そのまま)"),
                toggle,
            ))
            .child(div().flex().items_center().gap(px(8.)).child(ui::faint("プリセット")).child(presets))
            .child(div().when(!curve.enabled, |d| d.opacity(0.5)).child(graph))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(pts_tools)
                    .child(div().flex_1().min_w(px(160.)).text_size(px(11.)).text_color(p.faint).child(format!(
                        "{n} 点 · 点をドラッグして調整 · {pts_txt}"
                    ))),
            );
        Some(
            ui::card_titled("AdaDPI (速度連動 DPI)", "chart-spline", None)
                .child(body)
                .into_any_element(),
        )
    }

    fn adaptive_graph(&mut self, ac: &AdaptiveCaps, curve: &AdaptiveCurve, cx: &mut Context<Self>) -> Div {
        let p = theme::pal();
        let (smax, rmax) = (ac.speed_max as f32, ac.ratio_max);
        let pts: Vec<(f32, f32)> = curve
            .points
            .iter()
            .map(|q| (q.speed as f32 / smax, q.ratio / rmax))
            .collect();
        let pts2 = pts.clone();
        let grid = theme::alpha(p.faint, 0.15);
        let one = theme::alpha(p.faint, 0.45);
        let accent = p.accent;
        let one_y = 1. / rmax;
        let cv = canvas(
            move |_, _, _| pts2.clone(),
            move |b, pts, w, _| {
                let (wd, h) = (f32::from(b.size.width), f32::from(b.size.height));
                let at = |x: f32, y: f32| b.origin + point(px(wd * x), px(h * (1. - y)));
                for i in 1..7 {
                    let x = i as f32 / 7.;
                    let mut pb = PathBuilder::stroke(px(1.));
                    pb.move_to(at(x, 0.));
                    pb.line_to(at(x, 1.));
                    if let Ok(path) = pb.build() {
                        w.paint_path(path, grid);
                    }
                }
                let mut pb = PathBuilder::stroke(px(1.));
                pb.move_to(at(0., one_y));
                pb.line_to(at(1., one_y));
                if let Ok(path) = pb.build() {
                    w.paint_path(path, one);
                }
                if pts.is_empty() {
                    return;
                }
                // 塗り + 線 (最後の点から右端までは水平に延長)。
                let mut pf = PathBuilder::fill();
                pf.move_to(at(0., 0.));
                pf.line_to(at(0., pts[0].1));
                for &(x, y) in &pts {
                    pf.line_to(at(x, y));
                }
                pf.line_to(at(1., pts[pts.len() - 1].1));
                pf.line_to(at(1., 0.));
                pf.close();
                if let Ok(path) = pf.build() {
                    w.paint_path(path, theme::alpha(accent, 0.12));
                }
                let mut pb = PathBuilder::stroke(px(2.2));
                pb.move_to(at(0., pts[0].1));
                for &(x, y) in &pts {
                    pb.line_to(at(x, y));
                }
                pb.line_to(at(1., pts[pts.len() - 1].1));
                if let Ok(path) = pb.build() {
                    w.paint_path(path, accent);
                }
            },
        )
        .absolute()
        .inset_0();
        let mut plot = div()
            .id("ada-plot")
            .relative()
            .h(px(170.))
            .rounded(px(10.))
            .bg(theme::alpha(p.faint, 0.05))
            .border_1()
            .border_color(p.hair)
            .child(cv)
            .child(motion::track("ada-graph"));
        for (i, &(x, y)) in pts.iter().enumerate() {
            plot = plot.child(
                div()
                    .absolute()
                    .left(relative(x))
                    .top(relative(1. - y))
                    .ml(px(-9.))
                    .mt(px(-9.))
                    .size(px(18.))
                    .rounded_full()
                    .bg(p.accent)
                    .border_2()
                    .border_color(theme::hex(0xffffff, 0.9))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(9.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::hex(0x0b1020, 1.))
                    .child(format!("{}", i + 1)),
            );
        }
        // ドラッグ: 最寄りの点を掴んで (速度, 倍率) を動かす。速度は隣の点の間に制限。
        let base = curve.clone();
        let (min_pts, max_pts) = (ac.min_points, ac.max_points);
        let _ = (min_pts, max_pts);
        let ac2 = ac.clone();
        plot = plot.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                motion::set_down(true);
                let Some(r) = motion::rect("ada-graph") else { return };
                let (mx, my) = (f32::from(e.position.x) - r[0], f32::from(e.position.y) - r[1]);
                let near = base
                    .points
                    .iter()
                    .enumerate()
                    .map(|(i, q)| {
                        let dx = q.speed as f32 / ac2.speed_max as f32 * r[2] - mx;
                        let dy = (1. - q.ratio / ac2.ratio_max) * r[3] - my;
                        (i, dx * dx + dy * dy)
                    })
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|x| x.0);
                let Some(i) = near else { return };
                let base2 = base.clone();
                let ac3 = ac2.clone();
                let f: CbXY = Rc::new(move |this, tx, ty, cx| {
                    let mut c = base2.clone();
                    let lo = if i == 0 { 0 } else { c.points[i - 1].speed + 1 };
                    let hi = if i + 1 < c.points.len() {
                        c.points[i + 1].speed.saturating_sub(1)
                    } else {
                        ac3.speed_max
                    };
                    let speed = ((tx * ac3.speed_max as f32).round() as u16).clamp(lo, hi.max(lo));
                    let ratio = ((ty * ac3.ratio_max) * 20.).round() / 20.;
                    c.points[i] = AdaptivePoint {
                        speed,
                        ratio: ratio.clamp(0., ac3.ratio_max),
                    };
                    let cur = this
                        .cur()
                        .and_then(|s| s.state.as_ref())
                        .and_then(|s| s.mouse.as_ref())
                        .and_then(|m| m.adaptive.clone());
                    if cur.as_ref() != Some(&c) {
                        this.apply(Change::Adaptive(c), cx);
                    }
                });
                this.begin_drag_xy("ada-graph", f, e.position, cx);
            }),
        );
        // 軸の目盛り
        let xlab = div()
            .flex()
            .justify_between()
            .text_size(px(10.))
            .text_color(p.faint)
            .children((0..=7).map(|i| div().child(format!("{}", (smax as u32) * i / 7))));
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(10.))
                    .text_color(p.faint)
                    .child(format!("倍率 ×{rmax:.1}"))
                    .child("─ 1.0 = 等倍"),
            )
            .child(plot)
            .child(xlab)
            .child(div().text_size(px(10.)).text_color(p.faint).child("← 遅い     マウスの速度     速い →"))
    }
}
