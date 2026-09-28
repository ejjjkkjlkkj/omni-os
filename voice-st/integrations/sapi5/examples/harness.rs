//! Drives st_sapi.dll exactly as SAPI does, without any registry change:
//! a real SpObjectToken over an in-memory ISpDataKey (STConfig), and a
//! recording ISpTTSEngineSite. Checks audio, bookmarks, silences and abort.
//!
//!   cargo run --release --example harness -- <dir with st_sapi.dll + st_synth.dll> [config-json]
#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::time::Instant;

use windows::core::{implement, ComObject, Interface, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::E_NOTIMPL;

const SPERR_NOT_FOUND: HRESULT = HRESULT(0x8004503Au32 as i32);
use windows::Win32::Media::Audio::WAVEFORMATEX;
use windows::Win32::Media::Speech::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[implement(ISpDataKey)]
struct MemKey(HashMap<String, String>);

fn wstr(p: &PCWSTR) -> String {
    unsafe { p.to_string().unwrap_or_default() }
}

impl ISpDataKey_Impl for MemKey_Impl {
    fn SetData(&self, _: &PCWSTR, _: u32, _: *const u8) -> windows::core::Result<()> { Err(E_NOTIMPL.into()) }
    fn GetData(&self, _: &PCWSTR, _: *mut u32, _: *mut u8) -> windows::core::Result<()> { Err(SPERR_NOT_FOUND.into()) }
    fn SetStringValue(&self, _: &PCWSTR, _: &PCWSTR) -> windows::core::Result<()> { Err(E_NOTIMPL.into()) }
    fn GetStringValue(&self, name: &PCWSTR) -> windows::core::Result<PWSTR> {
        let v = self.0.get(&wstr(name)).ok_or(SPERR_NOT_FOUND)?;
        let w: Vec<u16> = v.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let p = CoTaskMemAlloc(w.len() * 2) as *mut u16;
            std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            Ok(PWSTR(p))
        }
    }
    fn SetDWORD(&self, _: &PCWSTR, _: u32) -> windows::core::Result<()> { Err(E_NOTIMPL.into()) }
    fn GetDWORD(&self, _: &PCWSTR, _: *mut u32) -> windows::core::Result<()> { Err(SPERR_NOT_FOUND.into()) }
    fn OpenKey(&self, _: &PCWSTR) -> windows::core::Result<ISpDataKey> { Err(SPERR_NOT_FOUND.into()) }
    fn CreateKey(&self, _: &PCWSTR) -> windows::core::Result<ISpDataKey> { Err(E_NOTIMPL.into()) }
    fn DeleteKey(&self, _: &PCWSTR) -> windows::core::Result<()> { Err(E_NOTIMPL.into()) }
    fn DeleteValue(&self, _: &PCWSTR) -> windows::core::Result<()> { Err(E_NOTIMPL.into()) }
    fn EnumKeys(&self, _: u32) -> windows::core::Result<PWSTR> { Err(SPERR_NOT_FOUND.into()) }
    fn EnumValues(&self, _: u32) -> windows::core::Result<PWSTR> { Err(SPERR_NOT_FOUND.into()) }
}

#[implement(ISpTTSEngineSite)]
struct Site {
    audio: RefCell<Vec<u8>>,
    events: RefCell<Vec<(i32, u64, usize, String)>>,
    abort_after: Option<usize>,
    aborted_at: Cell<Option<Instant>>,
}

impl ISpEventSink_Impl for Site_Impl {
    fn AddEvents(&self, events: *const SPEVENT, n: u32) -> windows::core::Result<()> {
        for e in unsafe { std::slice::from_raw_parts(events, n as usize) } {
            let id = e._bitfield & 0xffff;
            let text = if (e._bitfield >> 16) == SPET_LPARAM_IS_STRING.0 {
                unsafe { PCWSTR(e.lParam.0 as *const u16).to_string().unwrap_or_default() }
            } else {
                String::new()
            };
            self.events.borrow_mut().push((id, e.ullAudioStreamOffset, e.wParam.0, text));
        }
        Ok(())
    }
    fn GetEventInterest(&self, interest: *mut u64) -> windows::core::Result<()> {
        unsafe { *interest = u64::MAX };
        Ok(())
    }
}

