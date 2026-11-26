//! Logitech HID++ 2.0 プロトコル層。HID バックエンド (hidapi) には依存しない。
//!
//! 仕様の出典は公開されている実装との突き合わせ:
//! - Solaar (`lib/logitech_receiver/hidpp20*.py`, `settings_templates.py`)
//! - libratbag (`src/hidpp20.[ch]`)
//! - Linux カーネル `hid-logitech-hidpp.c` (PRO 2 LIGHTSPEED の VID/PID)
//!
//! 層の分担:
//! - [`Io`] … 1 フレームの送受信 (hidapi への薄い橋渡しはドライバ側)
//! - [`Session`] … 要求/応答の突き合わせ (swid)・エラーフレーム・機能 ID → index 解決
//! - [`features`] … 機能ごとの関数 (DPI / ポーリング / 電池 / FW / オンボードモード)
//!
//! 要求は常に long report (0x11, 20B) で送る。HID++ 2.0 機器はどれも long を受ける。

pub mod features;

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
pub const REPORT_VERY_LONG: u8 = 0x12;
pub const LONG_LEN: usize = 20;

/// 有線直結 (レシーバーを介さない) 機器のデバイス番号。
pub const DEV_DIRECT: u8 = 0xFF;
/// このアプリの Software ID (1..=15、0 は機器発の通知用に予約)。
/// Solaar=0x0B と衝突しないよう別の値にする。
pub const SOFTWARE_ID: u8 = 0x0D;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Io(String),
    /// 期限内に応答が無い (無線機のスリープ含む)。
    Timeout,
    /// HID++ 1.0 エラー (0x8F)。レシーバー本体や 1.0 機器が 2.0 要求に返す。
    Hidpp10(u8),
    /// HID++ 2.0 エラー (feature index 0xFF)。
    Hidpp20(ErrorCode),
    Protocol(String),
    /// この機器は機能を持たない。
    NoFeature(u16),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(s) => write!(f, "io: {s}"),
            Error::Timeout => write!(f, "timeout"),
            Error::Hidpp10(c) => write!(f, "HID++1.0 error {c:#04x}"),
            Error::Hidpp20(c) => write!(f, "HID++2.0 error {c:?}"),
            Error::Protocol(s) => write!(f, "protocol: {s}"),
            Error::NoFeature(id) => write!(f, "feature {id:#06x} not supported"),
        }
    }
}

impl std::error::Error for Error {}

/// HID++ 2.0 エラーコード (Solaar `ErrorCode`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    Unknown,
    InvalidArgument,
    OutOfRange,
    HardwareError,
    LogitechError,
    InvalidFeatureIndex,
    InvalidFunction,
    Busy,
    Unsupported,
    Other(u8),
}

impl From<u8> for ErrorCode {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Unknown,
            2 => Self::InvalidArgument,
            3 => Self::OutOfRange,
            4 => Self::HardwareError,
            5 => Self::LogitechError,
            6 => Self::InvalidFeatureIndex,
            7 => Self::InvalidFunction,
            8 => Self::Busy,
            9 => Self::Unsupported,
            x => Self::Other(x),
        }
    }
}

/// フレーム送受信。実装は 1 report = 1 呼び出し。
pub trait Io: Send {
    /// `frame` は report ID を先頭に含む完全なフレーム。
    fn write(&mut self, frame: &[u8]) -> Result<()>;
    /// 最大 `timeout` 待って 1 フレーム読む (先頭は report ID)。無ければ `Ok(None)`。
    fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>>;
}

/// long report を組む: `[0x11, dev, feat_idx, (func<<4)|swid, params…]` (20B・0 埋め)。
pub fn encode(dev: u8, feature_index: u8, function: u8, params: &[u8]) -> Vec<u8> {
    let mut f = vec![0u8; LONG_LEN];
    f[0] = REPORT_LONG;
    f[1] = dev;
    f[2] = feature_index;
    f[3] = ((function & 0x0F) << 4) | SOFTWARE_ID;
    let n = params.len().min(LONG_LEN - 4);
    f[4..4 + n].copy_from_slice(&params[..n]);
    f
}

/// 1 回の要求に対する受信フレームの分類。
#[derive(Debug, PartialEq, Eq)]
enum Reply {
    Ok(Vec<u8>),
    Err(Error),
    /// 別要求の応答 / 通知。読み捨てる。
    Other,
}

