//! The graphics cards ONNX Runtime finds here, and the one the analysis runs
//! on (design-i66_67 §5.1).
//!
//! - ORT's device discovery reports each card with the number DXGI's
//!   `EnumAdapters1` gives it (`DxgiAdapterNumber`): the number DirectML opens
//!   when it is handed a device id (`device_discovery.cc:331-420`,
//!   `dml_provider_factory.cc:461-520`, ORT 1.24.4). The name Settings shows
//!   and the device the analysis uses come from one record.
//! - **Automatic** is the card DirectML's own default opens: the first
//!   adapter of [`computer::machine`]'s list, matched by its LUID. So Fast
//!   Scan's default classes the very device the analysis runs on. Only when
//!   no card has that LUID (DXCore can't be read) is it the card DXGI ranks
//!   fastest (`DxgiHighPerformanceIndex`); without adapter numbers DirectML
//!   picks by its own preference, as it did before cards could be chosen.
//! - **A chosen card** matches by its PCI ids and its place among identical
//!   cards in DirectML's order, as [`computer::choose`] matches it: never by
//!   its description, which drivers and ORT each spell their own way. A card
//!   that isn't there any more falls back to Automatic.
//! - **Processor only** builds no GPU session.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::computer::{self, Choice};

/// What kind of device ORT's discovery reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Cpu,
    Gpu,
    Npu,
}

/// A device as ORT's discovery reports it (`OrtHardwareDevice`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawDevice {
    pub kind: Kind,
    /// PCI vendor and device id.
    pub vendor: u32,
    pub device: u32,
    /// `Description`, `DxgiAdapterNumber`, `DxgiHighPerformanceIndex`,
    /// `DxgiVideoMemory` ("8031 MB"), `LUID` (decimal) and `Discrete`, as far
    /// as discovery found them.
    pub metadata: BTreeMap<String, String>,
}

impl RawDevice {
    fn number(&self, key: &str) -> Option<u64> {
        self.metadata.get(key)?.trim().parse().ok()
    }

    fn description(&self) -> String {
        self.metadata.get("Description").map(|d| computer::clean(d)).unwrap_or_default()
    }
}

/// A graphics card the analysis can run on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gpu {
    /// What Settings stores when it is chosen ([`key`]): stable across
    /// restarts, which LUIDs and adapter numbers aren't.
    pub key: String,
    /// The driver's description, cleaned ("NVIDIA GeForce RTX 3060 Ti").
    pub name: String,
    /// The name, numbered among identical cards ("NVIDIA GeForce RTX 3090 (2)").
    pub label: String,
    pub vendor: u32,
    pub device: u32,
    /// Its place among the cards with these ids, in DirectML's order (from 0).
    pub nth: u32,
    pub discrete: Option<bool>,
    pub memory_mb: Option<u32>,
    /// `DxgiAdapterNumber`: what DirectML opens as its device id.
    pub dxgi: Option<u32>,
    /// `DxgiHighPerformanceIndex`: 0 for the card DXGI ranks fastest.
    pub high_performance: Option<u32>,
    pub luid: Option<u64>,
}

/// What a choice of device opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Using {
    /// `Hardware::gpus[index]`.
    Gpu { index: usize },
    /// A graphics card DirectML picks by its own preference: discovery named
    /// none it could match.
    Preferred,
    /// An Apple chip's graphics and Neural Engine.
    Chip,
    #[default]
    Processor,
}

/// The devices ORT's discovery found here, and what this runtime opened.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hardware {
    /// The processor, as its maker names it.
    pub cpu: String,
    /// The graphics cards DirectML can open, in DirectML's order (Windows).
    pub gpus: Vec<Gpu>,
    /// The Apple chip ("Apple M2"; macOS).
    pub chip: String,
    /// What Automatic opens.
    pub auto: Using,
    /// What this runtime opened for its choice.
    pub using: Using,
    /// The chosen card isn't connected: Automatic stands in.
    pub missing: bool,
}

