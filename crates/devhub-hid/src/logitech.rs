//! Logitech G (HID++ 2.0) マウス — 現状は PRO 2 LIGHTSPEED (有線 046d:c09a / レシーバー 046d:c543)。
//!
//! G HUB 置換の第 1 段階。プロトコル本体は `devhub-hidpp` (HID バックエンド非依存) にあり、
//! ここは `DeviceDriver` への橋渡しだけ。
//!
//! 対応: DPI (単一値) / ポーリングレート (8K 含む) / オンボード⇔ホストモード / バッテリー / FW。
//! 未対応 (実機トレースが要る): オンボードプロファイル書込み (DPI ステージ複数・ボタン割当)、
//! ボタンのダイバート (ReprogControlsV4)、ライティング。docs/LOGITECH_HIDPP.md 参照。
//!
//! **実機 (PRO 2) での検証は未了**。registry では `verified: false`。

use devhub_core::backend::*;
use devhub_core::model::*;
use devhub_hidpp::features::{self as f, DpiList, OnboardMode};
use devhub_hidpp::{Error as HErr, Io, Session};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// hidapi を `devhub_hidpp::Io` に繋ぐ。
pub struct HidIo {
    dev: hidapi::HidDevice,
}

impl HidIo {
    pub fn new(dev: hidapi::HidDevice) -> Self {
        Self { dev }
    }
}

impl Io for HidIo {
    fn write(&mut self, frame: &[u8]) -> Result<(), HErr> {
        // 先頭 1 バイトが report ID (0x11)。Windows は report 長ちょうどで書く必要がある (20B)。
        self.dev
            .write(frame)
            .map(|_| ())
            .map_err(|e| HErr::Io(e.to_string()))
    }

    fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, HErr> {
        let mut buf = [0u8; 64];
        let ms = timeout.as_millis().clamp(1, i32::MAX as u128) as i32;
        match self.dev.read_timeout(&mut buf, ms) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(buf[..n].to_vec())),
            Err(e) => Err(HErr::Io(e.to_string())),
        }
    }
}

fn map_err(e: HErr) -> HubError {
    match e {
        // 無線マウスのスリープ / 切断。
        HErr::Timeout => HubError::Disconnected,
        HErr::Io(s) => HubError::Io(s),
        HErr::Hidpp20(devhub_hidpp::ErrorCode::InvalidArgument | devhub_hidpp::ErrorCode::OutOfRange) => {
            HubError::Invalid(e.to_string())
        }
        other => HubError::Protocol(other.to_string()),
    }
}

const ID_POLLING: &str = "polling_rate";
const ID_ONBOARD: &str = "onboard_mode";

/// 機種ごとの差は機能テーブルから決まるので、ここに機種表は要らない。
pub struct LogitechDriver<I: Io> {
    info: Mutex<DeviceInfo>,
    sess: Mutex<Session<I>>,
    caps: Capabilities,
    dpi: DpiList,
    rates: Vec<u32>,
    has_onboard: bool,
    /// 0x8061 の接続種別の枠 (有線=0, 2.4GHz=1)。読み戻しはこの枠を見る。
    rate_conn: u8,
    /// 直近に適用した値の保存先 (再接続時に再適用する)。テストでは None。
    state_file: Option<PathBuf>,
}

/// 実機用の入口 (`lib.rs::make_driver` から)。
pub fn open(
    dev: hidapi::HidDevice,
    info: DeviceInfo,
) -> HubResult<LogitechDriver<HidIo>> {
    let path = state_path(info.vid, info.pid);
    LogitechDriver::open_io(HidIo::new(dev), info, path)
}

fn state_path(vid: u16, pid: u16) -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let mut p = PathBuf::from(base);
    p.push("DeviceHub");
    p.push("logitech");
    p.push(format!("{vid:04x}_{pid:04x}.json"));
    Some(p)
}

