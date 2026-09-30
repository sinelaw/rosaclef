//! Native CLAP plugin hosting for Rosaclef.
//!
//! * [`scan`] finds `.clap` plugins and lists their descriptors.
//! * [`params`] lists the parameters of one plugin.
//! * [`ClapHost`] implements [`rosaclef_engine::PluginHost`], so the engine can
//!   instantiate CLAP instruments and effects.
//!
//! # Safety model
//!
//! All `unsafe` code talks to the CLAP C ABI and lives in this file. The
//! invariants it relies on are:
//!
//! * A [`ClapLibrary`] keeps its shared object loaded (and `clap_entry.init`
//!   called) for as long as it exists; every plugin instance holds an `Arc` to
//!   its library, so no code pointer into the library outlives it. Libraries
//!   are tracked in a process-wide registry so that `init`/`deinit` of one
//!   binary never overlap, even when [`scan`], [`params`] and several
//!   [`ClapHost`]s use the same plugin.
//! * An [`Instance`] owns a `clap_plugin` pointer from `create_plugin`, plus
//!   the heap-allocated `clap_host` it was created with. The host struct is
//!   freed only after `destroy`, and `destroy` runs exactly once.
//! * The processor owns every buffer and event list it hands to the plugin;
//!   the raw pointers inside `clap_process` point into heap allocations that
//!   never move or resize after construction (events may grow, but only
//!   between `process` calls, never while the plugin holds the pointers).
//! * Callbacks we give the plugin (`clap_host` functions, event lists) never
//!   panic.

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::log::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{clap_process, CLAP_PROCESS_ERROR};
use clap_sys::version::{clap_version_is_compatible, CLAP_VERSION};
use libloading::Library;
use rosaclef_core::Device;
use rosaclef_engine::instruments::{NoteEvent, NoteKind};
use rosaclef_engine::{ExternalProcessor, PluginHost};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CStr, CString};
use std::io::Write as _;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

// =================================================================== public

/// A plugin found by [`scan`].
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    /// Always `"clap"`.
    pub format: String,
    /// The `.clap` file (or bundle directory on macOS).
    pub path: String,
    /// Plugin id inside the bundle.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub description: String,
    pub features: Vec<String>,
    /// Has the `instrument` feature.
    pub instrument: bool,
    /// Has the `audio-effect` feature.
    pub effect: bool,
}

/// A plugin parameter, as reported by the `clap.params` extension.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginParam {
    /// Decimal `clap_id`; the key used in `Device::params`.
    pub id: String,
    pub name: String,
    pub module: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub stepped: bool,
}