/// The key Settings stores for a card: `vendor:device:name` in lower-case
/// hex, then `#n` for the n-th (from 2) of identical cards. Only the ids and
/// `#n` decide ([`Choice::from_setting`]); the name says which card it was.
pub fn key(vendor: u32, device: u32, name: &str, nth: u32) -> String {
    let mut k = format!("{vendor:04x}:{device:04x}:{name}");
    if nth > 0 {
        k.push_str(&format!("#{}", nth + 1));
    }
    k
}

impl Choice {
    /// The choice Settings stores: `""` Automatic, `"cpu"` Processor only,
    /// else a card's [`key`]. Anything else reads as Automatic (a newer app's
    /// value, say).
    pub fn from_setting(setting: &str) -> Self {
        let setting = setting.trim();
        match setting {
            "" => return Self::Auto,
            "cpu" => return Self::Processor,
            _ => {}
        }
        let mut parts = setting.splitn(3, ':');
        let hex = |s: Option<&str>| s.and_then(|s| u32::from_str_radix(s, 16).ok());
        let (Some(vendor), Some(device), Some(rest)) = (hex(parts.next()), hex(parts.next()), parts.next()) else {
            return Self::Auto;
        };
        let nth =
            rest.rsplit_once('#').and_then(|(_, n)| n.parse::<u32>().ok()).filter(|&n| n >= 1).map_or(0, |n| n - 1);
        Self::Card { vendor, device, nth }
    }
}

/// The graphics cards of `devices` (each once, by LUID), in DirectML's order:
/// `order` lists the LUIDs of [`computer::machine`]'s adapters; cards it
/// doesn't list follow by adapter number. Each gets its key and label.
pub fn gpus_from(devices: &[RawDevice], order: &[u64]) -> Vec<Gpu> {
    let mut gpus: Vec<Gpu> = Vec::new();
    for d in devices.iter().filter(|d| d.kind == Kind::Gpu) {
        let luid = d.number("LUID");
        let dxgi = d.number("DxgiAdapterNumber").and_then(|n| u32::try_from(n).ok());
        // One card may be listed by several runtimes' providers.
        if gpus.iter().any(|g| match (luid, g.luid) {
            (Some(a), Some(b)) => a == b,
            _ => g.vendor == d.vendor && g.device == d.device && g.dxgi == dxgi,
        }) {
            continue;
        }
        let memory_mb =
            d.metadata.get("DxgiVideoMemory").and_then(|m| m.trim().trim_end_matches("MB").trim().parse().ok());
        gpus.push(Gpu {
            key: String::new(),
            name: d.description(),
            label: String::new(),
            vendor: d.vendor,
            device: d.device,
            nth: 0,
            discrete: d.metadata.get("Discrete").map(|v| v.trim() == "1"),
            memory_mb,
            dxgi,
            high_performance: d.number("DxgiHighPerformanceIndex").and_then(|n| u32::try_from(n).ok()),
            luid,
        });
    }
    let rank = |g: &Gpu| {
        let listed = g.luid.and_then(|l| order.iter().position(|&o| o == l)).unwrap_or(usize::MAX);
        (listed, g.dxgi.unwrap_or(u32::MAX), g.luid.unwrap_or(u64::MAX))
    };
    gpus.sort_by_key(rank);
    for i in 0..gpus.len() {
        let same = |g: &&Gpu| g.vendor == gpus[i].vendor && g.device == gpus[i].device;
        let nth = gpus[..i].iter().filter(same).count() as u32;
        let twins = gpus.iter().filter(same).count();
        let g = &mut gpus[i];
        if g.name.is_empty() {
            g.name = "Graphics card".into();
        }
        g.nth = nth;
        g.key = key(g.vendor, g.device, &g.name, nth);
        g.label = if twins > 1 { format!("{} ({})", g.name, nth + 1) } else { g.name.clone() };
    }
    gpus
}

/// The processor's name, as discovery reports it (trailing spaces and all).
fn cpu_from(devices: &[RawDevice]) -> String {
    devices.iter().find(|d| d.kind == Kind::Cpu).map(RawDevice::description).unwrap_or_default()
}

