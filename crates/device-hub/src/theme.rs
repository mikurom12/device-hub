//! デザイントークン (Liquid Glass)。
//!
//! 色はすべてここから取る。明暗・アクセント・ガラスの有無はプロセス全体の
//! 設定 (atomics) で、`pal()` が今の配色を返す。

use gpui_kit::*;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);
static GLASS: AtomicBool = AtomicBool::new(true);
static MOTION: AtomicBool = AtomicBool::new(true);
static ACCENT: AtomicU8 = AtomicU8::new(0);
/// テーマ系統 (0 = Liquid Glass, 1 = Frost Graphite)。
static FAMILY: AtomicU8 = AtomicU8::new(0);

pub fn set_light(v: bool) {
    LIGHT.store(v, Ordering::Relaxed);
}
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}
pub fn set_glass(v: bool) {
    GLASS.store(v, Ordering::Relaxed);
}
pub fn glass_on() -> bool {
    GLASS.load(Ordering::Relaxed)
}
pub fn set_motion(v: bool) {
    MOTION.store(v, Ordering::Relaxed);
}
pub fn motion_on() -> bool {
    MOTION.load(Ordering::Relaxed)
}
pub fn set_accent(i: u8) {
    ACCENT.store(i.min(ACCENTS.len() as u8 - 1), Ordering::Relaxed);
}
pub fn accent_index() -> u8 {
    ACCENT.load(Ordering::Relaxed)
}

/// テーマ系統の選択肢: (id, 表示名)。
pub const THEME_FAMILIES: &[(&str, &str)] =
    &[("liquid", "Liquid Glass"), ("graphite", "Frost Graphite")];

pub fn set_theme_family_id(id: &str) {
    let i = THEME_FAMILIES.iter().position(|o| o.0 == id).unwrap_or(0);
    FAMILY.store(i as u8, Ordering::Relaxed);
}
pub fn theme_family_index() -> usize {
    (FAMILY.load(Ordering::Relaxed) as usize).min(THEME_FAMILIES.len() - 1)
}
pub fn theme_family_id() -> &'static str {
    THEME_FAMILIES[theme_family_index()].0
}
pub fn is_graphite() -> bool {
    theme_family_id() == "graphite"
}

/// アクセント候補 (名前, 色)。
pub const ACCENTS: &[(&str, u32)] = &[
    ("アイス", 0x4cb4ff),
    ("ミント", 0x34d6a8),
    ("ライラック", 0xa98bff),
    ("サンセット", 0xff8a5c),
    ("ローズ", 0xff5c8a),
    ("ゴールド", 0xf5c24c),
];

static FONT_SEL: AtomicU8 = AtomicU8::new(0);

/// フォント選択肢: (id, 表示名, 主ファミリ, 日本語フォールバック)。
/// "Geist"/"IBM Plex Sans JP"/"Zen Maru Gothic" は fonts/ に同梱して
/// `install_fonts` で登録する。残りはシステムフォント前提だが、
/// 未インストールでもフォールバックで日本語が豆腐にならない。
pub const FONT_OPTIONS: &[(&str, &str, &str, &[&str])] = &[
    (
        "modern",
        "モダン (Geist + IBM Plex Sans JP)",
        "Geist",
        &["IBM Plex Sans JP", "Yu Gothic UI"],
    ),
    (
        "rounded",
        "ラウンド (Geist + Zen Maru Gothic)",
        "Geist",
        &["Zen Maru Gothic", "IBM Plex Sans JP", "Yu Gothic UI"],
    ),
    (
        "plexjp",
        "IBM Plex Sans JP",
        "IBM Plex Sans JP",
        &["Yu Gothic UI"],
    ),
    (
        "zenmaru",
        "Zen Maru Gothic",
        "Zen Maru Gothic",
        &["IBM Plex Sans JP", "Yu Gothic UI"],
    ),
    (
        "mplus",
        "M PLUS Rounded 1c",
        "M PLUS Rounded 1c",
        &["IBM Plex Sans JP", "Yu Gothic UI"],
    ),
    (
        "bizudp",
        "BIZ UDPGothic",
        "BIZ UDPGothic",
        &["IBM Plex Sans JP", "Yu Gothic UI"],
    ),
    ("yugothic", "Yu Gothic UI", "Yu Gothic UI", &[]),
    ("meiryo", "Meiryo UI", "Meiryo UI", &["Yu Gothic UI"]),
];

