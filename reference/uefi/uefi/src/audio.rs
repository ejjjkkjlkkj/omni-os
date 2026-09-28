//! Pluggable firmware-stage audio: one speaker interface over every self-built backend.
//!
//! UEFI defines no audio output protocol, so the project builds its own drivers rather
//! than depending on a vendor: [`crate::hda`] (Intel HDA) and [`crate::ac97`] (AC'97). This
//! picks whichever a machine actually has, so the same pre-recorded clips are spoken on any
//! codec; when neither is present the caller falls back to the PC speaker
//! ([`crate::sound`]), the universal last resort. New backends (USB Audio Class,
//! VirtIO-sound) slot in here without touching the screen reader or the setup.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::ac97;
use crate::hda;
use crate::virtio_snd;

/// Software playback gain, shared by every backend so volume control works on any codec -
/// the codec amplifier graphs differ from machine to machine, but scaling the PCM samples
/// is universal. `GAIN` is a fraction over [`GAIN_FULL`] (256 = the recorded level); `MUTED`
/// overrides it to silence. Adjusted from the setup with the volume-up, volume-down and mute
/// keys, and applied per sample in each backend so the whole spoken interface follows it.
pub const GAIN_FULL: u32 = 256;
const GAIN_STEP: u32 = 64;
static GAIN: AtomicU32 = AtomicU32::new(GAIN_FULL);
static MUTED: AtomicBool = AtomicBool::new(false);

/// Attenuate one 16-bit PCM sample by the current volume (and mute). Backends call this on
/// every sample before handing it to the codec.
pub fn scale(sample: i16) -> i16 {
    let g = if MUTED.load(Ordering::Relaxed) {
        0
    } else {
        GAIN.load(Ordering::Relaxed)
    };
    ((i32::from(sample) * g as i32) / GAIN_FULL as i32) as i16
}

/// Raise the volume one step (capped at full), unmuting. Returns the new level over
/// [`GAIN_FULL`].
pub fn volume_up() -> u32 {
    MUTED.store(false, Ordering::Relaxed);
    let g = (GAIN.load(Ordering::Relaxed) + GAIN_STEP).min(GAIN_FULL);
    GAIN.store(g, Ordering::Relaxed);
    g
}

/// Lower the volume one step, down to silence. Returns the new level over [`GAIN_FULL`].
pub fn volume_down() -> u32 {
    let g = GAIN.load(Ordering::Relaxed).saturating_sub(GAIN_STEP);
    GAIN.store(g, Ordering::Relaxed);
    g
}

/// Toggle mute. Returns true when the interface is now muted.
pub fn toggle_mute() -> bool {
    let muted = !MUTED.load(Ordering::Relaxed);
    MUTED.store(muted, Ordering::Relaxed);
    muted
}

/// A brought-up audio output, whichever backend it is.
pub enum Speaker {
    Hda(hda::Speaker),
    Ac97(ac97::Speaker),
    Virtio(virtio_snd::Speaker),
}

impl Speaker {
    /// Play a clip, stopping early if `interrupted` returns true (barge-in).
    pub fn speak_until(&mut self, clip: &[u8], interrupted: impl FnMut() -> bool) -> bool {
        match self {
            Speaker::Hda(speaker) => speaker.speak_until(clip, interrupted),
            Speaker::Ac97(speaker) => speaker.speak_until(clip, interrupted),
            Speaker::Virtio(speaker) => speaker.speak_until(clip, interrupted),
        }
    }

    /// Play a clip to completion, without barge-in.
    pub fn speak(&mut self, clip: &[u8]) -> bool {
        self.speak_until(clip, || false)
    }

    /// The backend's name, for the `AW_UEFI_AUDIO_BACKEND` marker and field logs.
    pub fn backend(&self) -> &'static str {
        match self {
            Speaker::Hda(_) => "hda",
            Speaker::Ac97(_) => "ac97",
            Speaker::Virtio(_) => "virtio",
        }
    }
}

/// Try each self-built audio backend in turn - HDA, then AC'97, then virtio-sound - returning
/// the first that comes up. HDA and AC'97 are emulated hardware codecs and come first so a
/// machine (or VM) that has one uses it; virtio-sound is the paravirtual device some VMs expose
/// instead. `None` means none was found, so the caller uses the PC speaker.
pub fn bring_up() -> Option<Speaker> {
    if let Some(speaker) = hda::bring_up() {
        return Some(Speaker::Hda(speaker));
    }
    if let Some(speaker) = ac97::bring_up() {
        return Some(Speaker::Ac97(speaker));
    }
    if let Some(speaker) = virtio_snd::bring_up() {
        return Some(Speaker::Virtio(speaker));
    }
    None
}