fn classify(buf: &[u8], dev: u8, feature_index: u8, function: u8) -> Reply {
    if buf.len() < 7 || !matches!(buf[0], REPORT_SHORT | REPORT_LONG | REPORT_VERY_LONG) {
        return Reply::Other;
    }
    // BT 経由は 0x00 を返す機器がある (Solaar: reply_devnumber == devnumber ^ 0xFF)。
    if buf[1] != dev && buf[1] != dev ^ 0xFF {
        return Reply::Other;
    }
    let tag = (function << 4) | SOFTWARE_ID;
    match buf[2] {
        0xFF if buf[3] == feature_index && buf[4] == tag => {
            Reply::Err(Error::Hidpp20(ErrorCode::from(buf[5])))
        }
        0x8F if buf[3] == feature_index && buf[4] == tag => Reply::Err(Error::Hidpp10(buf[5])),
        i if i == feature_index && buf[3] == tag => Reply::Ok(buf[4..].to_vec()),
        _ => Reply::Other,
    }
}

/// 機能 index の解決結果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureInfo {
    pub index: u8,
    pub flags: u8,
    pub version: u8,
}

/// 機器 1 台との HID++ 2.0 セッション。
pub struct Session<I: Io> {
    io: I,
    dev: u8,
    timeout: Duration,
    cache: HashMap<u16, Option<FeatureInfo>>,
}

impl<I: Io> Session<I> {
    pub fn new(io: I, dev: u8) -> Self {
        Self {
            io,
            dev,
            timeout: Duration::from_millis(1500),
            cache: HashMap::new(),
        }
    }

    pub fn device_index(&self) -> u8 {
        self.dev
    }

    pub fn set_timeout(&mut self, t: Duration) {
        self.timeout = t;
    }

    /// 生の要求。応答の `params` (feature/function バイトの後ろ。long なら 16B) を返す。
    pub fn request(&mut self, feature_index: u8, function: u8, params: &[u8]) -> Result<Vec<u8>> {
        self.io
            .write(&encode(self.dev, feature_index, function, params))?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            let Some(buf) = self.io.read(left)? else {
                return Err(Error::Timeout);
            };
            match classify(&buf, self.dev, feature_index, function) {
                Reply::Ok(p) => return Ok(p),
                Reply::Err(e) => return Err(e),
                Reply::Other => continue,
            }
        }
    }

    /// Root.GetProtocolVersion を ping として使う。成功なら `(major, minor)`。
    /// HID++ 1.0 機器は `Error::Hidpp10` を返す。
    pub fn ping(&mut self) -> Result<(u8, u8)> {
        let p = self.request(0x00, 1, &[0, 0, 0xA5])?;
        if p.len() < 3 || p[2] != 0xA5 {
            return Err(Error::Protocol("ping echo mismatch".into()));
        }
        Ok((p[0], p[1]))
    }

    /// 機能 ID → index。機器が持たなければ `None` (結果はキャッシュ)。
    pub fn feature(&mut self, id: u16) -> Result<Option<FeatureInfo>> {
        if let Some(c) = self.cache.get(&id) {
            return Ok(*c);
        }
        let info = if id == 0 {
            Some(FeatureInfo {
                index: 0,
                flags: 0,
                version: 0,
            })
        } else {
            let p = self.request(0x00, 0, &[(id >> 8) as u8, id as u8])?;
            if p.len() < 3 {
                return Err(Error::Protocol("short GetFeature reply".into()));
            }
            // index 0 は「無い」(Root 自身以外)。
            (p[0] != 0).then_some(FeatureInfo {
                index: p[0],
                flags: p[1],
                version: p[2],
            })
        };
        self.cache.insert(id, info);
        Ok(info)
    }

    /// 機能 `id` の `function` を呼ぶ。機能が無ければ `Error::NoFeature`。
    pub fn call(&mut self, id: u16, function: u8, params: &[u8]) -> Result<Vec<u8>> {
        let f = self.feature(id)?.ok_or(Error::NoFeature(id))?;
        self.request(f.index, function, params)
    }

    pub fn has(&mut self, id: u16) -> Result<bool> {
        Ok(self.feature(id)?.is_some())
    }

    /// 機能テーブルを全列挙 (診断用)。`(index, feature id, type flags, version)`。
    pub fn enumerate(&mut self) -> Result<Vec<(u8, u16, u8, u8)>> {
        let count = self.call(features::FEATURE_SET, 0, &[])?[0];
        let mut out = vec![(0, 0x0000, 0, 0)];
        for i in 1..=count {
            let p = self.call(features::FEATURE_SET, 1, &[i])?;
            if p.len() < 4 {
                return Err(Error::Protocol("short GetFeatureID reply".into()));
            }
            out.push((i, u16::from_be_bytes([p[0], p[1]]), p[2], p[3]));
        }
        Ok(out)
    }
}

