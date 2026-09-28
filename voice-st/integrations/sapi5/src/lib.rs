//! SAPI 5 text-to-speech engine for ST.
//!
//! Makes ST a regular Windows voice: Narrator in WinRE/WinPE (whose speech
//! stack is SAPI 5), NVDA's SAPI5 driver, JAWS and any SAPI client. The DLL
//! lives next to `st_synth.dll` (and the `neural\` runtime) of an ST release
//! and loads it by full path, independent of the host's DLL search path.
//!
//! Each registered voice token carries its ST configuration in the string
//! value `STConfig`, e.g. `{"backend":"neural","lang":"fr","voice":"ff_siwis"}`,
//! and optionally `STFallback` (a compact voice used if the neural runtime
//! cannot start, so the screen reader never goes silent).
//!
//! Output: 48 kHz, 16-bit, mono PCM (ST's native rate, no resampling).
//! Supported: text, silences, bookmarks (NVDA follows reading with them),
//! word boundaries, abort, live rate and volume. Panics never cross the COM
//! boundary: they become E_FAIL instead of taking the screen reader down.
#![allow(non_snake_case, non_upper_case_globals)]

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{implement, Interface, Ref, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_NOINTERFACE,
    E_POINTER, HMODULE, LPARAM, S_FALSE, S_OK, WPARAM,
};
use windows::Win32::Media::Audio::WAVEFORMATEX;
use windows::Win32::Media::Speech::*;
use windows::Win32::System::Com::{
    CoTaskMemAlloc, CoTaskMemFree, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GetProcAddress, LoadLibraryExW,
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    LOAD_WITH_ALTERED_SEARCH_PATH,
};
use windows_core::BOOL;

/// {76557F36-F953-4AD0-8C8A-08B6257DDCE0}
pub const CLSID_ST_ENGINE: GUID = GUID::from_u128(0x76557f36_f953_4ad0_8c8a_08b6257ddce0);
/// SPDFID_WaveFormatEx {C31ADBAE-527F-4FF5-A230-F62BB61FF70C}
const SPDFID_WAVEFORMATEX: GUID = GUID::from_u128(0xc31adbae_527f_4ff5_a230_f62bb61ff70c);
const SAMPLE_RATE: u32 = 48_000;

static LOCKS: AtomicUsize = AtomicUsize::new(0);
static OBJECTS: AtomicUsize = AtomicUsize::new(0);

// ------------------------------------------------------------------ ST ABI
type StCallback = unsafe extern "C" fn(*const f32, usize, u32, *mut c_void) -> u8;
type CreateFn = unsafe extern "C" fn(*const u8, usize, *mut *mut c_void) -> i32;
type StreamFn = unsafe extern "C" fn(*mut c_void, *const u8, usize, StCallback, *mut c_void) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut c_void);
type RateFn = unsafe extern "C" fn(*mut c_void, u32) -> i32;
type ErrorFn = unsafe extern "C" fn(*mut u8, usize) -> usize;

struct StApi {
    create: CreateFn,
    stream: StreamFn,
    destroy: DestroyFn,
    set_rate: RateFn,
    last_error: ErrorFn,
}

fn module_dir() -> Option<PathBuf> {
    unsafe {
        let mut module = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(module_dir as *const u16),
            &mut module,
        )
        .ok()?;
        let mut buf = vec![0u16; 32768];
        let n = GetModuleFileNameW(Some(module), &mut buf) as usize;
        if n == 0 || n >= buf.len() {
            return None;
        }
        PathBuf::from(String::from_utf16_lossy(&buf[..n]))
            .parent()
            .map(|p| p.to_path_buf())
    }
}

// Each symbol is transmuted to the fn-pointer type of the `StApi` field it
// initialises, which is the exact ABI declared in include/st_synth.h.
#[allow(clippy::missing_transmute_annotations)]
fn api() -> Result<&'static StApi, String> {
    static API: OnceLock<Result<StApi, String>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let path = module_dir()
            .ok_or("cannot locate st_sapi.dll")?
            .join("st_synth.dll");
        let wide: Vec<u16> = path.as_os_str().encode_wide_nul();
        let lib = LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH)
            .map_err(|e| format!("cannot load {}: {e}", path.display()))?;
        macro_rules! sym {
            ($name:literal) => {
                std::mem::transmute(
                    GetProcAddress(lib, windows::core::s!($name))
                        .ok_or(concat!("missing ", $name))?,
                )
            };
        }
        Ok(StApi {
            create: sym!("st_engine_create_v1"),
            stream: sym!("st_engine_stream_v1"),
            destroy: sym!("st_engine_destroy_v1"),
            set_rate: sym!("st_engine_set_rate_v1"),
            last_error: sym!("st_last_error_v1"),
        })
    })
    .as_ref()
    .map_err(Clone::clone)
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}
impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}