impl<I: Io> LogitechDriver<I> {
    pub fn open_io(io: I, mut info: DeviceInfo, state_file: Option<PathBuf>) -> HubResult<Self> {
        let (io, idx) = devhub_hidpp::find_device_index(io);
        let idx = idx.map_err(|e| match e {
            // どの番号にも応答が無い: 無線ならマウスが寝ている / HID++ の IF ではない。
            HErr::Timeout => HubError::Protocol(
                "HID++ の応答がありません (インターフェース違い、またはマウスがスリープ中)。\
                 `cargo run -p devhub-hid --example hidppprobe` で確認してください"
                    .into(),
            ),
            other => map_err(other),
        })?;
        let mut s = Session::new(io, idx);

        if let Ok(n) = f::device_name(&mut s) {
            if !n.is_empty() {
                info.name = n;
            }
        }
        if let Ok(fw) = f::firmware(&mut s) {
            if let Some(m) = fw.iter().find(|e| e.kind == 0) {
                info.firmware = Some(format!("{}{}", m.name, m.version));
            }
        }
        if let Ok(Some(b)) = f::battery(&mut s) {
            info.battery = Some(Battery {
                percent: b.percent,
                charging: b.charging,
            });
        }

        let dpi = f::dpi_list(&mut s, 0).map_err(map_err)?;
        let rates = f::report_rates(&mut s).unwrap_or_default();
        let has_onboard = s.has(f::ONBOARD_PROFILES).unwrap_or(false);

        let caps = build_caps(&dpi, &rates, has_onboard, info.battery.is_some());
        let rate_conn = if info.transport == Transport::Wired { 0 } else { 1 };
        let drv = Self {
            info: Mutex::new(info),
            sess: Mutex::new(s),
            caps,
            dpi,
            rates,
            has_onboard,
            rate_conn,
            state_file,
        };
        drv.restore_saved();
        Ok(drv)
    }

    fn with<T>(&self, op: impl FnOnce(&mut Session<I>) -> Result<T, HErr>) -> HubResult<T> {
        let mut s = self.sess.lock().unwrap_or_else(|p| p.into_inner());
        op(&mut s).map_err(map_err)
    }

    /// 書込み後の読み戻し。無線 (受信機経由) では反映まで少し掛かることがあるので、
    /// 一致するまで最大 ~0.6 秒 (100ms 間隔) 待つ。
    fn confirm<T: PartialEq + Copy>(
        &self,
        want: T,
        read: impl Fn(&mut Session<I>) -> Result<T, HErr>,
    ) -> HubResult<T> {
        let mut got = self.with(|s| read(s))?;
        for _ in 0..6 {
            if got == want {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            got = self.with(|s| read(s))?;
        }
        Ok(got)
    }

    // ---- 再接続時の再適用 (G HUB の「常駐して設定を保つ」相当)

    fn save(&self, key: &str, value: u32) {
        let Some(path) = &self.state_file else { return };
        let mut v: serde_json::Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        v[key] = serde_json::json!(value);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, v.to_string());
    }