/// The standard CLAP search locations for this OS, preceded by the entries of
/// the `CLAP_PATH` environment variable.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    if let Some(v) = std::env::var_os("CLAP_PATH") {
        out.extend(std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(h) = &home {
            out.push(h.join("Library/Audio/Plug-Ins/CLAP"));
        }
        out.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    } else if cfg!(windows) {
        if let Some(c) = std::env::var_os("COMMONPROGRAMFILES") {
            out.push(PathBuf::from(c).join("CLAP"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            out.push(
                PathBuf::from(l)
                    .join("Programs")
                    .join("Common")
                    .join("CLAP"),
            );
        }
    } else {
        if let Some(h) = &home {
            out.push(h.join(".clap"));
        }
        out.push(PathBuf::from("/usr/lib/clap"));
        out.push(PathBuf::from("/usr/local/lib/clap"));
    }
    out
}

/// Recursively find `.clap` plugins under `paths` and list every plugin in
/// their factories. Plugins that fail to load are reported on stderr and
/// skipped.
pub fn scan(paths: &[PathBuf]) -> Vec<PluginDescriptor> {
    let mut files = vec![];
    let mut seen = HashSet::new();
    for p in paths {
        find_clap_files(p, 0, &mut seen, &mut files);
    }
    let mut out = vec![];
    for f in files {
        let result =
            std::panic::catch_unwind(|| ClapLibrary::open(&f).map(|lib| lib.descriptors()));
        match result {
            Ok(Ok(descs)) => out.extend(descs),
            Ok(Err(e)) => log(&format!("skipping {}: {e}", f.display())),
            Err(_) => log(&format!("skipping {}: panicked while loading", f.display())),
        }
    }
    out
}

/// Instantiate plugin `id` (the first one when empty) from `path` and list
/// its parameters.
pub fn params(path: &str, id: &str) -> Result<Vec<PluginParam>, String> {
    let lib = ClapLibrary::open(Path::new(path))?;
    let inst = Instance::create(lib, id)?;
    Ok(inst.param_infos())
}

/// Hosts CLAP plugins for the engine. Loaded libraries are cached (by path)
/// for the lifetime of the host.
pub struct ClapHost {
    libs: Mutex<HashMap<PathBuf, Arc<ClapLibrary>>>,
}

impl ClapHost {
    pub fn new() -> ClapHost {
        ClapHost {
            libs: Mutex::new(HashMap::new()),
        }
    }

    fn library(&self, path: &Path) -> Result<Arc<ClapLibrary>, String> {
        let mut libs = lock(&self.libs);
        if let Some(l) = libs.get(path) {
            return Ok(l.clone());
        }
        let lib = ClapLibrary::open(path)?;
        libs.insert(path.to_path_buf(), lib.clone());
        Ok(lib)
    }
}

impl Default for ClapHost {
    fn default() -> Self {
        ClapHost::new()
    }
}

impl PluginHost for ClapHost {
    fn load(
        &self,
        dev: &Device,
        instrument: bool,
        sample_rate: f32,
        max_block: usize,
    ) -> Result<Box<dyn ExternalProcessor>, String> {
        let format = dev.option("format");
        if !format.is_empty() && format != "clap" {
            return Err(format!("unsupported plugin format \"{format}\""));
        }
        let path = dev.option("path");
        if path.is_empty() {
            return Err("plugin has no path".into());
        }
        let lib = self.library(Path::new(path))?;
        let inst = Instance::create(lib, dev.option("id"))?;
        let mut proc_ = ClapProcessor::new(inst, instrument, sample_rate, max_block)?;
        for (k, v) in &dev.params {
            proc_.set_param(k, *v);
        }
        Ok(Box::new(proc_))
    }
}

// ================================================================== helpers

fn log(msg: &str) {
    // Never panic (this can run inside FFI callbacks).
    let _ = writeln!(std::io::stderr(), "[rosaclef-clap] {msg}");
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Read a possibly-null C string.
///
/// # Safety
/// `p` must be null or point to a NUL-terminated string.
unsafe fn cstr_lossy(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// Read a fixed-size C string buffer, stopping at the first NUL (or the end).
fn cbuf_lossy(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn is_clap(p: &Path) -> bool {
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("clap"))
}

fn find_clap_files(p: &Path, depth: usize, seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    if depth > 16 {
        return;
    }
    let Ok(meta) = std::fs::metadata(p) else {
        return;
    };
    let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if !seen.insert(canon) {
        return;
    }
    if is_clap(p) && (meta.is_file() || (meta.is_dir() && p.join("Contents").is_dir())) {
        out.push(p.to_path_buf());
        return;
    }
    if meta.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for e in entries {
            find_clap_files(&e, depth + 1, seen, out);
        }
    }
}

/// The shared object inside a `.clap`: the file itself, or on macOS the
/// binary inside the bundle directory.
fn binary_path(bundle: &Path) -> PathBuf {
    if !bundle.is_dir() {
        return bundle.to_path_buf();
    }
    let macos = bundle.join("Contents").join("MacOS");
    if let Some(stem) = bundle.file_stem() {
        let candidate = macos.join(stem);
        if candidate.is_file() {
            return candidate;
        }
    }
    std::fs::read_dir(&macos)
        .ok()
        .and_then(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| p.is_file())
        })
        .unwrap_or(macos)
}

// ================================================================= library

/// A loaded CLAP binary with `clap_entry.init` called.
struct ClapLibrary {
    path: PathBuf,
    entry: *const clap_plugin_entry,
    factory: *const clap_plugin_factory,
    /// Dropped explicitly in `Drop`, after `deinit`, under the registry lock.
    lib: ManuallyDrop<Library>,
}

// SAFETY: the CLAP spec requires the entry and plugin factory functions to be
// thread-safe; the pointers are immutable and stay valid while `lib` is loaded.
unsafe impl Send for ClapLibrary {}
unsafe impl Sync for ClapLibrary {}

/// Every library currently loaded in this process, keyed by canonical path.
fn registry() -> &'static Mutex<HashMap<PathBuf, Weak<ClapLibrary>>> {
    static REG: OnceLock<Mutex<HashMap<PathBuf, Weak<ClapLibrary>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