impl ISpTTSEngineSite_Impl for Site_Impl {
    fn GetActions(&self) -> u32 {
        match self.abort_after {
            Some(n) if self.audio.borrow().len() >= n => {
                if self.aborted_at.get().is_none() {
                    self.aborted_at.set(Some(Instant::now()));
                }
                SPVES_ABORT.0 as u32
            }
            _ => 0,
        }
    }
    fn Write(&self, buf: *const c_void, cb: u32) -> windows::core::Result<u32> {
        let b = unsafe { std::slice::from_raw_parts(buf as *const u8, cb as usize) };
        self.audio.borrow_mut().extend_from_slice(b);
        Ok(cb)
    }
    fn GetRate(&self) -> windows::core::Result<i32> { Ok(0) }
    fn GetVolume(&self) -> windows::core::Result<u16> { Ok(100) }
    fn GetSkipInfo(&self, _: *mut SPVSKIPTYPE, _: *mut i32) -> windows::core::Result<()> { Err(E_NOTIMPL.into()) }
    fn CompleteSkip(&self, _: i32) -> windows::core::Result<()> { Err(E_NOTIMPL.into()) }
}

fn frag(action: SPVACTIONS, text: &[u16], silence: u32, offset: u32) -> SPVTEXTFRAG {
    let mut f = SPVTEXTFRAG::default();
    f.State.eAction = action;
    f.State.SilenceMSecs = silence;
    f.pTextStart = PCWSTR(text.as_ptr());
    f.ulTextLen = text.len() as u32;
    f.ulTextSrcOffset = offset;
    f
}

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).expect("dir with st_sapi.dll");
    let config = args.get(2).cloned().unwrap_or(r#"{"backend":"neural","lang":"fr","voice":"ff_siwis"}"#.into());
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };

    let dll: Vec<u16> = format!("{dir}\\st_sapi.dll").encode_utf16().chain(Some(0)).collect();
    let lib = unsafe { LoadLibraryW(PCWSTR(dll.as_ptr()))? };
    type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    let get: GetClassObject = unsafe { std::mem::transmute(GetProcAddress(lib, windows::core::s!("DllGetClassObject")).unwrap()) };
    let clsid = GUID::from_u128(0x76557f36_f953_4ad0_8c8a_08b6257ddce0);
    let mut p = std::ptr::null_mut();
    unsafe { get(&clsid, &IClassFactory::IID, &mut p).ok()? };
    let factory = unsafe { IClassFactory::from_raw(p) };
    let engine: ISpTTSEngine = unsafe { factory.CreateInstance(None)? };

    let key: ISpDataKey = MemKey(HashMap::from([
        ("STConfig".to_string(), config.clone()),
        ("STFallback".to_string(), r#"{"backend":"compact","lang":"fr","voice":"female"}"#.to_string()),
    ])).into();
    let token: ISpObjectToken = unsafe { CoCreateInstance(&SpObjectToken, None, CLSCTX_ALL)? };
    let init: ISpObjectTokenInit = token.cast()?;
    unsafe { init.InitFromDataKey(SPCAT_VOICES, windows::core::w!("HKEY_CURRENT_USER\\Software\\ST\\HarnessOnly"), &key)? };
    let t0 = Instant::now();
    unsafe { engine.cast::<ISpObjectWithToken>()?.SetObjectToken(&token)? };

    let mut id = GUID::zeroed();
    let mut wfx: *mut WAVEFORMATEX = std::ptr::null_mut();
    unsafe { engine.GetOutputFormat(std::ptr::null(), std::ptr::null(), &mut id, &mut wfx)? };
    let f = unsafe { *wfx };
    println!("format: {} Hz, {} bit, {} ch", { f.nSamplesPerSec }, { f.wBitsPerSample }, { f.nChannels });
    unsafe { CoTaskMemFree(Some(wfx as *const c_void)) };

    // Speak: text, bookmark "1", 300 ms silence, text, bookmark "2".
    let a: Vec<u16> = "Paramètres avancés.".encode_utf16().collect();
    let m1: Vec<u16> = "1".encode_utf16().collect();
    let b: Vec<u16> = "Mode SVM, liste de choix.".encode_utf16().collect();
    let m2: Vec<u16> = "2".encode_utf16().collect();
    let mut frags = [
        frag(SPVA_Speak, &a, 0, 0),
        frag(SPVA_Bookmark, &m1, 0, 20),
        frag(SPVA_Silence, &[], 300, 21),
        frag(SPVA_Speak, &b, 0, 22),
        frag(SPVA_Bookmark, &m2, 0, 48),
    ];
    for i in 0..frags.len() - 1 {
        frags[i].pNext = &mut frags[i + 1] as *mut _;
    }
    let s = ComObject::new(Site { audio: RefCell::default(), events: RefCell::default(), abort_after: None, aborted_at: Cell::new(None) });
    let site: ISpTTSEngineSite = s.to_interface();
    unsafe { engine.Speak(0, &id, std::ptr::null(), &frags[0], &site)? };
    let first = t0.elapsed();
    let audio = s.audio.borrow();
    let seconds = audio.len() as f64 / 96000.0;
    let peak = audio.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs()).max().unwrap_or(0);
    println!("speak: {:.2} s audio, peak {peak}, first call (incl. model load) {:.2} s", seconds, first.as_secs_f64());
    let marks: Vec<_> = s.events.borrow().iter().filter(|e| e.0 == SPEI_TTS_BOOKMARK.0).map(|e| (e.3.clone(), e.2, e.1)).collect();
    println!("bookmarks: {marks:?}");
    let words = s.events.borrow().iter().filter(|e| e.0 == SPEI_WORD_BOUNDARY.0).count();
    println!("word boundary events: {words}");
    // The 300 ms silence sits between bookmark 1 and the second phrase.
    let off1 = marks.first().map(|m| m.2 as usize).unwrap_or(0);
    let silent = audio[off1..(off1 + 28_800).min(audio.len())].chunks_exact(2).all(|c| i16::from_le_bytes([c[0], c[1]]) == 0);
    println!("silence after bookmark 1 exact: {silent}");
    let ok_marks = marks.len() == 2 && marks[0].0 == "1" && marks[1].0 == "2" && marks[0].2 < marks[1].2
        && marks[1].2 as usize == audio.len();
    drop(audio);

    // Abort: stop after 0.5 s of audio; the engine must return promptly.
    let long: Vec<u16> = "Ceci est une longue phrase qui doit être interrompue par le lecteur d'écran avant sa fin, \
        parce que l'utilisateur a appuyé sur une touche pour passer à l'élément suivant du menu."
        .encode_utf16().collect();
    let lf = frag(SPVA_Speak, &long, 0, 0);
    let s2 = ComObject::new(Site { audio: RefCell::default(), events: RefCell::default(), abort_after: Some(48_000), aborted_at: Cell::new(None) });
    let site2: ISpTTSEngineSite = s2.to_interface();
    let t = Instant::now();
    unsafe { engine.Speak(0, &id, std::ptr::null(), &lf, &site2)? };
    let after = s2.aborted_at.get().map(|a| a.elapsed().as_millis()).unwrap_or(u128::MAX);
    println!("abort: returned {} ms after SPVES_ABORT (total {} ms, {:.2} s audio)",
        after, t.elapsed().as_millis(), s2.audio.borrow().len() as f64 / 96000.0);

    // Keep what a real client would hear, as a WAV next to the DLLs.
    {
        let a = s.audio.borrow();
        let mut w = Vec::with_capacity(44 + a.len());
        w.extend_from_slice(b"RIFF"); w.extend_from_slice(&(36 + a.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt "); w.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 1] { w.extend_from_slice(&v.to_le_bytes()); }
        w.extend_from_slice(&48000u32.to_le_bytes()); w.extend_from_slice(&96000u32.to_le_bytes());
        for v in [2u16, 16] { w.extend_from_slice(&v.to_le_bytes()); }
        w.extend_from_slice(b"data"); w.extend_from_slice(&(a.len() as u32).to_le_bytes()); w.extend_from_slice(&a);
        let _ = std::fs::write(format!("{dir}\\sapi-harness.wav"), w);
    }
    // Release the engine (and the neural worker) before exiting, as SAPI does.
    drop(engine);
    drop(factory);
    let pass = seconds > 1.0 && peak > 3000 && ok_marks && silent && after < 500;
    println!("ST_SAPI_HARNESS={}", if pass { "PASS" } else { "FAIL" });
    std::process::exit(if pass { 0 } else { 1 });
}
