//! CPU で RGBA ビットマップを描く (依存は fontdue のみ)。
//!
//! デザイン: アンチエイリアスの角丸キーキャップ (上→下のグラデーション)、
//! 下から満ちるアクセントの押し込みフィル、押下時の柔らかい発光。
//! マウスは上から見た形 (左右ボタンが押し込みで満ちる) + 横バー。
//! テーマ (グラス / ライト / 背景なし / ネオン)・倍率・表示キーは
//! `config::OverlayCfg` (Device HUB アプリで編集) と OBS プロパティの両方から来る。

use crate::config::{Arrange, FillMode, MouseStyle, OverlayCfg, Theme};
use crate::live::{KbLive, Live, MouseLive};
use std::collections::HashSet;

/// 互換用 (旧 API)。実際のサイズは `size()`。
pub const WIDTH: u32 = 660;

/// ソース種別 (キーボードのみ / マウスのみ / 両方)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Both,
    Kb,
    Mouse,
}

/// 互換用 (旧 API): 既定レイアウトでの高さ。
pub fn height(mode: Mode) -> u32 {
    size(&Live::default(), mode, &Vis::default()).1
}

pub type Rgba = [u8; 4];

pub struct Pix {
    pub w: u32,
    pub h: u32,
    pub buf: Vec<u8>,
}

/// 描画設定。OBS プロパティ (update) + アプリの設定ファイル (config) を合成したもの。
#[derive(Clone)]
pub struct Vis {
    /// true = すべての表示設定を OBS プロパティから取る
    /// (アプリの obs_overlay.json を見ない。OSS 配布で単体利用する人向け)。
    pub obs_all: bool,
    pub title: bool,
    pub labels: bool,
    /// 表示するキー (layout index — OBS プロパティ)。**空 = すべて表示**。
    pub show_kb: HashSet<usize>,
    /// 表示するボタン (names index — OBS プロパティ)。空 = すべて表示。
    pub show_btn: HashSet<usize>,
    pub accent: Rgba,
    /// OBS 側でアクセント色を明示したか (false ならアプリ設定の色)。
    pub accent_set: bool,
    pub theme: Theme,
    pub scale: f32,
    pub numbers: bool,
    pub mouse_style: MouseStyle,
    /// 表示するキー (`LayoutKey::id` — アプリ設定)。空 = すべて表示。
    pub show_key_ids: HashSet<u16>,
    /// 表示するボタン (表示名 — アプリ設定)。空 = すべて表示。
    pub show_btn_names: HashSet<String>,
    /// 非表示にするキー (`LayoutKey::id` — 旧設定ファイル互換)。
    pub hide_key_ids: HashSet<u16>,
    /// 非表示にするボタン (表示名 — 旧設定ファイル互換)。
    pub hide_btn_names: HashSet<String>,
    // ---- 拡張カスタマイズ (アプリ設定ファイル由来) ----
    /// キーの大きさ係数。
    pub key_size: f32,
    /// 角丸係数。
    pub radius: f32,
    /// 余白係数。
    pub pad: f32,
    /// 統合ソースの配置。
    pub arrange: Arrange,
    /// 背景パネル不透明度係数 (0–1)。
    pub panel_alpha: f32,
    /// 押し込み色 (None = アクセント)。
    pub pressed: Option<Rgba>,
    /// 押下時の発光係数 (0–2)。
    pub glow: f32,
    /// フィル方向。
    pub fill_mode: FillMode,
    /// 作動点ライン。
    pub trigger_line: bool,
    /// キー内の AP/FT/R テキスト。
    pub badges: bool,
    /// 数字を % で出す。
    pub num_pct: bool,
    /// カスタムタイトル (空 = デバイス名)。
    pub title_text: String,
    /// 影。
    pub shadow: bool,
}

impl Default for Vis {
    fn default() -> Self {
        Self {
            obs_all: false,
            title: true,
            labels: true,
            show_kb: HashSet::new(),
            show_btn: HashSet::new(),
            accent: ACCENTS[0].1,
            accent_set: false,
            theme: Theme::Glass,
            scale: 1.0,
            numbers: false,
            mouse_style: MouseStyle::Both,
            show_key_ids: HashSet::new(),
            show_btn_names: HashSet::new(),
            hide_key_ids: HashSet::new(),
            hide_btn_names: HashSet::new(),
            key_size: 1.0,
            radius: 1.0,
            pad: 1.0,
            arrange: Arrange::Side,
            panel_alpha: 1.0,
            pressed: None,
            glow: 1.0,
            fill_mode: FillMode::Bottom,
            trigger_line: true,
            badges: true,
            num_pct: false,
            title_text: String::new(),
            shadow: true,
        }
    }
}

impl Vis {
    /// アプリの設定ファイルを重ねる (タイトル/ラベルは両方 ON のときだけ、
    /// キーは「アプリの表示集合 ∩ OBS の表示集合 − 旧 hide」)。
    pub fn with_cfg(mut self, c: &OverlayCfg) -> Self {
        self.theme = c.theme;
        self.scale = c.scale.clamp(0.75, 3.0);
        self.numbers = c.numbers;
        self.mouse_style = c.mouse_style;
        self.title &= c.title;
        self.labels &= c.labels;
        if !self.accent_set {
            self.accent = [c.accent[0], c.accent[1], c.accent[2], 255];
        }
        self.show_key_ids.extend(c.show_keys.iter().copied());
        self.show_btn_names.extend(c.show_buttons.iter().cloned());
        self.hide_key_ids.extend(c.hide_keys.iter().copied());
        self.hide_btn_names.extend(c.hide_buttons.iter().cloned());
        self.key_size = c.key_size.clamp(0.7, 1.4);
        self.radius = c.radius.clamp(0., 2.);
        self.pad = c.pad.clamp(0.5, 2.);
        self.arrange = c.arrange;
        self.panel_alpha = (c.panel_alpha.min(100) as f32) / 100.;
        self.pressed = c.pressed.map(|c| [c[0], c[1], c[2], 255]);
        self.glow = (c.glow.min(200) as f32) / 100.;
        self.fill_mode = c.fill_mode;
        self.trigger_line = c.trigger_line;
        self.badges = c.badges;
        self.num_pct = c.num_pct;
        self.title_text = c.title_text.clone();
        self.shadow = c.shadow;
        self
    }

    /// 表示フィルタは AND: どちらかの集合が指定されていれば両方に入っている必要あり。
    fn key_hidden(&self, i: usize, id: u16) -> bool {
        self.hide_key_ids.contains(&id)
            || (!self.show_kb.is_empty() && !self.show_kb.contains(&i))
            || (!self.show_key_ids.is_empty() && !self.show_key_ids.contains(&id))
    }

