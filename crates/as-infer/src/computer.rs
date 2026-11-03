//! What this computer analyses on, and whether those graphics are built for AI
//! work: what Fast Scan's default for new projects follows (design-i52 §4).
//!
//! "Built for AI work" means matrix units the runtimes use for these models:
//! NVIDIA's tensor cores, AMD's WMMA units (RDNA 3 and newer) and matrix cores
//! (Instinct), Intel's XMX engines (Arc). It decides only for separate cards;
//! built-in graphics, every Mac's included, are a class of their own.
//!
//! - **Windows**: the DXCore adapters in the order ONNX Runtime's DirectML
//!   default takes them ([`ort_order`]). `dxcore.dll` is loaded at run time,
//!   as ORT loads it, so a Windows without DXCore still starts AutoScene.
//! - **macOS**: every Mac's GPU is built in; the chip names it.
//! - **Linux and anything else**: the runtime AutoScene ships there computes
//!   on the processor only, so nothing is probed.
//!
//! The adapter list is read once per process ([`machine`]); the device the
//! person chose is applied on every call ([`choose`]).

use std::fmt;
use std::sync::OnceLock;

/// What a computer's analysis runs on, as Fast Scan's default sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// A separate graphics card built for AI work.
    ForAi,
    /// A separate graphics card without matrix units, or a virtual adapter.
    NotForAi,
    /// Built-in graphics that share the processor's memory, every Mac's included.
    Integrated,
    /// No graphics the runtime takes: Linux, Windows without a hardware
    /// adapter DirectML opens, or "Processor only".
    Processor,
    /// The probe couldn't tell: DXCore can't be read, or an unknown vendor.
    Unknown,
}

impl Class {
    /// New projects start with Fast Scan on this class (the owner's rule,
    /// 2026-11-03). `Unknown` keeps the full scan: the rule turns Fast Scan
    /// on only where the graphics are known not to be built for AI work.
    pub fn fast_by_default(self) -> bool {
        matches!(self, Self::NotForAi | Self::Integrated | Self::Processor)
    }
}

/// One DXCore adapter: its PCI ids, its driver's description (cleaned), the
/// driver's built-in flag and its LUID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub vendor: u32,
    pub device: u32,
    pub name: String,
    pub integrated: bool,
    /// `InstanceLuid`: `LowPart` in the low 32 bits, `HighPart`'s bits above.
    pub luid: u64,
}

/// What the probe found; read once per process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Machine {
    /// Windows: the adapters in the order ORT's DirectML default takes them
    /// ([`ort_order`]); `None` when DXCore can't be read.
    Windows(Option<Vec<Adapter>>),
    /// macOS: the chip's brand string ("Apple M2").
    Mac(String),
    /// The runtime shipped here computes on the processor only (Linux, other
    /// systems).
    CpuRuntime,
}

/// The computer as the analysis sees it, for one choice of device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Computer {
    pub class: Class,
    /// The graphics as their driver names them, cleaned of "(R)", "(TM)",
    /// "®", "™" and doubled spaces ("Intel Iris Xe Graphics", "Apple M2");
    /// empty for the processor and when nothing names them.
    pub name: String,
    /// The adapter's LUID (Windows), to check against the device the engine
    /// opened.
    pub luid: Option<u64>,
}

impl Computer {
    fn processor() -> Self {
        Self { class: Class::Processor, name: String::new(), luid: None }
    }

    fn unknown() -> Self {
        Self { class: Class::Unknown, name: String::new(), luid: None }
    }

    fn adapter(a: &Adapter) -> Self {
        Self { class: classify(a), name: a.name.clone(), luid: Some(a.luid) }
    }
}

impl fmt::Display for Computer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.class {
            Class::ForAi => "graphics built for AI work",
            Class::NotForAi => "graphics not built for AI work",
            Class::Integrated => "built-in graphics",
            Class::Processor => return f.write_str("the processor"),
            Class::Unknown => "graphics of unknown kind",
        };
        f.write_str(what)?;
        if !self.name.is_empty() {
            write!(f, " \"{}\"", self.name)?;
        }
        match self.luid {
            Some(luid) => write!(f, ", LUID {luid:#x}"),
            None => Ok(()),
        }
    }
}

/// Which device the analysis is asked to use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    /// The adapter DirectML's default opens: the first of [`ort_order`].
    #[default]
    Auto,
    /// The `nth` card (from 0, in [`ort_order`]'s order) with these PCI ids;
    /// identical cards differ only by `nth`. The driver's description never
    /// decides: drivers rename cards.
    Card { vendor: u32, device: u32, nth: u32 },
    /// "Processor only".
    Processor,
}

/// The computer as the analysis sees it when `choice` decides the device.
///
/// - "Processor only" is the processor everywhere.
/// - Windows: `Auto` is the first adapter, or the processor when there is
///   none (DirectML refuses software adapters; `as-infer` then builds every
///   session on the processor). A chosen card that isn't there any more falls
///   back to `Auto`. Without DXCore the graphics are `Unknown`.
/// - macOS: the chip's built-in graphics.
/// - A processor-only runtime: the processor.
pub fn choose(machine: &Machine, choice: &Choice) -> Computer {
    if *choice == Choice::Processor {
        return Computer::processor();
    }
    match machine {
        Machine::Windows(None) => Computer::unknown(),
        Machine::Windows(Some(adapters)) => {
            let chosen = match *choice {
                Choice::Card { vendor, device, nth } => {
                    let same: Vec<&Adapter> =
                        adapters.iter().filter(|a| a.vendor == vendor && a.device == device).collect();
                    same.get(nth as usize).or(same.first()).copied()
                }
                _ => None,
            };
            chosen.or(adapters.first()).map_or_else(Computer::processor, Computer::adapter)
        }
        Machine::Mac(brand) => apple(brand),
        Machine::CpuRuntime => Computer::processor(),
    }
}

