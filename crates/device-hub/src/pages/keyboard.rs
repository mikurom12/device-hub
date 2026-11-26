//! キーボード: キーボード図 (共通部品)・キー割り当て・トリガー (磁気スイッチ)。

use crate::app::Shell;
use crate::ui::{self, Btn};
use crate::{motion, theme};
use devhub_core::keys;
use devhub_core::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;

/// キーキャップ 1 個の見た目。
#[derive(Clone, Default)]
pub struct Cap {
    pub top: String,
    pub sub: Option<String>,
    /// 2 行目の sub (1u キーで "0.15/0.001" がはみ出るので縦積み用)。
    pub sub2: Option<String>,
    /// 面の色 (ヒートマップ・ライティング)。
    pub tint: Option<Hsla>,
    pub selected: bool,
    pub badge: Option<String>,
    /// 押下深さ 0..1 (ストロークテスト)。
    pub depth: Option<f32>,
    pub dim: bool,
    /// 他のキーとの関係 (SOCD の相手など) を示す枠。
    pub link: bool,
}

pub fn kb_parts(this: &Shell) -> Option<(KeyboardCaps, KeyboardState)> {
    let s = this.cur()?;
    Some((
        s.caps.as_ref()?.keyboard.clone()?,
        s.state.as_ref()?.keyboard.clone()?,
    ))
}

/// KeyAction の表示 (キーキャップ用の短い名前, 一覧用の名前)。
pub fn action_label(kc: &KeyboardCaps, a: &KeyAction) -> (String, String) {
    match a {
        KeyAction::None => ("—".into(), "無効".into()),
        KeyAction::Transparent => ("▽".into(), "透過 (下のレイヤー)".into()),
        KeyAction::Code(c) => match kc.code_label(*c) {
            Some((l, s)) => (s.unwrap_or(l).to_string(), l.to_string()),
            None => (format!("{c:X}"), format!("コード 0x{c:X}")),
        },
        KeyAction::Combo { mods, code } => {
            let k = kc
                .code_label(*code)
                .map(|(l, s)| s.unwrap_or(l).to_string())
                .unwrap_or_default();
            let m = keys::mods_label(*mods);
            (format!("{m}{k}"), format!("組み合わせ: {m}{k}"))
        }
        KeyAction::Macro(i) => (format!("M{}", i + 1), format!("マクロ {}", i + 1)),
    }
}

