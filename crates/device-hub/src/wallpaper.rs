//! ガラスの下地になる壁紙。
//!
//! GPU の backdrop は「アプリが描いたもの」から作るので、デスクトップは透けない。
//! そこで窓いっぱいに壁紙を敷く。既定は手続き生成のオーロラ (静止画 1 枚 = アイドル時の
//! 描画コストはゼロ)。設定で好きな画像 (PNG/JPEG) に差し替えられる。

use gpui_kit::*;
use image::{Frame, RgbaImage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct Blob {
    x: f32,
    y: f32,
    r: f32,
    c: [f32; 3],
    k: f32,
}

fn rgbf(v: u32) -> [f32; 3] {
    [
        ((v >> 16) & 255) as f32,
        ((v >> 8) & 255) as f32,
        (v & 255) as f32,
    ]
}

fn blobs(light: bool, accent: u32) -> Vec<Blob> {
    let a = rgbf(accent);
    let b = |x, y, r, c: [f32; 3], k| Blob { x, y, r, c, k };
    if light {
        vec![
            b(0.12, 0.10, 0.62, rgbf(0xa9dcff), 1.0),
            b(0.88, 0.06, 0.55, rgbf(0xd4c2ff), 0.9),
            b(0.80, 0.94, 0.66, rgbf(0x9ff0de), 0.85),
            b(0.18, 0.90, 0.58, rgbf(0xffcce2), 0.8),
            b(
                0.55,
                0.45,
                0.5,
                [a[0] * 0.5 + 127., a[1] * 0.5 + 127., a[2] * 0.5 + 127.],
                0.55,
            ),
        ]
    } else {
        vec![
            b(0.08, 0.04, 0.62, rgbf(0x15406a), 0.95),
            b(0.94, 0.10, 0.52, rgbf(0x3c1f70), 0.85),
            b(0.84, 0.96, 0.64, rgbf(0x0b4c4a), 0.9),
            b(0.14, 0.94, 0.56, rgbf(0x52163f), 0.8),
            b(
                0.56,
                0.52,
                0.42,
                [a[0] * 0.32, a[1] * 0.32, a[2] * 0.32],
                0.7,
            ),
        ]
    }
}

fn hash(x: f32, y: f32) -> f32 {
    let h = (x * 127.1 + y * 311.7).sin() * 43758.547;
    h - h.floor()
}

fn generate(light: bool, accent: u32, w: u32, h: u32) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    let (top, bot) = if light {
        ([234., 240., 248.], [247., 248., 252.])
    } else {
        ([5., 7., 12.], [9., 10., 17.])
    };
    let bl = blobs(light, accent);
    let (wf, hf) = (w as f32, h as f32);
    for y in 0..h {
        let v = y as f32 / hf;
        for x in 0..w {
            let u = x as f32 / wf;
            let mut c = [
                top[0] + (bot[0] - top[0]) * v,
                top[1] + (bot[1] - top[1]) * v,
                top[2] + (bot[2] - top[2]) * v,
            ];
            for b in &bl {
                let dx = (u - b.x) * (wf / hf);
                let dy = v - b.y;
                let k = (-(dx * dx + dy * dy) / (b.r * b.r * 0.33)).exp() * b.k;
                if k > 0.001 {
                    for (ci, bi) in c.iter_mut().zip(b.c.iter()) {
                        *ci += (bi - *ci) * k;
                    }
                }
            }
            let n = (hash(u * 91., v * 57.) - 0.5) * if light { 2.0 } else { 3.2 };
            img.put_pixel(
                x,
                y,
                image::Rgba([
                    (c[0] + n).clamp(0., 255.) as u8,
                    (c[1] + n).clamp(0., 255.) as u8,
                    (c[2] + n).clamp(0., 255.) as u8,
                    255,
                ]),
            );
        }
    }
    img
}