fn st_error(api: &StApi) -> String {
    let mut buf = [0u8; 512];
    unsafe { (api.last_error)(buf.as_mut_ptr(), buf.len()) };
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// One live ST handle (one voice). Destroyed exactly once.
struct Handle {
    ptr: *mut c_void,
    neural: bool,
}
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            unsafe { (api.destroy)(self.ptr) };
        }
    }
}

fn create(config: &str) -> Result<Handle, String> {
    let api = api()?;
    let mut ptr = std::ptr::null_mut();
    let code = unsafe { (api.create)(config.as_ptr(), config.len(), &mut ptr) };
    if code != 0 || ptr.is_null() {
        return Err(format!(
            "st_engine_create_v1({config}) = {code}: {}",
            st_error(api)
        ));
    }
    Ok(Handle {
        ptr,
        neural: config.contains("\"neural\""),
    })
}

/// SAPI rate -10..10 means 1/3x..3x; ST takes a percentage.
fn st_rate(sapi: i32, neural: bool) -> u32 {
    let pct = 100.0 * 3f64.powf(sapi.clamp(-10, 10) as f64 / 10.0);
    let (lo, hi) = if neural { (50.0, 200.0) } else { (50.0, 300.0) };
    pct.clamp(lo, hi).round() as u32
}

// ------------------------------------------------------------------ engine
#[implement(ISpTTSEngine, ISpObjectWithToken)]
pub struct Engine {
    token: Mutex<Option<ISpObjectToken>>,
    config: Mutex<(String, Option<String>)>,
    handle: Mutex<Option<Handle>>,
    rate: AtomicU32,
}

impl Engine {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        Engine {
            token: Mutex::new(None),
            config: Mutex::new((
                r#"{"backend":"compact","lang":"fr","voice":"female"}"#.into(),
                None,
            )),
            handle: Mutex::new(None),
            rate: AtomicU32::new(u32::MAX),
        }
    }

    /// The neural model loads once (~3 s) and stays; on failure the fallback
    /// voice is used so the user always hears something.
    fn ensure_handle(&self) -> windows::core::Result<()> {
        let mut h = self.handle.lock().unwrap();
        if h.is_some() {
            return Ok(());
        }
        let (primary, fallback) = self.config.lock().unwrap().clone();
        match create(&primary) {
            Ok(x) => *h = Some(x),
            Err(e) => match fallback.as_deref().map(create) {
                Some(Ok(x)) => {
                    debug(&format!("ST SAPI: {e}; using fallback voice"));
                    *h = Some(x)
                }
                _ => {
                    debug(&format!("ST SAPI: {e}"));
                    return Err(E_FAIL.into());
                }
            },
        }
        self.rate.store(u32::MAX, Ordering::SeqCst);
        Ok(())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

fn debug(msg: &str) {
    let wide: Vec<u16> = msg.encode_utf16().chain(Some(0)).collect();
    unsafe {
        windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(PCWSTR(wide.as_ptr()))
    };
}

impl ISpObjectWithToken_Impl for Engine_Impl {
    fn SetObjectToken(&self, ptoken: Ref<'_, ISpObjectToken>) -> windows::core::Result<()> {
        let token = ptoken.ok()?.clone();
        let read = |name: PCWSTR| -> Option<String> {
            unsafe {
                let p: PWSTR = token.GetStringValue(name).ok()?;
                let s = p.to_string().ok();
                CoTaskMemFree(Some(p.0 as *const c_void));
                s.filter(|s| !s.trim().is_empty())
            }
        };
        let primary = read(windows::core::w!("STConfig")).ok_or(E_INVALIDARG)?;
        let fallback = read(windows::core::w!("STFallback"));
        *self.config.lock().unwrap() = (primary, fallback);
        *self.handle.lock().unwrap() = None;
        *self.token.lock().unwrap() = Some(token);
        Ok(())
    }

    fn GetObjectToken(&self) -> windows::core::Result<ISpObjectToken> {
        self.token
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| S_FALSE.into())
    }
}