impl ClapLibrary {
    /// Load (or reuse) the library at `path` and initialise its entry.
    fn open(path: &Path) -> Result<Arc<ClapLibrary>, String> {
        let key = std::fs::canonicalize(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let mut reg = lock(registry());
        if let Some(lib) = reg.get(&key).and_then(Weak::upgrade) {
            return Ok(lib);
        }
        let bin = binary_path(&key);
        // SAFETY: loading a plugin runs its initialisers; that is inherent to
        // hosting native plugins.
        let lib = unsafe { Library::new(&bin) }
            .map_err(|e| format!("cannot load {}: {e}", bin.display()))?;
        // SAFETY: `clap_entry` is a data symbol of type `clap_plugin_entry`;
        // the symbol value is its address.
        let entry: *const clap_plugin_entry = unsafe {
            let sym = lib
                .get::<*const clap_plugin_entry>(b"clap_entry\0")
                .map_err(|_| format!("{} has no clap_entry symbol", bin.display()))?;
            *sym
        };
        if entry.is_null() {
            return Err(format!("{}: clap_entry is null", bin.display()));
        }
        // SAFETY: `entry` points to a static struct inside the loaded library.
        let e = unsafe { *entry };
        if !clap_version_is_compatible(e.clap_version) {
            return Err(format!(
                "{}: incompatible CLAP version {}.{}",
                bin.display(),
                e.clap_version.major,
                e.clap_version.minor
            ));
        }
        let (Some(init), Some(get_factory)) = (e.init, e.get_factory) else {
            return Err(format!("{}: incomplete clap_entry", bin.display()));
        };
        let cpath = CString::new(key.to_string_lossy().as_bytes())
            .map_err(|_| "path contains NUL".to_string())?;
        // SAFETY: per the CLAP spec; `cpath` outlives the call.
        if !unsafe { init(cpath.as_ptr()) } {
            return Err(format!("{}: clap_entry.init failed", bin.display()));
        }
        // SAFETY: the factory id is a valid C string.
        let factory =
            unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const clap_plugin_factory;
        let lib = Arc::new(ClapLibrary {
            path: path.to_path_buf(),
            entry,
            factory,
            lib: ManuallyDrop::new(lib),
        });
        if factory.is_null() {
            // Dropping `lib` calls deinit; the registry lock is released first.
            drop(reg);
            return Err(format!("{}: no plugin factory", bin.display()));
        }
        reg.insert(key, Arc::downgrade(&lib));
        Ok(lib)
    }

    fn factory(&self) -> &clap_plugin_factory {
        // SAFETY: non-null (checked in `open`) and valid while loaded.
        unsafe { &*self.factory }
    }

    /// Every plugin descriptor in the factory.
    fn raw_descriptors(&self) -> Vec<*const clap_plugin_descriptor> {
        let f = self.factory();
        let (Some(count), Some(get)) = (f.get_plugin_count, f.get_plugin_descriptor) else {
            return vec![];
        };
        // SAFETY: factory functions called per the CLAP spec.
        unsafe {
            (0..count(f))
                .map(|i| get(f, i))
                .filter(|d| !d.is_null())
                .collect()
        }
    }

    fn descriptors(&self) -> Vec<PluginDescriptor> {
        let path = self.path.to_string_lossy().into_owned();
        self.raw_descriptors()
            .into_iter()
            .map(|d| {
                // SAFETY: descriptor pointers returned by the factory are valid
                // while the library is loaded; all strings are C strings.
                unsafe {
                    let d = &*d;
                    let mut features = vec![];
                    if !d.features.is_null() {
                        let mut i = 0;
                        while !(*d.features.add(i)).is_null() && i < 256 {
                            features.push(cstr_lossy(*d.features.add(i)));
                            i += 1;
                        }
                    }
                    PluginDescriptor {
                        format: "clap".into(),
                        path: path.clone(),
                        id: cstr_lossy(d.id),
                        name: cstr_lossy(d.name),
                        vendor: cstr_lossy(d.vendor),
                        version: cstr_lossy(d.version),
                        description: cstr_lossy(d.description),
                        instrument: features.iter().any(|f| f == "instrument"),
                        effect: features.iter().any(|f| f == "audio-effect"),
                        features,
                    }
                }
            })
            .collect()
    }

    /// The id of plugin `id`, or of the first plugin when `id` is empty.
    fn resolve(&self, id: &str) -> Result<CString, String> {
        for d in self.raw_descriptors() {
            // SAFETY: see `descriptors`.
            let (pid, version) = unsafe { ((*d).id, (*d).clap_version) };
            if pid.is_null() || !clap_version_is_compatible(version) {
                continue;
            }
            // SAFETY: non-null C string.
            let pid = unsafe { CStr::from_ptr(pid) };
            if id.is_empty() || pid.to_bytes() == id.as_bytes() {
                return Ok(pid.to_owned());
            }
        }
        Err(if id.is_empty() {
            format!("{}: no plugins in factory", self.path.display())
        } else {
            format!("{}: plugin \"{id}\" not found", self.path.display())
        })
    }
}

impl Drop for ClapLibrary {
    fn drop(&mut self) {
        // Hold the registry lock so a concurrent `open` of the same binary
        // cannot interleave its `init` with our `deinit`.
        let _reg = lock(registry());
        // SAFETY: `entry` is valid until the library is unloaded below; init
        // succeeded (a ClapLibrary exists only after a successful init).
        unsafe {
            if let Some(deinit) = (*self.entry).deinit {
                deinit();
            }
            ManuallyDrop::drop(&mut self.lib);
        }
    }
}

// ==================================================================== host

/// Our `clap_host`, plus state its callbacks can set. Allocated with
/// `Box::into_raw` so its address is stable; freed by `HostBox::drop`.
struct HostData {
    raw: clap_host,
    restart_requested: AtomicBool,
    callback_requested: AtomicBool,
}

struct HostBox(*mut HostData);

impl HostBox {
    fn new() -> HostBox {
        let p = Box::into_raw(Box::new(HostData {
            raw: clap_host {
                clap_version: CLAP_VERSION,
                host_data: null_mut(),
                name: c"Rosaclef".as_ptr(),
                vendor: c"Rosaclef".as_ptr(),
                url: c"https://github.com/sinelaw/rosaclef".as_ptr(),
                version: c"0.1.0".as_ptr(),
                get_extension: Some(host_get_extension),
                request_restart: Some(host_request_restart),
                request_process: Some(host_request_process),
                request_callback: Some(host_request_callback),
            },
            restart_requested: AtomicBool::new(false),
            callback_requested: AtomicBool::new(false),
        }));
        // SAFETY: `p` was just allocated and is uniquely owned.
        unsafe { (*p).raw.host_data = p as *mut c_void };
        HostBox(p)
    }