/// 有線直結 (0xFF) とレシーバー配下 (1..=6) の候補からデバイス番号を見つける。
/// 先頭から ping し、HID++ 2.0 で応答した最初の番号を返す。
pub fn find_device_index<I: Io>(io: I) -> (I, Result<u8>) {
    let mut s = Session::new(io, DEV_DIRECT);
    s.set_timeout(Duration::from_millis(400));
    let mut last = Error::Timeout;
    for dev in [DEV_DIRECT, 1, 2, 3, 4, 5, 6] {
        s.dev = dev;
        match s.ping() {
            Ok((major, _)) if major >= 2 => return (s.io, Ok(dev)),
            Ok(_) => last = Error::Protocol("HID++ < 2.0".into()),
            // レシーバー本体 (0xFF) は 2.0 要求に 1.0 エラーを返す → 次の番号へ。
            Err(e) => last = e,
        }
    }
    (s.io, Err(last))
}

/// テスト・ドライバ層の単体テスト用スタブ (実機通信はしない)。
pub mod testing {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// 要求を記録し、応答関数で返事を作るスタブ。
    #[derive(Clone, Default)]
    pub struct Mock {
        pub sent: Arc<Mutex<Vec<Vec<u8>>>>,
        queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
        /// (feature_index, function) → 応答 params。
        #[allow(clippy::type_complexity)]
        pub handler: Arc<Mutex<Option<Box<dyn FnMut(&[u8]) -> Vec<Vec<u8>> + Send>>>>,
    }

    impl Mock {
        pub fn with(f: impl FnMut(&[u8]) -> Vec<Vec<u8>> + Send + 'static) -> Self {
            let m = Mock::default();
            *m.handler.lock().unwrap() = Some(Box::new(f));
            m
        }
    }

    impl Io for Mock {
        fn write(&mut self, frame: &[u8]) -> Result<()> {
            self.sent.lock().unwrap().push(frame.to_vec());
            if let Some(h) = self.handler.lock().unwrap().as_mut() {
                for r in h(frame) {
                    self.queue.lock().unwrap().push_back(r);
                }
            }
            Ok(())
        }
        fn read(&mut self, _t: Duration) -> Result<Option<Vec<u8>>> {
            Ok(self.queue.lock().unwrap().pop_front())
        }
    }