const NVIDIA: u32 = 0x10DE;
const AMD: u32 = 0x1002;
const INTEL: u32 = 0x8086;
/// Virtual adapters: VMware, VirtualBox, Parallels, QEMU, Red Hat (virtio,
/// QXL), Microsoft (only hardware-flagged ones reach [`classify`]), Xen.
const VIRTUAL: [u32; 8] = [0x15AD, 0x80EE, 0x1AB8, 0x1234, 0x1AF4, 0x1B36, 0x1414, 0x5853];

/// The class of a hardware adapter with D3D12 graphics (the only kind
/// [`ort_order`] keeps). Device ids from the PCI ID database 2026.10.31; the
/// whole table and a sweep over every NVIDIA and AMD entry are in
/// design-i52 E6. A new card family is one line here and one test row.
pub fn classify(a: &Adapter) -> Class {
    // Built-in graphics, whoever makes them (Intel, AMD, Qualcomm, NVIDIA's
    // own): the driver's flag decides.
    if a.integrated {
        return Class::Integrated;
    }
    let d = a.device;
    match a.vendor {
        NVIDIA => {
            let name = a.name.to_ascii_uppercase();
            let for_ai = if names_mx(&name) {
                // GeForce MX: entry laptop chips; even the MX570's tensor units
                // sit on 2 GB and a 64-bit bus (Notebookcheck).
                false
            } else {
                // Every RTX part has tensor cores (GeForce, Quadro and NVIDIA
                // RTX, TITAN RTX); Volta brought them (TITAN V, V100, Quadro
                // GV100); the Turing data-centre cards T10, T40, T4G, T4; GA100
                // (A100, A30, A800); Ampere and newer throughout. Below 0x2200:
                // Pascal and older, the GTX 16 series (Turing without tensor
                // cores, TU116/TU117), the GTX 1650 on any die, Quadro T, CMP.
                name.contains("RTX")
                    || (0x1D80..=0x1DFF).contains(&d)
                    || matches!(d, 0x1E35 | 0x1E38 | 0x1EB4 | 0x1EB8)
                    || (0x20B0..=0x20FF).contains(&d)
                    || d >= 0x2200
            };
            if for_ai { Class::ForAi } else { Class::NotForAi }
        }
        // RDNA 3 added WMMA (GPUOpen, "WMMA on RDNA 3"): RDNA 3 and 4 and
        // Instinct MI300/MI350 sit at 0x7440-0x77FF, with room for the next
        // generations (0x78xx-0x9Fxx are 2003-2014 chips); the RX 7600M XT
        // (Navi 33) at 0x73F0; CDNA's matrix cores on Instinct MI100 and MI200.
        // RDNA 2 and older have none, and an APU whose driver leaves the
        // built-in flag unset lands here too, which still gets Fast Scan.
        AMD if (0x7440..=0x77FF).contains(&d)
            || matches!(d, 0x73F0 | 0x7388 | 0x738C | 0x738E | 0x7408 | 0x740C | 0x740F) =>
        {
            Class::ForAi
        }
        AMD => Class::NotForAi,
        // DG1 (Iris Xe MAX) is Xe-LP without XMX engines.
        INTEL if (0x4905..=0x4909).contains(&d) => Class::NotForAi,
        // XMX engines: DG2 (Arc A, Arc Pro A, Flex), Battlemage (Arc B, Arc
        // Pro B), Ponte Vecchio.
        INTEL
            if (0x4F80..=0x4F88).contains(&d)
                || (0x5690..=0x56FF).contains(&d)
                || (0xE200..=0xE2FF).contains(&d)
                || (0x0B60..=0x0BFF).contains(&d) =>
        {
            Class::ForAi
        }
        // Intel makes no other separate graphics: a driver that leaves the
        // built-in flag unset.
        INTEL => Class::Integrated,
        v if VIRTUAL.contains(&v) => Class::NotForAi,
        _ => Class::Unknown,
    }
}

