//! Dynamic speech for the loader: omni-os's own voice (`voice-st`, through `aw-voice`),
//! mastered directly at the 24 kHz mono PCM the loader's audio backends play.

use alloc::vec::Vec;

pub use aw_voice::{pitch, pitch_down, pitch_up, rate, rate_down, rate_up};

/// Speak `text` into mastered 24 kHz mono 16-bit PCM.
pub fn say(text: &str, french: bool) -> Vec<u8> {
    aw_voice::speak(text, french, 24_000)
}