    fn raw(&self) -> *const clap_host {
        // SAFETY: `self.0` is valid until drop.
        unsafe { &(*self.0).raw }
    }

    fn data(&self) -> &HostData {
        // SAFETY: valid until drop; only shared access after construction.
        unsafe { &*self.0 }
    }
}

impl Drop for HostBox {
    fn drop(&mut self) {
        // SAFETY: allocated by `Box::into_raw` in `new`, freed once.
        unsafe { drop(Box::from_raw(self.0)) }
    }
}

/// # Safety
/// `host` must be null or a `clap_host` created by `HostBox::new`.
unsafe fn host_data<'a>(host: *const clap_host) -> Option<&'a HostData> {
    if host.is_null() {
        return None;
    }
    ((*host).host_data as *const HostData).as_ref()
}

static HOST_LOG: clap_host_log = clap_host_log {
    log: Some(host_log),
};
static HOST_PARAMS: clap_host_params = clap_host_params {
    rescan: Some(host_params_rescan),
    clear: Some(host_params_clear),
    request_flush: Some(host_params_request_flush),
};

unsafe extern "C" fn host_get_extension(
    _host: *const clap_host,
    id: *const c_char,
) -> *const c_void {
    if id.is_null() {
        return null();
    }
    let id = CStr::from_ptr(id);
    if id == CLAP_EXT_LOG {
        &HOST_LOG as *const clap_host_log as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &HOST_PARAMS as *const clap_host_params as *const c_void
    } else {
        null()
    }
}

unsafe extern "C" fn host_request_restart(host: *const clap_host) {
    if let Some(h) = host_data(host) {
        h.restart_requested.store(true, Ordering::Relaxed);
    }
}

/// We call `process` on every block anyway.
unsafe extern "C" fn host_request_process(_host: *const clap_host) {}

unsafe extern "C" fn host_request_callback(host: *const clap_host) {
    if let Some(h) = host_data(host) {
        h.callback_requested.store(true, Ordering::Relaxed);
    }
}

unsafe extern "C" fn host_log(
    _host: *const clap_host,
    severity: clap_log_severity,
    msg: *const c_char,
) {
    let level = match severity {
        CLAP_LOG_DEBUG => "debug",
        CLAP_LOG_INFO => "info",
        CLAP_LOG_WARNING => "warning",
        CLAP_LOG_ERROR => "error",
        CLAP_LOG_FATAL => "fatal",
        CLAP_LOG_HOST_MISBEHAVING => "host misbehaving",
        CLAP_LOG_PLUGIN_MISBEHAVING => "plugin misbehaving",
        _ => "log",
    };
    log(&format!("plugin {level}: {}", cstr_lossy(msg)));
}

unsafe extern "C" fn host_params_rescan(_host: *const clap_host, _flags: clap_param_rescan_flags) {}
unsafe extern "C" fn host_params_clear(
    _host: *const clap_host,
    _id: clap_id,
    _flags: clap_param_clear_flags,
) {
}
/// Parameter changes are flushed with the next `process` call.
unsafe extern "C" fn host_params_request_flush(_host: *const clap_host) {}

// ================================================================ instance

/// An initialised plugin instance (main-thread lifecycle). Dropping it
/// deactivates (if needed) and destroys the plugin.
struct Instance {
    plugin: *const clap_plugin,
    activated: bool,
    /// Must outlive `plugin` (dropped after `destroy` in `Drop`).
    host: HostBox,
    _lib: Arc<ClapLibrary>,
}

impl Instance {
    /// `create_plugin` + `init`.
    fn create(lib: Arc<ClapLibrary>, id: &str) -> Result<Instance, String> {
        let cid = lib.resolve(id)?;
        let host = HostBox::new();
        let f = lib.factory();
        let Some(create) = f.create_plugin else {
            return Err("factory cannot create plugins".into());
        };
        // SAFETY: factory, host and id are valid for the call.
        let plugin = unsafe { create(f, host.raw(), cid.as_ptr()) };
        if plugin.is_null() {
            return Err(format!(
                "could not create plugin \"{}\"",
                cid.to_string_lossy()
            ));
        }
        let inst = Instance {
            plugin,
            activated: false,
            host,
            _lib: lib,
        };
        // SAFETY: freshly created plugin; `init` is the first call.
        let ok = unsafe { inst.raw().init.is_some_and(|init| init(plugin)) };
        if !ok {
            // Dropping `inst` destroys the plugin, as the spec requires.
            return Err(format!(
                "plugin \"{}\" failed to initialise",
                cid.to_string_lossy()
            ));
        }
        inst.service_callback();
        Ok(inst)
    }