    fn btn_hidden(&self, i: usize, name: &str) -> bool {
        self.hide_btn_names.contains(name)
            || (!self.show_btn.is_empty() && !self.show_btn.contains(&i))
            || (!self.show_btn_names.is_empty() && !self.show_btn_names.contains(name))
    }
}

/// アクセント色の選択肢 (名前, RGBA)。OBS プロパティの val は index 文字列。
pub const ACCENTS: &[(&str, Rgba)] = &[
    ("シアン", [60, 190, 255, 255]),
    ("オレンジ", [255, 150, 60, 255]),
    ("マゼンタ", [255, 90, 190, 255]),
    ("ライム", [120, 230, 120, 255]),
    ("バイオレット", [170, 120, 255, 255]),
];

/// テーマごとの配色。
struct Pal {
    panel: Option<Rgba>,
    panel_edge: Rgba,
    cap_top: Rgba,
    cap_bot: Rgba,
    cap_edge: Rgba,
    hilite: Rgba,
    text: Rgba,
    faint: Rgba,
    track: Rgba,
    /// ネオン: 押し込みフィルを強めに。
    neon: bool,
    shadow: bool,
}

fn pal(t: Theme, accent: Rgba) -> Pal {
    match t {
        Theme::Glass => Pal {
            panel: Some([14, 17, 25, 214]),
            panel_edge: [255, 255, 255, 26],
            cap_top: [50, 56, 72, 255],
            cap_bot: [33, 37, 49, 255],
            cap_edge: [255, 255, 255, 30],
            hilite: [255, 255, 255, 38],
            text: [238, 241, 248, 255],
            faint: [150, 157, 176, 255],
            track: [255, 255, 255, 20],
            neon: false,
            shadow: true,
        },
        Theme::Light => Pal {
            panel: Some([247, 248, 251, 236]),
            panel_edge: [0, 0, 0, 22],
            cap_top: [255, 255, 255, 255],
            cap_bot: [229, 233, 240, 255],
            cap_edge: [0, 0, 0, 34],
            hilite: [255, 255, 255, 200],
            text: [22, 26, 36, 255],
            faint: [104, 112, 130, 255],
            track: [0, 0, 0, 18],
            neon: false,
            shadow: true,
        },
        Theme::Clear => Pal {
            panel: None,
            panel_edge: [0, 0, 0, 0],
            cap_top: [30, 34, 46, 205],
            cap_bot: [18, 21, 30, 205],
            cap_edge: [255, 255, 255, 40],
            hilite: [255, 255, 255, 30],
            text: [245, 247, 252, 255],
            faint: [200, 205, 218, 240],
            track: [16, 18, 26, 190],
            neon: false,
            shadow: true,
        },
        Theme::Neon => Pal {
            panel: Some([5, 6, 11, 232]),
            panel_edge: with_a(accent, 60),
            cap_top: [12, 14, 22, 255],
            cap_bot: [8, 9, 15, 255],
            cap_edge: with_a(accent, 90),
            hilite: [0, 0, 0, 0],
            text: [240, 244, 255, 255],
            faint: with_a(mix(accent, [255, 255, 255, 255], 0.35), 210),
            track: with_a(accent, 26),
            neon: true,
            shadow: false,
        },
    }
}

fn with_a(c: Rgba, a: u8) -> Rgba {
    [c[0], c[1], c[2], a]
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0., 1.);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3])]
}

/// 角丸矩形の符号付き距離 (内側が負)。
fn sd_rrect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let (cx, cy) = (x + w / 2., y + h / 2.);
    let r = r.min(w / 2.).min(h / 2.).max(0.);
    let qx = (px - cx).abs() - (w / 2. - r);
    let qy = (py - cy).abs() - (h / 2. - r);
    let ox = qx.max(0.);
    let oy = qy.max(0.);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.) - r
}

