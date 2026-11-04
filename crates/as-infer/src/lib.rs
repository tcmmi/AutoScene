//! ONNX Runtime for the analysis engine.
//!
//! The runtime is the copy the app ships (`cargo xtask fetch-sidecars` →
//! `onnxruntime/`), loaded by absolute path: Windows has an older
//! `onnxruntime.dll` and `DirectML.dll` in System32 that must never be picked
//! up. Sessions run on the GPU — DirectML on Windows (any DX12 GPU), CoreML on
//! macOS — and fall back to the CPU when the GPU provider can't take a model.
//!
//! `load-dynamic` binds one runtime per process, which is one reason the
//! engine is its own process: switching runtimes means restarting it.
//!
//! [`computer`] says which graphics the analysis runs on, and whether they are
//! built for AI work; [`hardware`] which cards ONNX Runtime finds, and opens
//! the one the person chose ([`Placement`]). On a Mac, Core ML keeps what it
//! compiled for the next start ([`cache`], [`Placement::cache`]). On a
//! GeForce RTX card, NVIDIA's engine can take the models allowed on it
//! ([`accel`], [`Placement::accel`]), DirectML the rest.

pub mod accel;
pub mod cache;
pub mod computer;
pub mod hardware;
pub mod onnx;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub use ort::inputs;
pub use ort::session::Session;
pub use ort::value::{Tensor, TensorRef};
use thiserror::Error;

pub type InferResult<T> = Result<T, InferError>;

#[derive(Debug, Error)]
pub enum InferError {
    #[error("AutoScene's AI runtime is missing. Reinstall AutoScene.")]
    Missing(PathBuf),
    #[error("AutoScene's AI runtime couldn't start. Details are in the log.")]
    Runtime(String),
    #[error("An AI model couldn't be loaded. Details are in the log.")]
    Model(String),
    #[error("The AI model failed on this frame. Details are in the log.")]
    Run(String),
}

impl InferError {
    pub fn detail(&self) -> String {
        match self {
            Self::Missing(p) => format!("onnxruntime not found at {}", p.display()),
            Self::Runtime(e) => format!("onnxruntime init: {e}"),
            Self::Model(e) => format!("model load: {e}"),
            Self::Run(e) => format!("inference: {e}"),
        }
    }
}

impl From<ort::Error> for InferError {
    fn from(e: ort::Error) -> Self {
        Self::Run(e.to_string())
    }
}

/// Where a session computes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// NVIDIA's engine for GeForce RTX cards ([`accel`]).
    NvRtx,
    DirectMl,
    CoreMl,
    Cpu,
}

impl Device {
    pub fn label(self) -> &'static str {
        match self {
            Self::NvRtx => "GPU (TensorRT for RTX)",
            Self::DirectMl => "GPU (DirectML)",
            Self::CoreMl => "Apple Neural Engine / GPU (Core ML)",
            Self::Cpu => "CPU",
        }
    }
}

/// File name of the runtime library in the bundle folder.
pub const LIBRARY: &str = if cfg!(windows) {
    "onnxruntime.dll"
} else if cfg!(target_os = "macos") {
    "libonnxruntime.dylib"
} else {
    "libonnxruntime.so"
};

/// Where a runtime's sessions run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// The graphics the person chose (Settings → General): Automatic, a
    /// card, or the processor only.
    pub gpu: computer::Choice,
    /// Where compiled models are kept between runs (`AppPaths::cache()`):
    /// Core ML's under `coreml/` ([`cache`]). `None` compiles them at every
    /// start.
    pub cache: Option<PathBuf>,
    /// The folder of NVIDIA's engine for GeForce RTX cards ([`accel`]), when
    /// it is installed and switched on: registered if it serves the card in
    /// use, for the sessions allowed on it ([`Runtime::without_accel`]).
    pub accel: Option<PathBuf>,
}

impl Placement {
    /// Automatic, or the processor only.
    pub fn from_gpu(gpu: bool) -> Self {
        Self { gpu: if gpu { computer::Choice::Auto } else { computer::Choice::Processor }, ..Self::default() }
    }
}