fn mix_to(c: &mut [f32; 3], target: [f32; 3], k: f32) {
    let k = k.clamp(0., 1.);
    for (ci, ti) in c.iter_mut().zip(target) {
        *ci += (ti - *ci) * k;
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Frost Graphite のモノクロ壁紙: クロームの球 3 個、細いリング 2 本、
/// 斜めの光の帯、左上の光。色は使わず、明暗だけで立体を作る。
fn generate_graphite(light: bool, w: u32, h: u32) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    let aspect = wf / hf;
    let bg = if light {
        rgbf(0xe9eaed)
    } else {
        rgbf(0x09090a)
    };
    let hi = rgbf(if light { 0xffffff } else { 0xf6f6f7 });
    let mid = rgbf(if light { 0xb4b6bd } else { 0x55555b });
    let lo = rgbf(if light { 0x5d5f67 } else { 0x0c0c0d });
    let white = [255., 255., 255.];
    let black = [0., 0., 0.];
    let ring_rgb = if light { black } else { white };
    let beam_rgb = white;
    let glow_a = if light { 0.95 } else { 0.13 };
    let ring_a = if light { 0.12 } else { 0.16 };
    let beam_a = if light { 0.8 } else { 0.07 };

    let sphere = |u: f32, v: f32, cx: f32, cy: f32, r: f32| -> Option<(f32, f32)> {
        let dx = (u - cx) * aspect;
        let dy = v - cy;
        let d = (dx * dx + dy * dy).sqrt() / r;
        if d > 1.02 {
            return None;
        }
        // CSS radial-gradient(circle at 34% 30%): 光源中心は球の左上。
        let hx = dx + r * 0.30;
        let hy = dy + r * 0.32;
        let q = (hx * hx + hy * hy).sqrt() / r;
        Some((d, q))
    };
    let chrome = |q: f32| {
        let mut c = hi;
        if q < 0.42 {
            mix_to(&mut c, mid, q / 0.42);
        } else {
            c = mid;
            mix_to(&mut c, lo, ((q - 0.42) / 0.72).min(1.));
        }
        c
    };

    for y in 0..h {
        let v = (y as f32 + 0.5) / hf;
        for x in 0..w {
            let u = (x as f32 + 0.5) / wf;
            let mut c = bg;
            // わずかな垂直グラデーション。
            mix_to(
                &mut c,
                if light { white } else { black },
                if light { v * 0.05 } else { v * 0.12 },
            );

            // 左上の大きな光。
            let gx = (u + 0.12) * aspect;
            let gy = v + 0.18;
            let g = (-(gx * gx + gy * gy) / 0.42).exp() * glow_a;
            mix_to(&mut c, white, g);

            // 右側の同心リング 2 本。
            let rx = (u - 0.909) * aspect;
            let ry = v - 0.286;
            let rd = (rx * rx + ry * ry).sqrt();
            for (rr, aa) in [(0.452, ring_a), (0.583, ring_a * 0.6)] {
                let k = (-((rd - rr) / 0.004).powi(2)).exp() * aa;
                mix_to(&mut c, ring_rgb, k);
            }

            // クローム球 (大 → 中 → 小)。
            for (cx, cy, r, shade) in [
                (0.894, 0.357, 0.310, 1.0),
                (0.356, 0.690, 0.167, 0.88),
                (0.859, 0.708, 0.089, 0.78),
            ] {
                if let Some((d, q)) = sphere(u, v, cx, cy, r) {
                    let mut sc = chrome(q * shade);
                    // 縁を少し沈めて球らしくする。
                    mix_to(&mut sc, lo, smoothstep(0.78, 1.0, d) * 0.32);
                    let mask = 1. - smoothstep(0.965, 1.0, d);
                    mix_to(&mut c, sc, mask);
                }
            }

            // 斜めの光の帯 (上に重ねる)。角度 -14°、中心 y=0.68。
            let ang = (-14f32).to_radians();
            let lx = (u - 0.5) * aspect;
            let ly = v - 0.68;
            let dist = (lx * ang.sin() + ly * ang.cos()).abs();
            let beam = (-(dist / 0.085).powi(2)).exp() * beam_a;
            mix_to(&mut c, beam_rgb, beam);

            img.put_pixel(
                x,
                y,
                image::Rgba([
                    c[0].clamp(0., 255.) as u8,
                    c[1].clamp(0., 255.) as u8,
                    c[2].clamp(0., 255.) as u8,
                    255,
                ]),
            );
        }
    }
    img
}

static CACHE: Mutex<Option<HashMap<String, Arc<RenderImage>>>> = Mutex::new(None);

fn to_render(img: RgbaImage) -> Arc<RenderImage> {
    // RenderImage は BGRA を期待する。
    let mut img = img;
    for p in img.pixels_mut() {
        p.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec::smallvec![Frame::new(img)]))
}

/// 今のテーマ系統・明暗・アクセント・ユーザー画像に合う壁紙。
/// ユーザー画像がある場合はテーマ系統よりそちらを優先する。
pub fn current(custom: Option<&str>) -> Arc<RenderImage> {
    let light = crate::theme::is_light();
    let graphite = crate::theme::is_graphite();
    let accent = crate::theme::ACCENTS[crate::theme::accent_index() as usize].1;
    let key = match custom {
        Some(p) => format!("file:{p}"),
        None => format!(
            "{}:{light}:{accent:06x}",
            if graphite { "graphite" } else { "aurora" }
        ),
    };
    let mut g = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(i) = map.get(&key) {
        return i.clone();
    }
    let img = custom
        .and_then(|p| image::open(p).ok())
        .map(|i| {
            let i = i.thumbnail(1920, 1200);
            i.to_rgba8()
        })
        .unwrap_or_else(|| {
            if graphite {
                generate_graphite(light, 720, 450)
            } else {
                generate(light, accent, 720, 450)
            }
        });
    let out = to_render(img);
    if map.len() > 8 {
        map.clear();
    }
    map.insert(key, out.clone());
    out
}
