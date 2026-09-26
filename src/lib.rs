extern crate alloc;

pub mod audio;
pub mod engine;
pub mod frontend;
mod ffi_v1;

#[path = "synth_inc.rs"]
pub mod synth;

// Re-export the synthesiser's public API so that FFI / CLI / Python users
// don't have to thread through the `synth::` module name.
pub use synth::{set_rate, set_pitch, set_voice,
                set_voice_quality, voice, voice_quality,
                rate, pitch, rate_up, rate_down, pitch_up, pitch_down,
                Voice, VoiceQuality};

use alloc::vec::Vec;

/// Converts raw 32kHz mono 16-bit PCM samples into a standard RIFF/WAVE byte buffer.
pub fn pcm_to_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let n = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + n).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // subchunk 1 size (16 for PCM)
    out.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat: 1 = PCM
    out.extend_from_slice(&1u16.to_le_bytes());  // NumChannels: 1 = Mono
    out.extend_from_slice(&sample_rate.to_le_bytes()); // SampleRate
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // ByteRate = SampleRate * NumChannels * BitsPerSample/8
    out.extend_from_slice(&2u16.to_le_bytes());  // BlockAlign = NumChannels * BitsPerSample/8
    out.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample = 16
    out.extend_from_slice(b"data");
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// High-level Rust helper: synthesizes text directly to WAV bytes.
pub fn synthesize_to_wav(text: &str, french: bool, rate: u32, pitch: u32) -> Vec<u8> {
    if rate > 0 {
        synth::set_rate(rate);
    }
    if pitch > 0 {
        synth::set_pitch(pitch);
    }
    let pcm = synth::say(text, french);
    pcm_to_wav(&pcm, synth::SAMPLE_RATE as u32)
}

// -----------------------------------------------------------------------------
// C-ABI Foreign Function Interface (FFI) for Python / C bindings
// -----------------------------------------------------------------------------

/// Synthesize text to WAV buffer via C-ABI.
///
/// # Safety
/// Input must point to text_len readable bytes; out_len must be writable.
/// Serialize calls and global configuration changes across threads.
///
/// # Arguments
/// - `text_ptr`: pointer to UTF-8 text string
/// - `text_len`: length of text in bytes
/// - `french`: true for French grapheme/phoneme rules, false for English
/// - `rate`: rate percent (100 = default, 50-300)
/// - `pitch`: fundamental pitch in Hz (115 = default, 50-350)
/// - `out_len`: pointer where generated WAV byte length will be written
///
/// Returns:
/// Pointer to heap-allocated buffer of WAV bytes, or null pointer on error.
/// Caller MUST free the buffer using `st_free_wav`.
#[no_mangle]
pub unsafe extern "C" fn st_synthesize_wav(
    text_ptr: *const u8,
    text_len: usize,
    french: bool,
    rate: u32,
    pitch: u32,
    out_len: *mut usize,
) -> *mut u8 {
    if text_ptr.is_null() || out_len.is_null() || text_len == 0 || text_len > isize::MAX as usize {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return core::ptr::null_mut();
    }

    let text_slice = core::slice::from_raw_parts(text_ptr, text_len);
    let text_str = match core::str::from_utf8(text_slice) {
        Ok(s) => s,
        Err(_) => {
            *out_len = 0;
            return core::ptr::null_mut();
        }
    };

    let wav = synthesize_to_wav(text_str, french, rate, pitch);
    let len = wav.len();
    *out_len = len;

    if len == 0 {
        return core::ptr::null_mut();
    }

    let mut boxed_slice = wav.into_boxed_slice();
    let ptr = boxed_slice.as_mut_ptr();
    core::mem::forget(boxed_slice);
    ptr
}

/// Frees a WAV buffer allocated by `st_synthesize_wav`.
///
/// # Safety
/// Use the exact pointer and length returned by synthesis, once only.
#[no_mangle]
pub unsafe extern "C" fn st_free_wav(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let _ = alloc::boxed::Box::from_raw(core::slice::from_raw_parts_mut(ptr, len));
    }
}