    fn raw(&self) -> &clap_plugin {
        // SAFETY: valid until `destroy` in `Drop`.
        unsafe { &*self.plugin }
    }

    /// Main thread: honour a pending `request_callback`.
    fn service_callback(&self) {
        if self
            .host
            .data()
            .callback_requested
            .swap(false, Ordering::Relaxed)
        {
            if let Some(f) = self.raw().on_main_thread {
                // SAFETY: called on the (logical) main thread.
                unsafe { f(self.plugin) };
            }
        }
    }

    /// Look up a plugin extension.
    ///
    /// # Safety
    /// `T` must be the struct type the CLAP spec defines for `id`.
    unsafe fn extension<T>(&self, id: &CStr) -> Option<&T> {
        let get = self.raw().get_extension?;
        (get(self.plugin, id.as_ptr()) as *const T).as_ref()
    }

    fn param_infos(&self) -> Vec<PluginParam> {
        // SAFETY: CLAP_EXT_PARAMS is a `clap_plugin_params`.
        let Some(ext) = (unsafe { self.extension::<clap_plugin_params>(CLAP_EXT_PARAMS) }) else {
            return vec![];
        };
        let (Some(count), Some(get_info)) = (ext.count, ext.get_info) else {
            return vec![];
        };
        let mut out = vec![];
        // SAFETY: main-thread calls per the spec; `info` is a valid out-param.
        unsafe {
            for i in 0..count(self.plugin) {
                let mut info: clap_param_info = std::mem::zeroed();
                if !get_info(self.plugin, i, &mut info) {
                    continue;
                }
                out.push(PluginParam {
                    id: info.id.to_string(),
                    name: cbuf_lossy(&info.name),
                    module: cbuf_lossy(&info.module),
                    min: info.min_value,
                    max: info.max_value,
                    default: info.default_value,
                    stepped: info.flags & CLAP_PARAM_IS_STEPPED != 0,
                });
            }
        }
        out
    }

    /// Channel counts of the plugin's audio ports and the index of the main
    /// one, or `None` when the plugin lacks the audio-ports extension.
    fn audio_ports(&self, input: bool) -> Option<(Vec<u32>, Option<usize>)> {
        // SAFETY: CLAP_EXT_AUDIO_PORTS is a `clap_plugin_audio_ports`.
        let ext = unsafe { self.extension::<clap_plugin_audio_ports>(CLAP_EXT_AUDIO_PORTS) }?;
        let (Some(count), Some(get)) = (ext.count, ext.get) else {
            return None;
        };
        let mut chans = vec![];
        let mut main = None;
        // SAFETY: main-thread calls while deactivated; valid out-param.
        unsafe {
            for i in 0..count(self.plugin, input).min(64) {
                let mut info: clap_audio_port_info = std::mem::zeroed();
                if !get(self.plugin, i, input, &mut info) {
                    info.channel_count = 0;
                }
                if main.is_none() && info.flags & CLAP_AUDIO_PORT_IS_MAIN != 0 {
                    main = Some(i as usize);
                }
                chans.push(info.channel_count.min(64));
            }
        }
        if main.is_none() && !chans.is_empty() {
            main = Some(0);
        }
        Some((chans, main))
    }