/// 選択中のフォント (prefs.font の index)。範囲外は 0 (modern)。
pub fn set_font_id(id: &str) {
    let i = FONT_OPTIONS.iter().position(|o| o.0 == id).unwrap_or(0);
    FONT_SEL.store(i as u8, Ordering::Relaxed);
}
pub fn font_index() -> usize {
    (FONT_SEL.load(Ordering::Relaxed) as usize).min(FONT_OPTIONS.len() - 1)
}
pub fn font_id() -> &'static str {
    FONT_OPTIONS[font_index()].0
}

fn opt_font(i: usize) -> Font {
    let (_, _, family, fallbacks) = &FONT_OPTIONS[i.min(FONT_OPTIONS.len() - 1)];
    let mut f = font(*family);
    if !fallbacks.is_empty() {
        f.fallbacks = Some(FontFallbacks::from_fonts(
            fallbacks.iter().map(|s| s.to_string()).collect(),
        ));
    }
    f
}

/// UI 本文フォント (選択中のもの)。
pub fn ui_font() -> Font {
    opt_font(font_index())
}

/// 見出し・大きな数字用。tnum (表形式数字) を有効化する。
pub fn display_font() -> Font {
    let mut f = ui_font();
    f.features = FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]));
    f
}

/// Zen Maru を登録済みか (選択されるまで登録しない)。
static FONTS_ZEN: AtomicBool = AtomicBool::new(false);

/// 同梱フォントをテキストシステムへ登録。
/// Geist + IBM Plex Sans JP は常時 (ほぼ全選択肢の本文/フォールバック)。
/// Zen Maru Gothic (7.6MB) は rounded/zenmaru 選択時だけ登録する —
/// 使わないのに DirectWrite 側にも常駐させないため。
/// フォント選択の変更時にも呼ぶ (`set_font_id` の後)。
pub fn install_fonts(cx: &App) {
    use std::borrow::Cow;
    let mut raw: Vec<&'static [u8]> = Vec::new();
    if !FONTS_GEIST.swap(true, Ordering::Relaxed) {
        raw.extend([
            include_bytes!("../fonts/Geist.ttf").as_slice(),
            include_bytes!("../fonts/IBMPlexSansJP-Regular.ttf").as_slice(),
            include_bytes!("../fonts/IBMPlexSansJP-Bold.ttf").as_slice(),
        ]);
    }
    let need_zen = matches!(font_id(), "rounded" | "zenmaru");
    if need_zen && !FONTS_ZEN.swap(true, Ordering::Relaxed) {
        raw.extend([
            include_bytes!("../fonts/ZenMaruGothic-Regular.ttf").as_slice(),
            include_bytes!("../fonts/ZenMaruGothic-Bold.ttf").as_slice(),
        ]);
    }
    if raw.is_empty() {
        return;
    }
    let fonts: Vec<Cow<'static, [u8]>> = raw.into_iter().map(Cow::Borrowed).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("[devhub] フォントの登録に失敗: {e}");
    }
}

/// Geist/IBM Plex を登録済みか。
static FONTS_GEIST: AtomicBool = AtomicBool::new(false);

/// 等幅 (生データ・コード表示用のシステムフォント)。
pub const FONT_MONO: &str = "Cascadia Mono";

pub const SIDEBAR_W: f32 = 248.;
pub const R_CARD: f32 = 22.;
pub const R_CTRL: f32 = 12.;
pub const GAP: f32 = 14.;