impl Pix {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            buf: vec![0; (w * h * 4) as usize],
        }
    }

    /// straight alpha の RGBA に "over" 合成 (coverage 付き)。
    fn blend(&mut self, x: i32, y: i32, c: Rgba, cov: f32) {
        if x < 0 || y < 0 || x as u32 >= self.w || y as u32 >= self.h || cov <= 0. {
            return;
        }
        let a = (c[3] as f32 / 255.) * cov.min(1.);
        if a <= 0.001 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        let da = self.buf[i + 3] as f32 / 255.;
        let oa = a + da * (1. - a);
        if oa <= 0. {
            return;
        }
        for (k, &src) in c.iter().take(3).enumerate() {
            let s = src as f32;
            let d = self.buf[i + k] as f32;
            self.buf[i + k] = ((s * a + d * da * (1. - a)) / oa).round().clamp(0., 255.) as u8;
        }
        self.buf[i + 3] = (oa * 255.).round() as u8;
    }

    /// 形の塗り。`f(x, y)` が (色, coverage) を返す。bounds 内だけ走査。
    fn shade(&mut self, x: f32, y: f32, w: f32, h: f32, f: impl Fn(f32, f32) -> (Rgba, f32)) {
        let x0 = x.floor().max(0.) as i32;
        let y0 = y.floor().max(0.) as i32;
        let x1 = (x + w).ceil().min(self.w as f32) as i32;
        let y1 = (y + h).ceil().min(self.h as f32) as i32;
        for yy in y0..y1 {
            for xx in x0..x1 {
                let (c, cov) = f(xx as f32 + 0.5, yy as f32 + 0.5);
                self.blend(xx, yy, c, cov);
            }
        }
    }

    /// 角丸矩形 (AA)。
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        self.shade(x - 1., y - 1., w + 2., h + 2., |px, py| {
            (c, (0.5 - sd_rrect(px, py, x, y, w, h, r)).clamp(0., 1.))
        });
    }

    /// 上→下グラデーションの角丸矩形。
    #[allow(clippy::too_many_arguments)]
    fn rrect_grad(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, top: Rgba, bot: Rgba) {
        self.shade(x - 1., y - 1., w + 2., h + 2., |px, py| {
            let t = ((py - y) / h.max(1.)).clamp(0., 1.);
            (
                mix(top, bot, t),
                (0.5 - sd_rrect(px, py, x, y, w, h, r)).clamp(0., 1.),
            )
        });
    }

    /// 角丸の輪郭線 (AA、太さ t)。
    #[allow(clippy::too_many_arguments)]
    fn rrect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, c: Rgba) {
        self.shade(x - 1., y - 1., w + 2., h + 2., |px, py| {
            let d = sd_rrect(
                px,
                py,
                x + t / 2.,
                y + t / 2.,
                w - t,
                h - t,
                (r - t / 2.).max(0.),
            );
            (c, (t / 2. + 0.5 - d.abs()).clamp(0., 1.))
        });
    }

    /// 柔らかい発光 (外側にフェード)。
    #[allow(clippy::too_many_arguments)]
    fn glow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, spread: f32, c: Rgba) {
        self.shade(
            x - spread,
            y - spread,
            w + spread * 2.,
            h + spread * 2.,
            |px, py| {
                let d = sd_rrect(px, py, x, y, w, h, r);
                if d <= 0. {
                    return (c, 0.);
                }
                let k = (1. - d / spread).clamp(0., 1.);
                (c, k * k)
            },
        );
    }

    /// 角丸矩形の内側を、下から `frac` の高さだけ塗る (押し込みフィル)。
    #[allow(clippy::too_many_arguments)]
    fn fill_bottom(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        frac: f32,
        top: Rgba,
        bot: Rgba,
    ) {
        let fy = y + h * (1. - frac.clamp(0., 1.));
        self.shade(x - 1., fy - 1., w + 2., y + h - fy + 2., |px, py| {
            let cov = (0.5 - sd_rrect(px, py, x, y, w, h, r)).clamp(0., 1.)
                * (py - fy + 0.5).clamp(0., 1.);
            let t = ((py - fy) / (y + h - fy).max(1.)).clamp(0., 1.);
            (mix(top, bot, t), cov)
        });
    }

    /// 上から `frac` の高さだけ塗る (押し込みフィルの反転)。
    #[allow(clippy::too_many_arguments)]
    fn fill_top(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        frac: f32,
        top: Rgba,
        bot: Rgba,
    ) {
        let fy = y + h * frac.clamp(0., 1.);
        self.shade(x - 1., y - 1., w + 2., fy - y + 2., |px, py| {
            let cov = (0.5 - sd_rrect(px, py, x, y, w, h, r)).clamp(0., 1.)
                * (fy + 0.5 - py).clamp(0., 1.);
            let t = (1. - (py - y) / (fy - y).max(1.)).clamp(0., 1.);
            (mix(bot, top, t), cov)
        });
    }

    // ---------------- 文字

    fn text_w(&self, fonts: &Fonts, px: f32, s: &str) -> f32 {
        s.chars()
            .map(|c| {
                fonts
                    .pick(c)
                    .map(|f| f.metrics(c, px).advance_width)
                    .unwrap_or(0.)
            })
            .sum()
    }

    /// ベースライン y で左揃え。
    fn text(&mut self, fonts: &Fonts, x: f32, baseline: f32, px: f32, c: Rgba, s: &str) {
        let mut pen = x;
        for ch in s.chars() {
            let Some(f) = fonts.pick(ch) else { continue };
            let g = glyph(f, ch, px);
            let (m, bmp) = (&g.0, &g.1);
            if m.width > 0 {
                let gx0 = pen.round() as i32 + m.xmin;
                let gy0 = baseline.round() as i32 - m.height as i32 - m.ymin;
                for (gy, row) in bmp.chunks(m.width).enumerate() {
                    for (gx, cov) in row.iter().enumerate() {
                        if *cov > 0 {
                            self.blend(gx0 + gx as i32, gy0 + gy as i32, c, *cov as f32 / 255.);
                        }
                    }
                }
            }
            pen += m.advance_width;
        }
    }

    /// 中央揃え (cx, 縦中心 cy)。
    fn text_c(&mut self, fonts: &Fonts, cx: f32, cy: f32, px: f32, c: Rgba, s: &str) {
        let w = self.text_w(fonts, px, s);
        self.text(fonts, cx - w / 2., cy + px * 0.36, px, c, s);
    }

    /// 枠に収まるまで縮めて中央揃え。
    #[allow(clippy::too_many_arguments)]
    fn text_fit(&mut self, fonts: &Fonts, cx: f32, cy: f32, max_w: f32, px: f32, c: Rgba, s: &str) {
        let mut size = px;
        while size > 6. && self.text_w(fonts, size, s) > max_w {
            size -= 0.5;
        }
        self.text_c(fonts, cx, cy, size, c, s);
    }
}

// ---------------- フォント

type Glyph = std::sync::Arc<(fontdue::Metrics, Vec<u8>)>;
type GlyphCache = std::collections::HashMap<(usize, char, u32), Glyph>;

/// ラスタライズ済みグリフのキャッシュ (毎フレームのラスタライズを避ける)。
fn glyph(f: &fontdue::Font, ch: char, px: f32) -> Glyph {
    use std::collections::HashMap;
    static CACHE: std::sync::Mutex<Option<GlyphCache>> = std::sync::Mutex::new(None);
    let key = (f as *const _ as usize, ch, (px * 4.).round() as u32);
    let mut g = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(v) = map.get(&key) {
        return v.clone();
    }
    if map.len() > 4096 {
        map.clear();
    }
    let v = std::sync::Arc::new(f.rasterize(ch, (key.2 as f32) / 4.));
    map.insert(key, v.clone());
    v
}

/// 欧文 (同梱 Geist) + 和文 (システムフォント) の組。
struct Fonts<'a> {
    latin: Option<&'static fontdue::Font>,
    jp: Option<&'a fontdue::Font>,
}

impl Fonts<'_> {
    fn pick(&self, c: char) -> Option<&fontdue::Font> {
        if c.is_ascii() {
            if let Some(l) = self.latin {
                if c == ' ' || l.lookup_glyph_index(c) != 0 {
                    return Some(l);
                }
            }
        }
        self.jp.or(self.latin)
    }
}

pub(crate) const RATE_WIDTH: u32 = 360;
pub(crate) const RATE_HEIGHT: u32 = 72;

pub(crate) fn usb_rate_label(label: &str) -> Pix {
    let mut p = Pix::new(RATE_WIDTH, RATE_HEIGHT);
    p.rrect(2., 2., 356., 68., 18., [18, 25, 37, 220]);
    let fonts = Fonts { latin: latin(), jp: None };
    p.text_fit(&fonts, 180., 36., 324., 36., [190, 236, 255, 255], label);
    p
}

/// 欧文用に Device HUB と同じ Geist を同梱 (169KB)。
fn latin() -> Option<&'static fontdue::Font> {
    static F: std::sync::OnceLock<Option<fontdue::Font>> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        fontdue::Font::from_bytes(
            include_bytes!("../../device-hub/fonts/Geist.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .ok()
    })
    .as_ref()
}

