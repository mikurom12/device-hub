//! 実時間アニメーションと要素の位置の記録。
//!
//! - `spring(key, target)` … 臨界減衰ばねで target へ寄せた今の値を返す。
//!   動いている間は `end_frame` が次フレームを要求する (止まれば描画も止まる =
//!   アイドル時 CPU/GPU 0 — ゲーム中に裏で開いていても軽い)。
//! - `track(id)` … 要素の矩形を毎フレーム記録する透明な canvas。
//!   スライダーのドラッグ・ポップアップの位置合わせ・ホバー判定に使う。

use gpui_kit::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// 補間値の持つ数。画面を離れても消えないように、古いものから回す。
const MAX_TRACKS: usize = 1024;
/// ウィンドウを離れている間に積まれた時間を打ち切る (復帰時に一気に進まない)。
const MAX_DT: f32 = 0.1;

// Valorant tracker tokens.rs / Web lib/motion.ts と同じ時間・曲線。
pub const DUR_PRESS: f32 = 100.;
pub const DUR_HOVER: f32 = 140.;
pub const DUR_MENU_IN: f32 = 180.;
pub const DUR_MENU_OUT: f32 = 120.;
pub const DUR_DIALOG_IN: f32 = 220.;
pub const DUR_DIALOG_OUT: f32 = 150.;
pub const DUR_INDICATOR: f32 = 200.;
pub const DUR_PANEL: f32 = 240.;
pub const DUR_ENTER: f32 = 320.;

pub const EASE_OUT: [f32; 4] = [0.22, 1., 0.36, 1.];
pub const EASE_IN_OUT: [f32; 4] = [0.65, 0., 0.35, 1.];
pub const EASE_IN: [f32; 4] = [0.4, 0., 1., 1.];

#[derive(Clone, Copy)]
struct Track {
    from: f32,
    target: f32,
    value: f32,
    vel: f32,
    elapsed: f32,
    dur: f32,
    curve: [f32; 4],
    omega: f32,
    zeta: f32,
    frame: u64,
}

#[derive(Default)]
struct Sys {
    tracks: HashMap<String, Track>,
    order: VecDeque<String>,
    rects: HashMap<String, [f32; 4]>,
    hot: HashMap<String, bool>,
    pointer: [f32; 2],
    down: bool,
    last: Option<Instant>,
    dt: f32,
    frame: u64,
    dirty: bool,
}

impl Sys {
    fn remember(&mut self, key: &str, tr: Track) {
        if let Some(existing) = self.tracks.get_mut(key) {
            *existing = tr;
            return;
        }
        self.order.push_back(key.to_owned());
        while self.order.len() > MAX_TRACKS {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            self.tracks.remove(&old);
        }
        self.tracks.insert(key.to_owned(), tr);
    }
}

thread_local! {
    static SYS: RefCell<Sys> = RefCell::new(Sys { dt: 1. / 60., ..Default::default() });
}

/// フレームの先頭で呼ぶ。経過時間を測る。
pub fn begin_frame() {
    SYS.with(|s| {
        let mut s = s.borrow_mut();
        let now = Instant::now();
        s.dt = s
            .last
            .map(|l| (now - l).as_secs_f32())
            .unwrap_or(1. / 60.)
            .clamp(0.0005, MAX_DT);
        s.last = Some(now);
        s.frame += 1;
        s.dirty = false;
    });
}

/// フレームの終わりで呼ぶ。ばねが動いていれば次のフレームを要求する。
pub fn end_frame(window: &mut Window) {
    let dirty = SYS.with(|s| {
        let mut s = s.borrow_mut();
        let d = s.dirty;
        if !d {
            s.last = None; // 止まっている間の時間は数えない
        }
        d
    });
    if dirty {
        window.request_animation_frame();
    }
}

/// CSS の `cubic-bezier(x1, y1, x2, y2)` と同じ曲線。進行 `t` から補間 `y` を出す。
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    let sample = |a: f32, b: f32, s: f32| {
        ((1. + 3. * a - 3. * b) * s + 3. * b - 6. * a) * s * s + 3. * a * s
    };
    let (mut lo, mut hi) = (0f32, 1f32);
    let mut s = t;
    for _ in 0..24 {
        let error = sample(x1, x2, s) - t;
        if error.abs() < 1e-7 {
            return sample(y1, y2, s);
        }
        if error < 0. {
            lo = s;
        } else {
            hi = s;
        }
        let slope = (3. * (1. + 3. * x1 - 3. * x2) * s + 6. * x2 - 12. * x1) * s + 3. * x1;
        let next = s - error / slope;
        s = if slope > 1e-6 && next > lo && next < hi {
            next
        } else {
            (lo + hi) / 2.
        };
    }
    sample(y1, y2, (lo + hi) / 2.)
}

fn curve_of([x1, y1, x2, y2]: [f32; 4], t: f32) -> f32 {
    bezier(x1, y1, x2, y2, t)
}