/// How exactly a session must compute. It decides how Core ML builds the
/// session; everywhere else sessions build alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// fp16 maths are fine: Core ML's NeuralNetwork format, on every compute
    /// unit, the Neural Engine included.
    Fp16Safe,
    /// fp32 throughout: an ML program without fp16 accumulation on the GPU,
    /// which Core ML runs on the processor and the GPU only. The anime
    /// models' embeddings bend under fp16 maths.
    Fp32,
}

/// Core ML's model formats (ORT's `ModelFormat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreMlFormat {
    NeuralNetwork,
    MlProgram,
}

impl CoreMlFormat {
    /// ORT's name for it.
    pub fn option(self) -> &'static str {
        match self {
            Self::NeuralNetwork => "NeuralNetwork",
            Self::MlProgram => "MLProgram",
        }
    }
}

/// What Core ML may compute on (ORT's `MLComputeUnits`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreMlUnits {
    /// The Neural Engine, the GPU and the processor.
    All,
    CpuAndNeuralEngine,
    CpuAndGpu,
    CpuOnly,
}

impl CoreMlUnits {
    /// ORT's name for it.
    pub fn option(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::CpuAndNeuralEngine => "CPUAndNeuralEngine",
            Self::CpuAndGpu => "CPUAndGPU",
            Self::CpuOnly => "CPUOnly",
        }
    }
}

/// Core ML's options for a session (ORT's names and values), as sessions are
/// built today; with `cache`, Core ML keeps its compile there. The formats and
/// compute units stay what they were before compiled models were kept, until
/// `coreml_bench` measures others on a real Mac (design-i66_67 §5.5).
pub fn coreml_options(precision: Precision, cache: Option<&Path>) -> Vec<(&'static str, String)> {
    CoreMlTrial::default().options(precision, cache)
}

/// Another Core ML setup than today's, which `coreml_bench` compares: another
/// model format, other compute units, or both. The default is today's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoreMlTrial {
    pub format: Option<CoreMlFormat>,
    pub units: Option<CoreMlUnits>,
}

impl CoreMlTrial {
    /// [`coreml_options`] with this trial's format and compute units.
    pub fn options(self, precision: Precision, cache: Option<&Path>) -> Vec<(&'static str, String)> {
        let mut options: Vec<(&'static str, String)> = match precision {
            Precision::Fp16Safe => vec![
                ("ModelFormat", CoreMlFormat::NeuralNetwork.option().into()),
                ("MLComputeUnits", CoreMlUnits::All.option().into()),
            ],
            Precision::Fp32 => vec![
                ("ModelFormat", CoreMlFormat::MlProgram.option().into()),
                ("AllowLowPrecisionAccumulationOnGPU", "0".into()),
            ],
        };
        let trial = [
            ("ModelFormat", self.format.map(CoreMlFormat::option)),
            ("MLComputeUnits", self.units.map(CoreMlUnits::option)),
        ];
        for (name, value) in trial {
            let Some(value) = value else { continue };
            match options.iter_mut().find(|(n, _)| *n == name) {
                Some(option) => option.1 = value.into(),
                None => options.push((name, value.into())),
            }
        }
        if let Some(dir) = cache {
            options.push(("ModelCacheDirectory", dir.to_string_lossy().into_owned()));
        }
        options
    }
}

/// The device sessions are built for first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// DirectML (with the adapter number to open, else its own pick) on
    /// Windows, Core ML on macOS.
    Gpu(Option<u32>),
    Cpu,
}

pub struct Runtime {
    env: ort::environment::Environment,
    target: Target,
    hardware: Arc<hardware::Hardware>,
    /// [`Placement::cache`]; read where Core ML builds sessions.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    cache: Option<PathBuf>,
    /// Today's Core ML setup, or the one `coreml_bench` tries.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    coreml: CoreMlTrial,
    /// NVIDIA's engine, registered with a device for the card in use.
    accel: Option<Arc<accel::Registered>>,
    /// What became of the pack [`Placement::accel`] named.
    accel_state: accel::AccelState,
    /// Sessions of this runtime may run on NVIDIA's engine
    /// ([`Runtime::without_accel`]).
    on_accel: bool,
}

static LOADED: OnceLock<PathBuf> = OnceLock::new();