/// "MX" as a word followed by a number, as in "GeForce MX450" or "MX 570".
fn names_mx(upper: &str) -> bool {
    upper.match_indices("MX").any(|(i, _)| {
        let word_starts = upper[..i].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        let rest = &upper[i + 2..];
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        word_starts && rest.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// A Mac: its GPU is always built in, named by the chip's brand string
/// ("Apple M2", "Apple M3 Max", "Apple M1 (Virtual)" on CI). An Intel Mac's
/// brand string names the processor instead, so it gives no name.
pub fn apple(brand: &str) -> Computer {
    let brand = clean(brand);
    let name = if brand.starts_with("Apple ") { brand } else { String::new() };
    Computer { class: Class::Integrated, name, luid: None }
}

/// A name as a driver or `sysctl` hands it over: it ends at its first NUL; it
/// is UTF-8, or else read as Windows-1252 for ® (0xAE) and ™ (0x99); then
/// cleaned.
#[cfg(any(windows, target_os = "macos", test))]
fn description(raw: &[u8]) -> String {
    let raw = &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())];
    match std::str::from_utf8(raw) {
        Ok(s) => clean(s),
        Err(_) => clean(&raw.iter().map(|&b| if b == 0x99 { '™' } else { char::from(b) }).collect::<String>()),
    }
}

/// Drops "(R)", "(TM)", "®" and "™" and collapses runs of spaces.
fn clean(name: &str) -> String {
    let mut s = name.replace(['®', '™'], "");
    for mark in ["(R)", "(r)", "(TM)", "(tm)"] {
        s = s.replace(mark, "");
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One adapter of a DXCore list, as the probe read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub adapter: Adapter,
    /// `IsHardware`: false for the Microsoft Basic Render Driver.
    pub hardware: bool,
    /// Supports `DXCORE_ADAPTER_ATTRIBUTE_D3D12_GRAPHICS` (an NPU doesn't).
    pub graphics: bool,
}

impl fmt::Display for Listed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let a = &self.adapter;
        // `IsHardware`, then the driver's `IsIntegrated`.
        let kind = match (self.hardware, a.integrated) {
            (false, _) => "software",
            (true, true) => "built in",
            (true, false) => "separate",
        };
        write!(f, "{:04x}:{:04x} \"{}\" LUID {:#x}, {kind}", a.vendor, a.device, a.name, a.luid)?;
        if !self.graphics {
            f.write_str(", no graphics")?;
        }
        Ok(())
    }
}

/// The DXCore list DirectML's default takes its adapters from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    GenericMl,
    CoreCompute,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::GenericMl => "GENERIC_ML",
            Self::CoreCompute => "CORE_COMPUTE",
        })
    }
}

/// The adapters DirectML can open, in the order ONNX Runtime's DirectML
/// default takes them. That is what [`crate::Runtime`] asks for: ORT 1.24.4
/// (the Windows runtime AutoScene ships), `SessionOptionsAppendExecutionProvider_DML2`
/// with the `Gpu` filter and `HighPerformance`, which ends in
/// `dml_provider_factory.cc`:
/// 1. `:208-227` the list of adapters with `D3D12_GENERIC_ML`; only when it
///    holds none, the list with `D3D12_CORE_COMPUTE`;
/// 2. `:229-250`, `:350` sorted by `[HighPerformance]` alone (the reader sorts
///    both lists that way, so they arrive here in that order);
/// 3. `:168-174`, `:252-269`, `:359` only hardware adapters with
///    `D3D12_GRAPHICS` kept, in that order (`:271-303` orders GPUs against
///    NPUs only, and every adapter kept here is a GPU);
/// 4. `:345-346`, `:360-361` none left: ORT throws, and `as-infer` builds
///    every session on the processor; `:628` otherwise it opens the first.
///
/// Returns the list it took and the adapters it kept.
pub fn ort_order(generic_ml: &[Listed], core_compute: &[Listed]) -> (Source, Vec<Adapter>) {
    let (source, list) =
        if generic_ml.is_empty() { (Source::CoreCompute, core_compute) } else { (Source::GenericMl, generic_ml) };
    (source, list.iter().filter(|l| l.hardware && l.graphics).map(|l| l.adapter.clone()).collect())
}

/// Both DXCore lists DirectML's default can take its adapters from, each
/// sorted by `[HighPerformance]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lists {
    pub generic_ml: Vec<Listed>,
    pub core_compute: Vec<Listed>,
}

impl Lists {
    /// The device `Auto` analyses on, and the list it came from with every
    /// adapter in it: the line the probe logs at start, for support.
    pub fn line(&self) -> String {
        let (source, adapters) = ort_order(&self.generic_ml, &self.core_compute);
        let auto = adapters.first().map_or_else(Computer::processor, Computer::adapter);
        let listed = match source {
            Source::GenericMl => &self.generic_ml,
            Source::CoreCompute => &self.core_compute,
        };
        let listed = listed.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
        format!("{auto}; DirectML's list {source}: [{listed}]")
    }

    /// Reads both lists with `dxcore.dll` loaded at run time; `None` when it,
    /// its factory or a list can't be read.
    #[cfg(windows)]
    pub fn read() -> Option<Self> {
        dxcore::read(dxcore::load)
    }
}

/// Reads what this computer analyses on (a few milliseconds on Windows) and
/// logs it. [`machine`] keeps the answer for the process.
pub fn probe() -> Machine {
    #[cfg(windows)]
    fn here() -> Machine {
        windows(dxcore::load)
    }
    #[cfg(target_os = "macos")]
    fn here() -> Machine {
        let m = Machine::Mac(mac_brand());
        tracing::info!("this computer: {}", choose(&m, &Choice::Auto));
        m
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    fn here() -> Machine {
        tracing::info!("this computer: the processor (AutoScene's runtime here computes on the processor only)");
        Machine::CpuRuntime
    }
    here()
}

/// What this computer analyses on, probed once per process.
pub fn machine() -> &'static Machine {
    static MACHINE: OnceLock<Machine> = OnceLock::new();
    MACHINE.get_or_init(probe)
}