impl Hardware {
    /// Windows: the cards DirectML can open. `order` is DirectML's own list
    /// ([`computer::machine`]'s adapters' LUIDs), `None` when DXCore can't be
    /// read.
    pub fn windows(devices: &[RawDevice], order: Option<&[u64]>) -> Self {
        let gpus = gpus_from(devices, order.unwrap_or_default());
        let fastest = || {
            let ranked = gpus.iter().enumerate().filter_map(|(i, g)| Some((g.high_performance?, i)));
            ranked.min().map(|(_, index)| Using::Gpu { index })
        };
        let auto = match order.map(|o| o.first()) {
            // DirectML's list holds no adapter: its default refuses, and every
            // session runs on the processor.
            Some(None) => Using::Processor,
            // What DirectML's default opens: its list's first adapter.
            first => first
                .flatten()
                .and_then(|&luid| gpus.iter().position(|g| g.luid == Some(luid)))
                .map(|index| Using::Gpu { index })
                .or_else(fastest)
                .unwrap_or(match gpus.len() {
                    0 => Using::Processor,
                    1 => Using::Gpu { index: 0 },
                    _ => Using::Preferred,
                }),
        };
        Self { cpu: cpu_from(devices), gpus, chip: String::new(), auto, using: auto, missing: false }
    }

    /// macOS: the chip's graphics and Neural Engine (Core ML), named by its
    /// brand string ("Apple M2"; empty on an Intel Mac).
    pub fn mac(devices: &[RawDevice], chip: &str) -> Self {
        let chip = computer::apple(chip).name;
        Self { cpu: cpu_from(devices), chip, auto: Using::Chip, using: Using::Chip, ..Self::default() }
    }

    /// A runtime that computes on the processor only (Linux).
    pub fn processor_only(devices: &[RawDevice]) -> Self {
        Self { cpu: cpu_from(devices), ..Self::default() }
    }

    /// What `choice` opens here, and whether the card it names is missing.
    pub fn resolve(&self, choice: &Choice) -> (Using, bool) {
        match *choice {
            Choice::Auto => (self.auto, false),
            Choice::Processor => (Using::Processor, false),
            // Nothing to choose from: no card can be missing.
            Choice::Card { .. } if self.gpus.is_empty() => (self.auto, false),
            Choice::Card { vendor, device, nth } => {
                let same: Vec<usize> = (0..self.gpus.len())
                    .filter(|&i| self.gpus[i].vendor == vendor && self.gpus[i].device == device)
                    .collect();
                // One identical card fewer than when it was chosen: the first of them.
                match same.get(nth as usize).or(same.first()).copied() {
                    // DirectML opens a card by its adapter number; without one
                    // only the card its default takes (Automatic's).
                    Some(index) if self.gpus[index].dxgi.is_some() || self.auto == (Using::Gpu { index }) => {
                        (Using::Gpu { index }, false)
                    }
                    Some(_) => (self.auto, false),
                    None => (self.auto, true),
                }
            }
        }
    }

    /// These devices with what `choice` opens.
    pub fn chosen(mut self, choice: &Choice) -> Self {
        (self.using, self.missing) = self.resolve(choice);
        self
    }

    /// The card in use, if one is.
    pub fn gpu(&self) -> Option<&Gpu> {
        match self.using {
            Using::Gpu { index } => self.gpus.get(index),
            _ => None,
        }
    }

    /// The adapter number DirectML opens: `None` lets it pick by its own
    /// preference (or there is no card).
    pub fn adapter(&self) -> Option<u32> {
        self.gpu().and_then(|g| g.dxgi)
    }

    /// The LUID of the card in use, to check against the device the app's
    /// probe expects.
    pub fn luid(&self) -> Option<u64> {
        self.gpu().and_then(|g| g.luid)
    }

