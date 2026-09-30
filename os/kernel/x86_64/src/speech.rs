//! Native speech for the operating system: any text, not only recorded clips.
//!
//! omni-os's own voice (`voice-st`, through `aw-voice`) renders the text and masters it
//! (DC block, 10.8 kHz band limit, normalization, fades) to 48 kHz 16-bit PCM in the kernel
//! heap; the kernel's own HDA driver plays it. Each utterance is also
//! written as an `AW_OS_SPEAK` marker, the machine-checkable trace.

use crate::debug_write;

/// Speak `text` aloud (French voice) and trace it.
pub fn say(text: &str) {
    trace(text);
    let pcm = aw_voice::speak(text, true, VOICE_RATE);
    crate::hda::speak_pcm(&pcm, VOICE_RATE);
}

/// The voice is mastered directly at the HDA output rate (no second resampling).
const VOICE_RATE: u32 = 48_000;

/// Trace an utterance without rendering audio (used by the deterministic proofs, where the
/// software-float synthesis of every landing would only slow the emulator down).
pub fn trace(text: &str) {
    debug_write("AW_OS_SPEAK \"");
    debug_write(text);
    debug_write("\"\n");
}

/// Render one utterance for real and play it: the proof that the OS speaks arbitrary text.
pub fn prove(text: &str) -> bool {
    let pcm = aw_voice::speak(text, true, VOICE_RATE);
    debug_write("AW_OS_TTS_RENDERED bytes=");
    crate::debug_write_u64(pcm.len() as u64);
    debug_write("\n");
    if pcm.len() < 9600 {
        debug_write("AW_OS_TTS_FAIL reason=too_short\n");
        return false;
    }
    // Real speech, not silence: some sample must be far from zero.
    let loud = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .any(|s| i16::from_le_bytes(*s).unsigned_abs() > 2000);
    if !loud {
        debug_write("AW_OS_TTS_FAIL reason=silent\n");
        return false;
    }
    crate::hda::speak_pcm(&pcm, VOICE_RATE);
    debug_write("AW_OS_TTS_PROOF_OK voice=st rate=");
    crate::debug_write_u64(u64::from(VOICE_RATE));
    debug_write("\n");
    true
}
