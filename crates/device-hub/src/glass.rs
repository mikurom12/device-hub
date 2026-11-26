//! Liquid Glass の面。
//!
//! vendor/gpui-pre に足した GPU backdrop-filter (`Window::paint_backdrop`) で、
//! **面の後ろに実際に描かれたもの** (壁紙・カード・文字) を毎フレーム GPU でぼかし・
//! 屈折させる (CSS の `backdrop-filter` + 縁のレンズ)。参照実装
//! `参照実装` と同じ描画順:
//!
//!   backdrop (ぼかし/屈折) → 面の塗り (上→下の勾配) → 上端の光 → 細い縁 → 下端の陰
//!
//! 使い方: 親 div を `.relative()` にして、最初の子に `glass::surface(Kind, r)` を置く。
//! ガラスを切ると (設定 / `DEVHUB_FAST=1`) 不透明な面にフォールバックする。

use crate::motion;
use crate::theme::{self, pal};
use gpui_kit::*;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 内容を載せるカード。強めの曇り + 縁だけ薄く屈折。
    Panel,
    /// 壁紙の上に直接浮く操作部 (タブバー・サイドバー・ツールバー)。鮮明なレンズ。
    /// Graphite ではカードの内側にあれば自動で Frost に落とす。
    Clear,
    /// カードの上に載る操作部 (ボタン・入力欄)。屈折なし、軽い曇り。
    Frost,
    /// ポップアップ・ダイアログ。最も強い曇り。
    Sheet,
}

/// 今フレームに描いた「入れ子の親になる面」。GPUI は親を子より先に描くので、
/// 子の操作部を描く時点では自分を囲むカードがここに載っている
/// (Valorant tracker の NESTED_IN / resolve と同じ考え方)。
static NEST_HOSTS: Mutex<Vec<[f32; 4]>> = Mutex::new(Vec::new());

/// 描画の先頭で呼び、入れ子判定をフレームごとに数え直す。
pub fn begin_frame() {
    NEST_HOSTS.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

fn note_host(b: Bounds<Pixels>) {
    let mut g = NEST_HOSTS.lock().unwrap_or_else(|p| p.into_inner());
    if g.len() < 512 {
        g.push([
            f32::from(b.origin.x),
            f32::from(b.origin.y),
            f32::from(b.size.width),
            f32::from(b.size.height),
        ]);
    }
}

/// 面の内側に置かれた `Clear` / `Panel` は、親のガラスを二度曲げないよう `Frost` に落とす。
fn resolve(kind: Kind, b: Bounds<Pixels>) -> Kind {
    if !matches!(kind, Kind::Clear | Kind::Panel) {
        return kind;
    }
    let (x, y, w, h) = (
        f32::from(b.origin.x),
        f32::from(b.origin.y),
        f32::from(b.size.width),
        f32::from(b.size.height),
    );
    let nested = NEST_HOSTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|r| {
            x >= r[0] - 0.5
                && y >= r[1] - 0.5
                && x + w <= r[0] + r[2] + 0.5
                && y + h <= r[1] + r[3] + 0.5
        });
    if nested {
        Kind::Frost
    } else {
        kind
    }
}

/// ガラスの面 (親いっぱいに広がる絶対配置の canvas)。
pub fn surface(kind: Kind, radius: f32) -> impl IntoElement {
    surface_keyed(kind, radius, None)
}

/// ホバーで照り返すガラス (`key` は motion のばねのキー)。
pub fn surface_hover(kind: Kind, radius: f32, key: impl Into<String>) -> impl IntoElement {
    surface_keyed(kind, radius, Some(key.into()))
}