    /// Whether note input should be sent as MIDI (the plugin's first note
    /// port does not speak the CLAP note dialect but does speak MIDI).
    fn wants_midi_notes(&self) -> bool {
        // SAFETY: CLAP_EXT_NOTE_PORTS is a `clap_plugin_note_ports`.
        let Some(ext) = (unsafe { self.extension::<clap_plugin_note_ports>(CLAP_EXT_NOTE_PORTS) })
        else {
            return false;
        };
        let (Some(count), Some(get)) = (ext.count, ext.get) else {
            return false;
        };
        // SAFETY: main-thread calls; valid out-param.
        unsafe {
            if count(self.plugin, true) == 0 {
                return false;
            }
            let mut info: clap_note_port_info = std::mem::zeroed();
            if !get(self.plugin, 0, true, &mut info) {
                return false;
            }
            info.supported_dialects & CLAP_NOTE_DIALECT_CLAP == 0
                && info.supported_dialects & CLAP_NOTE_DIALECT_MIDI != 0
        }
    }

    fn activate(&mut self, sample_rate: f64, max_block: u32) -> Result<(), String> {
        let Some(activate) = self.raw().activate else {
            return Err("plugin cannot be activated".into());
        };
        // SAFETY: main thread, plugin initialised and not active.
        if !unsafe { activate(self.plugin, sample_rate, 1, max_block) } {
            return Err("plugin failed to activate".into());
        }
        self.activated = true;
        self.service_callback();
        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: the plugin is valid; deactivate only if activated, then
        // destroy exactly once. `host` and the library are dropped afterwards.
        unsafe {
            let p = *self.raw();
            if self.activated {
                if let Some(f) = p.deactivate {
                    f(self.plugin);
                }
            }
            if let Some(f) = p.destroy {
                f(self.plugin);
            }
        }
    }
}

// =================================================================== events

/// Storage for one input event. Every variant starts with a
/// `clap_event_header`, so the header can always be read.
#[repr(C)]
#[derive(Clone, Copy)]
union Event {
    header: clap_event_header,
    note: clap_event_note,
    param: clap_event_param_value,
    midi: clap_event_midi,
}

impl Event {
    fn time(&self) -> u32 {
        // SAFETY: every variant begins with a header.
        unsafe { self.header.time }
    }

    fn header(ty: u16, size: usize, time: u32) -> clap_event_header {
        clap_event_header {
            size: size as u32,
            time,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: ty,
            flags: 0,
        }
    }

    fn param(id: clap_id, value: f64) -> Event {
        Event {
            param: clap_event_param_value {
                header: Self::header(
                    CLAP_EVENT_PARAM_VALUE,
                    std::mem::size_of::<clap_event_param_value>(),
                    0,
                ),
                param_id: id,
                cookie: null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value,
            },
        }
    }