    /// What Settings names the device in use: a card's label, the chip, or
    /// nothing (the processor, or a card DirectML picks itself).
    pub fn name(&self) -> &str {
        match self.using {
            Using::Gpu { .. } => self.gpu().map_or("", |g| g.label.as_str()),
            Using::Chip => &self.chip,
            Using::Preferred | Using::Processor => "",
        }
    }
}

impl fmt::Display for Hardware {
    /// The device in use, for the log: `"NVIDIA GeForce RTX 3060 Ti"
    /// (10de:24c9, adapter 0, LUID 0x10828); 1 card found`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.using, self.gpu()) {
            (Using::Gpu { .. }, Some(g)) => {
                write!(f, "\"{}\" ({:04x}:{:04x}", g.label, g.vendor, g.device)?;
                match g.dxgi {
                    Some(n) => write!(f, ", adapter {n}")?,
                    None => f.write_str(", DirectML's own pick")?,
                }
                if let Some(luid) = g.luid {
                    write!(f, ", LUID {luid:#x}")?;
                }
                f.write_str(")")?;
            }
            (Using::Chip, _) => write!(f, "the chip \"{}\"", self.chip)?,
            (Using::Preferred, _) | (Using::Gpu { .. }, None) => f.write_str("the graphics card DirectML picks")?,
            (Using::Processor, _) => write!(f, "the processor \"{}\"", self.cpu)?,
        }
        if self.missing {
            f.write_str(", standing in for the chosen card, which isn't connected")?;
        }
        if !self.gpus.is_empty() {
            let n = self.gpus.len();
            write!(f, "; {n} card{} found", if n == 1 { "" } else { "s" })?;
        }
        Ok(())
    }
}