/// 目標値へ `dur_ms`・`curve` で寄っていく現在の値。動き OFF のときは目標をそのまま返す。
/// はじめて見たキーは補間せずに採用する (CSS が初期値を遷移させないのと同じ)。
pub fn at(key: &str, target: f32, dur_ms: f32, curve: [f32; 4]) -> f32 {
    if !crate::theme::motion_on() {
        reset(key);
        return target;
    }
    SYS.with(|cell| {
        let mut s = cell.borrow_mut();
        let dt = s.dt;
        let frame = s.frame;
        let Some(mut tr) = s.tracks.get(key).copied() else {
            s.remember(
                key,
                Track {
                    from: target,
                    target,
                    value: target,
                    vel: 0.,
                    elapsed: 0.,
                    dur: dur_ms / 1000.,
                    curve,
                    omega: 0.,
                    zeta: 1.,
                    frame,
                },
            );
            return target;
        };
        let same_frame = tr.frame == frame;
        let changed = (tr.target - target).abs() > 1e-6;
        if same_frame && !changed {
            return tr.value;
        }
        tr.frame = frame;
        if changed {
            tr.from = tr.value;
            tr.target = target;
            tr.elapsed = 0.;
            tr.dur = dur_ms / 1000.;
            tr.curve = curve;
        }
        if tr.dur <= 0. || (tr.value - target).abs() < 1e-6 {
            tr.value = target;
            tr.vel = 0.;
        } else {
            tr.elapsed += if same_frame {
                0.
            } else if changed {
                dt.min(1. / 120.)
            } else {
                dt
            };
            let p = (tr.elapsed / tr.dur).clamp(0., 1.);
            tr.value = tr.from + (tr.target - tr.from) * curve_of(tr.curve, p);
            if p >= 1. {
                tr.value = tr.target;
            } else {
                s.dirty = true;
            }
        }
        s.remember(key, tr);
        tr.value
    })
}

/// ばねの 1 ステップを厳密解で進める (フレームの刻み方に依らない)。
pub fn spring_advance(x: f32, v: f32, target: f32, omega: f32, zeta: f32, dt: f32) -> (f32, f32) {
    if dt <= 0. {
        return (x, v);
    }
    let w = omega.max(0.0001);
    let z = zeta.max(0.);
    let d = x - target;
    let (nx, nv) = if z < 1. {
        let wd = w * (1. - z * z).sqrt();
        let a = d;
        let b = (v + z * w * a) / wd;
        let e = (-z * w * dt).exp();
        let (c, sn) = ((wd * dt).cos(), (wd * dt).sin());
        (
            target + e * (a * c + b * sn),
            e * ((b * wd - a * z * w) * c - (a * wd + b * z * w) * sn),
        )
    } else {
        let a = d;
        let b = v + w * a;
        let e = (-w * dt).exp();
        (target + (a + b * dt) * e, (b - w * (a + b * dt)) * e)
    };
    if d != 0. && (nx - target) * d <= 0. {
        (target, 0.)
    } else {
        (nx, nv)
    }
}

/// 目標へばねで寄る現在値。途中で目標が変わっても速度が連続したまま向きを変える。
pub fn at_spring(key: &str, target: f32, omega: f32, zeta: f32) -> f32 {
    if !crate::theme::motion_on() {
        reset(key);
        return target;
    }
    SYS.with(|cell| {
        let mut s = cell.borrow_mut();
        let dt = s.dt;
        let frame = s.frame;
        let Some(mut tr) = s.tracks.get(key).copied() else {
            s.remember(
                key,
                Track {
                    from: target,
                    target,
                    value: target,
                    vel: 0.,
                    elapsed: 0.,
                    dur: 0.,
                    curve: EASE_OUT,
                    omega,
                    zeta,
                    frame,
                },
            );
            return target;
        };
        let same_frame = tr.frame == frame;
        let target_changed = (tr.target - target).abs() > 1e-6;
        let retuned = tr.dur > 0.
            || (tr.omega - omega).abs() > 1e-4
            || (tr.zeta - zeta).abs() > 1e-4
            || target_changed;
        if same_frame && !retuned {
            return tr.value;
        }
        tr.frame = frame;
        tr.dur = 0.;
        tr.target = target;
        tr.omega = omega;
        tr.zeta = zeta;
        if target_changed && (target - tr.value) * tr.vel < 0. {
            tr.vel = 0.;
        }
        if same_frame {
            if (tr.value - target).abs() > 0.002 {
                s.dirty = true;
            }
        } else {
            let (x, v) = spring_advance(tr.value, tr.vel, target, omega, zeta, dt);
            tr.value = x;
            tr.vel = v;
            if (tr.value - target).abs() < 0.002 {
                tr.value = target;
                tr.vel = 0.;
            } else {
                s.dirty = true;
            }
        }
        s.remember(key, tr);
        tr.value
    })
}

