//! C ABI v1 exercised exactly as a C caller would (raw pointers, callbacks, codes).
use st_synth::ffi_v1::*;
use std::ffi::c_void;

unsafe fn create(json: &str) -> (i32, *mut StEngine) {
    let mut h = std::ptr::null_mut();
    (st_engine_create_v1(json.as_ptr(), json.len(), &mut h), h)
}
fn last_error() -> String {
    let mut buf = [0u8; 256];
    let n = unsafe { st_last_error_v1(buf.as_mut_ptr(), buf.len()) };
    String::from_utf8_lossy(&buf[..n.min(255)]).into()
}
unsafe extern "C" fn collect(pcm: *const f32, n: usize, rate: u32, user: *mut c_void) -> u8 {
    assert_eq!(rate, 48000);
    (*(user as *mut Vec<f32>)).extend_from_slice(std::slice::from_raw_parts(pcm, n));
    1
}
unsafe extern "C" fn cancel(_: *const f32, _: usize, _: u32, user: *mut c_void) -> u8 {
    *(user as *mut usize) += 1;
    0
}

#[test]
fn config_errors_are_reported() {
    unsafe {
        for (json, needle) in [("{", "Invalid JSON"), ("{\"colour\":1}", "Unknown key"), ("{\"lang\":\"de\"}", "lang"), ("{\"voice\":\"robot\"}", "Compact voice")] {
            let (code, h) = create(json);
            assert!(h.is_null());
            assert!(code == 1 || code == 2, "{json}: {code}");
            assert!(last_error().contains(needle), "{json}: {}", last_error());
        }
        assert_eq!(st_engine_create_v1(std::ptr::null(), 0, std::ptr::null_mut()), 1);
    }
}

#[test]
fn stream_wav_cancel_and_destroy() {
    unsafe {
        let (code, h) = create(r#"{"backend":"compact","lang":"fr","voice":"female","rate":120}"#);
        assert_eq!(code, 0, "{}", last_error());
        assert_eq!(st_last_error_v1(std::ptr::null_mut(), 0), 0);
        let text = "Bonjour, bienvenue dans ST.";
        let mut pcm: Vec<f32> = Vec::new();
        assert_eq!(st_engine_stream_v1(h, text.as_ptr(), text.len(), Some(collect), &mut pcm as *mut _ as *mut c_void), 0);
        assert!(pcm.len() > 48000 / 2 && pcm.iter().all(|x| x.is_finite() && x.abs() < 1.0));

        let mut calls = 0usize;
        assert_eq!(st_engine_stream_v1(h, text.as_ptr(), text.len(), Some(cancel), &mut calls as *mut _ as *mut c_void), 4);
        assert_eq!(calls, 1);

        let (mut wav, mut len) = (std::ptr::null_mut(), 0usize);
        assert_eq!(st_engine_wav_v1(h, text.as_ptr(), text.len(), &mut wav, &mut len), 0);
        let bytes = std::slice::from_raw_parts(wav, len);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 48000);
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 24);
        st_synth::st_free_wav(wav, len);

        let bad = [0xffu8, 0xfe];
        assert_eq!(st_engine_stream_v1(h, bad.as_ptr(), bad.len(), Some(collect), &mut pcm as *mut _ as *mut c_void), 1);
        assert!(last_error().contains("UTF-8"));
        assert_eq!(st_engine_stream_v1(h, text.as_ptr(), text.len(), None, std::ptr::null_mut()), 1);
        st_engine_destroy_v1(h);
        st_engine_destroy_v1(std::ptr::null_mut());
    }
}

/// Runs only when a neural runtime is configured (ST_PYTHON + ST_NEURAL_HOME).
#[test]
fn neural_backend_when_available() {
    if std::env::var_os("ST_PYTHON").is_none() || std::env::var_os("ST_NEURAL_HOME").is_none() {
        eprintln!("skipped: ST_PYTHON/ST_NEURAL_HOME not set");
        return;
    }
    unsafe {
        let (code, h) = create(r#"{"backend":"neural","lang":"fr"}"#);
        assert_eq!(code, 0, "{}", last_error());
        for text in ["Bonjour, bienvenue dans ST. Voulez-vous continuer ?", "Menu Fichier."] {
            let mut pcm: Vec<f32> = Vec::new();
            assert_eq!(st_engine_stream_v1(h, text.as_ptr(), text.len(), Some(collect), &mut pcm as *mut _ as *mut c_void), 0, "{}", last_error());
            let seconds = pcm.len() as f64 / 48000.0;
            assert!(seconds > 0.4 && seconds < 10.0, "{seconds}");
            let rms = (pcm.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt();
            assert!(rms > 0.01, "rms {rms}");
        }
        let (code, h2) = create(r#"{"backend":"neural","lang":"fr","voice":"af_heart"}"#);
        assert_eq!(code, 2);
        assert!(h2.is_null() && last_error().contains("incompatible"));
        st_engine_destroy_v1(h);
    }
}