/// 現在のテーマ系統でのカード角丸。
pub fn r_card() -> f32 {
    if is_graphite() {
        16.
    } else {
        R_CARD
    }
}
/// 現在のテーマ系統での操作部角丸。
pub fn r_ctrl() -> f32 {
    if is_graphite() {
        10.
    } else {
        R_CTRL
    }
}
/// ページの左右余白。
pub const PAGE_PAD: f32 = 10.;
/// ページ上部 (タイトルバーの下)。
pub const PAGE_TOP: f32 = 36.;
/// ページ下部の余白。
pub const PAGE_BOT: f32 = 48.;
/// ページ内のセクション間隔。
pub const PAGE_GAP: f32 = 18.;

/// 16 進 RGB → Hsla (alpha 付き)。
pub fn hex(v: u32, a: f32) -> Hsla {
    let mut c: Hsla = rgb(v).into();
    c.a = a;
    c
}

pub fn rgb_of(c: devhub_core::Rgb) -> Hsla {
    rgb(c.to_hex()).into()
}

/// 配色。
#[derive(Clone, Copy)]
pub struct Pal {
    pub light: bool,
    /// 本文。
    pub fg: Hsla,
    /// 補助テキスト。
    pub sub: Hsla,
    /// さらに弱いテキスト (プレースホルダ・単位)。
    pub faint: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    /// アクセント上の文字。
    pub on_accent: Hsla,
    pub danger: Hsla,
    pub ok: Hsla,
    pub warn: Hsla,
    /// ガラス面の塗り (上 → 下)。
    pub glass_top: Hsla,
    pub glass_bot: Hsla,
    /// ガラスを切ったときの不透明な面。
    pub solid: Hsla,
    /// 細い縁。
    pub edge: Hsla,
    /// 上端の光。
    pub rim: Hsla,
    /// 操作部の面 (カードの上に載るもの)。
    pub ctrl: Hsla,
    pub ctrl_hover: Hsla,
    /// トラック (スライダー・トグルのオフ)。
    pub track: Hsla,
    /// 区切り線。
    pub hair: Hsla,
    /// モーダルの背後。
    pub scrim: Hsla,
    /// 影。
    pub shadow: Hsla,
    /// キーキャップ。
    pub key: Hsla,
    pub key_edge: Hsla,
    /// ガラス内側のごく薄い縁 (Frost Graphite)。
    pub inner: Hsla,
    /// 値に比例して埋める中立の塗り。
    pub fill: Hsla,
    /// トグル/スライダーのつまみ。
    pub knob: Hsla,
    /// セグメント選択の浮きチップ。
    pub chip_bg: Hsla,
    pub chip_shadow: Hsla,
    /// サイドバー選択行。
    pub sel: Hsla,
    pub sel_shadow: Hsla,
    /// 反転色 (選択中の値と主ボタンだけ)。
    pub inv: Hsla,
    pub inv_fg: Hsla,
    /// 反転面の中で値に比例して埋める塗り。
    pub inv_fill: Hsla,
    /// スライダーつまみの外側の薄いリング。
    pub thumb_ring: Hsla,
    /// 粒子の強さ (0..1)。0 なら描かない。
    pub grain: f32,
}