/// キーを捨てる (画面を離れた補間値を次の画面へ引きずらない)。
pub fn reset(key: &str) {
    SYS.with(|cell| {
        let mut s = cell.borrow_mut();
        if s.tracks.remove(key).is_some() {
            s.order.retain(|k| k != key);
        }
    });
}

/// キーを指定した値で用意する (まだ無いときだけ)。入場は 0 に置いてから `at` / `at_spring` へ向ける。
pub fn seed(key: &str, value: f32) {
    SYS.with(|cell| {
        let mut s = cell.borrow_mut();
        if !s.tracks.contains_key(key) {
            let frame = s.frame;
            s.remember(
                key,
                Track {
                    from: value,
                    target: value,
                    value,
                    vel: 0.,
                    elapsed: 0.,
                    dur: 0.,
                    curve: EASE_OUT,
                    omega: 0.,
                    zeta: 1.,
                    frame,
                },
            );
        }
    });
}

/// 既存の値を捨てて入場をやり直す (ページ/タブ切替)。
pub fn restart(key: &str, value: f32) {
    reset(key);
    seed(key, value);
}

/// 出ている / 消えているを補間する (ダイアログ・メニュー・トーストの出入り)。
/// 時間は ms。閉じる側も `present=false` の間描き続けるとフェードアウトになる。
pub fn presence(key: &str, present: bool, dur_in: f32, dur_out: f32) -> f32 {
    let k = format!("v:{key}");
    seed(&k, 0.);
    let dur_ms = if present { dur_in } else { dur_out };
    at(&k, if present { 1. } else { 0. }, dur_ms, EASE_OUT)
}

/// ホバー・プレスの補間値 (0..1)。ホバーは少し行き過ぎるばね、押下は臨界減衰。
pub fn feel(key: &str) -> (f32, f32) {
    let on = hovered(key);
    (
        at(&format!("h:{key}"), if on { 1. } else { 0. }, DUR_HOVER, EASE_OUT),
        at(
            &format!("p:{key}"),
            if on && is_down() { 1. } else { 0. },
            DUR_PRESS,
            EASE_OUT,
        ),
    )
}

/// 臨界減衰ばね。`speed` は大きいほど速い (12 ≒ 0.35s)。
pub fn spring_with(key: &str, target: f32, speed: f32) -> f32 {
    at_spring(key, target, speed, 1.)
}

pub fn spring(key: &str, target: f32) -> f32 {
    spring_with(key, target, 16.)
}

/// 0/1 の状態をなめらかにした値。
pub fn ease(key: &str, on: bool) -> f32 {
    at(key, if on { 1. } else { 0. }, DUR_HOVER, EASE_OUT)
}

/// 外部から「次のフレームも描け」と伝える (時間で変わる表示用)。
pub fn keep_alive() {
    SYS.with(|s| s.borrow_mut().dirty = true);
}

// ------------------------------------------------------------------ 位置

/// 要素の矩形を記録する透明な層 (親いっぱいに広がる)。
pub fn track(id: impl Into<String>) -> impl IntoElement {
    let id: String = id.into();
    canvas(
        move |bounds, _, _| {
            SYS.with(|s| {
                s.borrow_mut().rects.insert(
                    id.clone(),
                    [
                        f32::from(bounds.origin.x),
                        f32::from(bounds.origin.y),
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                    ],
                );
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// 前のフレームで記録した矩形 [x, y, w, h]。
pub fn rect(id: &str) -> Option<[f32; 4]> {
    SYS.with(|s| s.borrow().rects.get(id).copied())
}

pub fn set_pointer(x: f32, y: f32) {
    SYS.with(|s| s.borrow_mut().pointer = [x, y]);
}

/// カーソルが動いたとき、記録済み矩形のどれかに入った/出たか。
/// GPUI は `.hover()` を持たない要素をマウス移動だけでは再描画しないので、
/// 変わったときだけ呼び出し側が notify する。
pub fn pointer_moved(x: f32, y: f32) -> bool {
    SYS.with(|cell| {
        let mut s = cell.borrow_mut();
        s.pointer = [x, y];
        let mut changed = false;
        let Sys { rects, hot, .. } = &mut *s;
        for (key, r) in rects {
            let now = x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3];
            if hot.get(key).copied().unwrap_or(false) != now {
                changed = true;
            }
            hot.insert(key.clone(), now);
        }
        changed || s.dirty
    })
}

pub fn pointer() -> [f32; 2] {
    SYS.with(|s| s.borrow().pointer)
}

pub fn set_down(d: bool) {
    SYS.with(|s| s.borrow_mut().down = d);
}

pub fn is_down() -> bool {
    SYS.with(|s| s.borrow().down)
}

/// カーソルがその要素の上にあるか (前フレームの矩形で判定)。
pub fn hovered(id: &str) -> bool {
    let p = pointer();
    rect(id)
        .is_some_and(|r| p[0] >= r[0] && p[0] <= r[0] + r[2] && p[1] >= r[1] && p[1] <= r[1] + r[3])
}