/// The version the ONNX Runtime at `lib`, already loaded, gives itself: it is
/// shown in About, so it is never written by hand. Empty if it won't say.
fn version(lib: &Path) -> String {
    type GetApiBase = unsafe extern "C" fn() -> *const ort::sys::OrtApiBase;
    // SAFETY: `lib` is the library ort loaded, so this only takes another
    // reference to it. `OrtGetApiBase` has this signature in every ONNX
    // Runtime, its base is valid for as long as the library stays loaded,
    // which ort keeps it, and `GetVersionString` returns a static C string.
    let read = || unsafe {
        let library = libloading::Library::new(lib)?;
        let base = library.get::<GetApiBase>(b"OrtGetApiBase")?();
        let version = ((*base).GetVersionString)();
        Ok::<_, libloading::Error>(std::ffi::CStr::from_ptr(version).to_string_lossy().into_owned())
    };
    read().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "the runtime didn't say its version");
        String::new()
    })
}

impl Runtime {
    /// Load the runtime from `dir` (once per process; later calls must name
    /// the same folder). `gpu = false` keeps every session on the CPU; `true`
    /// is Automatic ([`Runtime::load_with`]).
    pub fn load(dir: &Path, gpu: bool) -> InferResult<Self> {
        Self::load_with(dir, &Placement::from_gpu(gpu))
    }

    /// Load the runtime from `dir` (once per process; later calls must name
    /// the same folder), with sessions on the device `placement` chooses
    /// ([`hardware`]).
    pub fn load_with(dir: &Path, placement: &Placement) -> InferResult<Self> {
        // LoadLibraryEx with an altered search path needs an absolute path.
        let dir = &std::path::absolute(dir).map_err(|e| InferError::Runtime(format!("{}: {e}", dir.display())))?;
        let lib = dir.join(LIBRARY);
        if !lib.is_file() {
            return Err(InferError::Missing(lib));
        }
        if let Some(prev) = LOADED.get() {
            if prev != dir {
                return Err(InferError::Runtime(format!("already loaded from {}", prev.display())));
            }
        } else {
            #[cfg(windows)]
            preload_windows(dir);
            ort::init_from(&lib).map_err(|e| InferError::Runtime(e.to_string()))?;
            let _ = LOADED.set(dir.to_path_buf());
        }
        let env = ort::init().with_name("autoscene").build().map_err(|e| InferError::Runtime(e.to_string()))?;
        let hardware = hardware::Hardware { runtime: version(&lib), ..hardware::discover(&env).chosen(&placement.gpu) };
        let target = match hardware.using {
            hardware::Using::Processor => Target::Cpu,
            _ => Target::Gpu(hardware.adapter()),
        };
        tracing::info!(runtime = %name(&lib), "onnxruntime loaded; analysis runs on {hardware}");
        let (accel_state, accel) = match &placement.accel {
            // Never for the processor only: the pack is for a graphics card.
            Some(dir) if target != Target::Cpu => register_accel(&env, dir, hardware.gpu()),
            _ => (accel::AccelState::Off, None),
        };
        Ok(Self {
            env,
            target,
            hardware: Arc::new(hardware),
            cache: placement.cache.clone(),
            coreml: CoreMlTrial::default(),
            accel: accel.map(Arc::new),
            accel_state,
            on_accel: true,
        })
    }

    /// The same runtime with every session on the CPU (one runtime per
    /// process: there is no second `load`), e.g. to check GPU results.
    pub fn on_cpu(&self) -> Self {
        Self { target: Target::Cpu, ..self.copy() }
    }

    /// The same runtime with no session on NVIDIA's engine: for the models
    /// that are never allowed on it, and for one that missed its goldens
    /// there.
    pub fn without_accel(&self) -> Self {
        Self { on_accel: false, ..self.copy() }
    }

    /// What became of NVIDIA's engine ([`Placement::accel`]).
    pub fn accel_state(&self) -> &accel::AccelState {
        &self.accel_state
    }

    /// The same runtime, keeping compiled models under `cache` instead
    /// ([`Placement::cache`]; `None` keeps none), e.g. to time a compile.
    pub fn with_cache(&self, cache: Option<PathBuf>) -> Self {
        Self { cache, ..self.copy() }
    }

    /// The same runtime, with Core ML sessions built as `trial` says instead
    /// of today's way (`coreml_bench`).
    pub fn trying(&self, trial: CoreMlTrial) -> Self {
        Self { coreml: trial, ..self.copy() }
    }