pub fn pal() -> Pal {
    if is_graphite() {
        return graphite_pal(is_light());
    }
    let light = is_light();
    let acc = ACCENTS[accent_index() as usize].1;
    let accent = hex(acc, 1.);
    if light {
        Pal {
            light,
            fg: hex(0x0d1320, 0.94),
            sub: hex(0x2a3348, 0.66),
            faint: hex(0x2a3348, 0.42),
            accent,
            accent_soft: hex(acc, 0.16),
            on_accent: hex(0xffffff, 1.),
            danger: hex(0xe5484d, 1.),
            ok: hex(0x1fae6d, 1.),
            warn: hex(0xd9901a, 1.),
            glass_top: hex(0xffffff, 0.62),
            glass_bot: hex(0xf4f7fc, 0.44),
            solid: hex(0xf7f9fc, 1.),
            edge: hex(0xffffff, 0.75),
            rim: hex(0xffffff, 0.95),
            ctrl: hex(0xffffff, 0.55),
            ctrl_hover: hex(0xffffff, 0.8),
            track: hex(0x7d8aa5, 0.22),
            hair: hex(0x1b2440, 0.08),
            scrim: hex(0x1b2235, 0.18),
            shadow: hex(0x1b2a4a, 0.16),
            key: hex(0xffffff, 0.7),
            key_edge: hex(0x8090b0, 0.28),
            inner: hex(0xffffff, 0.2),
            fill: hex(acc, 0.18),
            knob: hex(0xffffff, 1.),
            chip_bg: hex(0xffffff, 0.95),
            chip_shadow: hex(0x000000, 0.12),
            sel: hex(acc, 0.18),
            sel_shadow: hex(0x000000, 0.08),
            inv: accent,
            inv_fg: hex(0xffffff, 1.),
            inv_fill: hex(0xffffff, 0.18),
            thumb_ring: hex(0x000000, 0.05),
            grain: 0.,
        }
    } else {
        Pal {
            light,
            fg: hex(0xf3f6ff, 0.95),
            sub: hex(0xd8e0f5, 0.62),
            faint: hex(0xd8e0f5, 0.38),
            accent,
            accent_soft: hex(acc, 0.2),
            on_accent: hex(0x061018, 1.),
            danger: hex(0xff6369, 1.),
            ok: hex(0x3fe08f, 1.),
            warn: hex(0xffc04d, 1.),
            glass_top: hex(0x2a3550, 0.42),
            glass_bot: hex(0x121a2c, 0.34),
            solid: hex(0x111725, 1.),
            edge: hex(0xffffff, 0.12),
            rim: hex(0xffffff, 0.34),
            ctrl: hex(0xffffff, 0.07),
            ctrl_hover: hex(0xffffff, 0.12),
            track: hex(0xffffff, 0.12),
            hair: hex(0xffffff, 0.07),
            scrim: hex(0x02040a, 0.45),
            shadow: hex(0x000000, 0.42),
            key: hex(0xffffff, 0.08),
            key_edge: hex(0xffffff, 0.14),
            inner: hex(0xffffff, 0.04),
            fill: hex(acc, 0.2),
            knob: hex(0xffffff, 1.),
            chip_bg: hex(0xffffff, 0.16),
            chip_shadow: hex(0x000000, 0.3),
            sel: hex(acc, 0.16),
            sel_shadow: hex(0x000000, 0.2),
            inv: accent,
            inv_fg: hex(0x061018, 1.),
            inv_fill: hex(0x000000, 0.12),
            thumb_ring: hex(0xffffff, 0.08),
            grain: 0.,
        }
    }
}