    /// 応答フレーム: 要求の dev/feature/func タグをエコーして params を付ける。
    pub fn reply(req: &[u8], params: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; LONG_LEN];
        f[0] = REPORT_LONG;
        f[1] = req[1];
        f[2] = req[2];
        f[3] = req[3];
        f[4..4 + params.len()].copy_from_slice(params);
        f
    }

    pub fn hidpp20_error(req: &[u8], code: u8) -> Vec<u8> {
        let mut f = vec![0u8; LONG_LEN];
        f[0] = REPORT_LONG;
        f[1] = req[1];
        f[2] = 0xFF;
        f[3] = req[2];
        f[4] = req[3];
        f[5] = code;
        f
    }

    pub fn hidpp10_error(req: &[u8], code: u8) -> Vec<u8> {
        let mut f = vec![0u8; 7];
        f[0] = REPORT_SHORT;
        f[1] = req[1];
        f[2] = 0x8F;
        f[3] = req[2];
        f[4] = req[3];
        f[5] = code;
        f
    }

    /// 通知フレーム (swid=0)。
    pub fn notification(dev: u8, idx: u8) -> Vec<u8> {
        let mut f = vec![0u8; LONG_LEN];
        f[0] = REPORT_LONG;
        f[1] = dev;
        f[2] = idx;
        f[3] = 0x00;
        f
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn encode_layout() {
        let f = encode(0xFF, 0x05, 3, &[0xAA, 0xBB]);
        assert_eq!(f.len(), 20);
        assert_eq!(&f[..6], &[0x11, 0xFF, 0x05, 0x30 | SOFTWARE_ID, 0xAA, 0xBB]);
    }

    #[test]
    fn request_skips_notifications_and_matches_tag() {
        let m = Mock::with(|req| {
            vec![
                notification(0xFF, 0x07),
                // 別 swid の応答 (他クライアント宛て) は無視されること
                {
                    let mut o = reply(req, &[9]);
                    o[3] = (req[3] & 0xF0) | 0x03;
                    o
                },
                reply(req, &[1, 2, 3]),
            ]
        });
        let mut s = Session::new(m, DEV_DIRECT);
        let p = s.request(0x04, 2, &[]).unwrap();
        assert_eq!(&p[..3], &[1, 2, 3]);
    }

    #[test]
    fn hidpp20_error_is_reported_for_matching_request_only() {
        let m = Mock::with(|req| vec![hidpp20_error(req, 3)]);
        let mut s = Session::new(m, DEV_DIRECT);
        assert_eq!(
            s.request(0x04, 2, &[]),
            Err(Error::Hidpp20(ErrorCode::OutOfRange))
        );
    }

    #[test]
    fn timeout_when_silent() {
        let m = Mock::default();
        let mut s = Session::new(m, DEV_DIRECT);
        s.set_timeout(Duration::from_millis(20));
        assert_eq!(s.request(0, 0, &[]), Err(Error::Timeout));
    }

    #[test]
    fn feature_lookup_caches_and_treats_index0_as_absent() {
        let m = Mock::with(|req| {
            let id = u16::from_be_bytes([req[4], req[5]]);
            match id {
                0x2201 => vec![reply(req, &[0x09, 0x00, 0x02])],
                _ => vec![reply(req, &[0x00, 0x00, 0x00])],
            }
        });
        let sent = m.sent.clone();
        let mut s = Session::new(m, DEV_DIRECT);
        let f = s.feature(0x2201).unwrap().unwrap();
        assert_eq!((f.index, f.version), (9, 2));
        assert!(s.feature(0x8100).unwrap().is_none());
        // 2 回目はキャッシュで、送信が増えない
        s.feature(0x2201).unwrap();
        s.feature(0x8100).unwrap();
        assert_eq!(sent.lock().unwrap().len(), 2);
        assert!(matches!(
            s.call(0x8100, 0, &[]),
            Err(Error::NoFeature(0x8100))
        ));
    }

    #[test]
    fn ping_reports_version_and_rejects_hidpp10() {
        let m = Mock::with(|req| vec![reply(req, &[4, 2, 0xA5])]);
        let mut s = Session::new(m, DEV_DIRECT);
        assert_eq!(s.ping(), Ok((4, 2)));

        let m = Mock::with(|req| vec![hidpp10_error(req, 0x01)]);
        let mut s = Session::new(m, DEV_DIRECT);
        assert_eq!(s.ping(), Err(Error::Hidpp10(1)));
    }

    #[test]
    fn find_device_index_skips_receiver_and_finds_paired_slot() {
        // 0xFF (レシーバー本体) は 1.0 エラー、スロット 2 だけが 2.0 で応答。
        let m = Mock::with(|req| match req[1] {
            0xFF => vec![hidpp10_error(req, 0x01)],
            2 => vec![reply(req, &[4, 2, 0xA5])],
            _ => vec![],
        });
        let (_, r) = find_device_index(m);
        assert_eq!(r, Ok(2));
    }

    #[test]
    fn find_device_index_prefers_direct_wired() {
        let m = Mock::with(|req| vec![reply(req, &[4, 2, 0xA5])]);
        let (_, r) = find_device_index(m);
        assert_eq!(r, Ok(0xFF));
    }

    #[test]
    fn enumerate_lists_feature_table() {
        let m = Mock::with(|req| {
            // Root.GetFeature(FEATURE_SET)=index 1。以降 index 1 が FeatureSet。
            if req[2] == 0 && req[4] == 0x00 && req[5] == 0x01 {
                return vec![reply(req, &[1, 0, 1])];
            }
            if req[2] == 1 && req[3] >> 4 == 0 {
                return vec![reply(req, &[2])]; // count=2
            }
            if req[2] == 1 && req[3] >> 4 == 1 {
                return match req[4] {
                    1 => vec![reply(req, &[0x22, 0x01, 0, 1])],
                    2 => vec![reply(req, &[0x80, 0x61, 0x40, 0])],
                    _ => vec![],
                };
            }
            vec![]
        });
        let mut s = Session::new(m, DEV_DIRECT);
        let t = s.enumerate().unwrap();
        assert_eq!(
            t,
            vec![(0, 0, 0, 0), (1, 0x2201, 0, 1), (2, 0x8061, 0x40, 0)]
        );
    }
}
