//! omni-os's own voice, `voice-st` (anti-robotic Klatt cascade/parallel formant synthesizer:
//! LF glottal source, coarticulation, intonation, French and English), built `no_std` for the
//! kernel and the loader. The synthesizer source is the one `voice-st` ships and tests against
//! its golden corpus (`voice-st/src/synth_inc.rs`), included here rather than copied, so there
//! is exactly one voice.
//!
//! [`speak`] produces playable PCM through the same mastering chain as `voice-st`'s engine and
//! the firmware clips (`tools/voice/gen-firmware-speech.py`): a 20 Hz DC blocker, a band-limited
//! windowed-sinc resampler with a 10.8 kHz low-pass (the raw 32 kHz synthesis carries energy up
//! to Nyquist, heard as crackle when played unfiltered), peak normalization to 70 % and 5 ms
//! fades.

#![no_std]
#![allow(clippy::all, clippy::pedantic, dead_code)]

extern crate alloc;

#[path = "../../../../voice-st/src/synth_inc.rs"]
mod synth;

use alloc::vec::Vec;

pub use synth::{
    SAMPLE_RATE, pitch, pitch_down, pitch_up, rate, rate_down, rate_up, say, set_pitch, set_rate,
};

/// Low-pass cutoff of the mastering chain (Hz), as for the firmware clips.
const CUTOFF_HZ: f64 = 10_800.0;
/// Resampler half-width, in source samples.
const HALF: i64 = 32;

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Speak `text` as mastered mono 16-bit PCM at `out_rate` Hz (24 000 or 48 000 typically).
pub fn speak(text: &str, french: bool, out_rate: u32) -> Vec<u8> {
    let config = synth::Config {
        rate: synth::rate(),
        pitch: synth::pitch(),
        ..synth::Config::default()
    };
    let raw = synth::render_samples(text, french, config);
    if raw.is_empty() || out_rate == 0 {
        return Vec::new();
    }
    let source = SAMPLE_RATE as u64;
    let out_rate = u64::from(out_rate);

    // DC blocker (1st-order high-pass at 20 Hz).
    let r = 1.0 - 2.0 * core::f64::consts::PI * 20.0 / SAMPLE_RATE;
    let (mut px, mut py) = (raw[0], 0.0);
    let input: Vec<f64> = raw
        .iter()
        .map(|&x| {
            py = x - px + r * py;
            px = x;
            py
        })
        .collect();

    // Rational resampling: output sample n sits at source position n * source / out_rate,
    // whose fractional part takes only `phases` values; weights are computed once per phase.
    let g = gcd(source, out_rate);
    let (step_num, phases) = (source / g, out_rate / g);
    let cutoff = CUTOFF_HZ.min(0.45 * out_rate as f64).min(0.45 * SAMPLE_RATE) / SAMPLE_RATE;
    let taps = (2 * HALF) as usize;
    let mut table: Vec<f64> = Vec::with_capacity(phases as usize * taps);
    for phase in 0..phases {
        let frac = phase as f64 / phases as f64;
        let mut row = [0.0_f64; 64];
        let mut sum = 0.0;
        for (k, slot) in row.iter_mut().enumerate() {
            let d = frac - (k as i64 - HALF + 1) as f64;
            let x = 2.0 * cutoff * d;
            let sinc = if libm::fabs(x) < 1e-12 {
                1.0
            } else {
                libm::sin(core::f64::consts::PI * x) / (core::f64::consts::PI * x)
            };
            let window = 0.5 + 0.5 * libm::cos(core::f64::consts::PI * d / HALF as f64);
            *slot = sinc * window;
            sum += *slot;
        }
        table.extend(row.iter().map(|w| w / sum));
    }
    let frames = (input.len() as u64 * out_rate / source) as usize;
    let mut out: Vec<f64> = Vec::with_capacity(frames);
    for n in 0..frames as u64 {
        let position = n * step_num;
        let center = (position / phases) as i64;
        let phase = (position % phases) as usize;
        let row = &table[phase * taps..(phase + 1) * taps];
        let mut acc = 0.0;
        for (k, w) in row.iter().enumerate() {
            let j = center + k as i64 - HALF + 1;
            if j >= 0 && (j as usize) < input.len() {
                acc += input[j as usize] * w;
            }
        }
        out.push(acc);
    }

    // Normalize to 70 % of full scale, 5 ms fades, 16-bit.
    let peak = out.iter().fold(0.0_f64, |p, x| p.max(libm::fabs(*x)));
    if !(peak > 0.0) {
        return Vec::new();
    }
    let gain = 0.70 * 32_767.0 / peak;
    let fade = ((out_rate / 200) as usize).min(out.len() / 2).max(1);
    let end = out.len() - 1;
    let mut pcm = Vec::with_capacity(out.len() * 2);
    for (i, x) in out.iter().enumerate() {
        let w = libm::fmin(i.min(end - i) as f64 / fade as f64, 1.0);
        let v = libm::round(x * gain * w).clamp(-32_768.0, 32_767.0) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    pcm
}