fn surface_keyed(kind: Kind, radius: f32, key: Option<String>) -> impl IntoElement {
    canvas(
        |b, _, _| b,
        move |_, bounds, window, _| paint(kind, radius, key.as_deref(), bounds, window),
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Frost Graphite の光沢リムと凹んだ窓。Valorant tracker `rim_spec` の値。
/// (`rim` = 帯幅・帯の白さ・上辺の主光・下辺の補助光、`lens` = 中心倍率・凹み影・継目・光沢)
fn graphite_rim_spec(light: bool, small: f32) -> ([f32; 4], [f32; 4]) {
    let w = if small <= 72. { 1.6 } else { 2.4 };
    if light {
        ([w, 0.12, 0.8, 0.38], [1., 0.10, 0., 0.32])
    } else {
        ([w, 0.07, 0.55, 0.22], [1., 0.18, 0., 0.28])
    }
}

/// 屈折縁に乗せる画素ごとの反射光 (上端/右下/内側の光/下側の陰)。
fn graphite_rim_light(light: bool) -> [f32; 4] {
    if light {
        [0.90, 0.35, 0.26, 0.06]
    } else {
        [0.45, 0.10, 0.16, 0.30]
    }
}

/// Web/Valorant の `specOf` と同じ帯・変位。小さい操作部は縁を厚く、大きな面は外周だけ。
fn graphite_bezel_scale(w: f32, h: f32) -> (f32, f32) {
    let small = w.min(h);
    if small <= 72. {
        (small * 0.4, (small * 0.5).min(34.))
    } else {
        let b = (small * 0.07).clamp(14., 28.);
        (b, (b * 1.3).min(40.))
    }
}

/// Frost Graphite の素材。白黒の霜ガラスを保ちながら、外周だけ薄く屈折させる。
/// 分散はモノクロなので参考元の 0.06 より弱い 0.018 に抑える。
fn graphite_filter(
    kind: Kind,
    light: bool,
    small: f32,
    radius: f32,
    w: f32,
    h: f32,
    energy: f32,
) -> BackdropFilter {
    let (mut rim, mut lens) = graphite_rim_spec(light, small);
    if energy > 0. {
        // GPU material の interaction glow を legacy 経路へ写した近似。
        // ホバー 0.5 / 押下 1.0 で、反射と明るさだけを少し持ち上げる。
        rim[1] += energy * 0.025;
        rim[2] += energy * 0.08;
        lens[3] += energy * 0.04;
    }
    let edge_light = graphite_rim_light(light);
    let dispersion = 0.018;
    match kind {
        Kind::Panel => {
            // 読む面なので、Valorant と同じく屈折を唇 (半径の 45%, 4–10px) だけに閉じ込める。
            let band = (radius * 0.45).clamp(4., 10.);
            BackdropFilter {
                blur: 34.,
                saturation: 1.3,
                brightness: if light { 0.90 } else { 0.92 },
                bezel: band,
                refraction: band * 0.9,
                dispersion,
                light: edge_light,
                rim,
                lens,
                ..Default::default()
            }
        }
        Kind::Clear => {
            // 壁紙の上に直接浮く面。霜を残しつつ、縁は鮮明な層から屈折させる。
            let (bezel, shift) = graphite_bezel_scale(w, h);
            BackdropFilter {
                blur: if light { 12. } else { 8. },
                saturation: if light { 1.5 } else { 1.15 },
                brightness: if light { 1.05 } else { 0.92 },
                bezel,
                refraction: shift,
                dispersion,
                light: edge_light,
                rim,
                lens,
                ..Default::default()
            }
        }
        Kind::Frost => BackdropFilter {
            // カード内の操作部。再屈折させず、曇りと細い反射だけ。
            blur: if small < 28. { 8. } else { 12. },
            saturation: if light { 1.5 } else { 1.3 },
            brightness: if light {
                1.04 + energy * 0.02
            } else {
                0.98 + energy * 0.03
            },
            rim: if small >= 20. { rim } else { [0.; 4] },
            lens,
            ..Default::default()
        },
        Kind::Sheet => BackdropFilter {
            // 読むことを優先する膜。屈折は付けず、強めの曇りと光学リムだけ。
            blur: 30.,
            saturation: 1.4,
            brightness: if light { 0.98 } else { 0.94 },
            rim,
            lens,
            ..Default::default()
        },
    }
}

fn paint(kind: Kind, radius: f32, key: Option<&str>, bounds: Bounds<Pixels>, window: &mut Window) {
    let p = pal();
    let kind = if theme::is_graphite() {
        resolve(kind, bounds)
    } else {
        kind
    };
    if theme::is_graphite() && matches!(kind, Kind::Panel | Kind::Frost | Kind::Sheet) {
        note_host(bounds);
    }
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if w < 1. || h < 1. {
        return;
    }
    let r = radius.min(w / 2.).min(h / 2.);
    let corners = Corners::all(px(r));
    let glass = theme::glass_on();
    let graphite = theme::is_graphite();

    // ホバーの照り返し (ばね)。
    let energy = key
        .map(|k| {
            let m = window.mouse_position();
            let inside = bounds.contains(&m);
            let target = if inside {
                if motion::is_down() {
                    1.
                } else {
                    0.5
                }
            } else {
                0.
            };
            motion::spring_with(&format!("glass:{k}"), target, 20.)
        })
        .unwrap_or(0.);

    if glass {
        let small = w.min(h);
        let filter = match kind {
            Kind::Panel if graphite => graphite_filter(kind, p.light, small, r, w, h, energy),
            Kind::Sheet if graphite => graphite_filter(kind, p.light, small, r, w, h, energy),
            Kind::Clear if graphite => graphite_filter(kind, p.light, small, r, w, h, energy),
            Kind::Frost if graphite => graphite_filter(kind, p.light, small, r, w, h, energy),
            Kind::Panel => BackdropFilter {
                blur: 22.,
                saturation: 1.7,
                brightness: if p.light { 1.06 } else { 0.86 },
                bezel: (r * 0.55).clamp(6., 16.),
                refraction: (r * 0.45).clamp(4., 12.),
                dispersion: 0.05,
                light: if p.light {
                    [0.9, 0.3, 0.22, 0.05]
                } else {
                    [0.5, 0.12, 0.14, 0.3]
                },
                rim: rim(p.light, small),
                lens: if p.light {
                    [1., 0.08, 0., 0.3]
                } else {
                    [1., 0.16, 0., 0.26]
                },
                ..Default::default()
            },
            Kind::Sheet => BackdropFilter {
                blur: 34.,
                saturation: 1.8,
                brightness: if p.light { 1.08 } else { 0.78 },
                bezel: (r * 0.5).clamp(6., 14.),
                refraction: (r * 0.4).clamp(4., 10.),
                dispersion: 0.05,
                rim: rim(p.light, small),
                ..Default::default()
            },
            Kind::Clear => BackdropFilter {
                blur: 2.,
                saturation: 1.25,
                brightness: if p.light { 1.04 } else { 0.95 },
                glass: [2., energy, if p.light { 1. } else { 0. }, 1.],
                ..Default::default()
            },
            Kind::Frost => BackdropFilter {
                blur: 10.,
                saturation: 1.3,
                brightness: if p.light { 1.03 } else { 1.05 },
                rim: if small >= 20. {
                    rim(p.light, small)
                } else {
                    [0.; 4]
                },
                ..Default::default()
            },
        };
        window.paint_backdrop(bounds, corners, filter);
    }

    // 面の塗り。
    let (top, bot) = match kind {
        Kind::Panel => (p.glass_top, p.glass_bot),
        Kind::Sheet => (
            theme::alpha(p.glass_top, (p.glass_top.a + 0.22).min(0.96)),
            theme::alpha(p.glass_bot, (p.glass_bot.a + 0.26).min(0.96)),
        ),
        Kind::Clear if graphite => (
            theme::alpha(p.glass_top, p.glass_top.a + energy * 0.04),
            theme::alpha(p.glass_bot, p.glass_bot.a + energy * 0.03),
        ),
        Kind::Clear => (
            theme::alpha(p.glass_top, p.glass_top.a * 0.55 + energy * 0.08),
            theme::alpha(p.glass_bot, p.glass_bot.a * 0.45 + energy * 0.06),
        ),
        Kind::Frost => (
            theme::alpha(p.ctrl, p.ctrl.a + energy * 0.08),
            theme::alpha(p.ctrl, p.ctrl.a * 0.7 + energy * 0.06),
        ),
    };
    let (top, bot) = if glass {
        (top, bot)
    } else {
        // 不透明フォールバック。
        let base = match kind {
            Kind::Frost => theme::mix(p.solid, p.fg, 0.06 + energy * 0.04),
            _ => theme::mix(p.solid, p.fg, if p.light { 0.0 } else { 0.03 }),
        };
        (base, base)
    };
    window.paint_quad(PaintQuad {
        bounds,
        corner_radii: corners,
        background: linear_gradient(180., linear_color_stop(top, 0.), linear_color_stop(bot, 1.)),
        border_widths: Edges::all(px(1.)),
        border_color: if glass { p.edge } else { p.hair },
        border_style: BorderStyle::default(),
    });

    if graphite && glass {
        // 霜ガラスの粒子。小さいノイズテクスチャを面いっぱいに引き伸ばす
        // (粒子自体はランダムなので、タイルを並べなくても継ぎ目は見えない)。
        if p.grain > 0. {
            let _ =
                window.paint_image_nearest(bounds, bounds, corners, grain_image(p.light), 0, false);
        }
        // 下端の柔らかい落ち影。Valorant tracker の Skin がインセット影で
        // 奥行きを足しているのに対応する (色はモノクロに留める)。
        if h >= 18. {
            let depth = 14f32.min(h * 0.28);
            let shade_a = if p.light { 0.045 } else { 0.14 };
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: point(bounds.origin.x, bounds.origin.y + px(h - depth)),
                    size: size(bounds.size.width, px(depth)),
                },
                corner_radii: Corners {
                    top_left: px(0.),
                    top_right: px(0.),
                    bottom_left: px(r),
                    bottom_right: px(r),
                },
                background: linear_gradient(
                    180.,
                    linear_color_stop(theme::hex(0x000000, 0.), 0.),
                    linear_color_stop(theme::hex(0x000000, shade_a), 1.),
                ),
                border_widths: Edges::default(),
                border_color: Hsla::default(),
                border_style: BorderStyle::default(),
            });
        }
        // inset 1px の内側の薄い縁。
        if w > 5. && h > 5. {
            let ir = (r - 1.).max(0.);
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: point(bounds.origin.x + px(1.), bounds.origin.y + px(1.)),
                    size: size(bounds.size.width - px(2.), bounds.size.height - px(2.)),
                },
                corner_radii: Corners::all(px(ir)),
                background: theme::alpha(p.inner, 0.).into(),
                border_widths: Edges::all(px(1.)),
                border_color: p.inner,
                border_style: BorderStyle::default(),
            });
        }
    }

    // 上端の光 (角丸の弧を避けて直線部だけ)。
    if glass && h >= 14. {
        let inset = (r * 0.8).min(w / 3.);
        let lw = (w - inset * 2.).max(0.);
        if lw > 2. {
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: point(bounds.origin.x + px(inset), bounds.origin.y + px(0.5)),
                    size: size(px(lw), px(1.)),
                },
                corner_radii: Corners::default(),
                background: linear_gradient(
                    90.,
                    linear_color_stop(theme::alpha(p.rim, 0.), 0.),
                    linear_color_stop(p.rim, 0.5),
                ),
                border_widths: Edges::default(),
                border_color: Hsla::default(),
                border_style: BorderStyle::default(),
            });
        }
        if graphite {
            // 下端の補助リム。主光よりずっと弱く、厚みだけを示す。
            let bottom = if p.light {
                theme::hex(0xffffff, 0.52)
            } else {
                theme::hex(0xffffff, 0.07)
            };
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: point(bounds.origin.x + px(inset), bounds.origin.y + px(h - 1.5)),
                    size: size(px(lw), px(1.)),
                },
                corner_radii: Corners::default(),
                background: linear_gradient(
                    90.,
                    linear_color_stop(theme::alpha(bottom, 0.), 0.),
                    linear_color_stop(bottom, 0.5),
                ),
                border_widths: Edges::default(),
                border_color: Hsla::default(),
                border_style: BorderStyle::default(),
            });
        }
    }
}