    fn copy(&self) -> Self {
        Self {
            env: self.env.clone(),
            target: self.target,
            hardware: Arc::clone(&self.hardware),
            cache: self.cache.clone(),
            coreml: self.coreml,
            accel: self.accel.clone(),
            accel_state: self.accel_state.clone(),
            on_accel: self.on_accel,
        }
    }

    /// The devices ORT found here, and the one this runtime opened.
    pub fn hardware(&self) -> &hardware::Hardware {
        &self.hardware
    }

    /// The device of the GPU runtime's own provider: the one sessions try
    /// first, after NVIDIA's engine where it may take them ([`Runtime::devices`]).
    pub fn preferred_device(&self) -> Device {
        match self.target {
            Target::Gpu(_) if cfg!(windows) => Device::DirectMl,
            Target::Gpu(_) if cfg!(target_os = "macos") => Device::CoreMl,
            _ => Device::Cpu,
        }
    }

    /// The devices a session of this runtime tries, in order.
    pub fn devices(&self) -> Vec<Device> {
        let gpu = Some(self.preferred_device()).filter(|d| *d != Device::Cpu);
        chain(gpu, self.on_accel && self.accel.is_some())
    }

    /// The same runtime, but sessions built so CoreML computes in fp32
    /// (`MLProgram`, no low-precision accumulation on the GPU): the anime
    /// models' embeddings bend under fp16 maths. Everywhere else it builds
    /// exactly like this runtime.
    pub fn fp32_sessions(&self) -> Fp32<'_> {
        Fp32 { rt: self }
    }

    /// Load `model`, on the GPU when possible. Returns where it runs.
    pub fn session(&self, model: &Path) -> InferResult<(Session, Device)> {
        self.session_from(model, None, Precision::Fp16Safe)
    }

    /// Like [`Runtime::session`], with the shapes the model declares for its
    /// outputs and intermediate values dropped and its input shaped as
    /// `input` says (see [`onnx::reshaped`]): for graphs that compute any
    /// input size or batch but were exported declaring one.
    pub fn session_shaped(&self, model: &Path, input: onnx::InputShape) -> InferResult<(Session, Device)> {
        let bytes = std::fs::read(model).map_err(|e| InferError::Model(format!("{}: {e}", model.display())))?;
        let patched = onnx::reshaped(&bytes, input).ok_or_else(|| {
            InferError::Model(format!("{}: not a readable ONNX model, or not shaped like {input:?}", model.display()))
        })?;
        self.session_from(model, Some(&patched), Precision::Fp16Safe)
    }

    /// Like [`Runtime::session_shaped`], for an export that pins its batch
    /// at 1 inside the graph too: its constant Reshape targets copy the
    /// batch instead ([`onnx::free_reshape_batch`]).
    pub fn session_batched(&self, model: &Path, input: onnx::InputShape) -> InferResult<(Session, Device)> {
        let bytes = std::fs::read(model).map_err(|e| InferError::Model(format!("{}: {e}", model.display())))?;
        let unreadable = |what: &str| InferError::Model(format!("{}: {what}", model.display()));
        let (free, targets) = onnx::free_reshape_batch(&bytes)
            .ok_or_else(|| unreadable("not a readable ONNX model, or a Reshape that takes 0 literally"))?;
        let patched = onnx::reshaped(&free, input).ok_or_else(|| unreadable(&format!("not shaped like {input:?}")))?;
        tracing::debug!(model = %model.display(), targets, "Reshapes copy the batch");
        self.session_from(model, Some(&patched), Precision::Fp16Safe)
    }

    fn session_from(&self, model: &Path, bytes: Option<&[u8]>, precision: Precision) -> InferResult<(Session, Device)> {
        for want in self.devices().into_iter().filter(|d| *d != Device::Cpu) {
            match self.build(model, bytes, want, precision) {
                Ok(s) => return Ok((s, want)),
                Err(e) => {
                    tracing::warn!(model = %name(model), device = ?want, error = %e.detail(), "GPU session failed; trying the next device")
                }
            }
        }
        Ok((self.build(model, bytes, Device::Cpu, precision)?, Device::Cpu))
    }

    /// A session of `model` (or of `bytes`, the model as edited) on `device`.
    /// Where there is a cache, Core ML's compile is kept for the next start.
    fn build(&self, model: &Path, bytes: Option<&[u8]>, device: Device, precision: Precision) -> InferResult<Session> {
        #[cfg(target_os = "macos")]
        if device == Device::CoreMl
            && let Some(root) = &self.cache
        {
            let options = cache::describe(&self.coreml.options(precision, None));
            return self.build_kept(root, cache::COREML, &options, model, bytes, device, precision);
        }
        if device == Device::NvRtx
            && let (Some(root), Some(accel)) = (&self.cache, &self.accel)
        {
            // The plugin's build is part of what it compiled.
            let options = format!("{FALLBACK_OFF}=1;plugin={};", accel.plugin);
            return self.build_kept(root, cache::TRTRTX, &options, model, bytes, device, precision);
        }
        self.build_in(model, bytes, device, precision, None)
    }

    /// [`Runtime::build`] compiled into the folder of this model, shape,
    /// runtime and provider `options` under `family`, or reusing what an
    /// earlier start compiled there ([`cache::kept`]).
    #[allow(clippy::too_many_arguments)]
    fn build_kept(
        &self,
        root: &Path,
        family: &str,
        options: &str,
        model: &Path,
        bytes: Option<&[u8]>,
        device: Device,
        precision: Precision,
    ) -> InferResult<Session> {
        let read;
        let exact = match bytes {
            Some(bytes) => bytes,
            None => {
                read = std::fs::read(model).map_err(|e| InferError::Model(format!("{}: {e}", model.display())))?;
                &read
            }
        };
        let key = cache::key(exact, ort::info(), options);
        cache::kept(root, family, &key, &name(model), |dir| self.build_in(model, bytes, device, precision, dir))
    }

    /// [`Runtime::build`] without the cache's bookkeeping: `cache` is the
    /// folder Core ML or NVIDIA's engine compiles into.
    fn build_in(
        &self,
        model: &Path,
        bytes: Option<&[u8]>,
        device: Device,
        precision: Precision,
        cache: Option<&Path>,
    ) -> InferResult<Session> {
        #[cfg(not(target_os = "macos"))]
        let _ = precision;
        let err = |e: String| InferError::Model(format!("{}: {e}", model.display()));
        let mut b = Session::builder(&self.env).map_err(|e| err(e.to_string()))?;
        match device {
            Device::NvRtx => {
                // The plugin's device for the card in use, never an empty
                // list (that would build a session on the processor), and
                // every node on it, or no session at all.
                let accel = self.accel.as_ref().ok_or_else(|| err("NVIDIA's engine isn't registered".into()))?;
                let device = self
                    .env
                    .devices()
                    .find(|d| d.ep().is_ok_and(|ep| ep == accel::EP) && hardware::luid_of(d) == Some(accel.luid))
                    .ok_or_else(|| err("NVIDIA's engine lists no device for this card".into()))?;
                let options: Vec<(String, String)> = cache
                    .map(|dir| (accel::RUNTIME_CACHE.to_string(), dir.to_string_lossy().into_owned()))
                    .into_iter()
                    .collect();
                b = b
                    .with_config_entry(FALLBACK_OFF, "1")
                    .map_err(|e| err(e.to_string()))?
                    .with_devices([device], Some(&options))
                    .map_err(|e| err(e.to_string()))?;
            }
            #[cfg(windows)]
            Device::DirectMl => {
                // DirectML needs sequential execution without memory patterns.
                // The card the person chose, or Automatic's, by the adapter
                // number discovery gave it ([`hardware`]). Without one, the
                // fastest GPU, not adapter 0: on a laptop that is often the
                // integrated one, with the dedicated GPU left idle.
                let dml = match self.target {
                    Target::Gpu(Some(adapter)) => ort::ep::DirectML::default().with_device_id(adapter as i32),
                    _ => ort::ep::DirectML::default()
                        .with_performance_preference(ort::ep::directml::PerformancePreference::HighPerformance),
                };
                b = b
                    .with_execution_providers([dml.build().error_on_failure()])
                    .map_err(|e| err(e.to_string()))?
                    .with_memory_pattern(false)
                    .map_err(|e| err(e.to_string()))?
                    .with_parallel_execution(false)
                    .map_err(|e| err(e.to_string()))?;
            }
            #[cfg(target_os = "macos")]
            Device::CoreMl => {
                // Today's format and compute units for the precision
                // ([`coreml_options`]), or the bench's trial.
                use ort::ep::ArbitrarilyConfigurableExecutionProvider;
                let ep = self
                    .coreml
                    .options(precision, cache)
                    .into_iter()
                    .fold(ort::ep::CoreML::default(), |ep, (name, value)| ep.with_arbitrary_config(name, value));
                b = b.with_execution_providers([ep.build().error_on_failure()]).map_err(|e| err(e.to_string()))?;
            }
            _ => {}
        }
        match bytes {
            Some(bytes) => b.commit_from_memory(bytes).map_err(|e| err(e.to_string())),
            None => b.commit_from_file(model).map_err(|e| err(e.to_string())),
        }
    }
}