    /// DPI / ポーリングの書込みはホストモードでしか受理されない (実機: オンボードのままだと
    /// DPI は LogitechError、ポーリングは InvalidArgument で拒否される)。再接続や有線⇔無線の
    /// 切替でマウスがオンボードに戻るので、書く前に毎回確認して必要ならホストへ切り替える。
    fn ensure_host(&self) {
        if !self.has_onboard {
            return;
        }
        if let Ok(OnboardMode::Onboard) = self.with(|s| f::onboard_mode(s)) {
            let _ = self.with(|s| f::set_onboard_mode(s, OnboardMode::Host));
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn restore_saved(&self) {
        let Some(path) = &self.state_file else { return };
        let Some(v) = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            return;
        };
        // 失敗は無視 (機器が寝ている等)。次の apply か再接続で再試行される。
        self.ensure_host();
        if let Some(d) = v["dpi"].as_u64() {
            let d = self.dpi.snap(d.min(u16::MAX as u64) as u16);
            let _ = self.with(|s| f::set_dpi(s, 0, d));
        }
        if let Some(hz) = v["rate_hz"].as_u64() {
            if self.rates.contains(&(hz as u32)) {
                let _ = self.with(|s| f::set_report_rate(s, hz as u32));
            }
        }
    }
}

fn build_caps(dpi: &DpiList, rates: &[u32], has_onboard: bool, battery: bool) -> Capabilities {
    let mut c = Capabilities::empty();
    c.battery = battery;
    c.mouse = Some(MouseCaps {
        dpi_min: dpi.min() as u32,
        dpi_max: dpi.max() as u32,
        dpi_step: dpi.step() as u32,
        // オンボードプロファイルの書込み未実装のため、ステージは現在値 1 つだけ。
        min_stages: 1,
        max_stages: 1,
        xy_split: false,
        stage_colors: false,
        stage_toggle: false,
        buttons: vec![],
        button_kinds: vec![],
        require_left_click: false,
        macros: None,
        live_buttons: vec![],
        adaptive_dpi: None,
    });
    if rates.len() > 1 {
        let opts = rates
            .iter()
            .map(|&hz| ChoiceOpt::new(hz as i64, format!("{hz} Hz")))
            .collect();
        c.settings.push(SettingSpec::choice(
            ID_POLLING,
            section::PERFORMANCE,
            "ポーリングレート",
            opts,
        ));
    }
    if has_onboard {
        c.settings.push(
            SettingSpec::choice(
                ID_ONBOARD,
                section::PERFORMANCE,
                "設定の保持先",
                vec![
                    ChoiceOpt::new(1, "オンボード (マウス内蔵)"),
                    ChoiceOpt::new(2, "ホスト (このアプリ)"),
                ],
            )
            .help(
                "DPI・ポーリングが効かないときはホストにする。\
                 ホストではマウス本体の DPI ボタン等がアプリ依存になる場合があります",
            ),
        );
    }
    c
}

impl<I: Io + 'static> DeviceDriver for LogitechDriver<I> {
    fn info(&self) -> DeviceInfo {
        let mut i = self.info.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match self.with(|s| f::battery(s)) {
            Ok(Some(b)) => {
                i.asleep = false;
                i.battery = Some(Battery {
                    percent: b.percent,
                    charging: b.charging,
                });
            }
            Ok(None) => i.asleep = false,
            Err(HubError::Disconnected) if i.transport == Transport::Wireless24G => i.asleep = true,
            Err(_) => {}
        }
        i
    }

    fn caps(&self) -> Capabilities {
        self.caps.clone()
    }

    fn read(&self) -> HubResult<DeviceState> {
        let dpi = self.with(|s| f::get_dpi(s, 0))?;
        let mut settings = BTreeMap::new();
        if self.caps.setting(ID_POLLING).is_some() {
            if let Ok(hz) = self.with(|s| f::report_rate(s, self.rate_conn)) {
                settings.insert(ID_POLLING.into(), SettingValue::Choice(hz as i64));
            }
        }
        if self.has_onboard {
            if let Ok(m) = self.with(|s| f::onboard_mode(s)) {
                let v = if m == OnboardMode::Onboard { 1 } else { 2 };
                settings.insert(ID_ONBOARD.into(), SettingValue::Choice(v));
            }
        }
        Ok(DeviceState {
            profile: ProfileState::default(),
            settings,
            mouse: Some(MouseState {
                stages: vec![DpiStage {
                    x: dpi as u32,
                    y: None,
                    color: Rgb { r: 0, g: 0, b: 0 },
                    enabled: true,
                }],
                active_stage: 0,
                buttons: vec![],
                macros: vec![],
                adaptive: None,
            }),
            keyboard: None,
            lighting: BTreeMap::new(),
            peq: None,
        })
    }