/// The Windows probe, with the loader of `DXCoreCreateAdapterFactory` handed
/// in so a test can give it one that finds nothing.
#[cfg(windows)]
fn windows(load: impl FnOnce() -> Option<dxcore::CreateFactory>) -> Machine {
    match dxcore::read(load) {
        Some(lists) => {
            tracing::info!("this computer: {}", lists.line());
            Machine::Windows(Some(ort_order(&lists.generic_ml, &lists.core_compute).1))
        }
        None => {
            tracing::warn!("this computer: DXCore can't be read, so its graphics are of unknown kind");
            Machine::Windows(None)
        }
    }
}

#[cfg(target_os = "macos")]
fn mac_brand() -> String {
    let key = c"machdep.cpu.brand_string";
    let mut size: libc::size_t = 0;
    // SAFETY: a NUL-terminated name; a null buffer asks for the size only.
    if unsafe { libc::sysctlbyname(key.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) } != 0 {
        return String::new();
    }
    let mut buf = vec![0u8; size];
    // SAFETY: `buf` holds `size` writable bytes; `size` comes back as the
    // number written.
    if unsafe { libc::sysctlbyname(key.as_ptr(), buf.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) } != 0 {
        return String::new();
    }
    description(&buf[..size.min(buf.len())])
}

/// DXCore through `dxcore.dll` loaded at run time, as ORT loads it
/// (`dml_provider_factory.cc:322`, "so there's not a hard dependency on
/// dxcore"). Only the interfaces come from the `windows` crate: its own
/// `DXCoreCreateAdapterFactory` is a load-time import, and without
/// `dxcore.dll` (Windows 10 before 2004) AutoScene wouldn't start.
#[cfg(windows)]
mod dxcore {
    use super::{Adapter, Listed, Lists, description};
    use core::ffi::c_void;
    use windows::Win32::Foundation::LUID;
    use windows::Win32::Graphics::DXCore::{
        DXCORE_ADAPTER_ATTRIBUTE_D3D12_CORE_COMPUTE, DXCORE_ADAPTER_ATTRIBUTE_D3D12_GRAPHICS, DXCoreAdapterProperty,
        DXCoreHardwareID, DriverDescription, HardwareID, HighPerformance, IDXCoreAdapter, IDXCoreAdapterFactory,
        IDXCoreAdapterList, InstanceLuid, IsHardware, IsIntegrated,
    };
    use windows::core::{GUID, HRESULT, Interface};

    /// `DXCORE_ADAPTER_ATTRIBUTE_D3D12_GENERIC_ML`, which `windows` 0.62.2
    /// lacks: `dxcore_interface.h` (SDK 10.0.26100), defined locally as ORT
    /// does (`device_discovery.cc:121`).
    const GENERIC_ML: GUID = GUID::from_u128(0xb71b0d41_1088_422f_a27c_0250b7d3a988);

    /// `DXCoreCreateAdapterFactory` (`dxcore.h`).
    pub(super) type CreateFactory = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> HRESULT;