/// 和文フォント (タイトル・ボタン名用)。
pub fn font() -> Option<fontdue::Font> {
    let mut paths: Vec<String> = [
        "YuGothB.ttc",
        "YuGothM.ttc",
        "meiryob.ttc",
        "meiryo.ttc",
        "msgothic.ttc",
        "segoeui.ttf",
    ]
    .iter()
    .map(|n| format!(r"C:\Windows\Fonts\{n}"))
    .collect();
    // Linux (開発時のプレビュー) 用。
    paths.push("/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc".into());
    paths.push("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into());
    paths.push("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
    for p in paths {
        if let Ok(b) = std::fs::read(&p) {
            if let Ok(f) = fontdue::Font::from_bytes(b, fontdue::FontSettings::default()) {
                return Some(f);
            }
        }
    }
    None
}

// ---------------- レイアウト計算

/// 全キーでは物理配列、絞り込み時は行の順番を残して空行・空列を詰める。
fn kb_slots(layout: &[devhub_core::LayoutKey], vis: &Vis) -> Vec<(usize, f32, f32)> {
    let mut keys: Vec<_> = layout
        .iter()
        .enumerate()
        .filter(|(i, k)| !vis.key_hidden(*i, k.id))
        .collect();
    if keys.len() == layout.len() {
        return keys.into_iter().map(|(i, k)| (i, k.x, k.y)).collect();
    }
    keys.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.1.x.total_cmp(&b.1.x)));
    let (mut row_y, mut x, mut y, mut height) = (None, 0., 0., 0.);
    keys.into_iter()
        .map(|(i, k)| {
            if row_y != Some(k.y) {
                y += height;
                x = 0.;
                height = 0.;
                row_y = Some(k.y);
            }
            let slot = (i, x, y);
            x += k.w;
            height = f32::max(height, k.h);
            slot
        })
        .collect()
}

const PAD: f32 = 9.;
const TITLE_H: f32 = 24.;
const UNIT: f32 = 50.;
const MOUSE_W: f32 = 84.;
const MOUSE_H: f32 = 112.;
const MOUSE_GAP: f32 = 12.;
const MOUSE_LABEL_W: f32 = 76.;
const BAR_W: f32 = 130.;
const ROW_H: f32 = 42.;

fn mouse_rows(m: Option<&MouseLive>, vis: &Vis) -> usize {
    match m {
        Some(m) => (0..m.depths.len().max(m.names.len()))
            .filter(|i| !vis.btn_hidden(*i, m.names.get(*i).map(String::as_str).unwrap_or("")))
            .count(),
        None => 2,
    }
}

/// テーマの配色 + `Vis` のパネル不透明度・影を適用する。
fn pal_of(vis: &Vis) -> Pal {
    let mut p = pal(vis.theme, vis.accent);
    p.panel = p
        .panel
        .map(|c| with_a(c, (c[3] as f32 * vis.panel_alpha) as u8));
    p.shadow &= vis.shadow;
    p
}

/// キーボードブロックの大きさ (倍率 1)。
fn kb_size(live: &Live, vis: &Vis) -> (f32, f32) {
    let fb;
    let layout = match &live.kb {
        Some(k) => &k.layout,
        None => {
            fb = devhub_core::registry::su66max_layout();
            &fb
        }
    };
    let (w, h) = kb_slots(layout, vis)
        .into_iter()
        .fold((UNIT, UNIT), |(w, h), (i, x, y)| {
            (
                w.max((x + layout[i].w) * UNIT),
                h.max((y + layout[i].h) * UNIT),
            )
        });
    (w * vis.key_size, h * vis.key_size)
}

/// マウスブロックの大きさ (倍率 1)。
fn mouse_size(live: &Live, vis: &Vis) -> (f32, f32) {
    let rows = mouse_rows(live.mouse.as_ref(), vis) as f32;
    let bars_h = rows * ROW_H;
    let bars_w = BAR_W + 38. + if vis.labels { MOUSE_LABEL_W } else { 0. };
    let (w, h) = match vis.mouse_style {
        MouseStyle::Shape => (MOUSE_W, MOUSE_H),
        MouseStyle::Bars => (bars_w, bars_h.max(ROW_H)),
        MouseStyle::Both => (MOUSE_W + MOUSE_GAP + bars_w, MOUSE_H.max(bars_h)),
    };
    (w, h)
}

/// キャンバスの大きさ (px、倍率込み)。
pub fn size(live: &Live, mode: Mode, vis: &Vis) -> (u32, u32) {
    let t = if vis.title { TITLE_H } else { 0. };
    let pad = PAD * vis.pad;
    let (w, h) = match mode {
        Mode::Kb => {
            let (w, h) = kb_size(live, vis);
            (w + pad * 2., h + t + pad * 2.)
        }
        Mode::Mouse => {
            let (w, h) = mouse_size(live, vis);
            (w + pad * 2., h + t + pad * 2.)
        }
        Mode::Both => {
            let (kw, kh) = kb_size(live, vis);
            let (mw, mh) = mouse_size(live, vis);
            match vis.arrange {
                Arrange::Stack => (kw.max(mw) + pad * 2., kh + 28. + mh + t + pad * 2.),
                _ => (kw + 28. + mw + pad * 2., kh.max(mh) + t + pad * 2.),
            }
        }
    };
    let s = vis.scale.clamp(0.75, 3.0);
    (
        ((w * s).ceil() as u32).max(64),
        ((h * s).ceil() as u32).max(48),
    )
}

// ---------------- 描画

struct Ctx<'a> {
    fonts: Fonts<'a>,
    pal: Pal,
    vis: &'a Vis,
    s: f32,
}