    fn note(on: bool, key: u8, velocity: f64, time: u32, midi: bool) -> Event {
        if midi {
            let vel = (velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
            let status = if on { 0x90 } else { 0x80 };
            return Event {
                midi: clap_event_midi {
                    header: Self::header(
                        CLAP_EVENT_MIDI,
                        std::mem::size_of::<clap_event_midi>(),
                        time,
                    ),
                    port_index: 0,
                    data: [status, key.min(127), if on { vel.max(1) } else { vel }],
                },
            };
        }
        Event {
            note: clap_event_note {
                header: Self::header(
                    if on {
                        CLAP_EVENT_NOTE_ON
                    } else {
                        CLAP_EVENT_NOTE_OFF
                    },
                    std::mem::size_of::<clap_event_note>(),
                    time,
                ),
                note_id: -1,
                port_index: 0,
                channel: 0,
                key: key as i16,
                velocity,
            },
        }
    }
}

unsafe extern "C" fn in_events_size(list: *const clap_input_events) -> u32 {
    match list
        .as_ref()
        .and_then(|l| (l.ctx as *const Vec<Event>).as_ref())
    {
        Some(v) => v.len() as u32,
        None => 0,
    }
}

unsafe extern "C" fn in_events_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    match list
        .as_ref()
        .and_then(|l| (l.ctx as *const Vec<Event>).as_ref())
    {
        Some(v) => v
            .get(index as usize)
            .map_or(null(), |e| e as *const Event as *const clap_event_header),
        None => null(),
    }
}

/// Output events (parameter gestures, note ends, ...) are accepted and ignored.
unsafe extern "C" fn out_events_try_push(
    _list: *const clap_output_events,
    _ev: *const clap_event_header,
) -> bool {
    true
}

// ================================================================ processor

/// Owned channel buffers of one audio port, and the pointer array handed to
/// the plugin.
struct Port {
    channels: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl Port {
    fn new(count: u32, frames: usize) -> Port {
        let mut channels: Vec<Vec<f32>> = (0..count).map(|_| vec![0.0; frames]).collect();
        let ptrs = channels.iter_mut().map(|c| c.as_mut_ptr()).collect();
        Port { channels, ptrs }
    }
}

/// A CLAP plugin wrapped as an engine processor.
struct ClapProcessor {
    inst: Instance,
    instrument: bool,
    started: bool,
    failed: bool,
    midi_notes: bool,
    steady_time: i64,
    max_block: usize,
    inputs: Vec<Port>,
    outputs: Vec<Port>,
    in_bufs: Vec<clap_audio_buffer>,
    out_bufs: Vec<clap_audio_buffer>,
    main_in: Option<usize>,
    main_out: Option<usize>,
    /// Parameter changes waiting for the next block.
    pending_params: Vec<(clap_id, f64)>,
    events: Vec<Event>,
    held: [bool; 128],
}

// SAFETY: the raw pointers refer to memory owned by this struct (buffers) or
// to the plugin instance, which CLAP allows to be driven from the audio thread
// after being created on the main thread. Nothing is shared with other
// threads without synchronisation (host callbacks only touch atomics).
unsafe impl Send for ClapProcessor {}

impl ClapProcessor {
    fn new(
        mut inst: Instance,
        instrument: bool,
        sample_rate: f32,
        max_block: usize,
    ) -> Result<ClapProcessor, String> {
        let max_block = max_block.max(1);
        let (in_chans, main_in) = inst.audio_ports(true).unwrap_or(if instrument {
            (vec![], None)
        } else {
            (vec![2], Some(0))
        });
        let (out_chans, main_out) = inst.audio_ports(false).unwrap_or((vec![2], Some(0)));
        let midi_notes = instrument && inst.wants_midi_notes();
        inst.activate(sample_rate as f64, max_block as u32)?;
        let mut inputs: Vec<Port> = in_chans.iter().map(|c| Port::new(*c, max_block)).collect();
        let mut outputs: Vec<Port> = out_chans.iter().map(|c| Port::new(*c, max_block)).collect();
        let buf = |p: &mut Port| clap_audio_buffer {
            data32: p.ptrs.as_mut_ptr(),
            data64: null_mut(),
            channel_count: p.ptrs.len() as u32,
            latency: 0,
            constant_mask: 0,
        };
        let in_bufs = inputs.iter_mut().map(buf).collect();
        let out_bufs = outputs.iter_mut().map(buf).collect();
        Ok(ClapProcessor {
            inst,
            instrument,
            started: false,
            failed: false,
            midi_notes,
            steady_time: 0,
            max_block,
            inputs,
            outputs,
            in_bufs,
            out_bufs,
            main_in,
            main_out,
            pending_params: Vec::with_capacity(64),
            events: Vec::with_capacity(512),
            held: [false; 128],
        })
    }

    /// Queue note events that fall in `from..from + n` (block-relative).
    fn push_notes(&mut self, events: &[NoteEvent], from: usize, n: usize) {
        for ev in events
            .iter()
            .filter(|e| e.offset >= from && e.offset < from + n)
        {
            let t = (ev.offset - from) as u32;
            match ev.kind {
                NoteKind::On { key, velocity } => {
                    let key = key.min(127);
                    self.held[key as usize] = true;
                    self.events
                        .push(Event::note(true, key, velocity as f64, t, self.midi_notes));
                }
                NoteKind::Off { key } => {
                    let key = key.min(127);
                    self.held[key as usize] = false;
                    self.events
                        .push(Event::note(false, key, 0.0, t, self.midi_notes));
                }
                NoteKind::AllOff => {
                    for key in 0..128u8 {
                        if std::mem::take(&mut self.held[key as usize]) {
                            self.events
                                .push(Event::note(false, key, 0.0, t, self.midi_notes));
                        }
                    }
                }
            }
        }
    }

