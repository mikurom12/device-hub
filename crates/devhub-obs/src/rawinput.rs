//! Raw Input で実測 input report レートを計測する (Windows)。
//!
//! マウス (usage 1/2) とキーボード (usage 1/6) の raw input をメッセージ専用
//! ウィンドウで `RIDEV_INPUTSINK` 受信し、hDevice → VID/PID ごとに到達イベントを
//! 数える。OS のカーソル/キーイベントとは違い、デバイスが実際に送った
//! input report の到達率になる (8KHz マウスなら動かすと ~8000/秒)。
//! キーボードはキー状態変化のたびに report が来るので、入力中のみカウントされる。
//!
//! 高頻度対策: `WM_INPUT` 受信時に `GetRawInputData` で当該イベントを読み、
//! 続けて `GetRawInputBuffer` で溜まった分をまとめてドレインする
//! (Microsoft が高周波デバイス向けに推奨する手順。GetMessage が現在イベントを
//! キューから除くため二重カウントにはならない)。
//!
//! 「ポーリング実測」の値の意味 (docs/POLLING_RATE.md):
//! - マウスは「動きがあるときだけ」レポートを送るので、届く数 ≦ カウント/秒。
//! - 区間 (gap ≤ 12ms が続く区間) ごとにレートを出し、止まっている時間を除外する。
//! - RAWMOUSE の |dx|+|dy| を集計して「この速度・DPI での理論上限 ≒ カウント/秒」を出す。
//!
//! Windows 11 は背面のマウス入力をまとめて配信する。集計できるのは OS から
//! 届いたイベント数。USB 転送側との比較は docs/POLLING_RATE.md を参照。

/// `stats` の区間 1 件の公開形。
#[derive(Clone, Copy, Debug)]
pub struct SegInfo {
    /// 区間内のレポート/秒。
    pub rate_hz: f32,
    /// 区間の時間幅 (ms)。
    pub span_ms: u64,
    /// 区間終了からの経過秒 (stats 呼び出し時点)。
    pub age_s: f32,
}

/// 計測スナップショット — `stats` が 1 回のロックでまとめて返す。
#[derive(Clone, Debug, Default)]
pub struct PollStats {
    /// 直近 1 秒のイベント数/秒 (止まっている時間も分母に入る下限値)。
    pub hz: f32,
    /// 250ms バケットのレート履歴 (古い→新しい、~15 秒)。
    pub hist: Vec<f32>,
    /// 直近イベントの最小間隔 (ms)。
    pub min_gap_ms: Option<f32>,
    /// 直近イベント間隔の (中央値, p95) [ms] — OS からの配信間隔。
    pub gap_stats: Option<(f32, f32)>,
    /// 直近の「動いている区間」のレート (進行中 ≥60ms or 直近確定 ≤1.5s)。
    pub seg_hz: Option<f32>,
    /// `seg_hz` の区間幅 (ms)。
    pub seg_span_ms: Option<u64>,
    /// 直近 10 秒に終わった区間の最大レート (「ピーク」)。
    pub peak_hz: Option<f32>,
    /// 直近区間の移動量合計/秒 (|dx|+|dy|) — この速度・DPI での理論上限 ≒ これ。
    pub counts_per_sec: Option<f32>,
    /// 直近区間で |d|≤1 のレポートの割合 — 高いほど DPI/速度が足りない。
    pub low_frac: Option<f32>,
    /// 確定区間 (新しい順、最大 128) — 5 秒計測等の窓切りに使う。
    pub segs: Vec<SegInfo>,
}