/// State shared with the ST audio callback during one Speak call.
struct Output<'a> {
    site: &'a ISpTTSEngineSite,
    written: u64,
    volume: u16,
    aborted: bool,
    failed: bool,
    buf: Vec<i16>,
}

impl Output<'_> {
    fn poll_actions(&mut self) {
        let actions = unsafe { self.site.GetActions() };
        if actions & SPVES_ABORT.0 as u32 != 0 {
            self.aborted = true;
        }
        if actions & SPVES_VOLUME.0 as u32 != 0 {
            if let Ok(v) = unsafe { self.site.GetVolume() } {
                self.volume = v.min(100);
            }
        }
    }

    fn write_samples(&mut self, samples: &[i16]) {
        let mut done = 0;
        let bytes =
            unsafe { std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 2) };
        while done < bytes.len() && !self.aborted {
            match unsafe {
                self.site.Write(
                    bytes[done..].as_ptr() as *const c_void,
                    (bytes.len() - done) as u32,
                )
            } {
                Ok(0) | Err(_) => {
                    self.failed = true;
                    return;
                }
                Ok(n) => done += n as usize,
            }
            self.poll_actions();
        }
        self.written += done as u64;
    }

    fn silence(&mut self, ms: u32) {
        let mut left = (SAMPLE_RATE as u64 * ms.min(60_000) as u64 / 1000) as usize;
        let zeros = [0i16; 4800];
        while left > 0 && !self.aborted && !self.failed {
            let n = left.min(zeros.len());
            self.write_samples(&zeros[..n]);
            left -= n;
        }
    }

    fn event(&self, id: SPEVENTENUM, lparam_type: SPEVENTLPARAMTYPE, wparam: usize, lparam: isize) {
        let ev = SPEVENT {
            _bitfield: (id.0 & 0xffff) | (lparam_type.0 << 16),
            ulStreamNum: 0,
            ullAudioStreamOffset: self.written,
            wParam: WPARAM(wparam),
            lParam: LPARAM(lparam),
        };
        let _ = unsafe { self.site.AddEvents(&ev, 1) };
    }
}

unsafe extern "C" fn on_audio(pcm: *const f32, n: usize, _rate: u32, ctx: *mut c_void) -> u8 {
    let out = &mut *(ctx as *mut Output);
    let result = catch_unwind(AssertUnwindSafe(|| {
        out.poll_actions();
        if out.aborted || out.failed || pcm.is_null() {
            return;
        }
        let gain = out.volume as f32 / 100.0 * 32767.0;
        let src = std::slice::from_raw_parts(pcm, n);
        let mut buf = std::mem::take(&mut out.buf);
        buf.clear();
        buf.extend(
            src.iter()
                .map(|&x| (x.clamp(-1.0, 1.0) * gain).round() as i16),
        );
        out.write_samples(&buf);
        out.buf = buf;
    }));
    if result.is_err() {
        out.failed = true;
    }
    (!(out.aborted || out.failed)) as u8
}

impl ISpTTSEngine_Impl for Engine_Impl {
    fn Speak(
        &self,
        _flags: u32,
        _format: *const GUID,
        _wfx: *const WAVEFORMATEX,
        frags: *const SPVTEXTFRAG,
        site: Ref<'_, ISpTTSEngineSite>,
    ) -> windows::core::Result<()> {
        let site = site.ok()?;
        match catch_unwind(AssertUnwindSafe(|| self.speak(frags, site))) {
            Ok(r) => r,
            Err(_) => Err(E_FAIL.into()),
        }
    }

    // COM method: the signature is fixed by the `windows` crate trait and cannot be
    // `unsafe fn`. SAPI owns both out-pointers; they are NULL-checked before use.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn GetOutputFormat(
        &self,
        _target_id: *const GUID,
        _target_wfx: *const WAVEFORMATEX,
        out_id: *mut GUID,
        out_wfx: *mut *mut WAVEFORMATEX,
    ) -> windows::core::Result<()> {
        if out_id.is_null() || out_wfx.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            let wfx = CoTaskMemAlloc(std::mem::size_of::<WAVEFORMATEX>()) as *mut WAVEFORMATEX;
            if wfx.is_null() {
                return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
            }
            wfx.write(WAVEFORMATEX {
                wFormatTag: 1, // WAVE_FORMAT_PCM
                nChannels: 1,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            });
            *out_id = SPDFID_WAVEFORMATEX;
            *out_wfx = wfx;
        }
        Ok(())
    }
}