/// `idle` = 押していない状態で全キーを描く (静的な下地用)。
/// それ以外は「動きのあるキー」だけを上から描き足す。
fn draw_kb(p: &mut Pix, c: &Ctx, kb: &KbLive, ox: f32, oy: f32, idle: bool) {
    let s = c.s;
    let acc = c.vis.accent;
    let pacc = c.vis.pressed.unwrap_or(acc);
    let u = UNIT * s * c.vis.key_size;
    let gap = 3.4 * s;
    let r = 7. * s * c.vis.radius;
    for (i, col, row) in kb_slots(&kb.layout, c.vis) {
        let k = &kb.layout[i];
        let smp = if idle {
            Default::default()
        } else {
            kb.samples.get(i).copied().unwrap_or_default()
        };
        if !idle && smp.status == 0 && smp.route / kb.travel.max(0.1) < 0.004 {
            continue;
        }
        let x = ox + col * u + gap / 2.;
        let y0k = oy + row * u + gap / 2.;
        let w = k.w * u - gap;
        let h = k.h * u - gap;
        let d = (smp.route / kb.travel.max(0.1)).clamp(0., 1.);
        let pressed = smp.status != 0;
        // 押下中は外側に発光
        if c.vis.glow > 0. && (pressed || d > 0.02) {
            let k_ = if pressed { 1. } else { d };
            let a = (if c.pal.neon { 170. } else { 110. }) * c.vis.glow;
            p.glow(x, y0k, w, h, r, 10. * s, with_a(pacc, (a * k_) as u8));
        }
        // 押下中は少し沈む
        let y = y0k + if pressed { 1.2 * s } else { 0. };
        if c.pal.shadow {
            p.rrect(x, y0k + 1.6 * s, w, h, r, [0, 0, 0, 64]);
        }
        p.rrect_grad(x, y, w, h, r, c.pal.cap_top, c.pal.cap_bot);
        // 押し込みフィル
        if d > 0. {
            let a = if c.pal.neon { 225. } else { 120. + 125. * d } as u8;
            let top = with_a(mix(pacc, [255, 255, 255, 255], 0.28), a);
            let bot = with_a(pacc, a);
            match c.vis.fill_mode {
                FillMode::Bottom => p.fill_bottom(
                    x + 1.,
                    y + 1.,
                    w - 2.,
                    h - 2.,
                    (r - 1.).max(0.),
                    d,
                    top,
                    bot,
                ),
                FillMode::Top => p.fill_top(
                    x + 1.,
                    y + 1.,
                    w - 2.,
                    h - 2.,
                    (r - 1.).max(0.),
                    d,
                    top,
                    bot,
                ),
                FillMode::Fade => p.rrect(
                    x + 1.,
                    y + 1.,
                    w - 2.,
                    h - 2.,
                    (r - 1.).max(0.),
                    with_a(pacc, ((a as f32) * d) as u8),
                ),
            }
        }
        // 作動点ライン (マウスの trigger 線と同じ見た目。この深さで ON になる)
        let perf = kb.perf.get(i);
        let rapid = perf.is_some_and(|p| p.mode == devhub_core::TriggerMode::Rapid);
        let trigger = perf
            .map(|p| if rapid { p.rt_first } else { p.actuation })
            .or_else(|| kb.actuation.get(i).copied());
        if c.vis.trigger_line {
            if let Some(a) = trigger {
                let t = (a / kb.travel.max(0.1)).clamp(0., 1.);
                if (0.01..1.).contains(&t) {
                    let ly = y + h - 1. - (h - 2.) * t;
                    p.rrect(
                        x + 2.5 * s,
                        ly - 0.6 * s,
                        w - 5. * s,
                        1.2 * s,
                        0.6 * s,
                        with_a(c.pal.text, 165),
                    );
                }
            }
        }
        if c.pal.hilite[3] > 0 {
            p.rrect(
                x + r,
                y + 1. * s,
                (w - 2. * r).max(1.),
                1. * s,
                0.,
                c.pal.hilite,
            );
        }
        if pressed {
            p.rrect_stroke(x, y, w, h, r, 1.8 * s, pacc);
        } else {
            p.rrect_stroke(x, y, w, h, r, 1. * s, c.pal.cap_edge);
        }
        let cx = x + w / 2.;
        let has_setting = c.vis.badges && trigger.is_some();
        let fit = w - 5. * s;
        if c.vis.labels {
            let label_y = if has_setting { 0.24 } else { 0.44 };
            let label = if has_setting && c.vis.numbers && d > 0.01 {
                if c.vis.num_pct {
                    format!("{} {:.0}%", k.label, d * 100.)
                } else {
                    format!("{} {:.1}", k.label, smp.route)
                }
            } else {
                k.label.clone()
            };
            p.text_fit(
                &c.fonts,
                cx,
                y + h * label_y,
                fit,
                12. * s,
                if pressed { c.pal.text } else { c.pal.faint },
                &label,
            );
        }
        if c.vis.badges {
            if let Some(a) = trigger {
                let first = format!("AP {a:.2}");
                p.text_fit(&c.fonts, cx, y + h * 0.52, fit, 9.5 * s, c.pal.text, &first);
                if let Some(v) = perf {
                    let next = if rapid {
                        format!("AP{:.2} R{:.2}", v.rt_press, v.rt_release)
                    } else {
                        format!("R {:.2}", v.release.unwrap_or(v.actuation))
                    };
                    p.text_fit(&c.fonts, cx, y + h * 0.77, fit, 9. * s, c.pal.text, &next);
                }
            } else if c.vis.numbers && d > 0.01 {
                let n = if c.vis.num_pct {
                    format!("{:.0}%", d * 100.)
                } else {
                    format!("{:.1} mm", smp.route)
                };
                p.text_fit(&c.fonts, cx, y + h * 0.77, fit, 9.5 * s, c.pal.text, &n);
            }
        }
    }
}

/// 左右ボタンのインデックス (名前から推定。分からなければ 0/1)。
fn lr_index(m: &MouseLive) -> (Option<usize>, Option<usize>) {
    let find = |keys: &[&str]| {
        m.names.iter().position(|n| {
            let l = n.to_lowercase();
            keys.iter().any(|k| l.contains(k))
        })
    };
    let n = m.depths.len();
    let l = find(&["左", "left"]).or((n > 0).then_some(0));
    let r = find(&["右", "right"]).or((n > 1).then_some(1));
    (l, r)
}

