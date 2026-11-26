//! HID++ 2.0 機能ごとの呼び出し。関数番号とバイト配置は Solaar / libratbag の実装と
//! 突き合わせたもの (各関数の doc に出典を書く)。**実機 (PRO 2 LIGHTSPEED) での検証は未了**。

use crate::{Error, Io, Result, Session};

pub const ROOT: u16 = 0x0000;
pub const FEATURE_SET: u16 = 0x0001;
pub const DEVICE_FW_VERSION: u16 = 0x0003;
pub const DEVICE_NAME: u16 = 0x0005;
pub const BATTERY_STATUS: u16 = 0x1000;
pub const BATTERY_VOLTAGE: u16 = 0x1001;
pub const UNIFIED_BATTERY: u16 = 0x1004;
pub const REPROG_CONTROLS_V4: u16 = 0x1B04;
pub const ADJUSTABLE_DPI: u16 = 0x2201;
pub const EXTENDED_ADJUSTABLE_DPI: u16 = 0x2202;
pub const REPORT_RATE: u16 = 0x8060;
pub const EXTENDED_REPORT_RATE: u16 = 0x8061;
pub const COLOR_LED_EFFECTS: u16 = 0x8070;
pub const RGB_EFFECTS: u16 = 0x8071;
pub const ONBOARD_PROFILES: u16 = 0x8100;

/// 機能 ID の人間向け名 (診断出力用)。
pub fn feature_name(id: u16) -> &'static str {
    match id {
        ROOT => "Root",
        FEATURE_SET => "FeatureSet",
        0x0002 => "FeatureInfo",
        DEVICE_FW_VERSION => "DeviceFwVersion",
        0x0004 => "DeviceUnitId",
        DEVICE_NAME => "DeviceName",
        0x0007 => "DeviceFriendlyName",
        BATTERY_STATUS => "BatteryStatus",
        BATTERY_VOLTAGE => "BatteryVoltage",
        UNIFIED_BATTERY => "UnifiedBattery",
        REPROG_CONTROLS_V4 => "ReprogControlsV4",
        0x1F20 => "AdcMeasurement",
        ADJUSTABLE_DPI => "AdjustableDpi",
        0x2202 => "ExtendedAdjustableDpi",
        REPORT_RATE => "ReportRate",
        EXTENDED_REPORT_RATE => "ExtendedReportRate",
        COLOR_LED_EFFECTS => "ColorLedEffects",
        RGB_EFFECTS => "RgbEffects",
        ONBOARD_PROFILES => "OnboardProfiles",
        _ => "?",
    }
}

// ------------------------------------------------------------------ 名前 / FW

