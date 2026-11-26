//! コマンドパレット (Ctrl+K): 画面・設定・操作・デバイスを名前で探して飛ぶ。

use crate::app::{tab_of_section, tabs_for, Modal, Route, Shell, Tab};
use crate::{theme, ui};
use devhub_core::*;
use gpui_kit::component::input::Input;
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone)]
pub enum PalAction {
    Route(Route),
    Device(DeviceId),
    Tab(Tab),
    Action(ActionSpec),
    DdToggle,
    StopLiveLighting,
    Undo,
    Redo,
    Theme(bool),
    ThemeFamily(&'static str),
}

#[derive(Clone)]
pub struct PalItem {
    pub label: String,
    pub sub: String,
    pub icon: &'static str,
    pub act: PalAction,
}

impl Shell {
    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popup = None;
        self.modal = Some(Modal::Palette);
        let e = self.input("palette", "", "画面・設定・操作を検索…", window, cx);
        e.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn palette_all(&self) -> Vec<PalItem> {
        let mut v = vec![];
        let it = |label: String, sub: &str, icon: &'static str, act: PalAction| PalItem {
            label,
            sub: sub.into(),
            icon,
            act,
        };
        v.push(it(
            "ホーム".into(),
            "画面",
            "layout-grid",
            PalAction::Route(Route::Home),
        ));
        v.push(it(
            "アプリ設定".into(),
            "画面",
            "settings",
            PalAction::Route(Route::Settings),
        ));
        if self.fx_on.is_some() {
            v.push(it(
                "ライブライティングを停止".into(),
                "ライブ・DirectDrive",
                "square",
                PalAction::StopLiveLighting,
            ));
        }
        for d in &self.hub.devices {
            v.push(it(
                d.name.clone(),
                "デバイスを開く",
                "usb",
                PalAction::Device(d.id.clone()),
            ));
        }
        if let Some(s) = self.cur() {
            if let Some(c) = &s.caps {
                let kb = c.keyboard.is_some();
                for t in tabs_for(c) {
                    v.push(it(t.label().into(), "タブ", t.icon(), PalAction::Tab(t)));
                }
                for sp in &c.settings {
                    let t = tab_of_section(&sp.section, kb);
                    v.push(it(
                        sp.label.clone(),
                        &format!("設定 · {}", t.label()),
                        "sliders-horizontal",
                        PalAction::Tab(t),
                    ));
                }
                for a in c.actions.iter().filter(|a| !a.id.ends_with("_finish")) {
                    v.push(it(
                        a.label.clone(),
                        "操作を実行",
                        "play",
                        PalAction::Action(a.clone()),
                    ));
                }
                // DirectDrive 対応デバイス: ライティングの実機プレビュー切替。
                if self.tab == Tab::Lighting
                    && self.fx_on.as_ref() != Some(&s.id)
                    && s.live_rgb.is_empty()
                    && c.lighting
                        .iter()
                        .any(|a| a.id == self.light_area && a.direct_drive)
                {
                    let on = s.dd_on;
                    v.push(it(
                        if on {
                            "実機プレビュー停止".into()
                        } else {
                            "実機でプレビュー".into()
                        },
                        "ライティング",
                        "monitor",
                        PalAction::DdToggle,
                    ));
                }
                if let Some(m) = &c.mouse {
                    for b in &m.buttons {
                        v.push(it(
                            format!("{} の割り当て", b.name),
                            "ボタン",
                            "mouse-pointer-click",
                            PalAction::Tab(Tab::Buttons),
                        ));
                    }
                }
            }
            v.push(it(
                "元に戻す (Ctrl+Z)".into(),
                "編集",
                "undo-2",
                PalAction::Undo,
            ));
            v.push(it(
                "やり直し (Ctrl+Y)".into(),
                "編集",
                "redo-2",
                PalAction::Redo,
            ));
        }
        v.push(it(
            "ダークテーマ".into(),
            "見た目",
            "moon",
            PalAction::Theme(false),
        ));
        v.push(it(
            "ライトテーマ".into(),
            "見た目",
            "sun",
            PalAction::Theme(true),
        ));
        for (id, name) in theme::THEME_FAMILIES {
            v.push(it(
                format!("{name} デザイン"),
                "見た目",
                "palette",
                PalAction::ThemeFamily(id),
            ));
        }
        v
    }

    fn palette_items(&self, q: &str) -> Vec<PalItem> {
        let q = q.trim().to_lowercase();
        let all = self.palette_all();
        if q.is_empty() {
            return all.into_iter().take(12).collect();
        }
        let mut scored: Vec<(usize, PalItem)> = all
            .into_iter()
            .filter_map(|i| {
                let l = i.label.to_lowercase();
                let s = i.sub.to_lowercase();
                if l.starts_with(&q) {
                    Some((0, i))
                } else if l.contains(&q) {
                    Some((1, i))
                } else if s.contains(&q) {
                    Some((2, i))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by_key(|(s, i)| (*s, i.label.chars().count()));
        scored.into_iter().map(|(_, i)| i).take(12).collect()
    }

    pub fn palette_pick(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let q = self.input_value("palette", cx).unwrap_or_default();
        let Some(item) = self.palette_items(&q).into_iter().nth(i) else {
            return;
        };
        self.modal = None;
        window.focus(&self.focus, cx);
        match item.act {
            PalAction::Route(r) => self.go(r, cx),
            PalAction::Device(id) => self.open_device(id),
            PalAction::Tab(t) => {
                self.tab = t;
                self.scroll.set_offset(point(px(0.), px(0.)));
            }
            PalAction::Action(a) => self.start_action(&a, window, cx),
            PalAction::DdToggle => {
                if let Some(id) = self.device_id() {
                    let cur = self.hub.session(&id).is_some_and(|s| s.dd_on);
                    self.hub.set_dd(&id, !cur);
                }
            }
            PalAction::StopLiveLighting => self.fx_stop(cx),
            PalAction::Undo => self.undo(cx),
            PalAction::Redo => self.redo(cx),
            PalAction::Theme(l) => self.set_light(l, cx),
            PalAction::ThemeFamily(id) => self.set_theme_family(id, cx),
        }
        cx.notify();
    }

    pub fn modal_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::pal();
        let input = self.input("palette", "", "画面・設定・操作を検索…", window, cx);
        let q = self.input_value("palette", cx).unwrap_or_default();
        let items = self.palette_items(&q);
        let mut list = div().flex().flex_col().gap(px(2.));
        if items.is_empty() {
            list = list.child(ui::faint("見つかりません").p(px(12.)));
        }
        for (i, it) in items.into_iter().enumerate() {
            list = list.child(
                div()
                    .id(SharedString::from(format!("pal-{i}")))
                    .h(px(38.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .when(i == 0, |d| d.bg(p.accent_soft))
                    .hover(|s| s.bg(p.ctrl_hover))
                    .child(ui::icon(it.icon, 15., p.sub))
                    .child(div().flex_1().text_size(px(13.)).child(it.label))
                    .child(ui::faint(it.sub))
                    .on_click(cx.listener(move |this, _, w, cx| this.palette_pick(i, w, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(Input::new(&input))
            .child(list)
            .child(ui::faint("Enter で一番上を実行 · Esc で閉じる"))
            .into_any_element()
    }
}