impl Shell {
    /// キーボード図。`on_click(shell, index, shift)`。
    pub fn keyboard_view(
        &mut self,
        id: &str,
        kc: &KeyboardCaps,
        caps: Vec<Cap>,
        cx: &mut Context<Self>,
        on_click: Rc<dyn Fn(&mut Shell, usize, bool, &mut Context<Shell>)>,
    ) -> Stateful<Div> {
        let p = theme::pal();
        let (w_units, h_units) = kc.layout.iter().fold((0f32, 0f32), |(w, h), k| {
            (w.max(k.x + k.w), h.max(k.y + k.h))
        });
        let wrap = format!("kbwrap:{id}");
        let avail = motion::rect(&wrap).map(|r| r[2]).unwrap_or(900.);
        let u = ((avail - 8.) / w_units.max(1.)).clamp(30., 62.);
        let gap = (u * 0.08).max(3.);
        let mut board = div()
            .relative()
            .w(px(w_units * u))
            .h(px(h_units * u))
            .mx_auto();
        // 同じ刻印のキーが複数あるとき (分割スペース ×3 など) は L/C/R を添える。
        let suffix: Vec<Option<String>> = kc
            .layout
            .iter()
            .map(|k| {
                let same: Vec<&LayoutKey> =
                    kc.layout.iter().filter(|o| o.label == k.label).collect();
                if same.len() < 2 {
                    return None;
                }
                let pos = same.iter().filter(|o| (o.y, o.x) < (k.y, k.x)).count();
                Some(match (same.len(), pos) {
                    (2, 0) => "L".into(),
                    (2, _) => "R".into(),
                    (3, 0) => "L".into(),
                    (3, 1) => "C".into(),
                    (3, _) => "R".into(),
                    (_, n) => format!("{}", n + 1),
                })
            })
            .collect();
        for (i, k) in kc.layout.iter().enumerate() {
            let mut c = caps.get(i).cloned().unwrap_or_default();
            if let Some(sfx) = &suffix[i] {
                if c.top == k.label {
                    c.top = format!("{} {sfx}", c.top);
                }
            }
            let on_click = on_click.clone();
            let hover_key = format!("cap:{id}:{i}");
            let hov = motion::hovered(&hover_key);
            let lift = motion::spring_with(
                &format!("capl:{id}:{i}"),
                if c.selected {
                    1.
                } else if hov {
                    0.5
                } else {
                    0.
                },
                24.,
            );
            let face = c.tint.unwrap_or(p.key);
            let mut cap = div()
                .id(SharedString::from(format!("cap-{id}-{i}")))
                .absolute()
                .left(px(k.x * u + gap / 2.))
                .top(px(k.y * u + gap / 2. - lift * 2.))
                .w(px(k.w * u - gap))
                .h(px(k.h * u - gap))
                .rounded(px(u * 0.18))
                .cursor_pointer()
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(face, 0.),
                    linear_color_stop(theme::alpha(face, face.a * 0.72), 1.),
                ))
                .border_1()
                .border_color(if c.selected {
                    p.accent
                } else if c.link {
                    p.warn
                } else {
                    p.key_edge
                })
                .shadow(vec![BoxShadow {
                    color: if c.selected {
                        theme::alpha(p.accent, 0.5)
                    } else {
                        theme::hex(0x000000, if p.light { 0.1 } else { 0.35 })
                    },
                    offset: point(px(0.), px(2. + lift * 3.)),
                    blur_radius: px(4. + lift * 8.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .when(c.dim, |d| d.opacity(0.45))
                .child(motion::track(hover_key))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(
                    div()
                        .text_size(px((u * 0.25).clamp(9., 14.)))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.fg)
                        .whitespace_nowrap()
                        .child(c.top.clone()),
                )
                .when_some(c.sub.clone(), |d, s| {
                    d.child(
                        div()
                            .text_size(px((u * 0.18).clamp(8., 10.)))
                            .text_color(p.faint)
                            .whitespace_nowrap()
                            .child(s),
                    )
                })
                .when_some(c.sub2.clone(), |d, s| {
                    d.child(
                        div()
                            .text_size(px((u * 0.18).clamp(8., 10.)))
                            .text_color(p.faint)
                            .whitespace_nowrap()
                            .child(s),
                    )
                })
                .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                    on_click(this, i, e.modifiers().shift, cx);
                    cx.notify();
                }));
            if let Some(b) = c.badge.clone() {
                cap = cap.child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .right(px(3.))
                        .text_size(px(8.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.accent)
                        .child(b),
                );
            }
            if let Some(dp) = c.depth {
                cap = cap.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(relative(dp.clamp(0., 1.)))
                        .bg(theme::alpha(p.accent, 0.45)),
                );
            }
            board = board.child(cap);
        }
        div()
            .id(SharedString::from(format!("kbscroll:{id}")))
            .relative()
            .w_full()
            .overflow_x_scroll().smooth_wheel()
            .child(motion::track(wrap))
            .child(board)
    }

    // ================================================================== キー割り当て

    pub fn page_keymap(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::pal();
        let Some((kc, ks)) = kb_parts(self) else {
            return div().into_any_element();
        };
        let layer = (self.kb_layer as usize).min(kc.layers.len().saturating_sub(1));
        let map = ks.keymap.get(layer).cloned().unwrap_or_default();
        let sel = self.kb_sel.iter().next().copied();
        let adv_keys: Vec<usize> = ks
            .advanced
            .iter()
            .filter(|a| a.layer as usize == layer)
            .map(|a| a.key)
            .collect();

        let caps: Vec<Cap> = kc
            .layout
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let a = map.get(i).copied().unwrap_or(KeyAction::None);
                let (short, _) = action_label(&kc, &a);
                let remapped = !same_key(&short, &k.label) && !matches!(a, KeyAction::Transparent);
                Cap {
                    top: short,
                    sub: remapped.then(|| k.label.clone()),
                    tint: remapped.then(|| theme::alpha(p.accent, 0.16)),
                    selected: sel == Some(i),
                    badge: adv_keys.contains(&i).then(|| "★".into()),
                    dim: matches!(a, KeyAction::Transparent | KeyAction::None),
                    ..Default::default()
                }
            })
            .collect();
        let view = self.keyboard_view(
            "keymap",
            &kc,
            caps,
            cx,
            Rc::new(|this, i, _, _| {
                if this.kb_sel.len() == 1 && this.kb_sel.contains(&i) {
                    this.kb_sel.clear();
                } else {
                    this.kb_sel.clear();
                    this.kb_sel.insert(i);
                }
            }),
        );

        let layers: Vec<String> = kc.layers.clone();
        let layer_ctl = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                ui::segmented("layer", &layers, layer, cx, |this, i, _| {
                    this.kb_layer = i as u8;
                })
                .w(px((layers.len() as f32 * 90.).min(380.))),
            )
            .child(ui::button(
                "layer-reset",
                "このレイヤーを初期化",
                Some("rotate-ccw"),
                Btn::Ghost,
                cx,
                move |this, _, cx| {
                    let l = layer as u8;
                    this.confirm(
                        "レイヤーを初期化",
                        "このレイヤーのキー割り当てを既定に戻します。",
                        "初期化",
                        true,
                        Rc::new(move |this, _, cx| {
                            this.apply(Change::KeyMapReset { layer: l }, cx);
                            if let Some(id) = this.device_id() {
                                this.hub.reload(&id);
                            }
                        }),
                        cx,
                    );
                },
            ))
            .when(layers.len() > 1, |d| {
                let others: Vec<(usize, String)> = layers
                    .iter()
                    .cloned()
                    .enumerate()
                    .filter(|(i, _)| *i != layer)
                    .collect();
                let items: Vec<crate::app::PopItem> = others
                    .iter()
                    .map(|(_, n)| crate::app::PopItem::new(format!("{n} へコピー"), false))
                    .collect();
                let src = map.clone();
                d.child(ui::select(
                    "layer-copy",
                    items,
                    "別レイヤーへコピー",
                    170.,
                    cx,
                    move |this, i, _, cx| {
                        let (dst, name) = others[i].clone();
                        let src = src.clone();
                        this.confirm(
                            "レイヤーをコピー",
                            &format!("表示中のレイヤーの割り当てで「{name}」を上書きします。"),
                            "コピー",
                            true,
                            Rc::new(move |this, _, cx| {
                                let cur = crate::pages::keyboard::kb_parts(this)
                                    .map(|(_, ks)| ks.keymap.get(dst).cloned().unwrap_or_default());
                                let cur = cur.unwrap_or_default();
                                for (k, a) in src.iter().enumerate() {
                                    if cur.get(k) != Some(a) {
                                        this.apply(
                                            Change::KeyMap {
                                                layer: dst as u8,
                                                key: k,
                                                action: *a,
                                            },
                                            cx,
                                        );
                                    }
                                }
                            }),
                            cx,
                        );
                    },
                ))
            });

        let board_card =
            ui::card_titled("キーボード", "keyboard", Some(layer_ctl.into_any_element()))
                .child(ui::body().pt(px(10.)).pb(px(18.)).child(view));

        let editor = match sel {
            None => ui::card().child(
                div()
                    .relative()
                    .p(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(ui::icon("mouse-pointer-click", 18., p.sub))
                    .child(ui::sub("割り当てを変えたいキーをクリックしてください")),
            ),
            Some(i) => self.key_picker(&kc, &ks, layer as u8, i, window, cx),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(theme::GAP))
            .child(board_card)
            .child(editor)
            .into_any_element()
    }

    fn key_picker(
        &mut self,
        kc: &KeyboardCaps,
        ks: &KeyboardState,
        layer: u8,
        key: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = theme::pal();
        let cur = ks
            .keymap
            .get(layer as usize)
            .and_then(|l| l.get(key))
            .copied()
            .unwrap_or(KeyAction::None);
        let (_, cur_long) = action_label(kc, &cur);
        let orig = kc
            .layout
            .get(key)
            .map(|k| k.label.clone())
            .unwrap_or_default();

        // グループ: catalog + 特殊 + マクロ + 組み合わせ。
        let mut groups: Vec<String> = kc.catalog.iter().map(|g| g.name.clone()).collect();
        groups.push("特殊".into());
        let macro_slots = kc.macros.as_ref().map(|m| m.slots).unwrap_or(0);
        if macro_slots > 0 {
            groups.push("マクロ".into());
        }
        if kc.combos {
            groups.push("組み合わせ".into());
        }
        self.key_group = self.key_group.min(groups.len() - 1);
        let g = self.key_group;
        let gname = groups[g].clone();

        let mut tabs = div().flex().flex_wrap().gap(px(6.));
        for (i, n) in groups.iter().enumerate() {
            let on = i == g;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("kg-{i}")))
                    .h(px(30.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(10.))
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .border_1()
                    .when(on, |d| {
                        d.bg(p.accent_soft)
                            .border_color(theme::alpha(p.accent, 0.5))
                            .text_color(p.fg)
                    })
                    .when(!on, |d| {
                        d.bg(p.ctrl)
                            .border_color(p.hair)
                            .text_color(p.sub)
                            .hover(|s| s.bg(p.ctrl_hover))
                    })
                    .child(n.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.key_group = i;
                        cx.notify();
                    })),
            );
        }

        let choice = |id: String, label: String, sub: Option<String>, on: bool| {
            div()
                .id(SharedString::from(id))
                .min_w(px(58.))
                .h(px(44.))
                .px(px(8.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .rounded(px(11.))
                .cursor_pointer()
                .border_1()
                .when(on, |d| {
                    d.bg(p.accent_soft)
                        .border_color(theme::alpha(p.accent, 0.6))
                })
                .when(!on, |d| {
                    d.bg(p.key)
                        .border_color(p.key_edge)
                        .hover(|s| s.bg(p.ctrl_hover))
                })
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .whitespace_nowrap()
                        .child(label),
                )
                .when_some(sub, |d, s| {
                    d.child(
                        div()
                            .text_size(px(9.5))
                            .text_color(p.faint)
                            .whitespace_nowrap()
                            .child(s),
                    )
                })
        };
        let assign = move |a: KeyAction| {
            move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                this.apply(
                    Change::KeyMap {
                        layer,
                        key,
                        action: a,
                    },
                    cx,
                );
            }
        };

        let search_in = self.input(
            "key_search",
            "",
            "キーを検索 (例: F5, 音量, マウス)",
            window,
            cx,
        );
        let q = self
            .input_value("key_search", cx)
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let mut grid = div().flex().flex_wrap().gap(px(6.));
        let combo_mode = gname == "組み合わせ" && q.is_empty();
        if !q.is_empty() {
            let mut n = 0;
            for group in &kc.catalog {
                for k in &group.keys {
                    let short = k.short.clone().unwrap_or_else(|| k.label.clone());
                    if !(k.label.to_lowercase().contains(&q)
                        || short.to_lowercase().contains(&q)
                        || group.name.to_lowercase().contains(&q))
                    {
                        continue;
                    }
                    n += 1;
                    if n > 80 {
                        break;
                    }
                    let a = KeyAction::Code(k.code);
                    grid = grid.child(
                        choice(
                            format!("ks-{}", k.code),
                            short,
                            Some(group.name.clone()),
                            cur == a,
                        )
                        .on_click(cx.listener(assign(a))),
                    );
                }
            }
            if n == 0 {
                grid = grid.child(ui::faint("見つかりません"));
            }
        } else if gname == "特殊" {
            grid = grid
                .child(
                    choice(
                        "k-none".into(),
                        "無効".into(),
                        Some("何もしない".into()),
                        cur == KeyAction::None,
                    )
                    .on_click(cx.listener(assign(KeyAction::None))),
                )
                .child(
                    choice(
                        "k-trans".into(),
                        "透過".into(),
                        Some("下のレイヤー".into()),
                        cur == KeyAction::Transparent,
                    )
                    .on_click(cx.listener(assign(KeyAction::Transparent))),
                );
        } else if gname == "マクロ" {
            for m in 0..macro_slots {
                let a = KeyAction::Macro(m);
                let defined = ks.macros.get(m as usize).is_some_and(|x| x.is_some());
                grid = grid.child(
                    choice(
                        format!("k-m{m}"),
                        format!("M{}", m + 1),
                        Some(if defined {
                            "定義済み".into()
                        } else {
                            "空".into()
                        }),
                        cur == a,
                    )
                    .on_click(cx.listener(assign(a))),
                );
            }
        } else if combo_mode {
            let mut mods_row = div().flex().gap(px(6.)).pb(px(6.));
            for (lab, bit) in [
                ("Ctrl", mods::LCTRL),
                ("Shift", mods::LSHIFT),
                ("Alt", mods::LALT),
                ("Win", mods::LGUI),
            ] {
                let on = self.combo_mods & bit != 0;
                mods_row =
                    mods_row.child(choice(format!("cm-{lab}"), lab.into(), None, on).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.combo_mods ^= bit;
                            cx.notify();
                        }),
                    ));
            }
            grid = grid
                .child(div().w_full().child(mods_row))
                .child(div().w_full().child(ui::faint(
                    "修飾を選んでから、組み合わせるキーを押してください",
                )));
            let mods = self.combo_mods;
            if let Some(basic) = kc.catalog.first() {
                for k in &basic.keys {
                    let a = KeyAction::Combo { mods, code: k.code };
                    grid = grid.child(
                        choice(
                            format!("k-c{}", k.code),
                            k.short.clone().unwrap_or(k.label.clone()),
                            None,
                            cur == a,
                        )
                        .on_click(cx.listener(assign(a))),
                    );
                }
            }
        } else if let Some(group) = kc.catalog.get(g) {
            for k in &group.keys {
                let a = KeyAction::Code(k.code);
                let short = k.short.clone().unwrap_or_else(|| k.label.clone());
                let sub = (k.label != short).then(|| k.label.clone());
                grid = grid.child(
                    choice(format!("k-{}", k.code), short, sub, cur == a)
                        .on_click(cx.listener(assign(a))),
                );
            }
        }

        ui::card_titled(
            &format!("「{orig}」に割り当てる"),
            "replace",
            Some(ui::chip(format!("現在: {cur_long}"), p.accent).into_any_element()),
        )
        .child(
            ui::body()
                .gap(px(12.))
                .pt(px(8.))
                .pb(px(18.))
                .child(
                    div()
                        .w(px(360.))
                        .child(gpui_kit::component::input::Input::new(&search_in)),
                )
                .when(q.is_empty(), |d| d.child(tabs))
                .child(grid),
        )
    }

    // ================================================================== トリガー

    pub fn page_trigger(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = theme::pal();
        let Some((kc, ks)) = kb_parts(self) else {
            return div().into_any_element();
        };
        let Some(pc) = kc.perf.clone() else {
            return div().into_any_element();
        };
        let live = self
            .cur()
            .map(|s| (s.travel_on, s.travel.clone(), s.travel_max))
            .unwrap_or((false, vec![], 0.));
        // 実測最大で正規化 (実際の底は公称全ストロークより浅い)。
        let live_max = live.2.max(0.5);
        let n = kc.layout.len();
        self.kb_sel.retain(|i| *i < n);
        let sel: Vec<usize> = self.kb_sel.iter().copied().collect();

        let caps: Vec<Cap> = kc
            .layout
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let pf = ks.perf.get(i).copied().unwrap_or_default();
                // モードに応じた トリガー/リセット のペアを出す:
                // 通常 = normalPress / normalRelease、RT = rt_first / rt_release。
                let (trig, reset) = if pf.mode == TriggerMode::Rapid {
                    (pf.rt_first, pf.rt_release)
                } else {
                    (pf.actuation, pf.release.unwrap_or(pf.actuation))
                };
                let t = (trig / pc.travel).clamp(0., 1.);
                let heat = theme::mix(theme::hex(0x3ec7ff, 0.38), theme::hex(0xff7a3d, 0.42), t);
                // 0.001 級が 0.00 に潰れて見えないよう、小さい値は桁を増やす。
                let fv = |v: f32| {
                    if v != 0. && v < 0.01 {
                        format!("{v:.3}")
                    } else {
                        format!("{v:.2}")
                    }
                };
                Cap {
                    top: k.label.clone(),
                    // アクチュエーションポイント/リセットを縦2行で (1u キーでも欠けない)
                    sub: Some(format!("AP{}", fv(trig))),
                    sub2: Some(format!("R{}", fv(reset))),
                    tint: Some(heat),
                    selected: self.kb_sel.contains(&i),
                    badge: (pf.mode == TriggerMode::Rapid).then(|| "RT".into()),
                    depth: if live.0 {
                        live.1.get(i).map(|d| d / live_max)
                    } else {
                        None
                    },
                    ..Default::default()
                }
            })
            .collect();
        let view = self.keyboard_view(
            "trigger",
            &kc,
            caps,
            cx,
            Rc::new(|this, i, _shift, _| {
                // クリックで選択トグル (複数選択が基本)。Shift も同じ動作。
                if !this.kb_sel.insert(i) {
                    this.kb_sel.remove(&i);
                }
            }),
        );

        let labels: Vec<String> = kc.layout.iter().map(|k| k.label.clone()).collect();
        let pick = move |names: &'static [&'static str]| {
            let labels = labels.clone();
            move |this: &mut Shell, _: &mut Window, cx: &mut Context<Shell>| {
                this.kb_sel = labels
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| names.contains(&l.as_str()))
                    .map(|(i, _)| i)
                    .collect();
                cx.notify();
            }
        };
        let tools = div()
            .flex()
            .gap(px(6.))
            .child(ui::button(
                "sel-all",
                "全選択",
                Some("square-check"),
                Btn::Ghost,
                cx,
                move |this, _, cx| {
                    this.kb_sel = (0..n).collect();
                    cx.notify();
                },
            ))
            .child(ui::button(
                "sel-wasd",
                "WASD",
                None,
                Btn::Ghost,
                cx,
                pick(&["W", "A", "S", "D"]),
            ))
            .child(ui::button(
                "sel-inv",
                "反転",
                None,
                Btn::Ghost,
                cx,
                move |this, _, cx| {
                    this.kb_sel = (0..n).filter(|i| !this.kb_sel.contains(i)).collect();
                    cx.notify();
                },
            ))
            .child(ui::button(
                "sel-none",
                "選択解除",
                None,
                Btn::Ghost,
                cx,
                |this, _, cx| {
                    this.kb_sel.clear();
                    cx.notify();
                },
            ))
            .when(pc.live_travel, |d| {
                let on = live.0;
                d.child(ui::button(
                    "live",
                    if on {
                        "テスト終了"
                    } else {
                        "ストロークテスト"
                    },
                    Some("activity"),
                    if on { Btn::Primary } else { Btn::Glass },
                    cx,
                    move |this, _, cx| {
                        if let Some(id) = this.device_id() {
                            this.hub.set_travel(&id, !on);
                        }
                        cx.notify();
                    },
                ))
            });

        let board = ui::card_titled("トリガーマップ", "arrow-down-to-line", Some(tools.into_any_element())).child(
            ui::body()
                .pt(px(10.))
                .pb(px(14.))
                .gap(px(10.))
                .child(view)
                .child(
                    // 凡例: キーの色 = トリガー位置 (浅い→深い)
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::faint(format!("浅い {:.1}mm", pc.min.max(0.1))))
                        .child(
                            div()
                                .w(px(160.))
                                .h(px(8.))
                                .rounded_full()
                                .bg(linear_gradient(
                                    90.,
                                    linear_color_stop(theme::hex(0x3ec7ff, 0.9), 0.),
                                    linear_color_stop(theme::hex(0xff7a3d, 0.9), 1.),
                                )),
                        )
                        .child(ui::faint(format!("深い {:.1}mm", pc.travel)))
                        .child(div().flex_1())
                        .child(ui::faint("AP = アクチュエーションポイント / R = リセット · RT = ラピッドトリガー · クリックで複数選択")),
                ),
        );

        let editor = if sel.is_empty() {
            ui::card().child(
                div()
                    .relative()
                    .p(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(ui::icon("mouse-pointer-click", 18., p.sub))
                    .child(ui::sub("設定するキーを選んでください (複数可)")),
            )
        } else {
            self.perf_editor(&kc, &ks, &pc, &sel, live, window, cx)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(theme::GAP))
            .child(board)
            .child(editor)
            .into_any_element()
    }

    fn perf_editor(
        &mut self,
        kc: &KeyboardCaps,
        ks: &KeyboardState,
        pc: &PerfCaps,
        sel: &[usize],
        live: (bool, Vec<f32>, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = theme::pal();
        let cur = ks.perf.get(sel[0]).copied().unwrap_or_default();
        let keys: Vec<usize> = sel.to_vec();
        let (min, max, step) = (pc.min, pc.travel, pc.step);
        let names: Vec<String> = sel
            .iter()
            .take(6)
            .filter_map(|i| kc.layout.get(*i).map(|k| k.label.clone()))
            .collect();
        let title = if sel.len() > 6 {
            format!("{} 他 {} キー", names.join(" "), sel.len() - 6)
        } else {
            names.join(" ")
        };
        // フィールド変更の apply クロージャを作る (スライダーと数値入力で共用)。
        let mkset = {
            let keys = keys.clone();
            move |f: Rc<dyn Fn(&mut KeyPerf, f32)>| {
                let keys = keys.clone();
                Rc::new(move |this: &mut Shell, v: f32, cx: &mut Context<Shell>| {
                    let mut np = cur;
                    f(&mut np, v);
                    this.apply(
                        Change::KeyPerf {
                            keys: keys.clone(),
                            perf: np,
                        },
                        cx,
                    );
                }) as Rc<dyn Fn(&mut Shell, f32, &mut Context<Shell>)>
            }
        };
        let rapid = cur.mode == TriggerMode::Rapid;
        let keys_mode = keys.clone();
        let mut body = ui::body().pt(px(6.));
        if pc.rapid_trigger {
            body = body.child(ui::row_first(
                "トリガー方式",
                Some(if rapid {
                    "押し戻し量で ON/OFF を判定 (素早い切り返し)"
                } else {
                    "決まった深さで ON / OFF"
                }),
                ui::segmented(
                    "perf-mode",
                    &["通常".into(), "ラピッドトリガー".into()],
                    rapid as usize,
                    cx,
                    move |this, i, cx| {
                        let mut np = cur;
                        np.mode = if i == 1 {
                            TriggerMode::Rapid
                        } else {
                            TriggerMode::Normal
                        };
                        this.apply(
                            Change::KeyPerf {
                                keys: keys_mode.clone(),
                                perf: np,
                            },
                            cx,
                        );
                    },
                )
                .w(px(280.)),
            ));
        }
        // 連続高速トリガーは公式と同じく方式選択のすぐ下に置く (RT 時のみ)。
        if rapid && pc.continuous_rt {
            let keys_c = keys.clone();
            body = body.child(ui::row(
                "連続高速トリガー",
                Some("完全に戻るまでラピッドトリガーを続ける"),
                ui::toggle("perf-cont", cur.continuous, cx, move |this, on, cx| {
                    let mut np = cur;
                    np.continuous = on;
                    this.apply(
                        Change::KeyPerf {
                            keys: keys_c.clone(),
                            perf: np,
                        },
                        cx,
                    );
                }),
            ));
        }
        // 公式と同じ並び: ファーストトラベル → 押下 (トリガー) → 離す (リセット) → 上下デッドゾーン。
        // RT 時の 押下/離す は rtPress/rtRelease (感度)、通常時は normalPress/normalRelease。
        if rapid {
            if pc.rt_first {
                let f = mkset(Rc::new(|p, v| p.rt_first = v));
                let inp = self.num_field(
                    "perf-first",
                    cur.rt_first,
                    min,
                    max,
                    step,
                    "mm",
                    window,
                    cx,
                    f.clone(),
                );
                body = body.child(ui::row(
                    "ファーストトラベル",
                    Some("最初に ON になる深さ"),
                    ui::slider_value_edit(
                        "perf-first",
                        cur.rt_first,
                        min,
                        max,
                        step,
                        &inp,
                        cx,
                        move |t, v, c| f(t, v, c),
                    ),
                ));
            }
            let rt_max = 2.0f32.min(max);
            {
                let f = mkset(Rc::new(|p, v| p.rt_press = v));
                let inp = self.num_field(
                    "perf-rtp",
                    cur.rt_press,
                    min,
                    rt_max,
                    step,
                    "mm",
                    window,
                    cx,
                    f.clone(),
                );
                body = body.child(ui::row(
                    "押下 (トリガー)",
                    Some("さらに押し込んで再 ON になる量"),
                    ui::slider_value_edit(
                        "perf-rtp",
                        cur.rt_press,
                        min,
                        rt_max,
                        step,
                        &inp,
                        cx,
                        move |t, v, c| f(t, v, c),
                    ),
                ));
            }
            {
                let f = mkset(Rc::new(|p, v| p.rt_release = v));
                let inp = self.num_field(
                    "perf-rtr",
                    cur.rt_release,
                    min,
                    rt_max,
                    step,
                    "mm",
                    window,
                    cx,
                    f.clone(),
                );
                body = body.child(ui::row(
                    "離す (リセット)",
                    Some("戻して OFF になる量"),
                    ui::slider_value_edit(
                        "perf-rtr",
                        cur.rt_release,
                        min,
                        rt_max,
                        step,
                        &inp,
                        cx,
                        move |t, v, c| f(t, v, c),
                    ),
                ));
            }
        } else {
            {
                let f = mkset(Rc::new(|p, v| p.actuation = v));
                let inp = self.num_field(
                    "perf-act",
                    cur.actuation,
                    min,
                    max,
                    step,
                    "mm",
                    window,
                    cx,
                    f.clone(),
                );
                body = body.child(ui::row(
                    "押下 (トリガー)",
                    Some("この深さで押下と判定"),
                    ui::slider_value_edit(
                        "perf-act",
                        cur.actuation,
                        min,
                        max,
                        step,
                        &inp,
                        cx,
                        move |t, v, c| f(t, v, c),
                    ),
                ));
            }
            if pc.separate_release {
                // 離す位置は公式同様常に表示。release=None (押下連動) なら
                // actuation の値を見せ、動かすと独立設定に切り替わる。
                let rel = cur.release.unwrap_or(cur.actuation);
                let f = mkset(Rc::new(|p, v| p.release = Some(v)));
                let inp =
                    self.num_field("perf-rel", rel, min, max, step, "mm", window, cx, f.clone());
                body = body.child(ui::row(
                    "離す (リセット)",
                    Some(if cur.release.is_some() {
                        "この深さまで戻ると離したと判定"
                    } else {
                        "押下と連動中 — 動かすと独立設定になる"
                    }),
                    ui::slider_value_edit(
                        "perf-rel",
                        rel,
                        min,
                        max,
                        step,
                        &inp,
                        cx,
                        move |t, v, c| f(t, v, c),
                    ),
                ));
            }
        }
        if pc.deadzones {
            let f = mkset(Rc::new(|p, v| p.top_deadzone = v));
            let inp = self.num_field(
                "perf-tdz",
                cur.top_deadzone,
                0.,
                1.0,
                step,
                "mm",
                window,
                cx,
                f.clone(),
            );
            body = body.child(ui::row(
                "上端デッドゾーン",
                Some("触れただけの誤入力を防ぐ"),
                ui::slider_value_edit(
                    "perf-tdz",
                    cur.top_deadzone,
                    0.,
                    1.0,
                    step,
                    &inp,
                    cx,
                    move |t, v, c| f(t, v, c),
                ),
            ));
            let f = mkset(Rc::new(|p, v| p.bottom_deadzone = v));
            let inp = self.num_field(
                "perf-bdz",
                cur.bottom_deadzone,
                0.,
                1.0,
                step,
                "mm",
                window,
                cx,
                f.clone(),
            );
            body = body.child(ui::row(
                "下端デッドゾーン",
                Some("底打ち付近の揺れを無視"),
                ui::slider_value_edit(
                    "perf-bdz",
                    cur.bottom_deadzone,
                    0.,
                    1.0,
                    step,
                    &inp,
                    cx,
                    move |t, v, c| f(t, v, c),
                ),
            ));
        }
        // ---- 詳細設定 (公式に無い追加項目) ----
        body = body.child(ui::section_label("詳細設定"));
        // ワンクリックのプリセット (Web 版には無い)。
        {
            let presets: Vec<(&str, KeyPerf)> = {
                let rt = |first: f32, s: f32| KeyPerf {
                    mode: TriggerMode::Rapid,
                    actuation: first.max(min),
                    rt_first: first.max(min),
                    rt_press: s.max(min),
                    rt_release: s.max(min),
                    continuous: true,
                    ..cur
                };
                let mut v = vec![];
                if pc.rapid_trigger {
                    v.push(("最速 RT", rt(0.1, 0.03)));
                    v.push(("競技 RT", rt(0.2, 0.1)));
                }
                v.push((
                    "標準",
                    KeyPerf {
                        mode: TriggerMode::Normal,
                        actuation: 1.2f32.min(max),
                        release: None,
                        ..cur
                    },
                ));
                v.push((
                    "タイピング",
                    KeyPerf {
                        mode: TriggerMode::Normal,
                        actuation: 2.0f32.min(max),
                        release: None,
                        ..cur
                    },
                ));
                v
            };
            let mut chips = div().flex().gap(px(6.));
            for (i, (name, pf)) in presets.into_iter().enumerate() {
                let keys_p = keys.clone();
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("perf-pre-{i}")))
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(9.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .bg(p.ctrl)
                        .border_1()
                        .border_color(p.hair)
                        .hover(|s| s.bg(p.ctrl_hover))
                        .child(name)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.apply(
                                Change::KeyPerf {
                                    keys: keys_p.clone(),
                                    perf: pf,
                                },
                                cx,
                            );
                        })),
                );
            }
            body = body.child(ui::row(
                "プリセット",
                Some("選択中のキーにまとめて適用"),
                chips,
            ));
        }
        if !rapid && pc.separate_release {
            // release=None は「押下に連動」。離す位置を別にしたい場合は OFF にする。
            let linked = cur.release.is_none();
            let keys_rel = keys.clone();
            body = body.child(ui::row(
                "離す位置を押下に連動",
                Some("ON ならリセット点は押下と同じ位置になる"),
                ui::toggle("perf-rel-link", linked, cx, move |this, on, cx| {
                    let mut np = cur;
                    np.release = if on { None } else { Some(cur.actuation) };
                    this.apply(
                        Change::KeyPerf {
                            keys: keys_rel.clone(),
                            perf: np,
                        },
                        cx,
                    );
                }),
            ));
        }
        // RT 時に使われない通常モード用の値 (押下/離す) は公式と同じく出さない。
        // 値は KeyPerf に残るので、通常に切り替えればまた編集できる。
        if !pc.switch_types.is_empty() {
            let items: Vec<crate::app::PopItem> = pc
                .switch_types
                .iter()
                .enumerate()
                .map(|(i, n)| crate::app::PopItem::new(n.clone(), i == cur.switch_type as usize))
                .collect();
            let keys_sw = keys.clone();
            body = body.child(ui::row(
                "スイッチ種別",
                Some("使っている磁気スイッチに合わせる"),
                ui::select(
                    "perf-sw",
                    items,
                    pc.switch_types
                        .get(cur.switch_type as usize)
                        .map(|s| s.as_str())
                        .unwrap_or("-"),
                    200.,
                    cx,
                    move |this, i, _, cx| {
                        let mut np = cur;
                        np.switch_type = i as u8;
                        this.apply(
                            Change::KeyPerf {
                                keys: keys_sw.clone(),
                                perf: np,
                            },
                            cx,
                        );
                    },
                ),
            ));
        }

        // 右側: ストローク図。
        let depth = if live.0 {
            sel.first().and_then(|i| live.1.get(*i)).copied()
        } else {
            None
        };
        let figure = travel_figure(&cur, pc, depth);
        let all = (0..kc.layout.len()).collect::<Vec<_>>();
        let apply_all = ui::button(
            "perf-all",
            "全キーに適用",
            Some("copy"),
            Btn::Ghost,
            cx,
            move |this, _, cx| {
                let a = all.clone();
                this.confirm(
                    "全キーに適用",
                    "この設定をすべてのキーにコピーします。",
                    "適用",
                    false,
                    Rc::new(move |this, _, cx| {
                        this.apply(
                            Change::KeyPerf {
                                keys: a.clone(),
                                perf: cur,
                            },
                            cx,
                        )
                    }),
                    cx,
                );
            },
        );
        let _ = p;
        ui::card_titled(
            &title,
            "sliders-vertical",
            Some(apply_all.into_any_element()),
        )
        .child(
            div()
                .relative()
                .flex()
                .items_start()
                .child(div().flex_1().min_w_0().child(body))
                .child(div().flex_none().p(px(18.)).child(figure)),
        )
    }
}