    /// Finds `DXCoreCreateAdapterFactory` in System32's `dxcore.dll`. The DLL
    /// stays loaded for the life of the process.
    pub(super) fn load() -> Option<CreateFactory> {
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
        let name: Vec<u16> = "dxcore.dll\0".encode_utf16().collect();
        // SAFETY: a NUL-terminated wide name, no file handle; System32 only.
        let module = unsafe { LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return None;
        }
        // SAFETY: a loaded module and a NUL-terminated export name.
        let f = unsafe { GetProcAddress(module, c"DXCoreCreateAdapterFactory".as_ptr().cast()) }?;
        // SAFETY: the export has `CreateFactory`'s signature (`dxcore.h`).
        Some(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateFactory>(f) })
    }

    pub(super) fn read(load: impl FnOnce() -> Option<CreateFactory>) -> Option<Lists> {
        let create = load()?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: `create` is `DXCoreCreateAdapterFactory`; a valid IID and out
        // pointer.
        unsafe { create(&IDXCoreAdapterFactory::IID, &mut raw) }.ok().ok()?;
        if raw.is_null() {
            return None;
        }
        // SAFETY: on success `raw` holds a reference to an
        // `IDXCoreAdapterFactory`, which this takes over.
        let factory = unsafe { IDXCoreAdapterFactory::from_raw(raw) };
        Some(Lists {
            generic_ml: list(&factory, &GENERIC_ML).ok()?,
            core_compute: list(&factory, &DXCORE_ADAPTER_ATTRIBUTE_D3D12_CORE_COMPUTE).ok()?,
        })
    }

    /// The adapters with `attribute`, sorted as ORT sorts them
    /// (`dml_provider_factory.cc:229-250`).
    fn list(factory: &IDXCoreAdapterFactory, attribute: &GUID) -> windows::core::Result<Vec<Listed>> {
        // SAFETY: plain COM calls on live interfaces.
        unsafe {
            let list: IDXCoreAdapterList = factory.CreateAdapterList(std::slice::from_ref(attribute))?;
            if list.GetAdapterCount() > 1 {
                list.Sort(&[HighPerformance])?;
            }
            (0..list.GetAdapterCount()).map(|i| list.GetAdapter::<IDXCoreAdapter>(i).map(|a| listed(&a))).collect()
        }
    }

    fn listed(a: &IDXCoreAdapter) -> Listed {
        // SAFETY: each buffer is as large as the size passed with it; a failed
        // read leaves its default (false, 0, an empty name).
        unsafe {
            let flag = |property: DXCoreAdapterProperty| {
                let mut v = 0u8;
                a.GetProperty(property, 1, (&raw mut v).cast()).is_ok() && v != 0
            };
            let mut luid = LUID::default();
            let _ = a.GetProperty(InstanceLuid, size_of::<LUID>(), (&raw mut luid).cast());
            let mut id = DXCoreHardwareID::default();
            let _ = a.GetProperty(HardwareID, size_of::<DXCoreHardwareID>(), (&raw mut id).cast());
            let size = a.GetPropertySize(DriverDescription).unwrap_or(0);
            let mut name = vec![0u8; size];
            if size > 0 && a.GetProperty(DriverDescription, size, name.as_mut_ptr().cast()).is_err() {
                name.clear();
            }
            Listed {
                adapter: Adapter {
                    vendor: id.vendorID,
                    device: id.deviceID,
                    name: description(&name),
                    integrated: flag(IsIntegrated),
                    luid: (u64::from(luid.HighPart as u32) << 32) | u64::from(luid.LowPart),
                },
                hardware: flag(IsHardware),
                graphics: a.IsAttributeSupported(&DXCORE_ADAPTER_ATTRIBUTE_D3D12_GRAPHICS),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(vendor: u32, device: u32, name: &str) -> Adapter {
        Adapter { vendor, device, name: name.into(), integrated: false, luid: u64::from(device) }
    }

    fn built_in(vendor: u32, device: u32, name: &str) -> Adapter {
        Adapter { integrated: true, ..card(vendor, device, name) }
    }

    fn check(rows: &[(u32, u32, &str)], make: fn(u32, u32, &str) -> Adapter, want: Class) {
        for &(vendor, device, name) in rows {
            assert_eq!(classify(&make(vendor, device, name)), want, "{vendor:04x}:{device:04x} {name}");
        }
    }

    fn gpu(a: Adapter) -> Listed {
        Listed { adapter: a, hardware: true, graphics: true }
    }

    fn npu(a: Adapter) -> Listed {
        Listed { adapter: a, hardware: true, graphics: false }
    }

    fn basic_render() -> Listed {
        Listed { adapter: card(0x1414, 0x008C, "Microsoft Basic Render Driver"), hardware: false, graphics: true }
    }

    fn auto(generic_ml: &[Listed], core_compute: &[Listed]) -> Computer {
        choose(&Machine::Windows(Some(ort_order(generic_ml, core_compute).1)), &Choice::Auto)
    }

    #[test]
    fn rtx_and_data_centre_cards_are_built_for_ai_work() {
        let rows = [
            (NVIDIA, 0x2486, "NVIDIA GeForce RTX 3060 Ti"),
            (NVIDIA, 0x2489, "NVIDIA GeForce RTX 3060 Ti"),
            (NVIDIA, 0x24C9, "NVIDIA GeForce RTX 3060 Ti"),
            (NVIDIA, 0x1F08, "NVIDIA GeForce RTX 2060"),
            (NVIDIA, 0x1F11, "NVIDIA GeForce RTX 2060"),
            (NVIDIA, 0x1E04, "NVIDIA GeForce RTX 2080 Ti"),
            (NVIDIA, 0x1F36, "Quadro RTX 3000"),
            (NVIDIA, 0x25A9, "NVIDIA GeForce RTX 2050"),
            (NVIDIA, 0x20B5, "NVIDIA A100 80GB PCIe"),
            (NVIDIA, 0x1EB8, "NVIDIA Tesla T4"),
            (NVIDIA, 0x1D81, "NVIDIA TITAN V"),
            (NVIDIA, 0x1DB4, "Tesla V100-PCIE-16GB"),
            (NVIDIA, 0x2684, "NVIDIA GeForce RTX 4090"),
            (NVIDIA, 0x2882, "NVIDIA GeForce RTX 4060"),
            (NVIDIA, 0x2B85, "NVIDIA GeForce RTX 5090"),
            (NVIDIA, 0x27B8, "NVIDIA L4"),
            // Ampere's first id range (GA102GL): no "RTX" in the name.
            (NVIDIA, 0x2236, "NVIDIA A10"),
        ];
        check(&rows, card, Class::ForAi);
        assert!(!Class::ForAi.fast_by_default());
    }

    #[test]
    fn gtx_mx_and_older_nvidia_cards_are_not() {
        let rows = [
            (NVIDIA, 0x1C03, "NVIDIA GeForce GTX 1060 6GB"),
            (NVIDIA, 0x1C02, "NVIDIA GeForce GTX 1060 3GB"),
            (NVIDIA, 0x1B06, "NVIDIA GeForce GTX 1080 Ti"),
            (NVIDIA, 0x2182, "NVIDIA GeForce GTX 1660 Ti"),
            (NVIDIA, 0x21C4, "NVIDIA GeForce GTX 1660 SUPER"),
            (NVIDIA, 0x1F82, "NVIDIA GeForce GTX 1650"),
            (NVIDIA, 0x1F0A, "NVIDIA GeForce GTX 1650"),
            (NVIDIA, 0x1F99, "NVIDIA GeForce GTX 1650 Laptop GPU"),
            (NVIDIA, 0x1FB0, "NVIDIA Quadro T1000"),
            (NVIDIA, 0x1F97, "NVIDIA GeForce MX450"),
            (NVIDIA, 0x1D13, "NVIDIA GeForce MX250"),
            // Ampere's id range, but an MX: the name decides.
            (NVIDIA, 0x25A6, "NVIDIA GeForce MX570"),
            (NVIDIA, 0x25A6, "NVIDIA GeForce MX 570"),
            (NVIDIA, 0x15F8, "Tesla P100-PCIE-16GB"),
        ];
        check(&rows, card, Class::NotForAi);
        assert!(Class::NotForAi.fast_by_default());
    }

    #[test]
    fn rdna3_and_newer_radeons_are_built_for_ai_work_and_rdna2_is_not() {
        let for_ai = [
            (AMD, 0x744C, "AMD Radeon RX 7900 XTX"),
            (AMD, 0x7480, "AMD Radeon RX 7600"),
            (AMD, 0x73F0, "AMD Radeon RX 7600M XT"),
            (AMD, 0x7550, "AMD Radeon RX 9070 XT"),
            (AMD, 0x7448, "AMD Radeon PRO W7900"),
            (AMD, 0x738C, "AMD Instinct MI100"),
            (AMD, 0x740F, "AMD Instinct MI210"),
        ];
        check(&for_ai, card, Class::ForAi);
        let not = [
            (AMD, 0x73BF, "AMD Radeon RX 6800 XT"),
            (AMD, 0x73AF, "AMD Radeon RX 6900 XT"),
            (AMD, 0x743F, "AMD Radeon RX 6500 XT"),
            (AMD, 0x67DF, "Radeon RX 580 Series"),
            (AMD, 0x9440, "ATI Radeon HD 4870"),
        ];
        check(&not, card, Class::NotForAi);
    }

    #[test]
    fn arc_cards_are_built_for_ai_work_and_iris_xe_max_is_not() {
        check(
            &[(INTEL, 0x56A0, "Intel Arc A770 Graphics"), (INTEL, 0xE20B, "Intel Arc B580 Graphics")],
            card,
            Class::ForAi,
        );
        check(&[(INTEL, 0x4905, "Intel Iris Xe MAX Graphics")], card, Class::NotForAi);
    }

    #[test]
    fn built_in_graphics_are_integrated_whoever_makes_them() {
        let rows = [
            (AMD, 0x15BF, "AMD Radeon 780M Graphics"),
            (AMD, 0x150E, "AMD Radeon 890M Graphics"),
            // Strix Halo: as fast as a 3060 Ti, still built in (the owner, Q1).
            (AMD, 0x1586, "AMD Radeon 8060S Graphics"),
            // NVIDIA's own built-in GPUs (GB10, N1X): the flag decides, not the id.
            (NVIDIA, 0x2E12, "NVIDIA built-in GPU"),
            (INTEL, 0xA7A0, "Intel Iris Xe Graphics"),
            (INTEL, 0x9BC8, "Intel UHD Graphics 630"),
            (INTEL, 0x64A0, "Intel Arc 140V GPU (16GB)"),
            (INTEL, 0x7D55, "Intel Arc Graphics"),
            (0x4D4F_4351, 0x0636, "Qualcomm Adreno X1-85 GPU"),
        ];
        check(&rows, built_in, Class::Integrated);
        // An Intel adapter without the flag is still built in.
        check(&[(INTEL, 0x9A49, "Intel Iris Xe Graphics")], card, Class::Integrated);
        assert!(Class::Integrated.fast_by_default());
    }

    #[test]
    fn built_in_graphics_without_the_flag_still_get_fast_scan() {
        for (device, name) in [(0x1586, "AMD Radeon 8060S Graphics"), (0x15BF, "AMD Radeon 780M Graphics")] {
            let class = classify(&card(AMD, device, name));
            assert_eq!(class, Class::NotForAi, "{name}");
            assert!(class.fast_by_default(), "{name}");
        }
    }

    #[test]
    fn every_apple_chip_is_built_in_graphics() {
        for brand in [
            "Apple M1",
            "Apple M2",
            "Apple M4",
            "Apple M5",
            "Apple M1 Pro",
            "Apple M2 Max",
            "Apple M2 Ultra",
            "Apple M3 Max",
            "Apple M4 Pro",
            "Apple A18 Pro",
            "Apple M1 (Virtual)",
        ] {
            assert_eq!(apple(brand), Computer { class: Class::Integrated, name: brand.into(), luid: None });
        }
        // An Intel Mac (source builds only): built in, but the brand names the
        // processor, so no name.
        let intel = apple("Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz");
        assert_eq!(intel, Computer { class: Class::Integrated, name: String::new(), luid: None });
        assert_eq!(choose(&Machine::Mac("Apple M2".into()), &Choice::Auto).name, "Apple M2");
    }

    #[test]
    fn virtual_adapters_are_not_and_unknown_vendors_stay_unknown() {
        let rows = [
            (0x15AD, 0x0405, "VMware SVGA 3D"),
            (0x80EE, 0xBEEF, "VirtualBox Graphics Adapter"),
            (0x1AB8, 0x4005, "Parallels Display Adapter"),
            (0x1234, 0x1111, "QEMU Standard VGA"),
            (0x1AF4, 0x1050, "Red Hat VirtIO GPU"),
            (0x1B36, 0x0100, "Red Hat QXL"),
            (0x1414, 0x008E, "Microsoft Hyper-V Video"),
            (0x5853, 0x0001, "Xen Platform Device"),
        ];
        check(&rows, card, Class::NotForAi);
        check(&[(0x1ED5, 0x0100, "Moore Threads MTT S80")], card, Class::Unknown);
        assert!(!Class::Unknown.fast_by_default());
    }

    #[test]
    fn the_list_follows_directmls_own_order() {
        let rtx = card(NVIDIA, 0x24C9, "NVIDIA GeForce RTX 3060 Ti");
        let gtx = card(NVIDIA, 0x1C03, "NVIDIA GeForce GTX 1060 6GB");
        let iris = built_in(INTEL, 0xA7A0, "Intel Iris Xe Graphics");
        // The dev machine: GENERIC_ML holds the card and the software renderer.
        let both = [gpu(rtx.clone()), basic_render()];
        assert_eq!(ort_order(&both, &both), (Source::GenericMl, vec![rtx.clone()]));
        // No GENERIC_ML adapter: CORE_COMPUTE.
        assert_eq!(ort_order(&[], &[gpu(gtx.clone()), basic_render()]), (Source::CoreCompute, vec![gtx.clone()]));
        // Software and non-graphics adapters are dropped; the order is kept.
        let mixed = [npu(card(INTEL, 0x7D1D, "Intel AI Boost")), gpu(iris.clone()), basic_render(), gpu(rtx.clone())];
        assert_eq!(ort_order(&mixed, &[]), (Source::GenericMl, vec![iris, rtx.clone()]));
        // GENERIC_ML with only the software renderer is still the list ORT
        // takes: nothing is left, so DirectML refuses and the processor runs.
        assert_eq!(ort_order(&[basic_render()], &[gpu(rtx)]), (Source::GenericMl, vec![]));
        assert_eq!(ort_order(&[], &[]), (Source::CoreCompute, vec![]));
    }

    #[test]
    fn a_laptop_whose_ml_list_leaves_out_the_card_is_classed_by_what_directml_opens() {
        // An older card driver beside an NPU: GENERIC_ML has the built-in GPU
        // and the NPU only, so DirectML opens the built-in GPU.
        let iris = built_in(INTEL, 0xA7A0, "Intel Iris Xe Graphics");
        let rtx = card(NVIDIA, 0x2882, "NVIDIA GeForce RTX 4060");
        let generic_ml = [gpu(iris.clone()), npu(card(INTEL, 0x7D1D, "Intel AI Boost"))];
        let core_compute = [gpu(rtx), gpu(iris.clone())];
        let c = auto(&generic_ml, &core_compute);
        assert_eq!(c, Computer { class: Class::Integrated, name: iris.name, luid: Some(iris.luid) });
    }

    #[test]
    fn the_fast_card_of_a_hybrid_laptop_decides() {
        let iris = built_in(INTEL, 0xA7A0, "Intel Iris Xe Graphics");
        for (fast, want) in [
            (card(NVIDIA, 0x2882, "NVIDIA GeForce RTX 4060"), Class::ForAi),
            (card(NVIDIA, 0x1F99, "NVIDIA GeForce GTX 1650 Laptop GPU"), Class::NotForAi),
        ] {
            let list = [gpu(fast.clone()), gpu(iris.clone())];
            assert_eq!(auto(&list, &list), Computer { class: want, name: fast.name, luid: Some(fast.luid) });
        }
    }

    #[test]
    fn only_a_software_adapter_means_the_processor() {
        let c = auto(&[basic_render()], &[basic_render()]);
        assert_eq!(c, Computer { class: Class::Processor, name: String::new(), luid: None });
        assert!(c.class.fast_by_default());
    }

    #[cfg(windows)]
    #[test]
    fn a_missing_dxcore_means_unknown() {
        unsafe extern "system" fn refuses(
            _: *const windows::core::GUID,
            _: *mut *mut core::ffi::c_void,
        ) -> windows::core::HRESULT {
            windows::Win32::Foundation::E_FAIL
        }
        assert_eq!(windows(|| None), Machine::Windows(None));
        assert_eq!(windows(|| Some(refuses)), Machine::Windows(None));
        for choice in [Choice::Auto, Choice::Card { vendor: NVIDIA, device: 0x24C9, nth: 0 }] {
            assert_eq!(choose(&Machine::Windows(None), &choice), Computer::unknown());
        }
    }

    #[test]
    fn a_chosen_card_matches_by_vendor_and_device_and_a_missing_one_falls_back() {
        // The driver renamed the built-in GPU since it was chosen: the ids
        // still match. It comes first here, as on a laptop whose GENERIC_ML
        // list leaves the cards out, so Automatic and "the first card with
        // these ids" differ.
        let iris = Adapter { luid: 1, ..built_in(INTEL, 0xA7A0, "Intel Iris Xe Graphics Family") };
        let first = Adapter { luid: 2, ..card(NVIDIA, 0x2486, "NVIDIA GeForce RTX 3060 Ti") };
        let second = Adapter { luid: 3, ..first.clone() };
        let m = Machine::Windows(Some(vec![iris.clone(), first, second]));
        let pick = |vendor, device, nth| choose(&m, &Choice::Card { vendor, device, nth });
        assert_eq!(pick(INTEL, 0xA7A0, 0), Computer { class: Class::Integrated, name: iris.name, luid: Some(1) });
        let rtx = |luid| Computer { class: Class::ForAi, name: "NVIDIA GeForce RTX 3060 Ti".into(), luid: Some(luid) };
        assert_eq!(pick(NVIDIA, 0x2486, 0), rtx(2));
        assert_eq!(pick(NVIDIA, 0x2486, 1), rtx(3));
        assert_eq!(pick(NVIDIA, 0x2486, 2), rtx(2), "one identical card fewer: the first of them");
        // A card that isn't there any more: Automatic.
        assert_eq!(pick(NVIDIA, 0x1C03, 0), choose(&m, &Choice::Auto));
        assert_eq!(pick(NVIDIA, 0x1C03, 0).luid, Some(1));
        // No card at all: the processor.
        assert_eq!(
            choose(&Machine::Windows(Some(vec![])), &Choice::Card { vendor: NVIDIA, device: 0x2486, nth: 0 }),
            Computer::processor()
        );
    }

    #[test]
    fn processor_only_is_the_processor_everywhere() {
        let rtx = card(NVIDIA, 0x24C9, "NVIDIA GeForce RTX 3060 Ti");
        for m in [
            Machine::Windows(Some(vec![rtx])),
            Machine::Windows(Some(vec![])),
            Machine::Windows(None),
            Machine::Mac("Apple M3 Max".into()),
            Machine::CpuRuntime,
        ] {
            assert_eq!(choose(&m, &Choice::Processor), Computer::processor(), "{m:?}");
        }
    }

    #[test]
    fn driver_descriptions_are_cleaned_and_end_at_their_first_nul() {
        for (raw, want) in [
            (&b"Intel(R) Iris(R) Xe Graphics\0\0garbage"[..], "Intel Iris Xe Graphics"),
            (b"NVIDIA  GeForce RTX 3060 Ti \0", "NVIDIA GeForce RTX 3060 Ti"),
            ("Qualcomm(R) Adreno(TM) X1-85 GPU".as_bytes(), "Qualcomm Adreno X1-85 GPU"),
            ("Intel® Arc™ A770 Graphics\0".as_bytes(), "Intel Arc A770 Graphics"),
            ("AMD Radeon(TM) 780M Graphics".as_bytes(), "AMD Radeon 780M Graphics"),
            // Not UTF-8: Windows-1252's ® and ™.
            (b"AMD Radeon\xAE RX 580\x99\0", "AMD Radeon RX 580"),
            (b"\0Microsoft Basic Render Driver", ""),
            (b"", ""),
        ] {
            assert_eq!(description(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn the_log_line_names_the_device_its_list_and_every_adapter_in_it() {
        let rtx = Adapter { luid: 0x10828, ..card(NVIDIA, 0x24C9, "NVIDIA GeForce RTX 3060 Ti") };
        let both =
            vec![gpu(rtx), Listed { adapter: Adapter { luid: 0x13289, ..basic_render().adapter }, ..basic_render() }];
        let line = Lists { generic_ml: both.clone(), core_compute: both }.line();
        assert_eq!(
            line,
            "graphics built for AI work \"NVIDIA GeForce RTX 3060 Ti\", LUID 0x10828; DirectML's list GENERIC_ML: \
             [10de:24c9 \"NVIDIA GeForce RTX 3060 Ti\" LUID 0x10828, separate; 1414:008c \"Microsoft Basic Render Driver\" \
             LUID 0x13289, software]"
        );
        let laptop =
            Lists { generic_ml: vec![], core_compute: vec![gpu(built_in(INTEL, 0xA7A0, "Intel Iris Xe Graphics"))] };
        assert!(
            laptop.line().starts_with("built-in graphics \"Intel Iris Xe Graphics\", LUID 0xa7a0;"),
            "{}",
            laptop.line()
        );
        assert!(laptop.line().contains("CORE_COMPUTE: [8086:a7a0 \"Intel Iris Xe Graphics\" LUID 0xa7a0, built in]"));
        assert_eq!(Lists::default().line(), "the processor; DirectML's list CORE_COMPUTE: []");
        let npu_only = Lists { generic_ml: vec![npu(card(INTEL, 0x7D1D, "Intel AI Boost"))], core_compute: vec![] };
        assert_eq!(
            npu_only.line(),
            "the processor; DirectML's list GENERIC_ML: [8086:7d1d \"Intel AI Boost\" LUID 0x7d1d, separate, no graphics]"
        );
    }

    #[test]
    fn this_computer_is_read_quickly() {
        let t = std::time::Instant::now();
        let m = probe();
        let elapsed = t.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
        // Whatever it finds, every choice answers.
        for choice in [Choice::Auto, Choice::Processor, Choice::Card { vendor: NVIDIA, device: 0x24C9, nth: 0 }] {
            let _ = choose(&m, &choice);
        }
        assert!(std::ptr::eq(machine(), machine()), "probed once");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_mac_names_its_chip() {
        let m = probe();
        assert!(matches!(m, Machine::Mac(_)), "{m:?}");
        let c = choose(&m, &Choice::Auto);
        assert_eq!(c.class, Class::Integrated);
        assert!(c.name.starts_with("Apple ") && c.name.len() > "Apple ".len(), "{c:?}");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn elsewhere_the_analysis_runs_on_the_processor() {
        assert_eq!(probe(), Machine::CpuRuntime);
        assert_eq!(choose(machine(), &Choice::Auto), Computer::processor());
    }

    #[test]
    fn a_cpu_runtime_means_the_processor() {
        for choice in [Choice::Auto, Choice::Card { vendor: NVIDIA, device: 0x24C9, nth: 0 }] {
            assert_eq!(choose(&Machine::CpuRuntime, &choice), Computer::processor());
        }
    }
}