/// DeviceName (0x0005): func0 → `[len]`、func1 `[offset]` → 最大 16 文字。
pub fn device_name<I: Io>(s: &mut Session<I>) -> Result<String> {
    let len = s.call(DEVICE_NAME, 0, &[])?[0] as usize;
    let mut name = Vec::new();
    while name.len() < len {
        let chunk = s.call(DEVICE_NAME, 1, &[name.len() as u8])?;
        let take = (len - name.len()).min(chunk.len());
        if take == 0 {
            return Err(Error::Protocol("empty DeviceName chunk".into()));
        }
        name.extend_from_slice(&chunk[..take]);
    }
    Ok(String::from_utf8_lossy(&name).trim_end_matches('\0').to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FwEntry {
    /// 0=Firmware 1=Bootloader 2=Hardware 3=Other。
    pub kind: u8,
    pub name: String,
    pub version: String,
}

/// DeviceFwVersion (0x0003): func0 → `[count]`、func1 `[i]` →
/// `[kind, name[3], major(BCD), minor(BCD), build(u16 BE), …]` (Solaar `get_firmware`)。
pub fn firmware<I: Io>(s: &mut Session<I>) -> Result<Vec<FwEntry>> {
    let count = s.call(DEVICE_FW_VERSION, 0, &[])?[0];
    let mut out = Vec::new();
    for i in 0..count {
        let p = s.call(DEVICE_FW_VERSION, 1, &[i])?;
        if p.len() < 8 {
            continue;
        }
        let kind = p[0] & 0x0F;
        match kind {
            0 | 1 => {
                let build = u16::from_be_bytes([p[6], p[7]]);
                let mut version = format!("{:02X}.{:02X}", p[4], p[5]);
                if build != 0 {
                    version.push_str(&format!(".B{build:04X}"));
                }
                out.push(FwEntry {
                    kind,
                    name: String::from_utf8_lossy(&p[1..4]).into_owned(),
                    version,
                });
            }
            2 => out.push(FwEntry {
                kind,
                name: String::new(),
                version: p[1].to_string(),
            }),
            _ => {}
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ 電池

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: Option<u8>,
    /// 充電中、または満充電で給電中。
    pub charging: bool,
    pub voltage_mv: Option<u16>,
}

/// BatteryVoltage の電圧→残量近似 (Solaar `estimate_battery_level_percentage`、線形補間)。
pub fn percent_from_voltage(mv: u16) -> u8 {
    const T: [(u16, u8); 12] = [
        (4186, 100),
        (4067, 90),
        (3989, 80),
        (3922, 70),
        (3859, 60),
        (3811, 50),
        (3778, 40),
        (3751, 30),
        (3717, 20),
        (3671, 10),
        (3646, 5),
        (3500, 0),
    ];
    if mv >= T[0].0 {
        return 100;
    }
    if mv <= T[T.len() - 1].0 {
        return 0;
    }
    for w in T.windows(2) {
        let (vh, ph) = (w[0].0 as f32, w[0].1 as f32);
        let (vl, pl) = (w[1].0 as f32, w[1].1 as f32);
        if (mv as f32) >= vl && (mv as f32) <= vh {
            return (pl + (ph - pl) * (mv as f32 - vl) / (vh - vl)).round() as u8;
        }
    }
    0
}

/// BatteryStatus の status バイト: 0 放電 1 充電 2 ほぼ満充電 3 満充電 4 低速充電 5 異常 6 熱異常。
fn charging_from_status(st: u8) -> bool {
    matches!(st, 1..=4)
}

/// 電池。持つ機能を UnifiedBattery → BatteryVoltage → BatteryStatus の順に試す。
/// どれも無ければ `Ok(None)`。
/// - UnifiedBattery 0x1004: func1 → `[percent, level_mask, status, ext]`
/// - BatteryVoltage 0x1001: func0 → `[mV BE, flags]` (flags bit7=充電中)
/// - BatteryStatus  0x1000: func0 → `[percent, next, status]`
pub fn battery<I: Io>(s: &mut Session<I>) -> Result<Option<Battery>> {
    if s.has(UNIFIED_BATTERY)? {
        let p = s.call(UNIFIED_BATTERY, 1, &[])?;
        return Ok(Some(Battery {
            percent: (p[0] != 0).then_some(p[0].min(100)),
            charging: charging_from_status(p[2]),
            voltage_mv: None,
        }));
    }
    if s.has(BATTERY_VOLTAGE)? {
        let p = s.call(BATTERY_VOLTAGE, 0, &[])?;
        let mv = u16::from_be_bytes([p[0], p[1]]);
        return Ok(Some(Battery {
            percent: Some(percent_from_voltage(mv)),
            charging: p[2] & 0x80 != 0,
            voltage_mv: Some(mv),
        }));
    }
    if s.has(BATTERY_STATUS)? {
        let p = s.call(BATTERY_STATUS, 0, &[])?;
        return Ok(Some(Battery {
            percent: (p[0] != 0).then_some(p[0].min(100)),
            charging: charging_from_status(p[2]),
            voltage_mv: None,
        }));
    }
    Ok(None)
}

// ------------------------------------------------------------------ DPI

/// DPI リストの 1 区間。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DpiSeg {
    One(u16),
    /// `from` の次から `to` まで `step` 刻み (`from` 自身は直前の `One`)。
    Range { from: u16, to: u16, step: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DpiList {
    pub segs: Vec<DpiSeg>,
}

impl DpiList {
    /// 応答の u16 列 (BE) から作る。`0x0000` で終端。上位 3bit=0b111 は刻み指定で、
    /// 次の値が上限 (Solaar `produce_dpi_list` / libratbag の 0xE000 処理)。
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        let mut segs: Vec<DpiSeg> = Vec::new();
        let mut last: Option<u16> = None;
        let mut i = 0;
        while i < words.len() {
            let v = words[i];
            if v == 0 {
                break;
            }
            if v >> 13 == 0b111 {
                let step = v & 0x1FFF;
                let (Some(from), Some(&to)) = (last, words.get(i + 1)) else {
                    return Err(Error::Protocol("bad DPI range".into()));
                };
                if step == 0 || to <= from {
                    return Err(Error::Protocol("bad DPI range".into()));
                }
                segs.push(DpiSeg::Range { from, to, step });
                last = Some(to);
                i += 2;
            } else {
                segs.push(DpiSeg::One(v));
                last = Some(v);
                i += 1;
            }
        }
        if segs.is_empty() {
            return Err(Error::Protocol("empty DPI list".into()));
        }
        Ok(Self { segs })
    }

    pub fn min(&self) -> u16 {
        match self.segs[0] {
            DpiSeg::One(v) => v,
            DpiSeg::Range { from, .. } => from,
        }
    }

    pub fn max(&self) -> u16 {
        match *self.segs.last().unwrap() {
            DpiSeg::One(v) => v,
            DpiSeg::Range { to, .. } => to,
        }
    }

    /// UI の刻み: 範囲区間があればその step、無ければ値の最小差。
    pub fn step(&self) -> u16 {
        for s in &self.segs {
            if let DpiSeg::Range { step, .. } = s {
                return *step;
            }
        }
        let mut v: Vec<u16> = self
            .segs
            .iter()
            .filter_map(|s| match s {
                DpiSeg::One(v) => Some(*v),
                _ => None,
            })
            .collect();
        v.sort_unstable();
        v.windows(2).map(|w| w[1] - w[0]).min().unwrap_or(1).max(1)
    }

    pub fn contains(&self, dpi: u16) -> bool {
        self.segs.iter().enumerate().any(|(i, s)| match *s {
            DpiSeg::One(v) => v == dpi,
            DpiSeg::Range { from, to, step } => {
                // 区間の先頭 (`from`) は直前の One が持つ。
                let _ = i;
                dpi >= from && dpi <= to && (dpi - from) % step == 0
            }
        })
    }

    /// 最も近い有効値へ丸める。
    pub fn snap(&self, dpi: u16) -> u16 {
        let mut best = self.min();
        let mut best_d = u32::MAX;
        for s in &self.segs {
            let cand = match *s {
                DpiSeg::One(v) => v,
                DpiSeg::Range { from, to, step } => {
                    let n = (dpi.clamp(from, to) - from + step / 2) / step;
                    (from + n * step).min(to)
                }
            };
            let d = (cand as i32 - dpi as i32).unsigned_abs();
            if d < best_d {
                best = cand;
                best_d = d;
            }
        }
        best
    }
}

/// DPI リストを chunk を進めながら集める。`skip` は応答先頭の読み飛ばしバイト数
/// (0x2201: sensor の 1 バイト / 0x2202: sensor・方向・chunk の 3 バイト)。
/// 終端 0x0000 が出るまで (最大 8 回)。
fn collect_dpi_list<I: Io>(
    s: &mut Session<I>,
    feature: u16,
    function: u8,
    skip: usize,
    params: impl Fn(u8) -> Vec<u8>,
) -> Result<DpiList> {
    // 応答ごとの本体は奇数バイト (16-3=13 など) で、u16 が chunk の境目をまたぐ。
    // 端数を捨てずに全 chunk を連結してから u16 に区切る (Solaar `produce_dpi_list` と同じ)。
    // 実機 (PRO 2) では端数を捨てると 2 chunk 目以降が 1 バイトずれて値が壊れた。
    let mut bytes = Vec::new();
    for chunk in 0..8u8 {
        let p = s.call(feature, function, &params(chunk))?;
        let body = p.get(skip..).unwrap_or(&[]);
        if body.is_empty() {
            break;
        }
        bytes.extend_from_slice(body);
        // 連結済みの u16 列に終端 (0x0000) が出たら終わり。
        if bytes.chunks_exact(2).any(|c| c == [0, 0]) {
            break;
        }
    }
    DpiList::parse(&bytes)
}

/// ExtendedAdjustableDpi (0x2202) func1 `[0]` → `[.., .., flags]` (bit0=Y 軸あり, bit1=LOD あり)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtDpiCaps {
    pub y: bool,
    pub lod: bool,
}

pub fn ext_dpi_caps<I: Io>(s: &mut Session<I>) -> Result<ExtDpiCaps> {
    let p = s.call(EXTENDED_ADJUSTABLE_DPI, 1, &[0])?;
    let flags = *p.get(2).ok_or_else(|| Error::Protocol("short ext DPI caps".into()))?;
    Ok(ExtDpiCaps {
        y: flags & 1 != 0,
        lod: flags & 2 != 0,
    })
}

/// DPI リスト (X 軸)。AdjustableDpi (0x2201) を優先、無ければ ExtendedAdjustableDpi (0x2202)。
/// - 0x2201 func1 `[sensor, 0, chunk]` → `[sensor, u16 BE…]`
/// - 0x2202 func2 `[0, 方向(0=X), chunk]` → `[0, 方向, chunk, u16 BE…]` (Solaar `produce_dpi_list`)
pub fn dpi_list<I: Io>(s: &mut Session<I>, sensor: u8) -> Result<DpiList> {
    if s.has(ADJUSTABLE_DPI)? {
        return collect_dpi_list(s, ADJUSTABLE_DPI, 1, 1, |c| vec![sensor, 0, c]);
    }
    if s.has(EXTENDED_ADJUSTABLE_DPI)? {
        return collect_dpi_list(s, EXTENDED_ADJUSTABLE_DPI, 2, 3, |c| vec![0, 0, c]);
    }
    Err(Error::NoFeature(ADJUSTABLE_DPI))
}

/// 現在の DPI (X 軸)。値が 0 なら既定値を返す。
/// - 0x2201 func2 `[sensor]` → `[sensor, dpi BE, default BE]`
/// - 0x2202 func5 `[0]` → `[sensor, X BE, X既定 BE, Y BE, Y既定 BE, LOD]`
pub fn get_dpi<I: Io>(s: &mut Session<I>, sensor: u8) -> Result<u16> {
    let (p, at) = if s.has(ADJUSTABLE_DPI)? {
        (s.call(ADJUSTABLE_DPI, 2, &[sensor])?, 1)
    } else {
        (s.call(EXTENDED_ADJUSTABLE_DPI, 5, &[0])?, 1)
    };
    if p.len() < at + 4 {
        return Err(Error::Protocol("short DPI reply".into()));
    }
    let cur = u16::from_be_bytes([p[at], p[at + 1]]);
    Ok(if cur == 0 {
        u16::from_be_bytes([p[at + 2], p[at + 3]])
    } else {
        cur
    })
}

/// DPI を設定する。
/// - 0x2201 func3 `[sensor, dpi BE]`
/// - 0x2202 func6 `[0, X BE, Y BE, LOD]`。Y 軸があれば X と同じ値 (連動) にし、LOD は現状維持。
pub fn set_dpi<I: Io>(s: &mut Session<I>, sensor: u8, dpi: u16) -> Result<()> {
    if s.has(ADJUSTABLE_DPI)? {
        s.call(ADJUSTABLE_DPI, 3, &[sensor, (dpi >> 8) as u8, dpi as u8])?;
        return Ok(());
    }
    let caps = ext_dpi_caps(s)?;
    let cur = s.call(EXTENDED_ADJUSTABLE_DPI, 5, &[0])?;
    let lod = if caps.lod { cur.get(9).copied().unwrap_or(0) } else { 0 };
    let (hi, lo) = ((dpi >> 8) as u8, dpi as u8);
    let (yh, yl) = if caps.y { (hi, lo) } else { (0, 0) };
    s.call(EXTENDED_ADJUSTABLE_DPI, 6, &[0, hi, lo, yh, yl, lod])?;
    Ok(())
}

// ------------------------------------------------------------------ ポーリングレート

/// ExtendedReportRate (0x8061) の index → Hz。
pub const EXT_RATE_HZ: [u32; 7] = [125, 250, 500, 1000, 2000, 4000, 8000];

/// 現在値を Hz で。ExtendedReportRate (0x8061) を優先、無ければ ReportRate (0x8060)。
/// - 0x8061: func1 → `[flags BE16]` (bit i = index i)、func2 → `[index]`、func3 `[index]` で設定
/// - 0x8060: func0 → `[flags]` (bit i = (i+1) ms)、func1 → `[ms]`、func2 `[ms]` で設定
pub fn report_rates<I: Io>(s: &mut Session<I>) -> Result<Vec<u32>> {
    if s.has(EXTENDED_REPORT_RATE)? {
        let p = s.call(EXTENDED_REPORT_RATE, 1, &[])?;
        let flags = u16::from_be_bytes([p[0], p[1]]);
        return Ok((0..7)
            .filter(|i| flags & (1 << i) != 0)
            .map(|i| EXT_RATE_HZ[i])
            .collect());
    }
    let p = s.call(REPORT_RATE, 0, &[])?;
    let mut v: Vec<u32> = (0..8)
        .filter(|i| p[0] & (1 << i) != 0)
        .map(|i| 1000 / (i as u32 + 1))
        .collect();
    v.sort_unstable();
    Ok(v)
}

pub fn report_rate<I: Io>(s: &mut Session<I>) -> Result<u32> {
    if s.has(EXTENDED_REPORT_RATE)? {
        let p = s.call(EXTENDED_REPORT_RATE, 2, &[])?;
        return EXT_RATE_HZ
            .get(p[0] as usize)
            .copied()
            .ok_or_else(|| Error::Protocol(format!("unknown report rate index {}", p[0])));
    }
    let p = s.call(REPORT_RATE, 1, &[])?;
    if p[0] == 0 {
        return Err(Error::Protocol("report rate 0 ms".into()));
    }
    Ok(1000 / p[0] as u32)
}

pub fn set_report_rate<I: Io>(s: &mut Session<I>, hz: u32) -> Result<()> {
    if s.has(EXTENDED_REPORT_RATE)? {
        let idx = EXT_RATE_HZ
            .iter()
            .position(|&h| h == hz)
            .ok_or_else(|| Error::Protocol(format!("unsupported report rate {hz} Hz")))?;
        s.call(EXTENDED_REPORT_RATE, 3, &[idx as u8])?;
        return Ok(());
    }
    if hz == 0 || 1000 % hz != 0 {
        return Err(Error::Protocol(format!("unsupported report rate {hz} Hz")));
    }
    s.call(REPORT_RATE, 2, &[(1000 / hz) as u8])?;
    Ok(())
}

// ------------------------------------------------------------------ オンボードモード

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardMode {
    /// 機器内蔵プロファイルが DPI・ポーリング・ボタンを決める。
    Onboard,
    /// ホスト (ソフト) 側の設定が有効。
    Host,
}

/// OnboardProfiles (0x8100) func2 → `[mode]`、func1 `[mode]` で設定。1=Onboard / 2=Host
/// (Solaar `OnboardProfiles` 設定の `read 0x20 / write 0x10`)。
/// Solaar は DPI / ポーリング設定の説明に「Onboard Profiles を Disable (=Host) にしないと
/// 効かないことがある」と明記している。
pub fn onboard_mode<I: Io>(s: &mut Session<I>) -> Result<OnboardMode> {
    match s.call(ONBOARD_PROFILES, 2, &[])?[0] {
        1 => Ok(OnboardMode::Onboard),
        2 => Ok(OnboardMode::Host),
        v => Err(Error::Protocol(format!("unknown onboard mode {v}"))),
    }
}

pub fn set_onboard_mode<I: Io>(s: &mut Session<I>, m: OnboardMode) -> Result<()> {
    let v = match m {
        OnboardMode::Onboard => 1,
        OnboardMode::Host => 2,
    };
    s.call(ONBOARD_PROFILES, 1, &[v])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use crate::DEV_DIRECT;

    /// feature id → index の表を持つ機器スタブ。`f(index, func, params)` が機能の応答。
    fn device(
        table: &'static [(u16, u8)],
        mut f: impl FnMut(u8, u8, &[u8]) -> Option<Vec<u8>> + Send + 'static,
    ) -> Session<Mock> {
        let m = Mock::with(move |req| {
            if req[2] == 0 && req[3] >> 4 == 0 {
                let id = u16::from_be_bytes([req[4], req[5]]);
                let idx = table.iter().find(|(i, _)| *i == id).map(|(_, x)| *x).unwrap_or(0);
                return vec![reply(req, &[idx, 0, 1])];
            }
            match f(req[2], req[3] >> 4, &req[4..]) {
                Some(p) => vec![reply(req, &p)],
                None => vec![],
            }
        });
        Session::new(m, DEV_DIRECT)
    }

    #[test]
    fn dpi_list_range_form() {
        // [min=100, 0xE000|50 (step), max=44000, 0] — Pro 系で想定される形式
        let l = DpiList::parse(&[0x00, 0x64, 0xE0, 0x32, 0xAB, 0xE0, 0, 0]).unwrap();
        assert_eq!((l.min(), l.max(), l.step()), (100, 44000, 50));
        assert!(l.contains(800) && l.contains(44000) && !l.contains(810));
        assert_eq!(l.snap(823), 800);
        assert_eq!(l.snap(826), 850);
        assert_eq!(l.snap(50), 100);
        assert_eq!(l.snap(60000), 44000);
    }

    #[test]
    fn dpi_list_discrete_form() {
        let l = DpiList::parse(&[0x03, 0x20, 0x06, 0x40, 0x0C, 0x80, 0, 0]).unwrap();
        assert_eq!((l.min(), l.max(), l.step()), (800, 3200, 800));
        assert_eq!(l.snap(1000), 800);
        assert_eq!(l.snap(2000), 1600);
    }

    #[test]
    fn dpi_list_rejects_garbage() {
        assert!(DpiList::parse(&[0, 0]).is_err());
        assert!(DpiList::parse(&[0xE0, 0x32, 0x10, 0x00]).is_err()); // 範囲が先頭
        assert!(DpiList::parse(&[0x00, 0x64, 0xE0, 0x00, 0x10, 0x00]).is_err()); // step 0
    }

    #[test]
    fn dpi_get_set_roundtrip_and_default_fallback() {
        let mut s = device(&[(ADJUSTABLE_DPI, 7)], |idx, func, p| {
            assert_eq!(idx, 7);
            match func {
                1 => Some(vec![0, 0x00, 0x64, 0xE0, 0x32, 0xAB, 0xE0, 0, 0]),
                2 => Some(vec![0, 0x00, 0x00, 0x06, 0x40]), // 現在値 0 → 既定 1600
                3 => {
                    assert_eq!(p[..3], [0, 0x03, 0x20]); // 800
                    Some(vec![])
                }
                _ => None,
            }
        });
        let l = dpi_list(&mut s, 0).unwrap();
        assert_eq!(l.max(), 44000);
        assert_eq!(get_dpi(&mut s, 0).unwrap(), 1600);
        set_dpi(&mut s, 0, 800).unwrap();
    }

    #[test]
    fn extended_dpi_list_get_set_keep_lod_and_link_y() {
        let writes = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let w = writes.clone();
        let mut s = device(&[(EXTENDED_ADJUSTABLE_DPI, 11)], move |idx, func, p| {
            assert_eq!(idx, 11);
            match func {
                1 => Some(vec![0, 0, 0x03]), // Y あり・LOD あり
                2 => {
                    assert_eq!(&p[..3], &[0, 0, 0]);
                    // 先頭 3 バイト (sensor, 方向, chunk) + 100, step 50, 44000, 終端
                    Some(vec![0, 0, 0, 0x00, 0x64, 0xE0, 0x32, 0xAB, 0xE0, 0, 0])
                }
                5 => Some(vec![0, 0x06, 0x40, 0x03, 0x20, 0x06, 0x40, 0x03, 0x20, 0x02]),
                6 => {
                    w.lock().unwrap().push(p[..6].to_vec());
                    Some(vec![])
                }
                _ => None,
            }
        });
        let l = dpi_list(&mut s, 0).unwrap();
        assert_eq!((l.min(), l.max(), l.step()), (100, 44000, 50));
        assert_eq!(get_dpi(&mut s, 0).unwrap(), 1600);
        set_dpi(&mut s, 0, 800).unwrap();
        // [sensor, X=800, Y=800, LOD=2 (現状維持)]
        assert_eq!(writes.lock().unwrap()[0], vec![0, 0x03, 0x20, 0x03, 0x20, 0x02]);
    }

    #[test]
    fn extended_dpi_list_spanning_chunks_stays_aligned() {
        // 実機で壊れた再現: u16 が chunk の境目 (13 バイト) をまたぐ長いリスト。
        let words: [u16; 14] = [
            100, 0xE001, 200, 0xE002, 500, 0xE005, 12000, 0xE064, 20000, 0xE07D, 32000, 0xE0C8,
            44000, 0,
        ];
        let stream: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
        let mut s = device(&[(EXTENDED_ADJUSTABLE_DPI, 11)], move |_, func, p| {
            assert_eq!(func, 2);
            let c = p[2] as usize;
            let part = stream.get(c * 13..(c * 13 + 13).min(stream.len()))?;
            let mut out = vec![0, 0, p[2]];
            out.extend_from_slice(part);
            Some(out)
        });
        let l = dpi_list(&mut s, 0).unwrap();
        assert_eq!((l.min(), l.max()), (100, 44000));
        assert_eq!(l.snap(823), 825); // 500→12000 は 5 刻み
        assert_eq!(l.snap(12040), 12000); // 12000→20000 は 100 刻み
        assert!(l.contains(20125) && !l.contains(20100)); // 20000→32000 は 125 刻み
        assert_eq!(l.snap(60000), 44000);
    }

    #[test]
    fn report_rate_extended_preferred_and_8k() {
        let mut s = device(
            &[(REPORT_RATE, 5), (EXTENDED_REPORT_RATE, 6)],
            |idx, func, p| {
                assert_eq!(idx, 6, "0x8061 must win over 0x8060");
                match func {
                    1 => Some(vec![0x00, 0x7F]), // 7 本全部
                    2 => Some(vec![3]),          // 1000Hz
                    3 => {
                        assert_eq!(p[0], 6); // 8000Hz → index 6
                        Some(vec![])
                    }
                    _ => None,
                }
            },
        );
        assert_eq!(
            report_rates(&mut s).unwrap(),
            vec![125, 250, 500, 1000, 2000, 4000, 8000]
        );
        assert_eq!(report_rate(&mut s).unwrap(), 1000);
        set_report_rate(&mut s, 8000).unwrap();
        assert!(set_report_rate(&mut s, 3000).is_err());
    }

    #[test]
    fn report_rate_legacy_ms() {
        let mut s = device(&[(REPORT_RATE, 5)], |_, func, p| match func {
            0 => Some(vec![0b0000_1011]), // 1ms, 2ms, 4ms
            1 => Some(vec![2]),
            2 => {
                assert_eq!(p[0], 4); // 250Hz → 4ms
                Some(vec![])
            }
            _ => None,
        });
        assert_eq!(report_rates(&mut s).unwrap(), vec![250, 500, 1000]);
        assert_eq!(report_rate(&mut s).unwrap(), 500);
        set_report_rate(&mut s, 250).unwrap();
    }

    #[test]
    fn battery_unified_voltage_and_none() {
        let mut s = device(&[(UNIFIED_BATTERY, 4)], |_, func, _| {
            assert_eq!(func, 1);
            Some(vec![73, 4, 1, 0]) // 73%, good, recharging
        });
        assert_eq!(
            battery(&mut s).unwrap(),
            Some(Battery { percent: Some(73), charging: true, voltage_mv: None })
        );

        let mut s = device(&[(BATTERY_VOLTAGE, 4)], |_, func, _| {
            assert_eq!(func, 0);
            Some(vec![0x0F, 0xC3, 0x00]) // 4035 mV 放電中
        });
        let b = battery(&mut s).unwrap().unwrap();
        assert!(!b.charging && b.voltage_mv == Some(4035));
        assert!((80..=90).contains(&b.percent.unwrap()));

        let mut s = device(&[], |_, _, _| None);
        assert_eq!(battery(&mut s).unwrap(), None);
    }

    #[test]
    fn voltage_table_edges() {
        assert_eq!(percent_from_voltage(4300), 100);
        assert_eq!(percent_from_voltage(3400), 0);
        assert_eq!(percent_from_voltage(3811), 50);
    }

    #[test]
    fn firmware_and_name_decode() {
        let mut s = device(&[(DEVICE_FW_VERSION, 2), (DEVICE_NAME, 3)], |idx, func, p| {
            match (idx, func) {
                (2, 0) => Some(vec![2]),
                (2, 1) if p[0] == 0 => {
                    Some(vec![0, b'R', b'B', b'M', 0x12, 0x03, 0x00, 0x2A, 1])
                }
                (2, 1) => Some(vec![2, 9]), // ハードウェア rev 9
                (3, 0) => Some(vec![5]),
                (3, 1) => Some(b"PRO 2\0\0\0".to_vec()),
                _ => None,
            }
        });
        let fw = firmware(&mut s).unwrap();
        assert_eq!(
            fw,
            vec![
                FwEntry { kind: 0, name: "RBM".into(), version: "12.03.B002A".into() },
                FwEntry { kind: 2, name: String::new(), version: "9".into() },
            ]
        );
        assert_eq!(device_name(&mut s).unwrap(), "PRO 2");
    }

    #[test]
    fn onboard_mode_roundtrip() {
        let mut s = device(&[(ONBOARD_PROFILES, 9)], |_, func, p| match func {
            2 => Some(vec![1]),
            1 => {
                assert_eq!(p[0], 2);
                Some(vec![])
            }
            _ => None,
        });
        assert_eq!(onboard_mode(&mut s).unwrap(), OnboardMode::Onboard);
        set_onboard_mode(&mut s, OnboardMode::Host).unwrap();
    }
}