/// 縦のストローク図 (上 = 0mm、下 = 全ストローク)。
/// 通常モードは作動/解放位置、RT モードは初回位置と再ON/OFFの変化量を描く。
/// ラベルは重ならないよう縦にずらし、ずれた分は引き出し線で元の位置につなぐ。
/// mm 値の表示。0.01 未満 (RT の 0.001mm 刻み) は小数 3 桁まで出す。
fn mm_fmt(v: f32) -> String {
    if v.abs() >= 0.01 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

fn travel_figure(pf: &KeyPerf, pc: &PerfCaps, live: Option<f32>) -> Div {
    let p = theme::pal();
    let h = 264.;
    let bar_x = 26.; // 左に目盛りラベル用の余白
    let bar_w = 44.;
    let label_x = bar_x + bar_w + 8.;
    let y = |mm: f32| (mm / pc.travel).clamp(0., 1.) * h;
    let rapid = pf.mode == TriggerMode::Rapid;

    // バーのトラック
    let mut fig = div().relative().w(px(212.)).h(px(h)).child(
        div()
            .absolute()
            .left(px(bar_x))
            .top_0()
            .w(px(bar_w))
            .h(px(h))
            .rounded(px(12.))
            .bg(p.track)
            .border_1()
            .border_color(p.hair),
    );

    // 目盛り (0 / 半分 / 全ストローク) — バーの左に数字、バーに届く短い刻み線
    for (mm, s) in [
        (0., "0".to_string()),
        (pc.travel / 2., format!("{:.1}", pc.travel / 2.)),
        (pc.travel, format!("{:.1}", pc.travel)),
    ] {
        fig = fig
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(y(mm) - 5.))
                    .w(px(bar_x - 8.))
                    .text_size(px(9.))
                    .text_color(p.faint)
                    .text_align(TextAlign::Right)
                    .child(s),
            )
            .child(
                div()
                    .absolute()
                    .left(px(bar_x - 4.))
                    .top(px(y(mm)))
                    .w(px(4.))
                    .h(px(1.))
                    .bg(p.hair),
            );
    }

    // デッドゾーン (薄い塗り。0 でもラベルは出すが帯は 1px 未満なら描かない)
    if pf.top_deadzone > 0.001 {
        fig = fig.child(
            div()
                .absolute()
                .left(px(bar_x))
                .top_0()
                .w(px(bar_w))
                .h(px(y(pf.top_deadzone).max(3.)))
                .rounded_t(px(12.))
                .bg(theme::alpha(p.fg, 0.10)),
        );
    }
    if pf.bottom_deadzone > 0.001 {
        let bh = y(pf.bottom_deadzone).max(3.);
        fig = fig.child(
            div()
                .absolute()
                .left(px(bar_x))
                .top(px(h - bh))
                .w(px(bar_w))
                .h(px(bh))
                .rounded_b(px(12.))
                .bg(theme::alpha(p.fg, 0.10)),
        );
    }

    // 現在の深さ (実機ライブ)
    if let Some(d) = live {
        let dd = motion::spring_with("live-depth", y(d), 40.);
        fig = fig.child(
            div()
                .absolute()
                .left(px(bar_x))
                .top_0()
                .w(px(bar_w))
                .h(px(dd))
                .rounded(px(12.))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(theme::alpha(p.accent, 0.15), 0.),
                    linear_color_stop(theme::alpha(p.accent, 0.65), 1.),
                )),
        );
    }

    // ---- マーク収集: (線の y [None=注釈], ラベル希望 y, 色, 文字) ----
    let mut marks: Vec<(Option<f32>, f32, Hsla, String)> = vec![];
    if rapid {
        let yf = y(pf.rt_first);
        marks.push((
            Some(yf),
            yf,
            p.warn,
            format!("初回 {}", mm_fmt(pf.rt_first)),
        ));
        // 変化量は位置ではないので、初回ラインの上下に矢印注釈として置く
        marks.push((
            None,
            yf - 17.,
            p.ok,
            format!("↑ -{} でOFF", mm_fmt(pf.rt_release)),
        ));
        marks.push((
            None,
            yf + 17.,
            p.accent,
            format!("↓ +{} で再ON", mm_fmt(pf.rt_press)),
        ));
    } else {
        let ya = y(pf.actuation);
        marks.push((
            Some(ya),
            ya,
            p.accent,
            format!("作動 {}", mm_fmt(pf.actuation)),
        ));
        if let Some(r) = pf.release {
            let yr = y(r);
            marks.push((Some(yr), yr, p.ok, format!("解放 {}", mm_fmt(r))));
        }
    }
    // デッドゾーン境界ラベル (帯の縁に合わせる)
    let dzt = y(pf.top_deadzone);
    let dzb = h - y(pf.bottom_deadzone);
    marks.push((
        None,
        dzt,
        p.sub,
        format!("上端 {}", mm_fmt(pf.top_deadzone)),
    ));
    marks.push((
        None,
        dzb,
        p.sub,
        format!("下端 {}", mm_fmt(pf.bottom_deadzone)),
    ));

    // ラベルの重なり解消: 希望 y でソート → 最小 14px 間隔に広げ → 図内に収める
    marks.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let gap = 14.;
    let (lo, hi) = (9., h - 9.);
    if let Some(m) = marks.first_mut() {
        m.1 = m.1.max(lo);
    }
    for i in 1..marks.len() {
        marks[i].1 = marks[i].1.max(marks[i - 1].1 + gap);
    }
    if let Some(last) = marks.last().map(|m| m.1) {
        let over = last - hi;
        if over > 0. {
            for m in &mut marks {
                m.1 -= over;
            }
        }
    }
    for i in 0..marks.len() {
        let prev = i.checked_sub(1).map(|j| marks[j].1);
        let m = &mut marks[i];
        m.1 = m.1.max(lo).min(hi);
        if let Some(pv) = prev {
            m.1 = m.1.max(pv + gap);
        }
    }

    // 描画: 位置ライン → 引き出し線 → ラベル
    for (line_y, label_y, c, text) in &marks {
        if let Some(ly) = line_y {
            fig = fig.child(
                div()
                    .absolute()
                    .left(px(bar_x - 4.))
                    .top(px(*ly - 0.5))
                    .w(px(bar_w + 8.))
                    .h(px(2.))
                    .bg(*c),
            );
            // ラベルがずれたときの引き出し線 (縦 1px + 横 4px)
            let diff = label_y - ly;
            if diff.abs() > 4. {
                fig = fig
                    .child(
                        div()
                            .absolute()
                            .left(px(bar_x + bar_w + 4.))
                            .top(px(ly.min(*label_y)))
                            .w(px(1.))
                            .h(px(diff.abs()))
                            .bg(*c),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(bar_x + bar_w + 4.))
                            .top(px(*label_y))
                            .w(px(4.))
                            .h(px(1.))
                            .bg(*c),
                    );
            }
        }
        fig = fig.child(
            div()
                .absolute()
                .left(px(label_x))
                .top(px(label_y - 6.))
                .text_size(px(10.5))
                .text_color(*c)
                .whitespace_nowrap()
                .child(text.clone()),
        );
    }

    // 凡例
    let mut legend = div().flex().gap_2().flex_wrap();
    let swatch = |c: Hsla, name: &str| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(div().w(px(10.)).h(px(2.)).bg(c))
            .child(
                div()
                    .text_size(px(9.))
                    .text_color(p.faint)
                    .whitespace_nowrap()
                    .child(name.to_string()),
            )
    };
    if rapid {
        legend = legend
            .child(swatch(p.warn, "初回のON位置"))
            .child(swatch(p.ok, "↑戻し量でOFF"))
            .child(swatch(p.accent, "↓押込量で再ON"));
    } else {
        legend = legend
            .child(swatch(p.accent, "作動 (ON)"))
            .child(swatch(p.ok, "解放 (OFF)"));
    }
    legend = legend.child(swatch(theme::alpha(p.fg, 0.35), "デッドゾーン"));

    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(fig)
        .child(legend)
        .when_some(live, |d, v| {
            d.child(ui::chip(format!("現在 {}mm", mm_fmt(v)), p.accent))
        })
}

/// 刻印と割り当て名が同じキーを指しているか (LShift / Shift、Backspace / BS などを同一視)。
fn same_key(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.to_lowercase().replace(' ', "");
        let s = match s.as_str() {
            "backspace" => "bs".to_string(),
            "fn" => "fn1".to_string(),
            "capslock" => "caps".to_string(),
            "delete" => "del".to_string(),
            _ => s,
        };
        s.strip_prefix('l')
            .filter(|r| ["shift", "ctrl", "alt", "win"].contains(r))
            .map(str::to_string)
            .unwrap_or(s)
    }
    norm(a) == norm(b)
}
