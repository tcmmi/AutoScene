//! The graphics the analysis runs on (Settings → General → Graphics card).

use as_api::{AccelInfo, AnalysisDevice, GpuOption, HardwareChoice, HardwareInfo};
use as_core::{CancelToken, msg};
use as_engine::hardware::{Hardware, Using};
use as_project::computer::Choice;
use tauri::{AppHandle, State};

use crate::accel;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::state::{AppState, blocking};

/// The graphics cards the analysis can run on, the one it runs on and why.
/// The first call of a session starts the engine to ask ONNX Runtime what it
/// finds (its idle exit stops it); later ones answer from what the last
/// engine found.
#[tauri::command]
#[specta::specta]
pub async fn hardware_info(state: State<'_, AppState>) -> AppResult<HardwareInfo> {
    let state = state.inner().clone();
    blocking(move || {
        let found = state.engine.probe(&CancelToken::new()).map_err(|e| {
            log::warn!("looking for graphics cards: {}", e.detail());
            AppError::new(ErrorCode::Internal, msg!("system.gpuCheckFailed", "Couldn't check the graphics cards."))
        })?;
        let gpu = state.library.gpu();
        let chosen = found.clone().chosen(&Choice::from_setting(&gpu));
        let accel = state.accel.info(&chosen, state.library.nvidia_acceleration(), &state.engine);
        Ok(info(&found, &gpu, state.engine.pending(), state.engine.fell_back(), accel))
    })
    .await
}

/// Download NVIDIA's files: the person agreed to NVIDIA's licence. The row
/// follows it (`hardware:changed`, `accel:progress`).
#[tauri::command]
#[specta::specta]
pub fn accel_install(app: AppHandle) {
    accel::install(&app, true);
}

/// Stop the download of NVIDIA's files.
#[tauri::command]
#[specta::specta]
pub fn accel_cancel(state: State<'_, AppState>) {
    state.accel.cancel();
}

/// Remove NVIDIA's files, once no analysis uses them.
#[tauri::command]
#[specta::specta]
pub async fn accel_remove(app: AppHandle) -> AppResult<()> {
    blocking(move || accel::remove(&app)).await
}

/// Use NVIDIA's files again after they stopped working.
#[tauri::command]
#[specta::specta]
pub async fn accel_try_again(app: AppHandle) -> AppResult<()> {
    blocking(move || {
        accel::try_again(&app);
        Ok(())
    })
    .await
}

/// Open NVIDIA's third-party notices (About).
#[tauri::command]
#[specta::specta]
pub fn accel_notices(state: State<'_, AppState>) -> AppResult<()> {
    accel::open_notices(&state)
}

/// What Settings shows for the devices an engine `found` and the graphics
/// the person chose (`AppSettings::gpu`).
fn info(found: &Hardware, gpu: &str, pending: bool, fell_back: bool, accel: AccelInfo) -> HardwareInfo {
    let choice = Choice::from_setting(gpu);
    let chosen = found.clone().chosen(&choice);
    // Cards DirectML opens by number; without numbers it picks one itself,
    // and only Automatic can be honoured.
    let gpus: Vec<GpuOption> = found
        .gpus
        .iter()
        .filter(|g| g.dxgi.is_some())
        .map(|g| GpuOption { key: g.key.clone(), name: g.said() })
        .collect();
    let kind = match (gpus.len(), found.auto) {
        (2.., _) => HardwareChoice::Cards,
        (_, Using::Processor) if found.gpus.is_empty() => HardwareChoice::Fixed,
        _ => HardwareChoice::AutoOrProcessor,
    };
    let selected = match (choice, chosen.gpu()) {
        (Choice::Processor, _) => "cpu".into(),
        (Choice::Card { .. }, Some(card)) if kind == HardwareChoice::Cards && !chosen.missing => card.key.clone(),
        _ => "auto".into(),
    };
    let using = match chosen.using {
        Using::Gpu { .. } | Using::Preferred => AnalysisDevice::Gpu { name: chosen.name().into() },
        Using::Chip => AnalysisDevice::Chip { name: chosen.name().into() },
        Using::Processor => AnalysisDevice::Processor,
    };
    let runtime = found.runtime.clone();
    HardwareInfo { gpus, choice: kind, selected, using, missing: chosen.missing, pending, fell_back, runtime, accel }
}

#[cfg(test)]
mod tests {
    use as_api::AccelRow;
    use as_engine::hardware::{Kind, RawDevice};

    use super::*;