/// 霜ガラス用のモノクロ粒子 (alpha 焼き込み)。暗色面には白粒子、明色面には黒粒子。
fn grain_image(light: bool) -> std::sync::Arc<RenderImage> {
    use std::sync::{Arc, Mutex};
    static CACHE: Mutex<[Option<Arc<RenderImage>>; 2]> = Mutex::new([None, None]);
    let idx = light as usize;
    let mut g = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(img) = &g[idx] {
        return img.clone();
    }
    let n = 512u32;
    let mut img = image::RgbaImage::new(n, n);
    let mut seed = if light { 0x71c3a59du32 } else { 0x9e3779b9u32 };
    let lum = if light { 0u8 } else { 255u8 };
    // TOKENS grain: dark .11 / light .20。GPUI には overlay 合成がないので、
    // 通常の alpha 合成では粒子が汚れに見えないよう弱めに焼き込む。
    let max_a = if light { 12. } else { 8. };
    for px in img.pixels_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = ((seed & 0xff) as f32 / 255.) * max_a;
        *px = image::Rgba([lum, lum, lum, a as u8]);
    }
    // RenderImage は BGRA。
    for p in img.pixels_mut() {
        p.0.swap(0, 2);
    }
    let out = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
        img
    )]));
    g[idx] = Some(out.clone());
    out
}

/// 縁の光沢リム (rim width, whiteness, key light, fill light)。
fn rim(light: bool, small: f32) -> [f32; 4] {
    let w = if small <= 72. { 1.4 } else { 2.2 };
    if light {
        [w, 0.12, 0.8, 0.36]
    } else {
        [w, 0.07, 0.55, 0.2]
    }
}

/// 丸いレンズ (トグルのつまみ・スライダーのつまみ)。押している間だけ泡のように膨らむ。
pub fn bubble(bounds: Bounds<Pixels>, r: f32, window: &mut Window) {
    if !theme::glass_on() || theme::is_graphite() {
        return;
    }
    let light = theme::is_light();
    let d = r * 2.;
    window.paint_backdrop(
        bounds,
        Corners::all(px(r)),
        BackdropFilter {
            lens: [1.22, if light { 0.14 } else { 0.42 }, 0., 0.],
            bezel: (d * 0.16).max(2.),
            refraction: (d * 0.13).max(2.),
            dispersion: 0.07,
            saturation: 1.08,
            brightness: 1.03,
            rim: if light {
                [0., 0., 0.85, 0.42]
            } else {
                [0., 0., 0.32, 0.16]
            },
            ..Default::default()
        },
    );
}