fn draw_mouse_shape(p: &mut Pix, c: &Ctx, m: Option<&MouseLive>, ox: f32, oy: f32) {
    let s = c.s;
    let acc = c.vis.pressed.unwrap_or(c.vis.accent);
    let gl = c.vis.glow;
    let (w, h) = (MOUSE_W * s, MOUSE_H * s);
    let r = w / 2.;
    let split = oy + h * 0.42;
    let mid = ox + w / 2.;
    if c.pal.shadow {
        p.rrect(ox, oy + 2.5 * s, w, h, r, [0, 0, 0, 70]);
    }
    p.rrect_grad(ox, oy, w, h, r, c.pal.cap_top, c.pal.cap_bot);
    let (li, ri) = m.map(lr_index).unwrap_or((None, None));
    // 「形だけ」モードではバーの情報 (作動点線・名前・深さ・AP/RT) をボタンの内側に描く。
    let detail = c.vis.mouse_style == MouseStyle::Shape;
    for (side, idx) in [(0, li), (1, ri)] {
        let (b, name, hidden) = match (m, idx) {
            (Some(m), Some(i)) => (
                m.depths.get(i).copied(),
                m.names.get(i).cloned().unwrap_or_else(|| format!("B{i}")),
                c.vis.btn_hidden(i, m.names.get(i).map(String::as_str).unwrap_or("")),
            ),
            _ => (None, String::new(), false),
        };
        if hidden {
            continue;
        }
        let d = b.map(|b| b.depth.clamp(0., 1.)).unwrap_or(0.);
        let pressed = b.and_then(|b| b.pressed).unwrap_or(false);
        let (bx0, bx1) = if side == 0 {
            (ox, mid - 1.5 * s)
        } else {
            (mid + 1.5 * s, ox + w)
        };
        if pressed && gl > 0. {
            p.shade(
                bx0 - 10. * s,
                oy - 10. * s,
                bx1 - bx0 + 20. * s,
                split - oy + 10. * s,
                |px, py| {
                    let dd = sd_rrect(px, py, ox, oy, w, h, r);
                    let inside_x = if side == 0 { px < mid } else { px >= mid };
                    if dd <= 0. || !inside_x || py > split {
                        return (acc, 0.);
                    }
                    let k = (1. - dd / (10. * s)).clamp(0., 1.);
                    (with_a(acc, (150. * gl) as u8), k * k)
                },
            );
        }
        if d > 0. {
            let fy = split - (split - oy) * d;
            let a = if c.pal.neon { 225. } else { 130. + 110. * d } as u8;
            let top = with_a(mix(acc, [255, 255, 255, 255], 0.28), a);
            let bot = with_a(acc, a);
            p.shade(bx0, oy, bx1 - bx0, split - oy, |px, py| {
                let body = (0.5 - sd_rrect(px, py, ox + 1., oy + 1., w - 2., h - 2., r - 1.))
                    .clamp(0., 1.);
                let fill =
                    (py - fy + 0.5).clamp(0., 1.) * (split - 1. * s - py + 0.5).clamp(0., 1.);
                let t = ((py - fy) / (split - fy).max(1.)).clamp(0., 1.);
                (mix(top, bot, t), body * fill)
            });
        }
        if pressed {
            p.shade(bx0, oy, bx1 - bx0, split - oy, |px, py| {
                let dd = sd_rrect(px, py, ox, oy, w, h, r);
                (
                    acc,
                    (1.6 * s - dd.abs()).clamp(0., 1.) * (split - py).clamp(0., 1.),
                )
            });
        }
        if detail {
            let bw = bx1 - bx0;
            let bcx = (bx0 + bx1) / 2.;
            let bh = split - oy;
            if c.vis.trigger_line {
                if let Some(t) = b.and_then(|b| b.trigger) {
                    let t = t.clamp(0., 1.);
                    if t > 0.01 {
                        let ly = split - bh * t;
                        p.rrect(
                            bx0 + 4. * s,
                            ly - 0.6 * s,
                            bw - 8. * s,
                            1.2 * s,
                            0.6 * s,
                            with_a(c.pal.text, 165),
                        );
                    }
                }
            }
            if c.vis.labels && !name.is_empty() {
                p.text_fit(
                    &c.fonts,
                    bcx,
                    oy + bh * 0.16,
                    bw - 6. * s,
                    8.5 * s,
                    c.pal.faint,
                    &name,
                );
            }
            if c.vis.numbers {
                p.text_fit(
                    &c.fonts,
                    bcx,
                    oy + bh * 0.44,
                    bw - 6. * s,
                    11. * s,
                    c.pal.text,
                    &format!("{:.0}%", d * 100.),
                );
            }
            if c.vis.badges {
                let fast = idx.and_then(|i| {
                    m.and_then(|m| m.fast.get(i).copied().flatten())
                });
                if b.is_some_and(|b| b.trigger.is_some()) {
                    let trig = b
                        .and_then(|b| b.trigger)
                        .map_or("—".into(), |t| format!("{:.0}", t * 100.));
                    p.text_fit(
                        &c.fonts,
                        bcx,
                        oy + bh * 0.70,
                        bw - 6. * s,
                        7.5 * s,
                        c.pal.text,
                        &format!("AP {trig}"),
                    );
                }
                if fast.is_some() {
                    let fast_s = match fast {
                        Some(0) => "OFF".into(),
                        Some(v) => v.to_string(),
                        None => "—".into(),
                    };
                    p.text_fit(
                        &c.fonts,
                        bcx,
                        oy + bh * 0.87,
                        bw - 6. * s,
                        7.5 * s,
                        c.pal.text,
                        &format!("RT {fast_s}"),
                    );
                }
            }
        }
    }
    // 左右の境目・ボタンと体の境目
    p.rrect(
        mid - 0.75 * s,
        oy + 4. * s,
        1.5 * s,
        split - oy - 4. * s,
        0.,
        c.pal.cap_edge,
    );
    p.rrect(
        ox + 8. * s,
        split - 0.75 * s,
        w - 16. * s,
        1.5 * s,
        0.,
        c.pal.cap_edge,
    );
    // ホイール
    let (ww, wh) = (12. * s, 30. * s);
    p.rrect(mid - ww / 2., oy + 22. * s, ww, wh, ww / 2., c.pal.cap_bot);
    p.rrect_stroke(
        mid - ww / 2.,
        oy + 22. * s,
        ww,
        wh,
        ww / 2.,
        1. * s,
        c.pal.cap_edge,
    );
    p.rrect_stroke(ox, oy, w, h, r, 1.2 * s, c.pal.cap_edge);
}

fn draw_mouse_bars(p: &mut Pix, c: &Ctx, m: &MouseLive, ox: f32, oy: f32) {
    let s = c.s;
    let acc = c.vis.pressed.unwrap_or(c.vis.accent);
    let label_w = MOUSE_LABEL_W * s;
    let tw = BAR_W * s;
    let th = 10. * s;
    let mut y = oy;
    for i in 0..m.depths.len().max(m.names.len()) {
        let name = m.names.get(i).cloned().unwrap_or_else(|| format!("B{i}"));
        if c.vis.btn_hidden(i, &name) {
            continue;
        }
        let b = m.depths.get(i).copied().unwrap_or_default();
        let d = b.depth.clamp(0., 1.);
        let pressed = b.pressed == Some(true);
        let cy = y + 12. * s;
        if c.vis.labels {
            let col = if pressed { c.pal.text } else { c.pal.faint };
            p.text(&c.fonts, ox, cy + 4.5 * s, 12.5 * s, col, &name);
        }
        let tx = ox + if c.vis.labels { label_w } else { 0. };
        let ty = cy - th / 2.;
        if pressed && c.vis.glow > 0. {
            p.glow(
                tx,
                ty,
                tw,
                th,
                th / 2.,
                8. * s,
                with_a(acc, (120. * c.vis.glow) as u8),
            );
        }
        p.rrect(tx, ty, tw, th, th / 2., c.pal.track);
        if d > 0. {
            let fw = (tw * d).max(th);
            p.shade(tx - 1., ty - 1., fw + 2., th + 2., |px, py| {
                let t = ((px - tx) / tw).clamp(0., 1.);
                (
                    mix(
                        with_a(acc, 170),
                        with_a(mix(acc, [255, 255, 255, 255], 0.25), 255),
                        t,
                    ),
                    (0.5 - sd_rrect(px, py, tx, ty, fw, th, th / 2.)).clamp(0., 1.),
                )
            });
        }
        if c.vis.trigger_line {
            if let Some(t) = b.trigger {
                let x = tx + tw * t.clamp(0., 1.);
                p.rrect(
                    x - 1. * s,
                    ty - 4. * s,
                    2. * s,
                    th + 8. * s,
                    1. * s,
                    with_a(c.pal.text, 200),
                );
            }
        }
        p.text(
            &c.fonts,
            tx + tw + 10. * s,
            cy + 4.5 * s,
            12. * s,
            if pressed { acc } else { c.pal.faint },
            &format!("{:.0}%", d * 100.),
        );
        if b.trigger.is_some() || m.fast.get(i).is_some_and(Option::is_some) {
            let trig = b
                .trigger
                .map_or("—".into(), |t| format!("{:.0}%", t * 100.));
            let fast = match m.fast.get(i).copied().flatten() {
                Some(0) => "OFF".into(),
                Some(v) => v.to_string(),
                None => "—".into(),
            };
            p.text(
                &c.fonts,
                tx,
                cy + 19. * s,
                10. * s,
                c.pal.faint,
                &format!("AP {trig}  ·  RT {fast}"),
            );
        }
        y += ROW_H * s;
    }
}