/// What [`Runtime::fp32_sessions`] returns: the same sessions — same
/// signatures — but built so CoreML keeps fp32 maths ([`Precision::Fp32`]).
/// Off CoreML there is nothing to build differently.
pub struct Fp32<'a> {
    rt: &'a Runtime,
}

impl Fp32<'_> {
    /// Like [`Runtime::session`].
    pub fn session(&self, model: &Path) -> InferResult<(Session, Device)> {
        self.rt.session_from(model, None, Precision::Fp32)
    }

    /// Like [`Runtime::session_shaped`].
    pub fn session_shaped(&self, model: &Path, input: onnx::InputShape) -> InferResult<(Session, Device)> {
        let bytes = std::fs::read(model).map_err(|e| InferError::Model(format!("{}: {e}", model.display())))?;
        let patched = onnx::reshaped(&bytes, input).ok_or_else(|| {
            InferError::Model(format!("{}: not a readable ONNX model, or not shaped like {input:?}", model.display()))
        })?;
        self.rt.session_from(model, Some(&patched), Precision::Fp32)
    }

    /// Like [`Runtime::session_batched`].
    pub fn session_batched(&self, model: &Path, input: onnx::InputShape) -> InferResult<(Session, Device)> {
        let bytes = std::fs::read(model).map_err(|e| InferError::Model(format!("{}: {e}", model.display())))?;
        let unreadable = |what: &str| InferError::Model(format!("{}: {what}", model.display()));
        let (free, targets) = onnx::free_reshape_batch(&bytes)
            .ok_or_else(|| unreadable("not a readable ONNX model, or a Reshape that takes 0 literally"))?;
        let patched = onnx::reshaped(&free, input).ok_or_else(|| unreadable(&format!("not shaped like {input:?}")))?;
        tracing::debug!(model = %model.display(), targets, "Reshapes copy the batch");
        self.rt.session_from(model, Some(&patched), Precision::Fp32)
    }
}