    fn apply(&self, change: &Change) -> HubResult<()> {
        match change {
            Change::DpiStages { stages, active } => {
                let want = stages
                    .get(*active)
                    .or_else(|| stages.first())
                    .ok_or_else(|| HubError::Invalid("no DPI stage".into()))?
                    .x
                    .min(u16::MAX as u32) as u16;
                let dpi = self.dpi.snap(want);
                self.ensure_host();
                self.with(|s| f::set_dpi(s, 0, dpi))?;
                // 受理されたか読み戻す。オンボードモードだと無視される機器があるため、
                // 黙って失敗させずトーストで知らせる。
                let got = self.confirm(dpi, |s| f::get_dpi(s, 0))?;
                if got != dpi {
                    return Err(HubError::Protocol(format!(
                        "DPI が反映されません (要求 {dpi} / 実際 {got})。\
                         G HUB が動いていると上書きされます (G HUB を終了して再試行)。\
                         それでも効かなければ「設定の保持先」を切り替えてください"
                    )));
                }
                self.save("dpi", dpi as u32);
                Ok(())
            }
            Change::ActiveStage(0) => Ok(()),
            Change::Setting { id, value } if id == ID_POLLING => {
                let SettingValue::Choice(hz) = value else {
                    return Err(HubError::Invalid("polling value".into()));
                };
                let hz = *hz as u32;
                if !self.rates.contains(&hz) {
                    return Err(HubError::Invalid(format!("{hz} Hz is not supported")));
                }
                self.ensure_host();
                self.with(|s| f::set_report_rate(s, hz))?;
                let got = self.confirm(hz, |s| f::report_rate(s, self.rate_conn))?;
                if got != hz {
                    return Err(HubError::Protocol(format!(
                        "ポーリングレートが反映されません (要求 {hz} / 実際 {got})。\
                         G HUB が動いていると上書きされます (G HUB を終了して再試行)"
                    )));
                }
                self.save("rate_hz", hz);
                Ok(())
            }
            Change::Setting { id, value } if id == ID_ONBOARD && self.has_onboard => {
                let mode = match value {
                    SettingValue::Choice(1) => OnboardMode::Onboard,
                    SettingValue::Choice(2) => OnboardMode::Host,
                    _ => return Err(HubError::Invalid("onboard mode value".into())),
                };
                self.with(|s| f::set_onboard_mode(s, mode))
            }
            _ => Err(HubError::Unsupported("logitech: この変更")),
        }
    }