    /// Process `left.len()` (<= max_block) frames; `events` offsets are
    /// relative to `from` within the engine's block.
    fn process_chunk(
        &mut self,
        events: &[NoteEvent],
        from: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let n = left.len();
        self.events.clear();
        for (id, v) in self.pending_params.drain(..) {
            self.events.push(Event::param(id, v));
        }
        self.push_notes(events, from, n);
        // Insertion sort by time (stable, allocation free; usually sorted).
        for i in 1..self.events.len() {
            let mut j = i;
            while j > 0 && self.events[j - 1].time() > self.events[j].time() {
                self.events.swap(j - 1, j);
                j -= 1;
            }
        }

        // Inputs: the main port gets a copy of the engine buffers, the rest silence.
        for (pi, port) in self.inputs.iter_mut().enumerate() {
            for (c, ch) in port.channels.iter_mut().enumerate() {
                let dst = &mut ch[..n];
                if Some(pi) != self.main_in || self.instrument {
                    dst.fill(0.0);
                } else if port.ptrs.len() == 1 {
                    for i in 0..n {
                        dst[i] = 0.5 * (left[i] + right[i]);
                    }
                } else if c == 0 {
                    dst.copy_from_slice(left);
                } else if c == 1 {
                    dst.copy_from_slice(right);
                } else {
                    dst.fill(0.0);
                }
            }
        }
        for b in self.in_bufs.iter_mut().chain(self.out_bufs.iter_mut()) {
            b.constant_mask = 0;
        }

        let in_events = clap_input_events {
            ctx: &self.events as *const Vec<Event> as *mut c_void,
            size: Some(in_events_size),
            get: Some(in_events_get),
        };
        let out_events = clap_output_events {
            ctx: null_mut(),
            try_push: Some(out_events_try_push),
        };
        let process = clap_process {
            steady_time: self.steady_time,
            frames_count: n as u32,
            transport: null(),
            audio_inputs: if self.in_bufs.is_empty() {
                null()
            } else {
                self.in_bufs.as_ptr()
            },
            audio_outputs: if self.out_bufs.is_empty() {
                null_mut()
            } else {
                self.out_bufs.as_mut_ptr()
            },
            audio_inputs_count: self.in_bufs.len() as u32,
            audio_outputs_count: self.out_bufs.len() as u32,
            in_events: &in_events,
            out_events: &out_events,
        };
        let status = match self.inst.raw().process {
            // SAFETY: audio thread, plugin active and processing; every
            // pointer in `process` is valid for `n` frames during the call.
            Some(f) => unsafe { f(self.inst.plugin, &process) },
            None => CLAP_PROCESS_ERROR,
        };
        self.steady_time += n as i64;

        if status == CLAP_PROCESS_ERROR {
            if self.instrument {
                left.fill(0.0);
                right.fill(0.0);
            }
            // Effects: leave the dry signal.
            return;
        }
        let Some(mo) = self.main_out.filter(|i| *i < self.outputs.len()) else {
            left.fill(0.0);
            right.fill(0.0);
            return;
        };
        let mask = self.out_bufs[mo].constant_mask;
        let port = &mut self.outputs[mo];
        for (c, ch) in port.channels.iter_mut().enumerate().take(64) {
            if mask & (1u64 << c) != 0 {
                let v = ch[0];
                ch[..n].fill(v);
            }
        }
        match port.channels.len() {
            0 => {
                left.fill(0.0);
                right.fill(0.0);
            }
            1 => {
                left.copy_from_slice(&port.channels[0][..n]);
                right.copy_from_slice(&port.channels[0][..n]);
            }
            _ => {
                left.copy_from_slice(&port.channels[0][..n]);
                right.copy_from_slice(&port.channels[1][..n]);
            }
        }
    }
}

impl ExternalProcessor for ClapProcessor {
    fn set_param(&mut self, id: &str, value: f64) {
        let Ok(id) = id.trim().parse::<clap_id>() else {
            return;
        };
        if !value.is_finite() {
            return;
        }
        match self.pending_params.iter_mut().find(|(i, _)| *i == id) {
            Some(p) => p.1 = value,
            None => self.pending_params.push((id, value)),
        }
    }

    fn process(&mut self, events: &[NoteEvent], left: &mut [f32], right: &mut [f32]) {
        let len = left.len().min(right.len());
        if !self.started && !self.failed {
            // SAFETY: first call on the audio thread, plugin is active.
            let ok = unsafe {
                self.inst
                    .raw()
                    .start_processing
                    .is_none_or(|f| f(self.inst.plugin))
            };
            if ok {
                self.started = true;
            } else {
                log("plugin failed to start processing");
                self.failed = true;
            }
        }
        if self.failed {
            if self.instrument {
                left.fill(0.0);
                right.fill(0.0);
            }
            return;
        }
        let mut from = 0;
        while from < len {
            let n = (len - from).min(self.max_block);
            self.process_chunk(
                events,
                from,
                &mut left[from..from + n],
                &mut right[from..from + n],
            );
            from += n;
        }
    }
}

impl Drop for ClapProcessor {
    fn drop(&mut self) {
        if self.started {
            if let Some(f) = self.inst.raw().stop_processing {
                // SAFETY: plugin is processing; stop before deactivate/destroy
                // (done by `Instance::drop` right after this).
                unsafe { f(self.inst.plugin) };
            }
        }
    }
}