fn draw_mouse(p: &mut Pix, c: &Ctx, live: &Live, ox: f32, oy: f32, block_h: f32) {
    let m = live.mouse.as_ref();
    let s = c.s;
    let bars_h = mouse_rows(m, c.vis) as f32 * ROW_H * s;
    let shape_y = oy + (block_h - MOUSE_H * s).max(0.) / 2.;
    let bars_y = oy + (block_h - bars_h).max(0.) / 2.;
    match c.vis.mouse_style {
        MouseStyle::Shape => draw_mouse_shape(p, c, m, ox, shape_y),
        MouseStyle::Bars => {
            if let Some(m) = m {
                draw_mouse_bars(p, c, m, ox, bars_y);
            }
        }
        MouseStyle::Both => {
            draw_mouse_shape(p, c, m, ox, shape_y);
            if let Some(m) = m {
                draw_mouse_bars(p, c, m, ox + (MOUSE_W + MOUSE_GAP) * s, bars_y);
            }
        }
    }
}

/// 下地のキャッシュキー (これが同じなら静的な部分は同じ絵)。
fn base_key(live: &Live, mode: Mode, vis: &Vis, w: u32, h: u32) -> String {
    let mut sk: Vec<_> = vis.show_kb.iter().collect();
    sk.sort();
    let mut si: Vec<_> = vis.show_key_ids.iter().collect();
    si.sort();
    let mut sb: Vec<_> = vis.show_btn.iter().collect();
    sb.sort();
    let mut sn: Vec<_> = vis.show_btn_names.iter().collect();
    sn.sort();
    let mut hi: Vec<_> = vis.hide_key_ids.iter().collect();
    hi.sort();
    let mut hn: Vec<_> = vis.hide_btn_names.iter().collect();
    hn.sort();
    format!(
        "{mode:?}|{w}x{h}|{:?}|{}|{}{}{}|{:?}|{:?}|{sk:?}{si:?}{sb:?}{sn:?}{hi:?}{hn:?}|{}|{}|{}|{}|{:.2}|{:.2}|{:.2}|{:?}|{:.2}|{:?}|{:?}|{:.2}|{}{}{}{}{}",
        vis.theme,
        vis.scale,
        vis.title,
        vis.labels,
        vis.numbers,
        vis.mouse_style,
        vis.accent,
        live.status,
        live.kb.as_ref().map(|k| format!("{}:{:?}:{:?}:{:?}:{}", k.name, k.layout, k.actuation, k.perf, k.travel)).unwrap_or_default(),
        live.mouse.as_ref().map(|m| format!("{}:{:?}", m.name, m.names)).unwrap_or_default(),
        live.kb.is_some() as u8 * 2 + live.mouse.is_some() as u8,
        vis.key_size,
        vis.radius,
        vis.pad,
        vis.arrange,
        vis.panel_alpha,
        vis.pressed,
        vis.fill_mode,
        vis.glow,
        vis.trigger_line,
        vis.badges,
        vis.num_pct,
        vis.title_text,
        vis.shadow,
    )
}

/// 最新フレームをビットマップに焼く。
/// 静的な部分 (パネル・タイトル・押していないキー) はキャッシュし、
/// 毎フレームは「動きのあるキー」とマウスだけを描き足す (1 フレーム ≲1ms)。
pub fn draw(live: &Live, font: Option<&fontdue::Font>, mode: Mode, vis: &Vis) -> Pix {
    use std::collections::HashMap;
    static BASES: std::sync::Mutex<Option<HashMap<String, std::sync::Arc<Vec<u8>>>>> =
        std::sync::Mutex::new(None);
    let (w, h) = size(live, mode, vis);
    let key = base_key(live, mode, vis, w, h);
    let cached = BASES
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(&key).cloned()));
    let base = match cached {
        Some(b) => b,
        None => {
            let b = std::sync::Arc::new(draw_layer(live, font, mode, vis, true).buf);
            if let Ok(mut g) = BASES.lock() {
                let m = g.get_or_insert_with(HashMap::new);
                if m.len() >= 8 {
                    m.clear();
                }
                m.insert(key, b.clone());
            }
            b
        }
    };
    let mut p = Pix {
        w,
        h,
        buf: (*base).clone(),
    };
    draw_dynamic(&mut p, live, font, mode, vis);
    p
}

/// 毎フレーム描く部分 (動きのあるキー + マウス)。
fn draw_dynamic(p: &mut Pix, live: &Live, font: Option<&fontdue::Font>, mode: Mode, vis: &Vis) {
    let has_kb = live.kb.is_some() && mode != Mode::Mouse;
    let has_mouse = live.mouse.is_some() && mode != Mode::Kb;
    if !has_kb && !has_mouse {
        return;
    }
    let s = vis.scale.clamp(0.75, 3.0);
    let c = Ctx {
        fonts: Fonts {
            latin: latin(),
            jp: font,
        },
        pal: pal_of(vis),
        vis,
        s,
    };
    let pad = PAD * s * vis.pad;
    let y = pad + if vis.title { TITLE_H * s } else { 0. };
    let body_h = p.h as f32 - y - pad;
    match mode {
        Mode::Kb => {
            if let Some(kb) = &live.kb {
                draw_kb(p, &c, kb, pad, y, false);
            }
        }
        Mode::Mouse => draw_mouse(p, &c, live, pad, y, body_h),
        Mode::Both => {
            let (kw, kh) = kb_size(live, vis);
            let (mw, _) = mouse_size(live, vis);
            match vis.arrange {
                Arrange::Side => {
                    if let Some(kb) = &live.kb {
                        draw_kb(p, &c, kb, pad, y + (body_h - kh * s).max(0.) / 2., false);
                    }
                    draw_mouse(p, &c, live, pad + (kw + 28.) * s, y, body_h);
                }
                Arrange::Stack => {
                    if let Some(kb) = &live.kb {
                        draw_kb(p, &c, kb, pad, y, false);
                    }
                    draw_mouse(
                        p,
                        &c,
                        live,
                        pad,
                        y + (kh + 28.) * s,
                        body_h - (kh + 28.) * s,
                    );
                }
                Arrange::Swap => {
                    draw_mouse(p, &c, live, pad, y, body_h);
                    if let Some(kb) = &live.kb {
                        draw_kb(
                            p,
                            &c,
                            kb,
                            pad + (mw + 28.) * s,
                            y + (body_h - kh * s).max(0.) / 2.,
                            false,
                        );
                    }
                }
            }
        }
    }
}