#[cfg(windows)]
mod imp {
    use super::{PollStats, SegInfo};
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};
    use std::mem::size_of;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::{
        GetRawInputBuffer, GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices,
        HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDI_DEVICENAME,
        RID_INPUT, RIM_TYPEMOUSE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW,
        TranslateMessage, CW_USEDEFAULT, MSG, WM_INPUT, WNDCLASSEXW, WS_OVERLAPPED,
    };

    /// Hz 計測窓。
    const WINDOW_SECS: f32 = 1.0;
    /// グラフのバケット幅。
    const BUCKET_MS: u64 = 250;
    /// グラフに持つバケット数 (~15 秒)。
    const HIST_LEN: usize = 60;
    /// 間隔統計に使う直近ギャップ数 (8KHz で ~64ms、1KHz で ~0.5s 分)。
    const GAP_LEN: usize = 512;
    /// これより間隔が空いたら「動くのをやめた」とみなして区間を切る (ms)。
    const SEG_BREAK_MS: u64 = 12; // 125 Hz (8 ms) も連続区間として集計する。
    /// 区間として採用する最小の時間幅 (ms)。
    const SEG_MIN_SPAN_MS: u64 = 100;
    /// 区間として採用する最小レポート数。
    const SEG_MIN_N: u32 = 8;
    /// 確定区間の保持数。
    const SEGS_LEN: usize = 128;
    /// 「ピーク」の対象期間 (ms)。
    const PEAK_WINDOW_MS: u64 = 10_000;
    /// 「現在の区間」とみなす終端の鮮度 (ms)。これより古い確定区間は使わない。
    const SEG_FRESH_MS: u64 = 1_500;
    /// 生入力バッファの初期サイズ (要素数 = u64 → 64KiB)。
    const BUF_LEN: usize = 8192;

    /// 確定した「動いている区間」。start/last はエポックからの ms。
    #[derive(Clone, Copy, Default)]
    struct Seg {
        start_ms: u64,
        last_ms: u64,
        n: u32,
        /// |dx|+|dy| ≤ 1 のレポート数 (DPI/速度不足の指標)。
        low: u32,
        /// 移動量合計 (|dx|+|dy|)。
        counts: u64,
    }

    impl Seg {
        fn span_ms(&self) -> u64 {
            self.last_ms.saturating_sub(self.start_ms)
        }
        fn rate_hz(&self) -> f32 {
            self.n.saturating_sub(1) as f32 / (self.span_ms() as f32 / 1000.).max(0.001)
        }
        /// 採用できる区間か (短い揺れや孤立イベントを除く)。
        fn usable(&self) -> bool {
            self.n >= SEG_MIN_N && self.span_ms() >= SEG_MIN_SPAN_MS
        }
    }

    struct Dev {
        /// デバイス名から引いた VID/PID (未取得なら None)。
        vidpid: Option<Option<(u16, u16)>>,
        /// 直近 ~1.2 秒のイベント時刻 (エポックからの µs)。
        events: VecDeque<u64>,
        /// 直近のイベント間隔 (ms)。
        gaps: VecDeque<f32>,
        /// 250ms バケット (bucket_idx, count)。
        buckets: VecDeque<(u64, u32)>,
        /// 集計中の区間 (マウスのみ — キーイベントは counts=None で区間を作らない)。
        seg: Seg,
        /// 確定した区間 (新しい順に末尾)。
        segs: VecDeque<Seg>,
        buttons: u8,
    }

    impl Dev {
        fn new() -> Self {
            Self {
                vidpid: None,
                events: VecDeque::new(),
                gaps: VecDeque::new(),
                buckets: VecDeque::new(),
                seg: Seg::default(),
                segs: VecDeque::new(),
                buttons: 0,
            }
        }

        fn bump(&mut self, us: u64, counts: Option<u32>) {
            let ms = us / 1000;
            if let Some(&prev) = self.events.back() {
                let gap = us.saturating_sub(prev) as f32 / 1000.;
                if gap > 0. {
                    self.gaps.push_back(gap);
                    while self.gaps.len() > GAP_LEN {
                        self.gaps.pop_front();
                    }
                }
            }
            self.events.push_back(us);
            while self
                .events
                .front()
                .is_some_and(|t| us.saturating_sub(*t) > 1_200_000)
            {
                self.events.pop_front();
            }
            let idx = ms / BUCKET_MS;
            match self.buckets.back_mut() {
                Some(back) if back.0 == idx => back.1 += 1,
                _ => self.buckets.push_back((idx, 1)),
            }
            while self.buckets.len() > HIST_LEN {
                self.buckets.pop_front();
            }
            if let Some(c) = counts {
                self.seg_bump(ms, c);
            }
        }

        fn seg_bump(&mut self, ms: u64, c: u32) {
            if self.seg.n > 0 && ms.saturating_sub(self.seg.last_ms) > SEG_BREAK_MS {
                self.close_seg();
            }
            if self.seg.n == 0 {
                self.seg.start_ms = ms;
            }
            self.seg.last_ms = ms;
            self.seg.n += 1;
            self.seg.counts += c as u64;
            if c <= 1 {
                self.seg.low += 1;
            }
        }

        /// 集計中の区間を確定し、採用できるなら履歴へ。
        fn close_seg(&mut self) {
            let s = self.seg;
            self.seg = Seg::default();
            if s.usable() {
                self.segs.push_back(s);
                while self.segs.len() > SEGS_LEN {
                    self.segs.pop_front();
                }
            }
        }
    }

    struct Meter {
        devs: HashMap<usize, Dev>,
        /// 時刻の起点 (最初のイベント時刻)。
        epoch: Option<Instant>,
        trial: Option<Trial>,
    }

    struct Trial {
        start: Instant,
        end: Instant,
        devs: HashMap<usize, Dev>,
    }

    /// テスト画面の計測窓。通常の履歴とは独立に、開始後のマウス入力だけを数える。
    pub fn begin_trial(start: Instant, end: Instant) {
        METER.lock().unwrap_or_else(|p| p.into_inner()).trial = Some(Trial {
            start,
            end,
            devs: HashMap::new(),
        });
    }

    pub fn finish_trial(vid: u16, pid: u16) -> Option<PollStats> {
        let mut trial = METER
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .trial
            .take()?;
        let end_us = (trial.end - trial.start).as_micros() as u64;
        snapshot(&mut trial.devs, vid, pid, end_us)
    }

    static METER: std::sync::LazyLock<Mutex<Meter>> = std::sync::LazyLock::new(|| {
        Mutex::new(Meter {
            devs: HashMap::new(),
            epoch: None,
            trial: None,
        })
    });
    static STARTED: AtomicBool = AtomicBool::new(false);

    /// 起動時に 1 度だけ呼ぶ。メッセージ専用ウィンドウ + raw input 登録。
    pub fn start() {
        start_meter(true);
    }
    pub fn start_mouse() {
        start_meter(false);
    }
    fn start_meter(keyboard: bool) {
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::Builder::new()
            .name("devhub-rawinput".into())
            .spawn(move || unsafe { run(keyboard) })
            .ok();
    }

    unsafe extern "system" fn wnd(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_INPUT {
            on_input(l);
        }
        DefWindowProcW(hwnd, msg, w, l)
    }

    unsafe fn run(keyboard: bool) {
        // 8KHz マウスでもキューを溢れさせないよう捌きを優先する。
        // MMCSS "Pro Audio" は単なる優先度以上にスケジューリングを優遇される
        // (計測専用スレッドの定石)。取れなければ通常の優先度上げにフォールバック。
        // (前面時の精度に効く。背面の配信制限は OS 側なのでこれでは解けない)
        let mmcss_name: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
        let mut idx = 0u32;
        if windows_sys::Win32::System::Threading::AvSetMmThreadCharacteristicsW(
            mmcss_name.as_ptr(),
            &mut idx,
        )
        .is_null()
        {
            windows_sys::Win32::System::Threading::SetThreadPriority(
                windows_sys::Win32::System::Threading::GetCurrentThread(),
                windows_sys::Win32::System::Threading::THREAD_PRIORITY_ABOVE_NORMAL,
            );
        }
        let name: Vec<u16> = "DevHubRawInput\0".encode_utf16().collect();
        let hinst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd),
            hInstance: hinst,
            lpszClassName: name.as_ptr(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        // メッセージ専用ウィンドウ (HWND_MESSAGE = -3)。
        let hwnd = CreateWindowExW(
            0,
            name.as_ptr(),
            name.as_ptr(),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            (-3isize) as HWND,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        let devs = [
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02, // マウス
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06, // キーボード
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            },
        ];
        RegisterRawInputDevices(
            devs.as_ptr(),
            if keyboard { devs.len() as u32 } else { 1 },
            size_of::<RAWINPUTDEVICE>() as u32,
        );
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    thread_local! {
        /// 生入力の読み取りバッファ (8B アライン、使い回し)。
        static RAW_BUF: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    }

    /// 1 レコードから (hDevice, マウス移動量 |dx|+|dy|, ボタン遷移フラグ)。
    /// キーボード等マウス以外は counts=None (区間・移動量の対象外)。
    fn read_raw(raw: &RAWINPUT) -> (usize, Option<u32>, u32) {
        let hdev = raw.header.hDevice as usize;
        if raw.header.dwType == RIM_TYPEMOUSE {
            let m = unsafe { raw.data.mouse };
            let counts = Some(
                m.lLastX
                    .unsigned_abs()
                    .saturating_add(m.lLastY.unsigned_abs()),
            );
            (hdev, counts, unsafe { m.Anonymous.ulButtons })
        } else {
            (hdev, None, 0)
        }
    }

    fn apply_button_flags(bits: &mut u8, flags: u32) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            RI_MOUSE_BUTTON_1_DOWN, RI_MOUSE_BUTTON_1_UP, RI_MOUSE_BUTTON_2_DOWN,
            RI_MOUSE_BUTTON_2_UP, RI_MOUSE_BUTTON_3_DOWN, RI_MOUSE_BUTTON_3_UP,
            RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP, RI_MOUSE_BUTTON_5_DOWN,
            RI_MOUSE_BUTTON_5_UP,
        };
        for (down, up, bit) in [
            (RI_MOUSE_BUTTON_1_DOWN, RI_MOUSE_BUTTON_1_UP, 0),
            (RI_MOUSE_BUTTON_2_DOWN, RI_MOUSE_BUTTON_2_UP, 1),
            (RI_MOUSE_BUTTON_3_DOWN, RI_MOUSE_BUTTON_3_UP, 2),
            (RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP, 3),
            (RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP, 4),
        ] {
            if flags & down != 0 {
                *bits |= 1 << bit;
            }
            if flags & up != 0 {
                *bits &= !(1 << bit);
            }
        }
    }

    fn push(m: &mut Meter, hdev: usize, counts: Option<u32>, button_flags: u32, now: Instant) {
        if button_flags != 0 {
            apply_button_flags(&mut m.devs.entry(hdev).or_insert_with(Dev::new).buttons, button_flags);
        }
        if let Some(trial) = m.trial.as_mut() {
            if counts.is_some() && (trial.start..trial.end).contains(&now) {
                let us = (now - trial.start).as_micros() as u64;
                trial
                    .devs
                    .entry(hdev)
                    .or_insert_with(Dev::new)
                    .bump(us, counts);
            }
        }
        let epoch = *m.epoch.get_or_insert(now);
        let us = now.duration_since(epoch).as_micros() as u64;
        m.devs.entry(hdev).or_insert_with(Dev::new).bump(us, counts);
    }

    fn on_input(lp: LPARAM) {
        unsafe {
            let hdr_size = size_of::<RAWINPUTHEADER>() as u32;
            let now = Instant::now();
            RAW_BUF.with(|cell| {
                let mut guard = cell.borrow_mut();
                if guard.is_empty() {
                    guard.resize(BUF_LEN, 0);
                }
                let buf = &mut *guard;
                // 1) この WM_INPUT が運んだイベント (GetMessage でキューから抜けるので
                //    バッファ読みには含まれない — 二重カウントしない)。
                let mut size = (buf.len() * 8) as u32;
                let got = GetRawInputData(
                    lp as HRAWINPUT,
                    RID_INPUT,
                    buf.as_mut_ptr().cast(),
                    &mut size,
                    hdr_size,
                );
                if got != u32::MAX && got as usize >= size_of::<RAWINPUTHEADER>() {
                    let mut meter = METER.lock().unwrap_or_else(|p| p.into_inner());
                    let (hdev, counts, buttons) = read_raw(&*(buf.as_ptr() as *const RAWINPUT));
                    push(&mut meter, hdev, counts, buttons, now);
                }
                // 2) キューに溜まった残りをまとめてドレイン (高周波向け公式手順)。
                loop {
                    let mut sz = (buf.len() * 8) as u32;
                    let n = GetRawInputBuffer(buf.as_mut_ptr().cast(), &mut sz, hdr_size);
                    if n == 0 {
                        break;
                    }
                    if n == u32::MAX {
                        use windows_sys::Win32::Foundation::{
                            GetLastError, ERROR_INSUFFICIENT_BUFFER,
                        };
                        if GetLastError() != ERROR_INSUFFICIENT_BUFFER {
                            break;
                        }
                        // バッファ不足 → 必要量か 2 倍で拡張してやり直し。
                        let need = (sz as usize).div_ceil(8);
                        buf.resize(need.max(buf.len() * 2), 0);
                        continue;
                    }
                    let mut p = buf.as_ptr() as *const RAWINPUT;
                    let now = Instant::now();
                    let mut meter = METER.lock().unwrap_or_else(|p| p.into_inner());
                    for _ in 0..n {
                        let raw = &*p;
                        let (hdev, counts, buttons) = read_raw(raw);
                        push(&mut meter, hdev, counts, buttons, now);
                        // NEXTRAWINPUTBLOCK 相当: header.dwSize をポインタ境界に揃えて進む。
                        let adv = (raw.header.dwSize as usize + size_of::<usize>() - 1)
                            & !(size_of::<usize>() - 1);
                        p = (p as *const u8).add(adv) as *const RAWINPUT;
                    }
                }
            });
        }
    }

    /// hDevice → (vid, pid)。デバイス名は `…VID_xxxx&PID_xxxx…`。
    fn vidpid_of(hdev: usize) -> Option<(u16, u16)> {
        unsafe {
            let mut size = 0u32;
            GetRawInputDeviceInfoW(hdev as _, RIDI_DEVICENAME, std::ptr::null_mut(), &mut size);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u16; size as usize];
            if GetRawInputDeviceInfoW(
                hdev as _,
                RIDI_DEVICENAME,
                buf.as_mut_ptr().cast(),
                &mut size,
            ) == u32::MAX
            {
                return None;
            }
            let name = String::from_utf16_lossy(&buf).to_ascii_uppercase();
            let hex = |tag: &str| {
                name.find(tag).and_then(|i| {
                    name.get(i + 4..i + 8)
                        .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                })
            };
            Some((hex("VID_")?, hex("PID_")?))
        }
    }

    pub fn mouse_buttons(vid: u16, pid: u16) -> Option<u8> {
        let mut m = METER.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = None;
        for (&hdev, d) in m.devs.iter_mut() {
            let vp = match d.vidpid {
                Some(v) => v,
                None => {
                    let v = vidpid_of(hdev);
                    d.vidpid = Some(v);
                    v
                }
            };
            if vp == Some((vid, pid)) {
                out = Some(out.unwrap_or(0) | d.buttons);
            }
        }
        out
    }

    pub fn any_mouse_buttons() -> Option<u8> {
        let m = METER.lock().unwrap_or_else(|p| p.into_inner());
        if m.devs.is_empty() {
            None
        } else {
            Some(m.devs.values().fold(0u8, |a, d| a | d.buttons))
        }
    }

    /// 指定 vid/pid のデバイスの計測スナップショット。無ければ None。
    pub fn stats(vid: u16, pid: u16) -> Option<PollStats> {
        let mut m = METER.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let now_us = m
            .epoch
            .map(|e| now.duration_since(e).as_micros() as u64)
            .unwrap_or(0);

        snapshot(&mut m.devs, vid, pid, now_us)
    }

    fn snapshot(
        devs: &mut HashMap<usize, Dev>,
        vid: u16,
        pid: u16,
        now_us: u64,
    ) -> Option<PollStats> {
        let now_ms = now_us / 1000;
        let mut st = PollStats::default();
        let mut merged_buckets: HashMap<u64, u32> = HashMap::new();
        let mut all_gaps: Vec<f32> = Vec::new();
        let mut found = false;

        for (&hdev, d) in devs.iter_mut() {
            let vp = match d.vidpid {
                Some(v) => v,
                None => {
                    let v = vidpid_of(hdev);
                    d.vidpid = Some(v);
                    v
                }
            };
            if vp != Some((vid, pid)) {
                continue;
            }
            found = true;

            // 直近 1 秒のイベント数。
            st.hz += d
                .events
                .iter()
                .filter(|t| now_us.saturating_sub(**t) < (WINDOW_SECS * 1_000_000.) as u64)
                .count() as f32;
            for &(i, c) in &d.buckets {
                *merged_buckets.entry(i).or_default() += c;
            }
            all_gaps.extend(d.gaps.iter().copied());

            // 区間: 確定分は公開 & ピーク計算、進行中はピークと「現在」にだけ使う。
            let mut newest: Option<(u64, Seg, bool)> = None; // (last_ms, seg, is_running)
            for &s in &d.segs {
                let age_ms = now_ms.saturating_sub(s.last_ms);
                if age_ms <= PEAK_WINDOW_MS {
                    st.peak_hz = Some(st.peak_hz.map_or(s.rate_hz(), |p: f32| p.max(s.rate_hz())));
                }
                st.segs.push(SegInfo {
                    rate_hz: s.rate_hz(),
                    span_ms: s.span_ms(),
                    age_s: age_ms as f32 / 1000.,
                });
                if newest.is_none_or(|(t, ..)| s.last_ms > t) {
                    newest = Some((s.last_ms, s, false));
                }
            }
            if d.seg.n > 0 {
                let s = d.seg;
                if s.usable() && now_ms.saturating_sub(s.last_ms) <= PEAK_WINDOW_MS {
                    st.peak_hz = Some(st.peak_hz.map_or(s.rate_hz(), |p: f32| p.max(s.rate_hz())));
                }
                if newest.is_none_or(|(t, ..)| s.last_ms > t) {
                    newest = Some((s.last_ms, s, true));
                }
            }
            // 「現在の区間」= 進行中 ≥60ms かつ新鮮、または確定 ≤1.5s。
            if let Some((last, s, live)) = newest {
                let ok = if live {
                    s.span_ms() >= 60 && now_ms.saturating_sub(last) <= 300
                } else {
                    s.usable() && now_ms.saturating_sub(last) <= SEG_FRESH_MS
                };
                if ok && st.seg_hz.is_none_or(|rate| s.rate_hz() > rate) {
                    st.seg_hz = Some(s.rate_hz());
                    st.seg_span_ms = Some(s.span_ms());
                    let secs = (s.span_ms() as f32 / 1000.).max(0.001);
                    st.counts_per_sec = Some(s.counts as f32 / secs);
                    st.low_frac = Some(s.low as f32 / s.n.max(1) as f32);
                }
            }
        }
        if !found {
            return None;
        }
        st.hz /= WINDOW_SECS;

        // 履歴グラフ (バケットを連番に展開)。
        let hi = now_ms / BUCKET_MS;
        let lo = hi.saturating_sub(HIST_LEN as u64 - 1);
        st.hist = (lo..=hi)
            .map(|i| *merged_buckets.get(&i).unwrap_or(&0) as f32 / (BUCKET_MS as f32 / 1000.))
            .collect();

        // 間隔統計 (0.05ms 未満は dispatch 合体ノイズとして除外 — 8K の真間隔は 0.125ms)。
        const FLOOR_MS: f32 = 0.05;
        all_gaps.retain(|&g| g >= FLOOR_MS);
        if !all_gaps.is_empty() {
            st.min_gap_ms = all_gaps.iter().copied().reduce(f32::min);
        }
        if all_gaps.len() >= 8 {
            all_gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let pct = |q: f32| all_gaps[((all_gaps.len() - 1) as f32 * q) as usize];
            st.gap_stats = Some((pct(0.5), pct(0.95)));
        }
        st.segs.sort_by(|a, b| {
            a.age_s
                .partial_cmp(&b.age_s)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Some(st)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn trial_excludes_keyboard_and_events_outside_window() {
            use std::time::Duration;
            let start = Instant::now();
            let end = start + Duration::from_secs(5);
            let mut meter = Meter {
                epoch: None,
                devs: HashMap::new(),
                trial: Some(Trial {
                    start,
                    end,
                    devs: HashMap::new(),
                }),
            };
            push(&mut meter, 1, Some(2), 0, start - Duration::from_millis(1));
            push(&mut meter, 1, Some(2), 0, start);
            push(&mut meter, 2, None, 0, start);
            push(&mut meter, 1, Some(2), 0, end);
            let trial = meter.trial.unwrap();
            assert_eq!(trial.devs.len(), 1);
            assert_eq!(trial.devs[&1].events, [0]);
            assert_eq!(meter.devs[&1].seg.n, 1);
        }

        #[test]
        fn raw_button_flags_update_bits() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                RI_MOUSE_BUTTON_1_DOWN, RI_MOUSE_BUTTON_1_UP, RI_MOUSE_BUTTON_4_DOWN,
                RI_MOUSE_BUTTON_4_UP,
            };
            let mut bits = 0;
            apply_button_flags(&mut bits, RI_MOUSE_BUTTON_1_DOWN | RI_MOUSE_BUTTON_4_DOWN);
            assert_eq!(bits, 0b1001);
            apply_button_flags(&mut bits, RI_MOUSE_BUTTON_1_UP);
            assert_eq!(bits, 0b1000);
            apply_button_flags(&mut bits, RI_MOUSE_BUTTON_4_UP);
            assert_eq!(bits, 0);
        }

        #[test]
        fn continuous_125hz_motion_forms_a_segment() {
            let mut dev = Dev::new();
            for i in 0..=125 {
                dev.bump(i * 8000, Some(1));
            }
            assert!(dev.seg.usable());
            assert_eq!(dev.seg.rate_hz(), 125.);
        }

        #[test]
        fn high_rate_motion_then_long_idle() {
            let mut dev = Dev::new();
            dev.vidpid = Some(Some((1, 2)));
            let mut meter = Meter {
                epoch: None,
                trial: None,
                devs: HashMap::from([(1, dev)]),
            };
            meter.devs.get_mut(&1).unwrap().bump(0, Some(1));
            assert!(snapshot(&mut meter.devs, 1, 2, 0)
                .unwrap()
                .peak_hz
                .is_none());
            for i in 1..=8000 {
                meter.devs.get_mut(&1).unwrap().bump(i * 125, Some(1));
            }
            let st = snapshot(&mut meter.devs, 1, 2, 1_000_000).unwrap();
            assert_eq!(st.hz, 8000.);
            assert_eq!(st.gap_stats, Some((0.125, 0.125)));
            assert_eq!(st.peak_hz, Some(8000.));
            let later = 3_600_000_000;
            meter.devs.get_mut(&1).unwrap().bump(later, Some(1));
            let st = snapshot(&mut meter.devs, 1, 2, later).unwrap();
            assert_eq!(st.hist.len(), HIST_LEN);
            assert_eq!(st.hz, 1.);
            assert!(st.peak_hz.is_none());
        }
    }
}

#[cfg(windows)]
pub use imp::{any_mouse_buttons, begin_trial, finish_trial, mouse_buttons, start, start_mouse, stats};

#[cfg(not(windows))]
mod stub {
    use super::PollStats;
    pub fn begin_trial(_: std::time::Instant, _: std::time::Instant) {}
    pub fn finish_trial(_: u16, _: u16) -> Option<PollStats> {
        None
    }
    pub fn start() {}
    pub fn start_mouse() {}
    pub fn stats(_vid: u16, _pid: u16) -> Option<PollStats> {
        None
    }
    pub fn mouse_buttons(_vid: u16, _pid: u16) -> Option<u8> {
        None
    }
    pub fn any_mouse_buttons() -> Option<u8> {
        None
    }
}

#[cfg(not(windows))]
pub use stub::{any_mouse_buttons, begin_trial, finish_trial, mouse_buttons, start, start_mouse, stats};