    /// [`super::info`] with the row of NVIDIA acceleration left out.
    fn info(found: &Hardware, gpu: &str, pending: bool, fell_back: bool) -> HardwareInfo {
        let accel = AccelInfo {
            row: AccelRow::Hidden,
            installed: false,
            removable: false,
            active: false,
            update_stopped: false,
            download_bytes: 0,
            licence: String::new(),
            engine: String::new(),
        };
        super::info(found, gpu, pending, fell_back, accel)
    }

    fn card(vendor: u32, device: u32, name: &str, dxgi: Option<u32>, luid: u64) -> RawDevice {
        let mut metadata: std::collections::BTreeMap<String, String> =
            [("Description", name.to_string()), ("LUID", luid.to_string())].map(|(k, v)| (k.to_string(), v)).into();
        if let Some(n) = dxgi {
            metadata.insert("DxgiAdapterNumber".into(), n.to_string());
        }
        RawDevice { kind: Kind::Gpu, vendor, device, metadata }
    }

    fn hybrid(dxgi: bool) -> Hardware {
        let raw = [
            card(0x10DE, 0x28E0, "NVIDIA GeForce RTX 4060 Laptop GPU", dxgi.then_some(1), 2),
            card(0x8086, 0xA7A0, "Intel Iris Xe Graphics", dxgi.then_some(0), 1),
        ];
        Hardware::windows(&raw, Some(&[2, 1]))
    }

    fn gpu(name: &str) -> AnalysisDevice {
        AnalysisDevice::Gpu { name: name.into() }
    }

    #[test]
    fn settings_names_each_card_and_the_one_in_use() {
        let hw = hybrid(true);
        let auto = info(&hw, "", false, false);
        assert_eq!(auto.choice, HardwareChoice::Cards);
        let names: Vec<&str> = auto.gpus.iter().map(|g| g.name.text.as_str()).collect();
        assert_eq!(names, ["NVIDIA GeForce RTX 4060 Laptop GPU", "Intel Iris Xe Graphics"]);
        assert_eq!((auto.selected.as_str(), &auto.using), ("auto", &gpu("NVIDIA GeForce RTX 4060 Laptop GPU")));
        // A card chosen under another spelling: shown as that card, by its key now.
        let iris = info(&hw, "8086:a7a0:Intel(R) Iris(R) Xe Graphics Family", true, true);
        assert_eq!(
            (iris.selected.as_str(), &iris.using),
            ("8086:a7a0:Intel Iris Xe Graphics", &gpu("Intel Iris Xe Graphics"))
        );
        assert!(iris.pending && iris.fell_back && !iris.missing);
        let cpu = info(&hw, "cpu", false, false);
        assert_eq!((cpu.selected.as_str(), &cpu.using), ("cpu", &AnalysisDevice::Processor));
        // A card that isn't connected: Automatic stands in and says so.
        let gone = info(&hw, "1002:744c:AMD Radeon RX 7900 XTX", false, false);
        assert_eq!(
            (gone.selected.as_str(), &gone.using, gone.missing),
            ("auto", &gpu("NVIDIA GeForce RTX 4060 Laptop GPU"), true)
        );
    }

    #[test]
    fn with_one_card_or_none_there_is_less_to_choose() {
        let one = Hardware::windows(&[card(0x10DE, 0x24C9, "NVIDIA GeForce RTX 3060 Ti", Some(0), 7)], Some(&[7]));
        let i = info(&one, "10de:24c9:NVIDIA GeForce RTX 3060 Ti", false, false);
        assert_eq!((i.choice, i.selected.as_str(), i.gpus.len()), (HardwareChoice::AutoOrProcessor, "auto", 1));
        // Without adapter numbers only Automatic, named when it is known.
        let numbless = info(&hybrid(false), "", false, false);
        assert_eq!((numbless.choice, numbless.gpus.len()), (HardwareChoice::AutoOrProcessor, 0));
        assert_eq!(numbless.using, gpu("NVIDIA GeForce RTX 4060 Laptop GPU"));
        // Windows without a card, and Linux: nothing to choose.
        let none = info(&Hardware::windows(&[], Some(&[])), "", false, false);
        assert_eq!((none.choice, &none.using), (HardwareChoice::Fixed, &AnalysisDevice::Processor));
        let linux = info(&Hardware::processor_only(&[]), "", false, false);
        assert_eq!((linux.choice, &linux.using), (HardwareChoice::Fixed, &AnalysisDevice::Processor));
        // A Mac: Automatic (the chip) or the processor.
        let mac = Hardware::mac(&[], "Apple M2");
        let auto = info(&mac, "", false, false);
        assert_eq!(
            (auto.choice, &auto.using),
            (HardwareChoice::AutoOrProcessor, &AnalysisDevice::Chip { name: "Apple M2".into() })
        );
        assert_eq!(info(&mac, "cpu", false, false).using, AnalysisDevice::Processor);
    }
}