/// What ORT's discovery reports here, read through its C API: `ort` has no
/// safe accessor for a device's vendor id or metadata (rc.15).
pub(crate) fn discover(env: &ort::environment::Environment) -> Hardware {
    let devices: Vec<RawDevice> = env
        .devices()
        .filter(|d| {
            // The cards come from DirectML's provider; another provider's
            // devices are the same cards again.
            let ep = d.ep().unwrap_or_default();
            d.hardware_device().ty() != ort::memory::DeviceType::GPU || ep == "DmlExecutionProvider"
        })
        .map(|d| raw(&d.hardware_device()))
        .collect();
    #[cfg(windows)]
    {
        match computer::machine() {
            computer::Machine::Windows(Some(adapters)) => {
                let order: Vec<u64> = adapters.iter().map(|a| a.luid).collect();
                Hardware::windows(&devices, Some(&order))
            }
            _ => Hardware::windows(&devices, None),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let brand = match computer::machine() {
            computer::Machine::Mac(brand) => brand.as_str(),
            _ => "",
        };
        Hardware::mac(&devices, brand)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Hardware::processor_only(&devices)
    }
}

fn raw(hw: &ort::device::HardwareDevice<'_>) -> RawDevice {
    use std::ffi::{CStr, c_char};

    use ort::AsPointer;
    let kind = match hw.ty() {
        ort::memory::DeviceType::GPU => Kind::Gpu,
        ort::memory::DeviceType::NPU => Kind::Npu,
        _ => Kind::Cpu,
    };
    let api = ort::api();
    let mut metadata = BTreeMap::new();
    // SAFETY: `hw` is a live device of the environment; the key-value pairs
    // belong to it and outlive this call; each key and value is a
    // NUL-terminated string.
    let vendor = unsafe {
        let pairs = (api.HardwareDevice_Metadata)(hw.ptr());
        if !pairs.is_null() {
            let (mut keys, mut values, mut n): (*const *const c_char, *const *const c_char, usize) =
                (std::ptr::null(), std::ptr::null(), 0);
            (api.GetKeyValuePairs)(pairs, &mut keys, &mut values, &mut n);
            if !keys.is_null() && !values.is_null() {
                for i in 0..n {
                    let (k, v) = (*keys.add(i), *values.add(i));
                    if !k.is_null() && !v.is_null() {
                        let text = |p: *const c_char| CStr::from_ptr(p).to_string_lossy().into_owned();
                        metadata.insert(text(k), text(v));
                    }
                }
            }
        }
        (api.HardwareDevice_VendorId)(hw.ptr())
    };
    if kind == Kind::Cpu && !metadata.contains_key("Description") {
        metadata.insert("Description".into(), hw.vendor().unwrap_or_default().to_string());
    }
    RawDevice { kind, vendor, device: hw.id(), metadata }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::{Adapter, Class, Machine, choose};

    const NVIDIA: u32 = 0x10DE;
    const INTEL: u32 = 0x8086;

    fn device(kind: Kind, vendor: u32, device: u32, meta: &[(&str, &str)]) -> RawDevice {
        let metadata = meta.iter().map(|&(k, v)| (k.to_string(), v.to_string())).collect();
        RawDevice { kind, vendor, device, metadata }
    }

    fn cpu() -> RawDevice {
        device(Kind::Cpu, 0x1022, 0, &[("Description", "AMD Ryzen 7 5800X3D 8-Core Processor           ")])
    }

    /// A card as ORT 1.24.4 reports it after its D3D12 pass.
    fn card(vendor: u32, dev: u32, name: &str, dxgi: u32, fast: u32, luid: u64) -> RawDevice {
        let (dxgi, fast, luid) = (dxgi.to_string(), fast.to_string(), luid.to_string());
        device(
            Kind::Gpu,
            vendor,
            dev,
            &[
                ("Description", name),
                ("DxgiAdapterNumber", &dxgi),
                ("DxgiHighPerformanceIndex", &fast),
                ("DxgiVideoMemory", "8031 MB"),
                ("LUID", &luid),
            ],
        )
    }

    /// A card as discovery reports it when only DXCore could be read.
    fn dxcore_only(vendor: u32, dev: u32, name: &str, luid: u64, discrete: bool) -> RawDevice {
        let luid = luid.to_string();
        let discrete = if discrete { "1" } else { "0" };
        device(Kind::Gpu, vendor, dev, &[("Description", name), ("LUID", &luid), ("Discrete", discrete)])
    }

    /// A hybrid laptop: DXGI numbers the built-in GPU 0 but ranks the RTX
    /// fastest; DirectML's own list (`order`) starts with the RTX.
    fn hybrid() -> Vec<RawDevice> {
        vec![
            cpu(),
            card(INTEL, 0xA7A0, "Intel(R) Iris(R) Xe Graphics", 0, 1, 0x1_0001),
            card(NVIDIA, 0x28E0, "NVIDIA GeForce RTX 4060 Laptop GPU", 1, 0, 0x1_0002),
            device(Kind::Npu, INTEL, 0x7D1D, &[("Description", "Intel(R) AI Boost")]),
        ]
    }

    #[test]
    fn gpus_from_ort_metadata_names_each_card_and_the_fastest() {
        let hw = Hardware::windows(&hybrid(), Some(&[0x1_0002, 0x1_0001]));
        assert_eq!(hw.cpu, "AMD Ryzen 7 5800X3D 8-Core Processor");
        let names: Vec<&str> = hw.gpus.iter().map(|g| g.name.as_str()).collect();
        // DirectML's order, the NPU left out, the names cleaned.
        assert_eq!(names, ["NVIDIA GeForce RTX 4060 Laptop GPU", "Intel Iris Xe Graphics"]);
        let rtx = &hw.gpus[0];
        assert_eq!((rtx.vendor, rtx.device, rtx.dxgi, rtx.high_performance), (NVIDIA, 0x28E0, Some(1), Some(0)));
        assert_eq!((rtx.luid, rtx.memory_mb, rtx.nth), (Some(0x1_0002), Some(8031), 0));
        assert_eq!(rtx.key, "10de:28e0:NVIDIA GeForce RTX 4060 Laptop GPU");
        assert_eq!(rtx.label, rtx.name);
        // Automatic opens the fast card, by its adapter number.
        assert_eq!(hw.auto, Using::Gpu { index: 0 });
        assert_eq!(
            (hw.adapter(), hw.luid(), hw.name()),
            (Some(1), Some(0x1_0002), "NVIDIA GeForce RTX 4060 Laptop GPU")
        );
        // Without DirectML's list, the card DXGI ranks fastest: the same one.
        assert_eq!(Hardware::windows(&hybrid(), None).gpu().map(|g| g.luid), Some(Some(0x1_0002)));
        assert_eq!(
            hw.to_string(),
            "\"NVIDIA GeForce RTX 4060 Laptop GPU\" (10de:28e0, adapter 1, LUID 0x10002); 2 cards found"
        );
    }

    #[test]
    fn identical_cards_get_distinct_keys() {
        let twins = [
            cpu(),
            card(NVIDIA, 0x2204, "NVIDIA GeForce RTX 3090", 1, 1, 0x200),
            card(NVIDIA, 0x2204, "NVIDIA GeForce RTX 3090", 0, 0, 0x100),
        ];
        let hw = Hardware::windows(&twins, Some(&[0x100, 0x200]));
        let keys: Vec<&str> = hw.gpus.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, ["10de:2204:NVIDIA GeForce RTX 3090", "10de:2204:NVIDIA GeForce RTX 3090#2"]);
        let labels: Vec<&str> = hw.gpus.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["NVIDIA GeForce RTX 3090 (1)", "NVIDIA GeForce RTX 3090 (2)"]);
        // Each key opens its own card, and reads back as the same choice.
        for (i, g) in hw.gpus.iter().enumerate() {
            let choice = Choice::from_setting(&g.key);
            assert_eq!(choice, Choice::Card { vendor: NVIDIA, device: 0x2204, nth: i as u32 });
            assert_eq!(hw.resolve(&choice), (Using::Gpu { index: i }, false));
        }
        assert_eq!(hw.clone().chosen(&Choice::from_setting(&hw.gpus[1].key)).adapter(), Some(1));
        // A card listed by two providers is one card.
        let again = [twins.as_slice(), &twins[1..]].concat();
        assert_eq!(Hardware::windows(&again, Some(&[0x100, 0x200])).gpus.len(), 2);
    }

    #[test]
    fn names_are_trimmed() {
        let raw = [card(NVIDIA, 0x24C9, "  NVIDIA  GeForce RTX(TM) 3060 Ti   ", 0, 0, 1)];
        let hw = Hardware::windows(&raw, Some(&[1]));
        assert_eq!((hw.gpus[0].name.as_str(), hw.cpu.as_str()), ("NVIDIA GeForce RTX 3060 Ti", ""));
        let raw = [cpu(), device(Kind::Gpu, NVIDIA, 0x24C9, &[("DxgiAdapterNumber", " 0 "), ("LUID", "1")])];
        let hw = Hardware::windows(&raw, Some(&[1]));
        assert_eq!((hw.gpus[0].name.as_str(), hw.gpus[0].dxgi), ("Graphics card", Some(0)));
        assert_eq!(hw.cpu, "AMD Ryzen 7 5800X3D 8-Core Processor");
    }

    #[test]
    fn a_chosen_card_resolves_to_its_adapter_number() {
        let hw = Hardware::windows(&hybrid(), Some(&[0x1_0002, 0x1_0001]));
        let iris = Choice::from_setting("8086:a7a0:Intel Iris Xe Graphics");
        assert_eq!(iris, Choice::Card { vendor: INTEL, device: 0xA7A0, nth: 0 });
        let chosen = hw.clone().chosen(&iris);
        assert_eq!((chosen.using, chosen.missing, chosen.adapter()), (Using::Gpu { index: 1 }, false, Some(0)));
        assert_eq!(chosen.name(), "Intel Iris Xe Graphics");
    }

    #[test]
    fn a_missing_card_falls_back_to_automatic() {
        let hw = Hardware::windows(&hybrid(), Some(&[0x1_0002, 0x1_0001]));
        let gone = hw.clone().chosen(&Choice::from_setting("1002:744c:AMD Radeon RX 7900 XTX"));
        assert_eq!((gone.using, gone.missing, gone.adapter()), (hw.auto, true, Some(1)));
        assert!(gone.to_string().contains("standing in for the chosen card"), "{gone}");
        // Settings' other values: Automatic, Processor only, and whatever a
        // newer app stored, which reads as Automatic.
        for (setting, want) in
            [("", Choice::Auto), ("cpu", Choice::Processor), ("gpu:2", Choice::Auto), ("x:y:z", Choice::Auto)]
        {
            assert_eq!(Choice::from_setting(setting), want, "{setting:?}");
        }
        // No card to choose from: nothing is missing.
        let none = Hardware::windows(&[cpu()], Some(&[]));
        assert_eq!(
            none.resolve(&Choice::from_setting("10de:24c9:NVIDIA GeForce RTX 3060 Ti")),
            (Using::Processor, false)
        );
    }

    #[test]
    fn without_dxgi_data_automatic_keeps_the_preference_path() {
        // The D3D12 pass failed: DXCore's records only, no adapter numbers.
        let raw = [
            cpu(),
            dxcore_only(INTEL, 0xA7A0, "Intel Iris Xe Graphics", 0x1_0001, false),
            dxcore_only(NVIDIA, 0x28E0, "NVIDIA GeForce RTX 4060 Laptop GPU", 0x1_0002, true),
        ];
        // DirectML's list names the RTX: it is opened by DirectML's own pick,
        // which is that card, and named.
        let hw = Hardware::windows(&raw, Some(&[0x1_0002, 0x1_0001]));
        assert_eq!(
            (hw.auto, hw.adapter(), hw.name()),
            (Using::Gpu { index: 0 }, None, "NVIDIA GeForce RTX 4060 Laptop GPU")
        );
        assert_eq!(hw.gpus[0].discrete, Some(true));
        // A card that can't be opened by number isn't chosen: Automatic.
        assert_eq!(hw.resolve(&Choice::from_setting("8086:a7a0:Intel Iris Xe Graphics")), (hw.auto, false));
        // No list to match against and no ranking: DirectML picks, unnamed.
        let unmatched = Hardware::windows(&raw, None);
        assert_eq!((unmatched.auto, unmatched.adapter(), unmatched.name()), (Using::Preferred, None, ""));
        // One card: that one.
        let one = Hardware::windows(&raw[..2], None);
        assert_eq!((one.auto, one.name()), (Using::Gpu { index: 0 }, "Intel Iris Xe Graphics"));
    }

    #[test]
    fn no_gpu_means_the_processor() {
        // CI's Windows: only the software adapter, which neither discovery
        // nor DirectML's list keeps.
        let hw = Hardware::windows(&[cpu()], Some(&[]));
        assert_eq!((hw.auto, hw.using, hw.name(), hw.adapter()), (Using::Processor, Using::Processor, "", None));
        assert_eq!(hw.to_string(), "the processor \"AMD Ryzen 7 5800X3D 8-Core Processor\"");
        // DirectML's list is empty although discovery found a card (a list of
        // NPUs only): its default refuses, so the processor, as before.
        let raw = [cpu(), card(NVIDIA, 0x24C9, "NVIDIA GeForce RTX 3060 Ti", 0, 0, 1)];
        assert_eq!(Hardware::windows(&raw, Some(&[])).auto, Using::Processor);
        // Linux.
        assert_eq!(Hardware::processor_only(&[cpu()]).resolve(&Choice::Auto), (Using::Processor, false));
    }

    #[test]
    fn processor_only_builds_no_gpu_session() {
        let hw = Hardware::windows(&hybrid(), Some(&[0x1_0002, 0x1_0001])).chosen(&Choice::Processor);
        assert_eq!((hw.using, hw.adapter(), hw.luid(), hw.name()), (Using::Processor, None, None, ""));
        let mac = Hardware::mac(&[cpu()], "Apple M2");
        assert_eq!((mac.auto, mac.name()), (Using::Chip, "Apple M2"));
        assert_eq!(mac.chosen(&Choice::Processor).using, Using::Processor);
        // An Intel Mac's brand names its processor, not a chip.
        assert_eq!(Hardware::mac(&[], "Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz").chip, "");
    }

    #[test]
    fn auto_after_hw1_classes_the_adapter_directml_opens() {
        // DXGI numbers and ranks the built-in GPU first, but DirectML's own
        // list starts with the RTX (a laptop whose ML list leaves the GPU
        // order to DXCore): Automatic opens what DirectML's default opens,
        // and Fast Scan's default classes that very card.
        let raw = [
            cpu(),
            card(INTEL, 0xA7A0, "Intel(R) Iris(R) Xe Graphics", 0, 0, 0x1_0001),
            card(NVIDIA, 0x28E0, "NVIDIA GeForce RTX 4060 Laptop GPU", 1, 1, 0x1_0002),
        ];
        let rtx = Adapter {
            vendor: NVIDIA,
            device: 0x28E0,
            name: "NVIDIA GeForce RTX 4060 Laptop GPU".into(),
            integrated: false,
            luid: 0x1_0002,
        };
        let iris = Adapter {
            vendor: INTEL,
            device: 0xA7A0,
            name: "Intel Iris Xe Graphics".into(),
            integrated: true,
            luid: 0x1_0001,
        };
        for (list, class) in [(vec![rtx.clone(), iris.clone()], Class::ForAi), (vec![iris, rtx], Class::Integrated)] {
            let order: Vec<u64> = list.iter().map(|a| a.luid).collect();
            let hw = Hardware::windows(&raw, Some(&order));
            let probe = choose(&Machine::Windows(Some(list)), &Choice::Auto);
            assert_eq!((hw.luid(), probe.class), (probe.luid, class));
            // Adapter numbers follow DXGI, whatever the order.
            assert_eq!(hw.adapter(), Some(if class == Class::ForAi { 1 } else { 0 }));
        }
    }

    #[test]
    fn a_chosen_card_matches_across_description_spellings() {
        // ORT spells the card from DXGI, DXCore from the driver, and a driver
        // update renamed it since it was chosen: still that card, both sides.
        let raw = [cpu(), card(NVIDIA, 0x1F82, "NVIDIA GeForce GTX 1650", 0, 0, 0x300)];
        let gtx = Adapter {
            vendor: NVIDIA,
            device: 0x1F82,
            name: "NVIDIA GeForce GTX 1650 (Notebook)".into(),
            integrated: false,
            luid: 0x300,
        };
        let iris = Adapter {
            vendor: INTEL,
            device: 0xA7A0,
            name: "Intel Iris Xe Graphics".into(),
            integrated: true,
            luid: 0x400,
        };
        let m = Machine::Windows(Some(vec![iris, gtx]));
        let hw = Hardware::windows(&raw, Some(&[0x400, 0x300]));
        for stored in
            ["10de:1f82:NVIDIA GeForce GTX 1650", "10de:1f82:NVIDIA GeForce GTX 1650 SUPER (old driver)", "10DE:1F82:"]
        {
            let choice = Choice::from_setting(stored);
            let probe = choose(&m, &choice);
            let engine = hw.clone().chosen(&choice);
            assert_eq!((engine.luid(), engine.missing), (Some(0x300), false), "{stored}");
            assert_eq!((probe.luid, probe.class), (Some(0x300), Class::NotForAi), "{stored}");
        }
    }

    #[test]
    fn the_hardware_travels_and_an_old_line_reads_as_unknown() {
        let hw = Hardware::windows(&hybrid(), Some(&[0x1_0002, 0x1_0001])).chosen(&Choice::Processor);
        let back: Hardware = serde_json::from_str(&serde_json::to_string(&hw).unwrap()).unwrap();
        assert_eq!(back, hw);
        let old: Hardware = serde_json::from_str("{}").unwrap();
        assert_eq!(old, Hardware::default());
    }
}