/// Frost Graphite (docs/mockups/frost-graphite.html の TOKENS)。
/// `accent` は反転色 inv、`on_accent` は invFg に対応させる。
fn graphite_pal(light: bool) -> Pal {
    if light {
        let inv = hex(0x0b0b0c, 1.);
        Pal {
            light,
            fg: hex(0x0b0b0c, 1.),
            sub: hex(0x0b0b0c, 0.60),
            faint: hex(0x0b0b0c, 0.38),
            accent: inv,
            accent_soft: hex(0x0b0b0c, 0.07),
            on_accent: hex(0xffffff, 1.),
            danger: hex(0xc93636, 1.),
            ok: hex(0x0b0b0c, 0.82),
            warn: hex(0x0b0b0c, 0.55),
            glass_top: hex(0xffffff, 0.52),
            glass_bot: hex(0xffffff, 0.46),
            solid: hex(0xe9eaed, 1.),
            edge: hex(0x000000, 0.06),
            rim: hex(0xffffff, 0.95),
            ctrl: hex(0x000000, 0.035),
            ctrl_hover: hex(0x000000, 0.065),
            track: hex(0x000000, 0.13),
            hair: hex(0x000000, 0.075),
            scrim: hex(0x565a64, 0.22),
            shadow: hex(0x282a34, 0.22),
            key: hex(0xffffff, 0.72),
            key_edge: hex(0x000000, 0.1),
            inner: hex(0xffffff, 0.25),
            fill: hex(0x000000, 0.07),
            knob: hex(0xffffff, 1.),
            chip_bg: hex(0xffffff, 1.),
            chip_shadow: hex(0x000000, 0.10),
            sel: hex(0xffffff, 0.78),
            sel_shadow: hex(0x000000, 0.12),
            inv,
            inv_fg: hex(0xffffff, 1.),
            inv_fill: hex(0xffffff, 0.10),
            thumb_ring: hex(0x000000, 0.05),
            grain: 0.20,
        }
    } else {
        let inv = hex(0xf4f4f5, 1.);
        Pal {
            light,
            fg: hex(0xf4f4f5, 1.),
            sub: hex(0xf4f4f5, 0.62),
            faint: hex(0xf4f4f5, 0.36),
            accent: inv,
            accent_soft: hex(0xffffff, 0.08),
            on_accent: hex(0x0a0a0b, 1.),
            danger: hex(0xff7474, 1.),
            ok: hex(0xf4f4f5, 0.9),
            warn: hex(0xf4f4f5, 0.55),
            glass_top: hex(0x141416, 0.46),
            glass_bot: hex(0x141416, 0.40),
            solid: hex(0x09090a, 1.),
            edge: hex(0xffffff, 0.09),
            rim: hex(0xffffff, 0.16),
            ctrl: hex(0xffffff, 0.05),
            ctrl_hover: hex(0xffffff, 0.09),
            track: hex(0xffffff, 0.16),
            hair: hex(0xffffff, 0.08),
            scrim: hex(0x000000, 0.5),
            shadow: hex(0x000000, 0.55),
            key: hex(0xffffff, 0.07),
            key_edge: hex(0xffffff, 0.12),
            inner: hex(0xffffff, 0.02),
            fill: hex(0xffffff, 0.12),
            knob: hex(0xe9e9eb, 1.),
            chip_bg: hex(0xffffff, 0.14),
            chip_shadow: hex(0x000000, 0.35),
            sel: hex(0xffffff, 0.08),
            sel_shadow: hex(0xffffff, 0.08),
            inv,
            inv_fg: hex(0x0a0a0b, 1.),
            inv_fill: hex(0x000000, 0.08),
            thumb_ring: hex(0xffffff, 0.08),
            grain: 0.11,
        }
    }
}

/// Frost Graphite の霜チップの影 (TOKENS chipShadow)。
pub fn chip_shadows(p: Pal) -> Vec<BoxShadow> {
    if p.light {
        vec![
            BoxShadow {
                color: p.chip_shadow,
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            },
            BoxShadow {
                color: alpha(p.chip_shadow, 0.06),
                offset: point(px(0.), px(2.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            },
        ]
    } else {
        vec![
            BoxShadow {
                color: hex(0xffffff, 0.12),
                offset: point(px(0.), px(1.)),
                blur_radius: px(0.),
                spread_radius: px(0.),
                inset: true,
            },
            BoxShadow {
                color: p.chip_shadow,
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
                inset: false,
            },
        ]
    }
}


/// アクセントを少し明るく/暗くした派生色 (グラデーション用)。
pub fn accent_grad() -> Background {
    let p = pal();
    if is_graphite() {
        return linear_gradient(
            135.,
            linear_color_stop(p.inv, 0.),
            linear_color_stop(p.inv, 1.),
        );
    }
    let mut a2 = p.accent;
    a2.h = (a2.h + 0.06) % 1.;
    a2.l = (a2.l + 0.08).min(0.85);
    linear_gradient(
        135.,
        linear_color_stop(a2, 0.),
        linear_color_stop(p.accent, 1.),
    )
}

/// 2 色の線形補間。
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let a: Rgba = a.into();
    let b: Rgba = b.into();
    let t = t.clamp(0., 1.);
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
    .into()
}

pub fn alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}