/// The session option that refuses a session with any node left for the
/// processor.
const FALLBACK_OFF: &str = "session.disable_cpu_ep_fallback";

/// The devices a session tries, in order: NVIDIA's engine when `accel` (it
/// is registered, and the session may use it), then the GPU runtime's own
/// provider (`gpu`), then the processor.
fn chain(gpu: Option<Device>, accel: bool) -> Vec<Device> {
    let nvidia = (accel && gpu.is_some()).then_some(Device::NvRtx);
    nvidia.into_iter().chain(gpu).chain([Device::Cpu]).collect()
}

/// [`accel::register`] where NVIDIA's engine runs (Windows on x64).
#[cfg(all(windows, target_arch = "x86_64"))]
fn register_accel(
    env: &ort::environment::Environment,
    dir: &Path,
    gpu: Option<&hardware::Gpu>,
) -> (accel::AccelState, Option<accel::Registered>) {
    accel::register(env, dir, gpu)
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn register_accel(
    _: &ort::environment::Environment,
    _: &Path,
    _: Option<&hardware::Gpu>,
) -> (accel::AccelState, Option<accel::Registered>) {
    (accel::AccelState::Off, None)
}

/// Just the file name, for the log: full paths name the user (their home
/// folder), and logs get shared.
fn name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name().map_or("?".into(), |n| n.to_string_lossy())
}