    fn battery(&self) -> HubResult<Battery> {
        match self.with(|s| f::battery(s))? {
            Some(b) => Ok(Battery {
                percent: b.percent,
                charging: b.charging,
            }),
            None => Err(HubError::Unsupported("battery")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devhub_hidpp::testing::{reply, Mock};

    fn info() -> DeviceInfo {
        DeviceInfo {
            id: DeviceId("hid:046d:c09a:test".into()),
            kind: DeviceKind::Mouse,
            name: "PRO 2".into(),
            vendor: "Logitech".into(),
            model: "pro2-lightspeed".into(),
            vid: 0x046D,
            pid: 0xC09A,
            transport: Transport::Wired,
            firmware: None,
            dongle_firmware: None,
            serial: None,
            driver: DriverKind::Logitech,
            battery: None,
            supported: true,
            asleep: false,
            board_id: None,
        }
    }

    /// 機能 index: DPI=3, ExtRate=4, Onboard=5, UnifiedBattery=6。
    /// 状態 (DPI・レート・モード) は書込みで更新される。
    fn fake_mouse(accept_dpi: bool) -> Mock {
        let st = std::sync::Arc::new(std::sync::Mutex::new((800u16, 3u8, 1u8)));
        Mock::with(move |req| {
            let (idx, func) = (req[2], req[3] >> 4);
            let p = &req[4..];
            let mut st = st.lock().unwrap();
            let out: Option<Vec<u8>> = match (idx, func) {
                (0, 1) => Some(vec![4, 2, 0xA5]), // ping
                (0, 0) => {
                    let id = u16::from_be_bytes([p[0], p[1]]);
                    let i = match id {
                        0x2201 => 3,
                        0x8061 => 4,
                        0x8100 => 5,
                        0x1004 => 6,
                        _ => 0,
                    };
                    Some(vec![i, 0, 1])
                }
                (3, 1) => Some(vec![0, 0x00, 0x64, 0xE0, 0x32, 0xAB, 0xE0, 0, 0]),
                (3, 2) => Some(vec![0, (st.0 >> 8) as u8, st.0 as u8, 0x03, 0x20]),
                (3, 3) => {
                    if accept_dpi && st.2 == 2 {
                        st.0 = u16::from_be_bytes([p[1], p[2]]);
                    }
                    Some(vec![])
                }
                (4, 1) => Some(vec![0x00, 0x7F]),
                (4, 2) => Some(vec![st.1]),
                (4, 3) => {
                    if st.2 == 2 {
                        st.1 = p[0];
                    }
                    Some(vec![])
                }
                (5, 2) => Some(vec![st.2]),
                (5, 1) => {
                    st.2 = p[0];
                    Some(vec![])
                }
                (6, 1) => Some(vec![64, 4, 0, 0]),
                _ => None,
            };
            out.map(|o| vec![reply(req, &o)]).unwrap_or_default()
        })
    }

    fn open(accept_dpi: bool) -> LogitechDriver<Mock> {
        LogitechDriver::open_io(fake_mouse(accept_dpi), info(), None).unwrap()
    }

    #[test]
    fn caps_come_from_device_tables() {
        let d = open(true);
        let c = d.caps();
        let m = c.mouse.as_ref().unwrap();
        assert_eq!((m.dpi_min, m.dpi_max, m.dpi_step), (100, 44000, 50));
        assert!(c.battery);
        let polling = c.setting(ID_POLLING).unwrap();
        match &polling.kind {
            SettingKind::Choice { options } => {
                assert_eq!(options.len(), 7);
                assert_eq!(options.last().unwrap().value, 8000);
            }
            _ => panic!("polling must be a choice"),
        }
        assert!(c.setting(ID_ONBOARD).is_some());
    }

    #[test]
    fn read_reports_live_values() {
        let d = open(true);
        let st = d.read().unwrap();
        assert_eq!(st.mouse.unwrap().stages[0].x, 800);
        assert_eq!(st.settings[ID_POLLING], SettingValue::Choice(1000));
        assert_eq!(st.settings[ID_ONBOARD], SettingValue::Choice(1));
        assert_eq!(d.battery().unwrap().percent, Some(64));
    }

    #[test]
    fn dpi_is_snapped_applied_and_verified() {
        let d = open(true);
        let stage = DpiStage { x: 1627, y: None, color: Rgb { r: 0, g: 0, b: 0 }, enabled: true };
        d.apply(&Change::DpiStages { stages: vec![stage], active: 0 }).unwrap();
        assert_eq!(d.read().unwrap().mouse.unwrap().stages[0].x, 1650);
    }

    #[test]
    fn ignored_dpi_write_surfaces_as_error() {
        // オンボードモードで書込みが無視される機器 → 黙って成功扱いにしない
        let d = open(false);
        let stage = DpiStage { x: 1600, y: None, color: Rgb { r: 0, g: 0, b: 0 }, enabled: true };
        let e = d.apply(&Change::DpiStages { stages: vec![stage], active: 0 }).unwrap_err();
        assert!(matches!(e, HubError::Protocol(m) if m.contains("反映されません")));
    }

    #[test]
    fn polling_and_mode_changes_roundtrip() {
        let d = open(true);
        d.apply(&Change::Setting { id: ID_POLLING.into(), value: SettingValue::Choice(8000) }).unwrap();
        d.apply(&Change::Setting { id: ID_ONBOARD.into(), value: SettingValue::Choice(2) }).unwrap();
        let st = d.read().unwrap();
        assert_eq!(st.settings[ID_POLLING], SettingValue::Choice(8000));
        assert_eq!(st.settings[ID_ONBOARD], SettingValue::Choice(2));
        assert!(d
            .apply(&Change::Setting { id: ID_POLLING.into(), value: SettingValue::Choice(3000) })
            .is_err());
        assert!(matches!(
            d.apply(&Change::ActiveStage(2)),
            Err(HubError::Unsupported(_))
        ));
    }

    #[test]
    fn saved_state_is_restored_on_open() {
        let dir = std::env::temp_dir().join(format!("devhub-logi-{}", std::process::id()));
        let file = dir.join("s.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, r#"{"dpi":1600,"rate_hz":4000}"#).unwrap();
        let d = LogitechDriver::open_io(fake_mouse(true), info(), Some(file)).unwrap();
        let st = d.read().unwrap();
        assert_eq!(st.mouse.unwrap().stages[0].x, 1600);
        assert_eq!(st.settings[ID_POLLING], SettingValue::Choice(4000));
        let _ = std::fs::remove_dir_all(dir);
    }
}