/// 静的な下地 (`base` = true) を描く。
fn draw_layer(live: &Live, font: Option<&fontdue::Font>, mode: Mode, vis: &Vis, base: bool) -> Pix {
    let _ = base;
    let (w, h) = size(live, mode, vis);
    let mut p = Pix::new(w, h);
    let s = vis.scale.clamp(0.75, 3.0);
    let c = Ctx {
        fonts: Fonts {
            latin: latin(),
            jp: font,
        },
        pal: pal_of(vis),
        vis,
        s,
    };
    let (wf, hf) = (w as f32, h as f32);
    let rad = 18. * s * vis.radius;
    if let Some(bg) = c.pal.panel {
        p.rrect(0., 0., wf, hf, rad, bg);
        p.rrect_stroke(0., 0., wf, hf, rad, 1. * s, c.pal.panel_edge);
    }
    let pad = PAD * s * vis.pad;
    let mut y = pad;
    let has_kb = live.kb.is_some() && mode != Mode::Mouse;
    let has_mouse = live.mouse.is_some() && mode != Mode::Kb;
    if !has_kb && !has_mouse {
        let msg = if live.status.is_empty() {
            match mode {
                Mode::Kb => "キーボードを探しています…",
                Mode::Mouse => "マウスを探しています…",
                Mode::Both => "デバイスを探しています…",
            }
            .to_string()
        } else {
            live.status.clone()
        };
        let (dx, dy) = (wf / 2. - 4. * s, hf / 2. - 24. * s);
        p.glow(
            dx,
            dy,
            8. * s,
            8. * s,
            4. * s,
            9. * s,
            with_a(vis.accent, 120),
        );
        p.rrect(dx, dy, 8. * s, 8. * s, 4. * s, vis.accent);
        p.text_c(
            &c.fonts,
            wf / 2.,
            hf / 2. + 2. * s,
            14. * s,
            c.pal.faint,
            &msg,
        );
        return p;
    }
    if vis.title {
        let mut names = vec![];
        if has_kb {
            names.push(live.kb.as_ref().map(|k| k.name.clone()).unwrap_or_default());
        }
        if has_mouse {
            names.push(
                live.mouse
                    .as_ref()
                    .map(|m| m.name.clone())
                    .unwrap_or_default(),
            );
        }
        p.rrect(pad, y + 6. * s, 7. * s, 7. * s, 3.5 * s, vis.accent);
        let title = if !vis.title_text.is_empty() {
            vis.title_text.clone()
        } else if has_kb
            && !has_mouse
            && live
                .kb
                .as_ref()
                .is_some_and(|k| !k.perf.is_empty() || !k.actuation.is_empty())
        {
            format!("{}  /  mm", names.join("  ·  "))
        } else {
            names.join("  ·  ")
        };
        let max_w = wf - 2. * pad - 14. * s;
        let title_w = p.text_w(&c.fonts, 13. * s, &title).min(max_w);
        p.text_fit(
            &c.fonts,
            pad + 14. * s + title_w / 2.,
            y + 10. * s,
            max_w,
            13. * s,
            c.pal.text,
            &title,
        );
        y += TITLE_H * s;
    }
    let body_h = hf - y - pad;
    // 下地: 押していない状態のキー (マウスは毎フレーム描くので入れない)。
    match mode {
        Mode::Kb => {
            if let Some(kb) = &live.kb {
                draw_kb(&mut p, &c, kb, pad, y, true);
            }
        }
        Mode::Mouse => {}
        Mode::Both => {
            let (_, kh) = kb_size(live, vis);
            let (mw, _) = mouse_size(live, vis);
            if let Some(kb) = &live.kb {
                let kx = match vis.arrange {
                    Arrange::Swap => pad + (mw + 28.) * s,
                    _ => pad,
                };
                let ky = match vis.arrange {
                    Arrange::Stack => y,
                    _ => y + (body_h - kh * s).max(0.) / 2.,
                };
                draw_kb(&mut p, &c, kb, kx, ky, true);
            }
        }
    }
    p
}

/// プレビュー用のデモデータ (Device HUB の編集画面・example で使う)。
/// `t` 秒の時点で WASD とスペース、マウスボタンを擬似的に押す。
pub fn demo_live(
    kb: Option<(String, Vec<devhub_core::model::LayoutKey>, f32)>,
    mouse: Option<(String, Vec<String>)>,
    t: f32,
) -> Live {
    use devhub_core::model::{AxisSample, ButtonDepth};
    let wave = |phase: f32| ((t * 2.2 + phase).sin() * 1.4 - 0.2).clamp(0., 1.);
    Live {
        kb: kb.map(|(name, layout, travel)| {
            let samples = layout
                .iter()
                .map(|k| {
                    let d = match k.label.as_str() {
                        "W" => wave(0.),
                        "A" => wave(1.6),
                        "S" => wave(3.1),
                        "D" => wave(4.7),
                        "Space" if k.x > 5. && k.x < 7. => wave(2.4),
                        _ => 0.,
                    };
                    AxisSample {
                        route: d * travel,
                        status: (d > 0.35) as u8,
                        ..Default::default()
                    }
                })
                .collect();
            KbLive {
                name,
                actuation: vec![],
                perf: vec![
                    devhub_core::KeyPerf {
                        mode: devhub_core::TriggerMode::Rapid,
                        rt_first: 0.4,
                        rt_press: 0.15,
                        rt_release: 0.20,
                        ..Default::default()
                    };
                    layout.len()
                ],
                layout,
                travel,
                samples,
            }
        }),
        mouse: mouse.map(|(name, names)| {
            let depths = (0..names.len())
                .map(|i| {
                    let d = wave(i as f32 * 2.3 + 0.8);
                    ButtonDepth {
                        depth: d,
                        trigger: Some(0.35),
                        pressed: Some(d > 0.35),
                    }
                })
                .collect();
            MouseLive {
                name,
                names,
                depths,
                fast: vec![Some(4), Some(0)],
            }
        }),
        status: String::new(),
        seq: (t * 1000.) as u64,
    }
}
