//! Native speech for the operating system: any text, not only recorded clips.
//!
//! The same formant synthesizer the loader speaks with (`aw-synth`) renders the text to 24 kHz
//! 16-bit PCM in the kernel heap, and the kernel's own HDA driver plays it. Each utterance is also
//! written as an `AW_OS_SPEAK` marker, the machine-checkable trace.

use crate::debug_write;

/// Speak `text` aloud (French voice) and trace it.
pub fn say(text: &str) {
    trace(text);
    let pcm = aw_synth::say(text, true);
    crate::hda::speak(&pcm);
}

/// Trace an utterance without rendering audio (used by the deterministic proofs, where the
/// software-float synthesis of every landing would only slow the emulator down).
pub fn trace(text: &str) {
    debug_write("AW_OS_SPEAK \"");
    debug_write(text);
    debug_write("\"\n");
}

/// Render one utterance for real and play it: the proof that the OS speaks arbitrary text.
pub fn prove(text: &str) -> bool {
    let pcm = aw_synth::say(text, true);
    debug_write("AW_OS_TTS_RENDERED bytes=");
    crate::debug_write_u64(pcm.len() as u64);
    debug_write("\n");
    if pcm.len() < 4800 {
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
    crate::hda::speak(&pcm);
    debug_write("AW_OS_TTS_PROOF_OK\n");
    true
}