impl Engine_Impl {
    fn speak(
        &self,
        mut frag: *const SPVTEXTFRAG,
        site: &ISpTTSEngineSite,
    ) -> windows::core::Result<()> {
        self.ensure_handle()?;
        let api = api().map_err(|_| E_FAIL)?;
        let guard = self.handle.lock().unwrap();
        let handle = guard.as_ref().ok_or(E_FAIL)?;
        let mut out = Output {
            site,
            written: 0,
            volume: unsafe { site.GetVolume() }.unwrap_or(100).min(100),
            aborted: false,
            failed: false,
            buf: Vec::with_capacity(8192),
        };
        while !frag.is_null() && !out.aborted && !out.failed {
            let f = unsafe { &*frag };
            // Rate: engine-wide SAPI rate plus the fragment's own adjustment.
            let sapi_rate = unsafe { site.GetRate() }.unwrap_or(0) + f.State.RateAdj;
            let rate = st_rate(sapi_rate, handle.neural);
            if self.rate.swap(rate, Ordering::SeqCst) != rate {
                unsafe { (api.set_rate)(handle.ptr, rate) };
            }
            let text = if f.pTextStart.is_null() || f.ulTextLen == 0 {
                String::new()
            } else {
                String::from_utf16_lossy(unsafe {
                    std::slice::from_raw_parts(f.pTextStart.0, f.ulTextLen as usize)
                })
            };
            match f.State.eAction {
                SPVA_Bookmark => {
                    // NVDA's SAPI5 driver indexes speech with numeric bookmarks.
                    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                    let number = text.trim().parse::<isize>().unwrap_or(0);
                    out.event(
                        SPEI_TTS_BOOKMARK,
                        SPET_LPARAM_IS_STRING,
                        number as usize,
                        wide.as_ptr() as isize,
                    );
                }
                SPVA_Silence => out.silence(f.State.SilenceMSecs),
                SPVA_Speak | SPVA_Pronounce | SPVA_SpellOut if !text.trim().is_empty() => {
                    let spoken = if f.State.eAction == SPVA_SpellOut {
                        text.chars()
                            .filter(|c| !c.is_whitespace())
                            .map(|c| c.to_string())
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        text
                    };
                    out.event(
                        SPEI_WORD_BOUNDARY,
                        SPET_LPARAM_IS_UNDEFINED,
                        f.ulTextLen as usize,
                        f.ulTextSrcOffset as isize,
                    );
                    let code = unsafe {
                        (api.stream)(
                            handle.ptr,
                            spoken.as_ptr(),
                            spoken.len(),
                            on_audio,
                            &mut out as *mut _ as *mut c_void,
                        )
                    };
                    if code != 0 && code != 4 {
                        debug(&format!("ST SAPI: stream = {code}: {}", st_error(api)));
                        out.failed = true;
                    }
                }
                _ => {}
            }
            frag = f.pNext;
        }
        if out.failed && !out.aborted {
            return Err(E_FAIL.into());
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ COM plumbing
#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<'_, windows::core::IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if object.is_null() || iid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let engine: ISpTTSEngine = Engine::new().into();
        unsafe { engine.query(iid, object).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> windows::core::Result<()> {
        if lock.as_bool() {
            LOCKS.fetch_add(1, Ordering::SeqCst);
        } else {
            LOCKS.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// COM entry point: returns the class factory for the ST SAPI engine.
///
/// # Safety
/// Called by COM. `clsid` and `iid` must be NULL (rejected) or point to valid GUIDs,
/// and `out` must be NULL (rejected) or valid for one pointer write.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    *out = std::ptr::null_mut();
    if *clsid != CLSID_ST_ENGINE {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    let hr = factory.query(iid, out);
    if hr.is_err() {
        return E_NOINTERFACE;
    }
    hr
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LOCKS.load(Ordering::SeqCst) == 0 && OBJECTS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_mapping() {
        assert_eq!(st_rate(0, true), 100);
        assert_eq!(st_rate(10, true), 200);
        assert_eq!(st_rate(10, false), 300);
        assert_eq!(st_rate(-10, true), 50);
        assert_eq!(st_rate(5, false), 173);
    }
}