/// Make DirectML.dll resolve to the bundled copy: load it by full path (the
/// runtime's later `LoadLibrary("DirectML.dll")` then finds it already loaded)
/// and put the folder on the DLL search path for anything else it needs.
#[cfg(windows)]
fn preload_windows(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{LoadLibraryW, SetDllDirectoryW};
    let wide = |p: &Path| p.as_os_str().encode_wide().chain([0]).collect::<Vec<u16>>();
    // SAFETY: valid NUL-terminated wide strings; the module stays loaded for
    // the life of the process, which is what the runtime expects.
    unsafe {
        SetDllDirectoryW(wide(dir).as_ptr());
        let dml = dir.join("DirectML.dll");
        if dml.is_file() && LoadLibraryW(wide(&dml).as_ptr()).is_null() {
            tracing::warn!(path = %name(&dml), "couldn't preload DirectML.dll");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(options: Vec<(&'static str, String)>) -> Vec<String> {
        options.into_iter().map(|(name, value)| format!("{name}={value}")).collect()
    }

    #[test]
    fn coreml_options_keep_todays_formats_and_add_the_cache() {
        // Today's sessions, named: the NeuralNetwork format on every compute
        // unit (ORT's defaults), and for fp32 an ML program without fp16
        // accumulation on the GPU.
        assert_eq!(
            pairs(coreml_options(Precision::Fp16Safe, None)),
            ["ModelFormat=NeuralNetwork", "MLComputeUnits=ALL"]
        );
        assert_eq!(
            pairs(coreml_options(Precision::Fp32, None)),
            ["ModelFormat=MLProgram", "AllowLowPrecisionAccumulationOnGPU=0"]
        );
        // The cache adds the folder to compile into, and nothing else.
        let dir = Path::new("cache").join(cache::COREML).join("0123456789abcdef");
        for precision in [Precision::Fp16Safe, Precision::Fp32] {
            let mut kept = coreml_options(precision, Some(&dir));
            assert_eq!(kept.pop(), Some(("ModelCacheDirectory", dir.to_string_lossy().into_owned())));
            assert_eq!(kept, coreml_options(precision, None));
        }
        // What the bench tries replaces today's value, or adds its own.
        let trial = CoreMlTrial { format: Some(CoreMlFormat::MlProgram), units: Some(CoreMlUnits::CpuAndNeuralEngine) };
        assert_eq!(
            pairs(trial.options(Precision::Fp16Safe, None)),
            ["ModelFormat=MLProgram", "MLComputeUnits=CPUAndNeuralEngine"]
        );
        let processor = CoreMlTrial { units: Some(CoreMlUnits::CpuOnly), ..CoreMlTrial::default() };
        assert_eq!(
            pairs(processor.options(Precision::Fp32, None)),
            ["ModelFormat=MLProgram", "AllowLowPrecisionAccumulationOnGPU=0", "MLComputeUnits=CPUOnly"]
        );
        // ORT's spellings (`coreml_options.cc`).
        let units = [CoreMlUnits::All, CoreMlUnits::CpuAndNeuralEngine, CoreMlUnits::CpuAndGpu, CoreMlUnits::CpuOnly];
        assert_eq!(units.map(CoreMlUnits::option), ["ALL", "CPUAndNeuralEngine", "CPUAndGPU", "CPUOnly"]);
        assert_eq!(
            [CoreMlFormat::NeuralNetwork, CoreMlFormat::MlProgram].map(CoreMlFormat::option),
            ["NeuralNetwork", "MLProgram"]
        );
    }

    #[test]
    fn sessions_try_nvidias_engine_first_only_where_it_may_take_them() {
        use Device::{CoreMl, Cpu, DirectMl, NvRtx};
        // Registered, and the session may use it: then DirectML, then the processor.
        assert_eq!(chain(Some(DirectMl), true), [NvRtx, DirectMl, Cpu]);
        // Not registered, or a model never allowed on it (`without_accel`).
        assert_eq!(chain(Some(DirectMl), false), [DirectMl, Cpu]);
        assert_eq!(chain(Some(CoreMl), false), [CoreMl, Cpu]);
        // The processor only: no graphics card, so no engine for one either.
        assert_eq!(chain(None, true), [Cpu]);
        assert_eq!(chain(None, false), [Cpu]);
        // Logs and project notes tell the engines apart.
        assert_eq!(NvRtx.label(), "GPU (TensorRT for RTX)");
        assert_ne!(NvRtx.label(), DirectMl.label());
    }
}
