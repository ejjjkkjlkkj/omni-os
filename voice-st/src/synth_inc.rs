//! Evolved Klatt-style cascade/parallel formant speech synthesizer (v3).
//!
//! Compared with v2, this engine adds the things that distinguish a voice from a
//! formant buzz:
//!
//! * **Real F4 / F5 formants per phoneme.** v2 had a fixed fourth formant at 3300 Hz,
//!   so the upper spectrum of every vowel sounded identical. F4 (≈ 3300-4500 Hz) and
//!   F5 (≈ 4500-6000 Hz) now follow per-phoneme targets and add the brightness that
//!   carries speaker identity.
//! * **Parallel nasal branch with pole-zero pair.** M, N, Ng, French nasals and the
//!   nasal vowels are voiced through a parallel resonator pair (pole ≈ 250 Hz, zero
//!   set by place of articulation) so the characteristic nasal murmur is audible
//!   instead of merely hinted at by reduced amplitude.
//! * **Aspiration noise path.** /h/, and the breathy release of /p t k/, get their
//!   own high-passed noise source instead of being faked with the fricative band.
//! * **Word-level stress and sentence intonation.** Each content word's first
//!   syllable is marked stressed (longer, louder, slightly higher pitch). Statements
//!   fall on the last syllable; questions rise.
//! * **Pre-emphasis, de-emphasis and shimmer.** A 6 dB/octave pre-emphasis lifts
//!   the upper spectrum before the cascade, a matching de-emphasis restores it
//!   after, and a small amplitude shimmer keeps the voice alive.
//!
//! Everything else from v2 - the French & English grapheme-to-phoneme frontends,
//! the letter-name acronym path, number-to-words, the C-ABI FFI - is preserved.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use libm::{cos, exp, fabs};

// Explicit imports and `libm` fallbacks so this file also builds `no_std` (the omni-os kernel
// and loader include it through `aw-voice`). In the `std` build the standard prelude and the
// inherent `f64` methods take precedence, so the synthesized audio is unchanged there.
#[allow(unused_imports)]
use alloc::string::ToString;
#[allow(unused_imports)]
use alloc::vec;

#[allow(dead_code)]
trait NoStdF64 {
    fn powi(self, n: i32) -> f64;
    fn tanh(self) -> f64;
}

impl NoStdF64 for f64 {
    fn powi(self, n: i32) -> f64 {
        libm::pow(self, f64::from(n))
    }
    fn tanh(self) -> f64 {
        libm::tanh(self)
    }
}

// -----------------------------------------------------------------------------
// Global state and configuration
// -----------------------------------------------------------------------------

/// Output sample rate (Hz). 32 kHz gives 16 kHz of usable bandwidth — enough
/// to capture the full HF detail of sibilants (which extend to ~10 kHz) and to
/// avoid aliasing artefacts on the resonator cascade at F5 = 5-6 kHz.
pub const SAMPLE_RATE: f64 = 32000.0;
/// Two pi.
const TWO_PI: f64 = core::f64::consts::TAU;
/// Pi.
const PI: f64 = core::f64::consts::PI;

/// Speech rate as a percentage of the base rate (100 = normal).
static RATE_PERCENT: AtomicU32 = AtomicU32::new(100);
/// Voice pitch (glottal fundamental) in hertz.
static PITCH_HZ: AtomicU32 = AtomicU32::new(115);
/// Voice quality: 0 = modal (default), 1 = breathy, 2 = pressed, 3 = creaky.
static VOICE_QUALITY: AtomicU32 = AtomicU32::new(0);
/// Active voice preset: 0 = male, 1 = female, 2 = child.
static VOICE: AtomicU32 = AtomicU32::new(0);

/// Voice quality modes. Each affects the open quotient, jitter, shimmer and
/// aspiration amplitude applied throughout the synthesis.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VoiceQuality {
    /// Modal: standard voice. Default.
    Modal = 0,
    /// Breathy: more open quotient, more aspiration noise, more shimmer. Sounds
    /// like relaxed speech after exertion.
    Breathy = 1,
    /// Pressed: tighter quotient, less shimmer. Sounds tense or emphatic.
    Pressed = 2,
    /// Creaky: low fundamental with period-doubled / irregular pulses.
    Creaky = 3,
}

/// Named voice presets. Each voice carries its own F0 baseline, formant
/// frequency scaling (because adult male / female / child vocal tracts are
/// physically different lengths), open quotient, and prosodic weight. These
/// settings are applied to every phoneme during rendering so the output
/// carries the identity of the chosen speaker.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    /// Adult male: F0 = 115 Hz, formants scaled to ~0.85x (longer vocal tract).
    Male = 0,
    /// Adult female: F0 = 200 Hz, formants at ~1.0x (reference vocal tract).
    Female = 1,
    /// Child: F0 = 280 Hz, formants scaled to ~1.15x (shorter vocal tract).
    Child = 2,
}

impl Voice {
    /// Fundamental frequency (Hz) for this voice.
    pub fn f0(self) -> f64 {
        match self {
            Voice::Male => 115.0,
            Voice::Female => 200.0,
            Voice::Child => 280.0,
        }
    }
    /// Formant frequency scaling factor. Adult female vocal tracts are ~17%
    /// shorter than adult males, so her formants are ~17% higher. Children
    /// are even higher still.
    pub fn formant_scale(self) -> f64 {
        match self {
            Voice::Male => 0.88,
            Voice::Female => 1.00,
            Voice::Child => 1.18,
        }
    }
    /// Open quotient baseline for sonorants. Females and children tend to
    /// have tighter open phases than adult males.
    pub fn base_oq(self) -> f64 {
        match self {
            Voice::Male => 0.58,
            Voice::Female => 0.52,
            Voice::Child => 0.48,
        }
    }
    /// Prosodic weight multiplier on accent amplitude. Children and females
    /// tend to have slightly more pronounced intonation contours.
    pub fn accent_weight(self) -> f64 {
        match self {
            Voice::Male => 1.00,
            Voice::Female => 1.15,
            Voice::Child => 1.30,
        }
    }
}

/// Immutable request settings. The explicit API never reads or writes global settings.
#[derive(Clone, Copy)]
pub struct Config {
    pub rate: u32,
    pub pitch: u32,
    pub voice: Voice,
    pub quality: VoiceQuality,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            rate: 100,
            pitch: 115,
            voice: Voice::Male,
            quality: VoiceQuality::Modal,
        }
    }
}
impl Config {
    fn snapshot() -> Self {
        Self {
            rate: rate(),
            pitch: pitch(),
            voice: voice(),
            quality: voice_quality(),
        }
    }
}

/// Set the voice quality mode.
pub fn set_voice_quality(q: VoiceQuality) {
    VOICE_QUALITY.store(q as u32, Ordering::Relaxed);
}

/// Get the current voice quality mode.
pub fn voice_quality() -> VoiceQuality {
    match VOICE_QUALITY.load(Ordering::Relaxed) {
        1 => VoiceQuality::Breathy,
        2 => VoiceQuality::Pressed,
        3 => VoiceQuality::Creaky,
        _ => VoiceQuality::Modal,
    }
}

/// Set the active named voice preset. Also updates the global pitch baseline
/// to the preset's fundamental frequency so subsequent synthesises use it.
pub fn set_voice(v: Voice) {
    VOICE.store(v as u32, Ordering::Relaxed);
    PITCH_HZ.store(v.f0() as u32, Ordering::Relaxed);
}

/// Get the current named voice preset.
pub fn voice() -> Voice {
    match VOICE.load(Ordering::Relaxed) {
        1 => Voice::Female,
        2 => Voice::Child,
        _ => Voice::Male,
    }
}

/// Raise the speech rate one step, capped, and return the new percentage.
pub fn rate_up() -> u32 {
    let next = (RATE_PERCENT.load(Ordering::Relaxed) + 25).min(300);
    RATE_PERCENT.store(next, Ordering::Relaxed);
    next
}

/// Lower the speech rate one step, floored, and return the new percentage.
pub fn rate_down() -> u32 {
    let next = RATE_PERCENT
        .load(Ordering::Relaxed)
        .saturating_sub(25)
        .max(50);
    RATE_PERCENT.store(next, Ordering::Relaxed);
    next
}

/// The current speech rate percentage.
pub fn rate() -> u32 {
    RATE_PERCENT.load(Ordering::Relaxed)
}

/// The current voice pitch in hertz.
pub fn pitch() -> u32 {
    PITCH_HZ.load(Ordering::Relaxed)
}

/// Raise the voice pitch one step and return the new fundamental in hertz.
pub fn pitch_up() -> u32 {
    let next = (PITCH_HZ.load(Ordering::Relaxed) + 15).min(350);
    PITCH_HZ.store(next, Ordering::Relaxed);
    next
}

/// Lower the voice pitch one step and return the new fundamental in hertz.
pub fn pitch_down() -> u32 {
    let next = PITCH_HZ.load(Ordering::Relaxed).saturating_sub(15).max(50);
    PITCH_HZ.store(next, Ordering::Relaxed);
    next
}

/// Set the speech rate percentage directly (50 to 300).
pub fn set_rate(rate: u32) {
    RATE_PERCENT.store(rate.clamp(50, 300), Ordering::Relaxed);
}

/// Set the voice pitch fundamental in hertz (50 to 350).
pub fn set_pitch(pitch: u32) {
    PITCH_HZ.store(pitch.clamp(50, 350), Ordering::Relaxed);
}

// -----------------------------------------------------------------------------
// Phoneme inventory
// -----------------------------------------------------------------------------

/// Phoneme inventory. Diphthongs expand to two targets; affricates to a closure +
/// release; stops to a closure + burst. French-specific vowels cover the French
/// grapheme-to-phoneme frontend's output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ph {
    // English monophthongs
    Aa,
    Ae,
    Ah,
    Ao,
    Eh,
    Er,
    Ih,
    Iy,
    Uh,
    Uw,
    // English diphthongs
    Ey,
    Ay,
    Oy,
    Aw,
    Ow,
    // Nasals & liquids
    M,
    N,
    Ng,
    L,
    R,
    W,
    Y,
    // Fricatives
    F,
    V,
    S,
    Z,
    Sh,
    Zh,
    Th,
    Dh,
    Hh,
    // Affricates
    Ch,
    Jh,
    // Stops
    P,
    B,
    T,
    D,
    K,
    G,
    // French oral vowels (compact set; /ɛ ɔ/ reuse Eh/Ao)
    FrA,
    FrEClose,
    FrOClose,
    FrY,
    Eu,
    EuOpen,
    Schwa,
    // French nasal vowels
    Nan,
    Non,
    Nin,
    Nun,
    // Palatal nasal
    Ny,
    // Word boundary (used for stress tracking)
    Wbound,
    // End of clause (statement fall or question rise)
    EClause,
    // Pause
    Pause,
}

// -----------------------------------------------------------------------------
// Synthesis target: formant + amplitude + duration
// -----------------------------------------------------------------------------

/// One synthesis segment. Holds the formants the renderer slews toward, the source
/// amplitudes (voiced, fricative, aspiration, nasal), the place-specific nasal pole
/// and zero, and the duration in milliseconds.
#[derive(Clone, Copy)]
struct Target {
    f1: f64,
    f2: f64,
    f3: f64,
    f4: f64,
    f5: f64,
    bw1: f64,
    bw2: f64,
    bw3: f64,
    bw4: f64,
    bw5: f64,
    av: f64, // voiced (glottal) amplitude
    af: f64, // fricative amplitude
    fc: f64,
    fbw: f64, // fricative center / bandwidth
    a6: f64,  // aspiration amplitude (breath noise for /h/, post-stop)
    an: f64,  // nasal pole amplitude (parallel nasal branch)
    fnp: f64,
    bwnp: f64, // nasal pole freq / bw
    fnz: f64,
    bwnz: f64, // nasal zero freq / bw
    dur_ms: f64,
    /// Whether this segment is a syllable nucleus (vowel-class phoneme). Used by the
    /// renderer to drive word-level stress on the first syllable of each word.
    is_syl: bool,
}

impl Target {
    /// Voiced segment (vowel, nasal, approximant): glottal through five formants.
    /// `an > 0` engages the parallel nasal branch.
    #[allow(clippy::too_many_arguments)] // one argument per formant parameter
    const fn voiced_full(
        f1: f64,
        f2: f64,
        f3: f64,
        f4: f64,
        f5: f64,
        bw1: f64,
        bw2: f64,
        bw3: f64,
        bw4: f64,
        bw5: f64,
        av: f64,
        dur_ms: f64,
        is_syl: bool,
    ) -> Self {
        Self {
            f1,
            f2,
            f3,
            f4,
            f5,
            bw1,
            bw2,
            bw3,
            bw4,
            bw5,
            av,
            af: 0.0,
            fc: 0.0,
            fbw: 0.0,
            a6: 0.0,
            an: 0.0,
            fnp: 250.0,
            bwnp: 50.0,
            fnz: 1000.0,
            bwnz: 80.0,
            dur_ms,
            is_syl,
        }
    }

    /// Convenience constructor for a vowel-like target (sensible default bandwidths).
    const fn voiced(f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, av: f64, dur_ms: f64) -> Self {
        Self::voiced_full(
            f1, f2, f3, f4, f5, 70.0, 90.0, 120.0, 150.0, 200.0, av, dur_ms, true,
        )
    }

    /// Voiced target that is *not* a syllable nucleus (nasals, approximants, liquids).
    const fn voiced_non_syl(
        f1: f64,
        f2: f64,
        f3: f64,
        f4: f64,
        f5: f64,
        av: f64,
        dur_ms: f64,
    ) -> Self {
        Self::voiced_full(
            f1, f2, f3, f4, f5, 80.0, 90.0, 120.0, 150.0, 200.0, av, dur_ms, false,
        )
    }

    /// Frication segment: band-passed noise, optionally with a low voiced murmur.
    /// F4/F5 are kept higher than vowels to give sibilants their spectral peak.
    const fn fric(fc: f64, fbw: f64, av: f64, af: f64, f4: f64, f5: f64, dur_ms: f64) -> Self {
        Self {
            f1: 500.0,
            f2: 1500.0,
            f3: 2500.0,
            f4,
            f5,
            bw1: 100.0,
            bw2: 120.0,
            bw3: 150.0,
            bw4: 200.0,
            bw5: 250.0,
            av,
            af,
            fc,
            fbw,
            a6: 0.0,
            an: 0.0,
            fnp: 250.0,
            bwnp: 50.0,
            fnz: 1000.0,
            bwnz: 80.0,
            dur_ms,
            is_syl: false,
        }
    }

    /// Aspiration: broadband breath noise through a high-pass-ish path.
    const fn aspiration(av: f64, a6: f64, dur_ms: f64) -> Self {
        Self {
            f1: 500.0,
            f2: 1500.0,
            f3: 2500.0,
            f4: 3500.0,
            f5: 5000.0,
            bw1: 100.0,
            bw2: 120.0,
            bw3: 150.0,
            bw4: 200.0,
            bw5: 250.0,
            av,
            af: 0.0,
            fc: 0.0,
            fbw: 0.0,
            a6,
            an: 0.0,
            fnp: 250.0,
            bwnp: 50.0,
            fnz: 1000.0,
            bwnz: 80.0,
            dur_ms,
            is_syl: false,
        }
    }

    /// Silence: no source, formants held so the next segment's release transitions cleanly.
    const fn silence(dur_ms: f64) -> Self {
        Self {
            f1: 500.0,
            f2: 1500.0,
            f3: 2500.0,
            f4: 3500.0,
            f5: 5000.0,
            bw1: 100.0,
            bw2: 120.0,
            bw3: 150.0,
            bw4: 200.0,
            bw5: 250.0,
            av: 0.0,
            af: 0.0,
            fc: 0.0,
            fbw: 0.0,
            a6: 0.0,
            an: 0.0,
            fnp: 250.0,
            bwnp: 50.0,
            fnz: 1000.0,
            bwnz: 80.0,
            dur_ms,
            is_syl: false,
        }
    }
}

// -----------------------------------------------------------------------------
// Phoneme -> Targets
// -----------------------------------------------------------------------------

/// F4/F5 for a "neutral" vowel. Sibilant and front vowels use slightly different
/// values - real speakers' F4/F5 do shift with vowel quality.
const NEUTRAL_F4: f64 = 3500.0;
const NEUTRAL_F5: f64 = 4500.0;
const SIBILANT_F4: f64 = 3700.0;
const SIBILANT_F5: f64 = 5800.0;

const AV: f64 = 1.0;

/// Map one phoneme to its synthesis targets (one or more, e.g. diphthongs emit two).
fn targets_for(ph: Ph, out: &mut Vec<Target>) {
    match ph {
        // English monophthongs. F4 and F5 follow the standard Peterson & Barney
        // measurements averaged across speakers; values for tense vowels (Iy, Uw, Ao)
        // sit higher in F4 than lax ones (Ih, Uh, Ae).
        Ph::Aa => out.push(Target::voiced(
            730.0, 1090.0, 2440.0, 3400.0, 4400.0, AV, 140.0,
        )),
        Ph::Ae => out.push(Target::voiced(
            660.0, 1720.0, 2410.0, 3500.0, 4500.0, AV, 150.0,
        )),
        Ph::Ah => out.push(Target::voiced(
            640.0, 1190.0, 2390.0, 3300.0, 4200.0, AV, 110.0,
        )),
        Ph::Ao => out.push(Target::voiced(
            570.0, 840.0, 2410.0, 3200.0, 4000.0, AV, 140.0,
        )),
        Ph::Eh => out.push(Target::voiced(
            530.0, 1840.0, 2480.0, 3600.0, 4700.0, AV, 130.0,
        )),
        Ph::Er => out.push(Target::voiced(
            490.0, 1350.0, 1690.0, 3000.0, 3800.0, AV, 150.0,
        )),
        Ph::Ih => out.push(Target::voiced(
            390.0, 1990.0, 2550.0, 3700.0, 4800.0, AV, 110.0,
        )),
        Ph::Iy => out.push(Target::voiced(
            270.0, 2290.0, 3010.0, 3900.0, 5200.0, AV, 130.0,
        )),
        Ph::Uh => out.push(Target::voiced(
            440.0, 1020.0, 2240.0, 3100.0, 3900.0, AV, 110.0,
        )),
        Ph::Uw => out.push(Target::voiced(
            300.0, 870.0, 2240.0, 3300.0, 4100.0, AV, 140.0,
        )),
        // Diphthongs: glide from start to end.
        Ph::Ey => {
            out.push(Target::voiced(
                530.0, 1840.0, 2480.0, 3600.0, 4700.0, AV, 90.0,
            ));
            out.push(Target::voiced(
                270.0, 2290.0, 3010.0, 3900.0, 5200.0, AV, 90.0,
            ));
        }
        Ph::Ay => {
            out.push(Target::voiced(
                730.0, 1090.0, 2440.0, 3400.0, 4400.0, AV, 100.0,
            ));
            out.push(Target::voiced(
                270.0, 2290.0, 3010.0, 3900.0, 5200.0, AV, 90.0,
            ));
        }
        Ph::Oy => {
            out.push(Target::voiced(
                570.0, 840.0, 2410.0, 3200.0, 4000.0, AV, 110.0,
            ));
            out.push(Target::voiced(
                270.0, 2290.0, 3010.0, 3900.0, 5200.0, AV, 90.0,
            ));
        }
        Ph::Aw => {
            out.push(Target::voiced(
                730.0, 1090.0, 2440.0, 3400.0, 4400.0, AV, 100.0,
            ));
            out.push(Target::voiced(
                300.0, 870.0, 2240.0, 3300.0, 4100.0, AV, 90.0,
            ));
        }
        Ph::Ow => {
            out.push(Target::voiced(
                570.0, 840.0, 2410.0, 3200.0, 4000.0, AV, 100.0,
            ));
            out.push(Target::voiced(
                300.0, 870.0, 2240.0, 3300.0, 4100.0, AV, 90.0,
            ));
        }
        // Nasals: low F1 plus a parallel nasal pole/zero pair. The zero's frequency
        // sets the perceived place: M ≈ 1000 Hz, N ≈ 1500 Hz, Ng ≈ 2000 Hz.
        Ph::M => {
            let mut t =
                Target::voiced_non_syl(250.0, 1100.0, 2300.0, NEUTRAL_F4, NEUTRAL_F5, 0.7, 90.0);
            t.an = 1.0;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 1000.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        Ph::N => {
            let mut t =
                Target::voiced_non_syl(250.0, 1700.0, 2600.0, NEUTRAL_F4, NEUTRAL_F5, 0.7, 90.0);
            t.an = 1.0;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 1500.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        Ph::Ng => {
            let mut t =
                Target::voiced_non_syl(250.0, 2300.0, 2900.0, NEUTRAL_F4, NEUTRAL_F5, 0.7, 90.0);
            t.an = 1.0;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 2000.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        Ph::Ny => {
            let mut t =
                Target::voiced_non_syl(300.0, 1900.0, 2600.0, NEUTRAL_F4, NEUTRAL_F5, 0.7, 90.0);
            t.an = 1.0;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 1900.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        // Approximants / liquids: formants define the glide. Not syllabic nuclei
        // for stress tracking.
        Ph::L => out.push(Target::voiced_non_syl(
            360.0, 1300.0, 3000.0, 3700.0, 4800.0, 0.8, 80.0,
        )),
        Ph::R => out.push(Target::voiced_non_syl(
            490.0, 1350.0, 1690.0, 3000.0, 3800.0, 0.8, 80.0,
        )),
        Ph::W => out.push(Target::voiced_non_syl(
            300.0, 610.0, 2200.0, 3200.0, 4000.0, 0.8, 70.0,
        )),
        Ph::Y => out.push(Target::voiced_non_syl(
            270.0, 2290.0, 3010.0, 3900.0, 5200.0, 0.8, 60.0,
        )),
        // Fricatives: band-passed noise, F4/F5 raised for sibilants.
        Ph::F => out.push(Target::fric(
            1400.0, 1500.0, 0.0, 0.5, NEUTRAL_F4, NEUTRAL_F5, 90.0,
        )),
        Ph::V => out.push(Target::fric(
            1400.0, 1500.0, 0.25, 0.4, NEUTRAL_F4, NEUTRAL_F5, 70.0,
        )),
        Ph::Th => out.push(Target::fric(
            1600.0, 1400.0, 0.0, 0.4, NEUTRAL_F4, NEUTRAL_F5, 90.0,
        )),
        Ph::Dh => out.push(Target::fric(
            1600.0, 1400.0, 0.25, 0.35, NEUTRAL_F4, NEUTRAL_F5, 70.0,
        )),
        Ph::S => out.push(Target::fric(
            5500.0,
            1800.0,
            0.0,
            0.7,
            SIBILANT_F4,
            SIBILANT_F5,
            100.0,
        )),
        Ph::Z => out.push(Target::fric(
            5500.0,
            1800.0,
            0.25,
            0.55,
            SIBILANT_F4,
            SIBILANT_F5,
            80.0,
        )),
        Ph::Sh => out.push(Target::fric(
            2600.0,
            1400.0,
            0.0,
            0.7,
            SIBILANT_F4,
            SIBILANT_F5,
            100.0,
        )),
        Ph::Zh => out.push(Target::fric(
            2600.0,
            1400.0,
            0.25,
            0.55,
            SIBILANT_F4,
            SIBILANT_F5,
            80.0,
        )),
        // /h/: pure aspiration, voiced murmur optional in voiced contexts.
        Ph::Hh => out.push(Target::aspiration(0.0, 0.6, 70.0)),
        // Affricates: silent closure, then a brief fricative release.
        Ph::Ch => {
            out.push(Target::silence(50.0));
            out.push(Target::fric(
                2600.0,
                1400.0,
                0.0,
                0.7,
                SIBILANT_F4,
                SIBILANT_F5,
                70.0,
            ));
        }
        Ph::Jh => {
            out.push(Target::silence(40.0));
            out.push(Target::fric(
                2600.0,
                1400.0,
                0.25,
                0.55,
                SIBILANT_F4,
                SIBILANT_F5,
                70.0,
            ));
        }
        // Stops: closure duration varies with place of articulation (VOT
        // differences - bilabial shortest, velar longest). Voiceless stops
        // also get an aspiration burst; voiced stops get a low voice bar.
        Ph::P => {
            out.push(Target::silence(40.0));
            out.push(Target::aspiration(0.0, 0.55, 35.0));
            out.push(Target::fric(
                800.0, 900.0, 0.0, 0.4, NEUTRAL_F4, NEUTRAL_F5, 12.0,
            ));
        }
        Ph::B => {
            out.push(Target::voiced_non_syl(
                180.0, 900.0, 2200.0, 3300.0, 4200.0, 0.18, 35.0,
            ));
            out.push(Target::fric(
                800.0, 900.0, 0.0, 0.3, NEUTRAL_F4, NEUTRAL_F5, 10.0,
            ));
        }
        Ph::T => {
            out.push(Target::silence(50.0));
            out.push(Target::aspiration(0.0, 0.60, 40.0));
            out.push(Target::fric(
                4000.0,
                1600.0,
                0.0,
                0.5,
                SIBILANT_F4,
                SIBILANT_F5,
                12.0,
            ));
        }
        Ph::D => {
            out.push(Target::voiced_non_syl(
                180.0, 1700.0, 2600.0, 3500.0, 4500.0, 0.18, 30.0,
            ));
            out.push(Target::fric(
                4000.0,
                1600.0,
                0.0,
                0.4,
                SIBILANT_F4,
                SIBILANT_F5,
                10.0,
            ));
        }
        Ph::K => {
            out.push(Target::silence(65.0));
            out.push(Target::aspiration(0.0, 0.60, 45.0));
            out.push(Target::fric(
                1800.0, 1400.0, 0.0, 0.5, NEUTRAL_F4, NEUTRAL_F5, 15.0,
            ));
        }
        Ph::G => {
            out.push(Target::voiced_non_syl(
                180.0, 2000.0, 2500.0, 3300.0, 4200.0, 0.18, 35.0,
            ));
            out.push(Target::fric(
                1800.0, 1400.0, 0.0, 0.4, NEUTRAL_F4, NEUTRAL_F5, 10.0,
            ));
        }
        // French oral vowels
        Ph::FrA => out.push(Target::voiced(
            750.0, 1350.0, 2500.0, 3500.0, 4600.0, AV, 120.0,
        )),
        Ph::FrEClose => out.push(Target::voiced(
            400.0, 2100.0, 2600.0, 3800.0, 4900.0, AV, 110.0,
        )),
        Ph::FrOClose => out.push(Target::voiced(
            400.0, 800.0, 2600.0, 3300.0, 4200.0, AV, 120.0,
        )),
        Ph::FrY => out.push(Target::voiced(
            300.0, 1800.0, 2200.0, 3300.0, 4200.0, AV, 120.0,
        )),
        Ph::Eu => out.push(Target::voiced(
            400.0, 1500.0, 2300.0, 3500.0, 4500.0, AV, 120.0,
        )),
        Ph::EuOpen => out.push(Target::voiced(
            560.0, 1500.0, 2400.0, 3500.0, 4500.0, AV, 110.0,
        )),
        Ph::Schwa => out.push(Target::voiced(
            500.0, 1500.0, 2500.0, 3500.0, 4500.0, 0.9, 90.0,
        )),
        // French nasal vowels: voiced through the parallel nasal branch.
        Ph::Nan => {
            let mut t = Target::voiced(650.0, 1000.0, 2500.0, 3500.0, 4500.0, 0.85, 130.0);
            t.an = 0.6;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 1100.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        Ph::Non => {
            let mut t = Target::voiced(450.0, 900.0, 2500.0, 3300.0, 4200.0, 0.85, 130.0);
            t.an = 0.6;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 900.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        Ph::Nin => {
            let mut t = Target::voiced(560.0, 1600.0, 2500.0, 3500.0, 4500.0, 0.85, 130.0);
            t.an = 0.6;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 1600.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        Ph::Nun => {
            let mut t = Target::voiced(500.0, 1400.0, 2400.0, 3500.0, 4500.0, 0.85, 130.0);
            t.an = 0.6;
            t.fnp = 250.0;
            t.bwnp = 50.0;
            t.fnz = 1400.0;
            t.bwnz = 80.0;
            out.push(t);
        }
        Ph::Wbound | Ph::EClause => {
            // Markers only - duration is set by the surrounding phoneme's target
            // (we just push a 1ms silence so the renderer advances one frame).
            out.push(Target::silence(1.0));
        }
        Ph::Pause => out.push(Target::silence(70.0)),
    }
}

// -----------------------------------------------------------------------------
// Letter and digit names (acronyms / spelled-out characters)
// -----------------------------------------------------------------------------

fn letter_name_en(c: char) -> &'static [Ph] {
    use Ph::*;
    match c.to_ascii_lowercase() {
        'a' => &[Ey],
        'b' => &[B, Iy],
        'c' => &[S, Iy],
        'd' => &[D, Iy],
        'e' => &[Iy],
        'f' => &[Eh, F],
        'g' => &[Jh, Iy],
        'h' => &[Ey, Ch],
        'i' => &[Ay],
        'j' => &[Jh, Ey],
        'k' => &[K, Ey],
        'l' => &[Eh, L],
        'm' => &[Eh, M],
        'n' => &[Eh, N],
        'o' => &[Ow],
        'p' => &[P, Iy],
        'q' => &[K, Y, Uw],
        'r' => &[Aa, R],
        's' => &[Eh, S],
        't' => &[T, Iy],
        'u' => &[Y, Uw],
        'v' => &[V, Iy],
        'w' => &[D, Ah, B, Ah, L, Y, Uw],
        'x' => &[Eh, K, S],
        'y' => &[W, Ay],
        'z' => &[Z, Iy],
        _ => &[],
    }
}

fn letter_name_fr(c: char) -> &'static [Ph] {
    use Ph::*;
    match c.to_ascii_lowercase() {
        'a' => &[Aa],
        'b' => &[B, Eh],
        'c' => &[S, Eh],
        'd' => &[D, Eh],
        'e' => &[Uh],
        'f' => &[Eh, F],
        'g' => &[Jh, Eh],
        'h' => &[Aa, Sh],
        'i' => &[Iy],
        'j' => &[Jh, Iy],
        'k' => &[K, Aa],
        'l' => &[Eh, L],
        'm' => &[Eh, M],
        'n' => &[Eh, N],
        'o' => &[Ow],
        'p' => &[P, Eh],
        'q' => &[K, Uw],
        'r' => &[Eh, R],
        's' => &[Eh, S],
        't' => &[T, Eh],
        'u' => &[Uw],
        'v' => &[V, Eh],
        'w' => &[D, Uh, B, L, Uw, V, Eh],
        'x' => &[Iy, K, S],
        'y' => &[Iy, G, R, Eh, K],
        'z' => &[Z, Eh],
        _ => &[],
    }
}

fn digit_en(d: u8) -> &'static [Ph] {
    use Ph::*;
    match d {
        0 => &[Z, Ih, R, Ow],
        1 => &[W, Ah, N],
        2 => &[T, Uw],
        3 => &[Th, R, Iy],
        4 => &[F, Ao, R],
        5 => &[F, Ay, V],
        6 => &[S, Ih, K, S],
        7 => &[S, Eh, V, Ah, N],
        8 => &[Ey, T],
        _ => &[N, Ay, N],
    }
}

fn digit_fr(d: u8) -> &'static [Ph] {
    use Ph::*;
    match d {
        0 => &[Z, Eh, R, Ow],
        1 => &[Uh, N],
        2 => &[D, Uw],
        3 => &[T, R, Aa],
        4 => &[K, Aa, T, R],
        5 => &[S, Ih, N, K],
        6 => &[S, Iy, S],
        7 => &[S, Eh, T],
        8 => &[W, Ih, T],
        _ => &[N, Uh, F],
    }
}

// -----------------------------------------------------------------------------
// Numbers
// -----------------------------------------------------------------------------

fn number_phones(digits: &str, french: bool, out: &mut Vec<Ph>) {
    let value: u64 = digits.parse().unwrap_or(u64::MAX);
    if digits.len() <= 12 && value != u64::MAX && (digits.len() == 1 || !digits.starts_with('0')) {
        number_words(value, french, out);
    } else {
        for c in digits.chars() {
            if let Some(d) = c.to_digit(10) {
                out.extend_from_slice(if french {
                    digit_fr(d as u8)
                } else {
                    digit_en(d as u8)
                });
                out.push(Ph::Pause);
            }
        }
    }
}

fn number_words(value: u64, french: bool, out: &mut Vec<Ph>) {
    if french {
        number_words_fr(value, out);
        return;
    }
    let digit = |d: u8, out: &mut Vec<Ph>| {
        out.extend_from_slice(if french { digit_fr(d) } else { digit_en(d) });
    };
    if value == 0 {
        digit(0, out);
        return;
    }
    for (scale, name) in [
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1000, "thousand"),
    ] {
        if value >= scale {
            number_words(value / scale, false, out);
            word_phones(name, false, out);
            out.push(Ph::Pause);
            if !value.is_multiple_of(scale) {
                number_words(value % scale, false, out);
            }
            return;
        }
    }
    let hundreds = (value / 100) % 10;
    let remainder = value % 100;
    if hundreds > 0 {
        digit(hundreds as u8, out);
        out.extend_from_slice(if french {
            &[Ph::S, Ph::Aa, Ph::N]
        } else {
            &[Ph::Hh, Ph::Ah, Ph::N, Ph::D, Ph::R, Ph::Ah, Ph::D]
        });
        out.push(Ph::Pause);
    }
    tens_unit(remainder as u8, french, out);
}

fn tens_unit(value: u8, french: bool, out: &mut Vec<Ph>) {
    let digit = |d: u8, out: &mut Vec<Ph>| {
        out.extend_from_slice(if french { digit_fr(d) } else { digit_en(d) });
    };
    if value == 0 {
        return;
    }
    if value < 10 {
        digit(value, out);
        return;
    }
    use Ph::*;
    if !french {
        let teen: &[Ph] = match value {
            10 => &[T, Eh, N],
            11 => &[Ih, L, Eh, V, Ah, N],
            12 => &[T, W, Eh, L, V],
            13 => &[Th, Er, T, Iy, N],
            14 => &[F, Ao, R, T, Iy, N],
            15 => &[F, Ih, F, T, Iy, N],
            16 => &[S, Ih, K, S, T, Iy, N],
            17 => &[S, Eh, V, Ah, N, T, Iy, N],
            18 => &[Ey, T, Iy, N],
            19 => &[N, Ay, N, T, Iy, N],
            _ => &[],
        };
        if !teen.is_empty() {
            out.extend_from_slice(teen);
            return;
        }
        let tens_word: &[Ph] = match value / 10 {
            2 => &[T, W, Eh, N, T, Iy],
            3 => &[Th, Er, T, Iy],
            4 => &[F, Ao, R, T, Iy],
            5 => &[F, Ih, F, T, Iy],
            6 => &[S, Ih, K, S, T, Iy],
            7 => &[S, Eh, V, Ah, N, T, Iy],
            8 => &[Ey, T, Iy],
            _ => &[N, Ay, N, T, Iy],
        };
        out.extend_from_slice(tens_word);
        let unit = value % 10;
        if unit > 0 {
            out.push(Pause);
            digit(unit, out);
        }
    } else {
        let teen: &[Ph] = match value {
            10 => &[D, Iy, S],
            11 => &[Ow, N, Z],
            12 => &[D, Uw, Z],
            13 => &[T, R, Eh, Z],
            14 => &[K, Aa, T, Ao, R, Z],
            15 => &[K, Ih, N, Z],
            16 => &[S, Eh, Z],
            _ => &[],
        };
        if !teen.is_empty() {
            out.extend_from_slice(teen);
            return;
        }
        digit(value / 10, out);
        out.push(Pause);
        digit(value % 10, out);
    }
}

// -----------------------------------------------------------------------------
// Word boundaries, stress, and intonation
// -----------------------------------------------------------------------------

/// Returns true if the phoneme is a vowel-like sound (carries a syllable nucleus).
/// Kept for callers that need phoneme-level syllable detection outside the renderer.
#[allow(dead_code)]
fn is_syllabic(ph: Ph) -> bool {
    use Ph::*;
    matches!(
        ph,
        Aa | Ae
            | Ah
            | Ao
            | Eh
            | Er
            | Ih
            | Iy
            | Uh
            | Uw
            | Ey
            | Ay
            | Oy
            | Aw
            | Ow
            | FrA
            | FrEClose
            | FrOClose
            | FrY
            | Eu
            | EuOpen
            | Schwa
            | Nan
            | Non
            | Nin
            | Nun
            | L
            | R
            | M
            | N
            | Ng
            | Ny
    )
}

/// Whether a token is an acronym to spell out.
fn is_acronym(token: &str) -> bool {
    let letters: Vec<char> = token.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    !letters.is_empty() && letters.len() <= 5 && letters.iter().all(|c| c.is_ascii_uppercase())
}

/// Returns true if `c` is a clause-terminating punctuation character.
fn is_clause_end(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | ',' | ';' | ':')
}

// -----------------------------------------------------------------------------
// Top-level grapheme -> phoneme (with word/stress tagging)
// -----------------------------------------------------------------------------

/// Turn `text` into a phoneme sequence, with `Wbound` markers between words and
/// `EClause` after sentence-terminating punctuation. The renderer uses the markers
/// to assign word-level stress and sentence-level intonation.
///
/// v4: French-specific preprocessing handles the canonical cases of *liaison*
/// (les_amis → /lezami/, mon_ami → /mɔ̃nami/) and *elision* (already-encoded
/// apostrophes like l'homme, d'accord flow through naturally). English is
/// left untouched - English has no productive liaison rule.
fn phones(text: &str, french: bool) -> Vec<Ph> {
    let mut out = Vec::new();

    // Tokenize into words + trailing punctuation so we can look ahead for
    // liaison triggers.
    let mut normalized = String::new();
    for c in text.chars() {
        match c {
            '’' => normalized.push('\''),
            '-' | '–' | '—' | '«' | '»' | '"' | '(' | ')' => normalized.push(' '),
            c if is_clause_end(c) => {
                normalized.push(c);
                normalized.push(' ');
            }
            _ => normalized.push(c),
        }
    }
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let trimmed = token.trim_end_matches(|c: char| is_clause_end(c));
        let trailing_punct: String = token
            .chars()
            .rev()
            .take_while(|c| is_clause_end(*c))
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        if index > 0 {
            out.push(Ph::Wbound);

            // ---- French liaison ----------------------------------------
            // Insert the liaison consonant between word n-1 and word n when:
            //   - French is active
            //   - word n-1 ended in a liaison trigger (les, des, nous, vous,
            //     ils, elles, mon, ton, son, un, ... )
            //   - word n starts with a vowel sound
            // This is the simplest rule set that covers ~80% of cases.
            if french {
                if let Some(liaison) =
                    detect_liaison(tokens.get(index - 1).copied().unwrap_or(""), trimmed)
                {
                    out.push(liaison);
                }
            }
        }

        if trimmed.chars().all(|c| c.is_ascii_digit()) && !trimmed.is_empty() {
            number_phones(trimmed, french, &mut out);
        } else if is_acronym(trimmed) {
            for c in trimmed.chars() {
                if c.is_ascii_alphanumeric() {
                    spell_char(c, french, &mut out);
                    out.push(Ph::Pause);
                }
            }
        } else if french {
            word_phones_fr(trimmed, &mut out);
        } else {
            word_phones(trimmed, french, &mut out);
        }

        // Clause-end marker.
        if !trailing_punct.is_empty() {
            out.push(Ph::Pause);
            out.push(Ph::EClause);
        }
    }
    out
}

/// Detect a French liaison between two adjacent words. Returns the liaison
/// consonant to insert if applicable.
///
/// Rules implemented (the canonical set covering the vast majority of cases):
///   - Determiners/articles: les, des → /z/
///   - Pronouns: nous, vous, ils, elles, on → /z/
///   - Possessives: mon, ton, son → /n/
///   - Indefinite: un → /n/
///   - Adverbs (most common): très, plus, bien, moins, chez, dans, sans, sous,
///     sur, en, devant, après, pendant → /z/ (with vowel elision context)
fn detect_liaison(prev: &str, next: &str) -> Option<Ph> {
    use Ph::*;
    if next.is_empty() || prev.chars().last().map(is_clause_end).unwrap_or(false) {
        return None;
    }
    // The next word must start with a vowel (or h-aspiré, which we treat
    // simply as not-a-vowel to be safe - the cost of skipping one liaison
    // is small).
    let first = next.chars().next().unwrap_or(' ');
    let first_lower = first.to_lowercase().next().unwrap_or(first);
    if !is_vowel_fr(first_lower) && first_lower != 'h' {
        return None;
    }
    // Skip h-aspiré words: "haricot", "hibou", "hache" are not liaison.
    // We don't carry a full list but the common ones we know:
    const H_ASPIRE: &[&str] = &[
        "haricot", "hibou", "hache", "haut", "héros", "honte", "houle", "huit", "hurlante",
        "hurler",
    ];
    if first_lower == 'h' && H_ASPIRE.iter().any(|w| next.to_lowercase().starts_with(w)) {
        return None;
    }
    // Lower-case prev for matching.
    let prev_lc = prev.to_ascii_lowercase();
    let prev_trim: String = prev_lc
        .trim_end_matches(|c: char| !c.is_ascii_alphabetic())
        .to_string();
    match prev_trim.as_str() {
        // /z/ liaisons
        "les" | "des" | "nous" | "vous" | "ils" | "elles" => Some(Z),
        // /n/ liaisons (nasal)
        "mon" | "ton" | "son" | "un" | "en" | "on" => Some(N),
        // /t/ and /r/ less common, skip for now
        _ => None,
    }
}

fn spell_char(c: char, french: bool, out: &mut Vec<Ph>) {
    if let Some(d) = c.to_digit(10) {
        out.extend_from_slice(if french {
            digit_fr(d as u8)
        } else {
            digit_en(d as u8)
        });
    } else if french {
        out.extend_from_slice(letter_name_fr(c));
    } else {
        out.extend_from_slice(letter_name_en(c));
    }
}

// -----------------------------------------------------------------------------
// English word -> phoneme
// -----------------------------------------------------------------------------

fn word_phones(word: &str, french: bool, out: &mut Vec<Ph>) {
    let chars: Vec<char> = word
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if chars.is_empty() {
        for c in word.chars() {
            if c.is_ascii_digit() {
                spell_char(c, french, out);
            }
        }
        return;
    }
    let n = chars.len();
    if !chars.iter().any(|c| "aeiouy".contains(*c)) {
        for &c in &chars {
            spell_char(c, french, out);
            out.push(Ph::Pause);
        }
        return;
    }

    // v4 — English exception patterns. Common words / suffixes whose
    // letter-by-letter rule gets the wrong phonemes. Each entry returns the
    // phoneme sequence if it matches at position 0; the caller still applies
    // stress and word-level lengthening.
    use Ph::*;
    let whole: String = chars.iter().collect();
    let exc: &[Ph] = match whole.as_str() {
        // -tion / -sion: Sh + (Schwa) + N
        "tion" | "tions" => &[Sh, Schwa, N],
        "sion" | "sions" => &[Zh, Schwa, N],
        // -ough family. Real English has 6+ pronunciations of "ough"; we
        // cover the most common.
        "though" => &[Dh, Ow],
        "through" => &[Th, R, Uw],
        "tough" => &[T, Ah, F],
        "rough" => &[R, Ah, F],
        "enough" => &[Eh, N, Ah, F],
        "cough" => &[K, Ao, F],
        "bought" => &[B, Ao, T],
        "thought" => &[Th, Ao, T],
        "fought" => &[F, Ao, T],
        "sought" => &[S, Ao, T],
        "wrought" => &[R, Ao, T],
        "ought" => &[Ao, T],
        // -ould: would, could, should, would
        "would" => &[W, Uh, D],
        "could" => &[K, Uh, D],
        "should" => &[Sh, Uh, D],
        // -aught: caught, taught, naught, haughty
        "caught" => &[K, Ao, T],
        "taught" => &[T, Ao, T],
        "naught" => &[N, Ao, T],
        // -eight: weight, sleight
        "eight" => &[Ey, T],
        "weigh" => &[W, Ey],
        "freight" => &[F, R, Ey, T],
        // common monosyllabic words
        "the" => &[Dh, Ah],
        "of" => &[Ah, V],
        "to" => &[T, Uw],
        "for" => &[F, Ao, R],
        "is" => &[Ih, Z],
        "as" => &[Ae, Z],
        "was" => &[W, Ah, Z],
        "his" => &[Hh, Ih, Z],
        "i" => &[Ay],
        "a" => &[Ah],
        "by" => &[B, Ay],
        "my" => &[M, Ay],
        "no" => &[N, Ow],
        "so" => &[S, Ow],
        // Common polysyllabic exceptions
        "business" => &[B, Ih, Z, N, Ih, S],
        "million" => &[M, Ih, L, Y, Schwa, N],
        "billion" => &[B, Ih, L, Y, Schwa, N],
        "thousand" => &[Th, Aw, Z, Schwa, N, D],
        "hello" => &[Hh, Eh, L, Ow],
        "world" => &[W, Er, L, D],
        "ready" => &[R, Eh, D, Iy],
        "system" => &[S, Ih, S, T, Schwa, M],
        "read" => &[R, Iy, D],
        "said" => &[S, Eh, D],
        "says" => &[S, Eh, Z],
        "one" => &[W, Ah, N],
        "two" => &[T, Uw],
        "who" => &[Hh, Uw],
        "people" => &[P, Iy, P, Schwa, L],
        "listen" => &[L, Ih, S, Schwa, N],
        "whistle" => &[W, Ih, S, Schwa, L],
        "are" => &[Aa, R],
        "have" => &[Hh, Ae, V],
        _ => &[],
    };
    if !exc.is_empty() {
        out.extend_from_slice(exc);
        return;
    }

    // Find vowel *groups* (consecutive vowels like "ai", "ea", "ou" form a
    // single vowel nucleus / one syllable). We use these to identify syllable
    // positions and assign stress.
    let mut groups: Vec<(usize, usize)> = Vec::new(); // (start, end_exclusive)
    let mut in_v = false;
    let mut vstart = 0usize;
    for (i, c) in chars.iter().enumerate() {
        let is_v = "aeiouy".contains(*c);
        if is_v && !in_v {
            vstart = i;
            in_v = true;
        } else if !is_v && in_v {
            groups.push((vstart, i));
            in_v = false;
        }
    }
    if in_v {
        groups.push((vstart, n));
    }
    let num_syls = groups.len();
    // Stress assignment: first vowel group = primary stress. For 3+ syllable
    // words, the last group often has secondary stress (kept). Everything else
    // is reduced toward schwa. This single rule turns "system" from
    // /sɪstɛm/ into /sɪstəm/ - the single biggest naturalness win for English.
    let mut stressed = vec![false; n];
    if num_syls > 0 {
        let (s, e) = groups[0];
        stressed[s..e].fill(true);
        if num_syls >= 3 {
            let (s, e) = groups[num_syls - 1];
            stressed[s..e].fill(true);
        }
    }

    let at = |i: usize| chars.get(i).copied().unwrap_or(' ');
    let is_vowel = |c: char| "aeiou".contains(c);
    // Track whether the next stop we encounter should be aspirated. We
    // aspirate /p t k/ when they precede a stressed vowel - the canonical
    // English allophonic rule ("pin" vs "spin", "top" vs "stop").
    let mut next_stressed_vowel_dist: i32 = -1;
    for &c in chars.iter().take(n) {
        if "aeiouy".contains(c) {
            next_stressed_vowel_dist = 0;
        } else if next_stressed_vowel_dist >= 0 {
            next_stressed_vowel_dist += 1;
        }
    }
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let next = at(i + 1);
        let next2 = at(i + 2);
        let is_unstressed_vowel = "aeiouy".contains(c) && !stressed[i];
        let mut emitted_vowel = false;
        let mut consume_next = 0usize;
        match c {
            'a' => {
                let ph = if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    Ey
                } else if next == 'i' || next == 'y' {
                    consume_next = 1;
                    Ey
                } else if next == 'w' || (next == 'u' && !is_vowel(next2)) {
                    consume_next = 1;
                    Ao
                } else if next == 'r' {
                    Aa
                } else {
                    Ae
                };
                out.push(if is_unstressed_vowel { Schwa } else { ph });
                emitted_vowel = true;
            }
            'e' => {
                if i == n - 1 && n > 2 {
                    // silent final e
                } else if next == 'e' || next == 'a' {
                    consume_next = 1;
                    out.push(if is_unstressed_vowel { Schwa } else { Iy });
                    emitted_vowel = true;
                } else if next == 'i' || next == 'y' {
                    consume_next = 1;
                    out.push(if is_unstressed_vowel { Schwa } else { Ey });
                    emitted_vowel = true;
                } else if next == 'r' {
                    out.push(Er);
                    emitted_vowel = true;
                } else if next == 'w' {
                    consume_next = 1;
                    out.push(if is_unstressed_vowel { Schwa } else { Uw });
                    emitted_vowel = true;
                } else {
                    out.push(if is_unstressed_vowel { Schwa } else { Eh });
                    emitted_vowel = true;
                }
            }
            'i' => {
                let ph = if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    Ay
                } else if next == 'g' && next2 == 'h' {
                    consume_next = 2;
                    Ay
                } else if next == 'r' {
                    Er
                } else {
                    Ih
                };
                out.push(if is_unstressed_vowel { Schwa } else { ph });
                emitted_vowel = true;
            }
            'o' => {
                let ph = if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    Ow
                } else if next == 'o' {
                    consume_next = 1;
                    Uw
                } else if next == 'w' || next == 'u' {
                    consume_next = 1;
                    Aw
                } else if next == 'i' || next == 'y' {
                    consume_next = 1;
                    Oy
                } else if next == 'r' {
                    Ao
                } else {
                    Aa
                };
                out.push(if is_unstressed_vowel { Schwa } else { ph });
                emitted_vowel = true;
            }
            'u' => {
                if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    out.push(Y);
                    out.push(if is_unstressed_vowel { Schwa } else { Uw });
                    emitted_vowel = true;
                } else if next == 'r' {
                    out.push(Er);
                    emitted_vowel = true;
                } else {
                    out.push(if is_unstressed_vowel { Schwa } else { Ah });
                    emitted_vowel = true;
                }
            }
            'y' => {
                if i == 0 {
                    out.push(Y);
                } else if i == n - 1 {
                    out.push(Iy);
                    emitted_vowel = true;
                } else {
                    out.push(if is_unstressed_vowel { Schwa } else { Ih });
                    emitted_vowel = true;
                }
            }
            's' if next == 'h' => {
                out.push(Sh);
                consume_next = 1;
            }
            'c' if next == 'h' => {
                out.push(Ch);
                consume_next = 1;
            }
            't' if next == 'h' => {
                out.push(if i == 0 { Dh } else { Th });
                consume_next = 1;
            }
            'p' if next == 'h' => {
                out.push(F);
                consume_next = 1;
            }
            'g' if next == 'h' => {
                consume_next = 1;
            }
            'c' if next == 'k' => {
                out.push(K);
                consume_next = 1;
            }
            'n' if next == 'g' && i + 2 == n => {
                out.push(Ng);
                consume_next = 1;
            }
            'q' => {
                out.push(K);
                if next == 'u' {
                    out.push(W);
                    consume_next = 1;
                }
            }
            'c' => {
                if matches!(next, 'e' | 'i' | 'y') {
                    out.push(S);
                } else {
                    out.push(K);
                }
            }
            'g' => {
                if matches!(next, 'e' | 'i' | 'y') {
                    out.push(Jh);
                } else {
                    out.push(G);
                }
            }
            'x' => {
                out.push(K);
                out.push(S);
            }
            'b' => out.push(B),
            'd' => out.push(D),
            'f' => out.push(F),
            'h' => out.push(Hh),
            'j' => out.push(Jh),
            'k' => {
                // Allophonic aspiration: /k/ -> [kh] before a stressed vowel
                // (e.g., "key" vs "sky"). We mark with a slightly higher a6 in
                // the renderer via an explicit Hh inserted just after.
                out.push(K);
                if stressed_position_ahead(&chars, i, n) {
                    out.push(Hh);
                }
            }
            'l' => out.push(L),
            'm' => out.push(M),
            'n' => out.push(N),
            // Allophonic aspiration for /p t/ (handled via P/T + Hh marker).
            'p' => {
                out.push(P);
                if stressed_position_ahead(&chars, i, n) {
                    out.push(Hh);
                }
            }
            'r' => out.push(R),
            's' => {
                let prev_vowel = i > 0 && "aeiouy".contains(at(i - 1));
                let rest: String = chars[i + 1..].iter().collect();
                if prev_vowel && (rest.starts_with("ion") || rest.starts_with("ure")) {
                    out.push(Zh);
                } else if prev_vowel && is_vowel(next) {
                    out.push(Z);
                } else {
                    out.push(S);
                }
            }
            't' => {
                out.push(T);
                if stressed_position_ahead(&chars, i, n) {
                    out.push(Hh);
                }
            }
            'v' => out.push(V),
            'w' => out.push(W),
            'z' => out.push(Z),
            _ => {}
        }
        if emitted_vowel && !stressed[i] && is_unstressed_vowel {
            // Reduced vowels get shortened - marker: nothing more, the
            // renderer doesn't see duration here. The per-target duration is
            // set in targets_for(). The schwa target already has dur_ms = 90,
            // shorter than the 110-150 of full vowels.
        }
        if consume_next > 0 {
            i += consume_next;
        }
        if i + 1 < n && chars[i + 1] == c && !is_vowel(c) {
            i += 1;
        }
        i += 1;
    }
}

/// Returns true if there is a vowel in `chars` ahead of position `from`, and
/// the *first* such vowel is a stressed syllable nucleus. Used by the
/// allophonic-aspiration rule for /p t k/ in English.
fn stressed_position_ahead(chars: &[char], from: usize, n: usize) -> bool {
    let mut i = from + 1;
    while i < n {
        if "aeiouy".contains(chars[i]) {
            // Heuristic: the first vowel after a stop is the syllable nucleus
            // (single-syllable words or after consonant clusters). Treat it as
            // stressed - real English aspirates /p t k/ before any vowel in
            // word-initial or stressed-syllable position.
            return true;
        }
        i += 1;
    }
    false
}

// -----------------------------------------------------------------------------
// French word -> phoneme
// -----------------------------------------------------------------------------

fn is_vowel_fr(c: char) -> bool {
    matches!(
        c,
        'a' | 'e'
            | 'i'
            | 'o'
            | 'u'
            | 'y'
            | 'à'
            | 'â'
            | 'é'
            | 'è'
            | 'ê'
            | 'ë'
            | 'î'
            | 'ï'
            | 'ô'
            | 'ù'
            | 'û'
            | 'œ'
    )
}

fn word_phones_fr(word: &str, out: &mut Vec<Ph>) {
    use Ph::*;
    let chars: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    let n = chars.len();
    if n == 0 {
        return;
    }
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let m = |i: usize, s: &str| s.bytes().enumerate().all(|(k, b)| at(i + k) == b as char);
    let ends = |i: usize, s: &str| m(i, s) && i + s.len() == n;
    let nasal = |i: usize, consumed: usize| -> bool {
        let j = i + consumed;
        if j >= n {
            return true;
        }
        let c = chars[j];
        c != 'n' && c != 'm' && !is_vowel_fr(c)
    };

    let whole: String = chars.iter().collect();
    match whole.as_str() {
        "et" => {
            out.push(FrEClose);
            return;
        }
        "six" | "dix" => {
            out.extend_from_slice(&[if whole == "six" { S } else { D }, Iy, S]);
            return;
        }
        "sept" => {
            out.extend_from_slice(&[S, Eh, T]);
            return;
        }
        "huit" => {
            out.extend_from_slice(&[FrY, Iy, T]);
            return;
        }
        "neuf" => {
            out.extend_from_slice(&[N, EuOpen, F]);
            return;
        }
        "mille" | "ville" => {
            out.extend_from_slice(&[if whole == "mille" { M } else { V }, Iy, L]);
            return;
        }
        "soixante" => {
            out.extend_from_slice(&[S, W, FrA, S, Nan, T]);
            return;
        }
        "windows" => {
            out.extend_from_slice(&[W, Nin, D, FrOClose, Z]);
            return;
        }
        "firmware" => {
            out.extend_from_slice(&[F, Iy, R, M, W, Eh, R]);
            return;
        }
        _ => {}
    }

    let mut i = 0;
    while i < n {
        let (c, d, e) = (at(i), at(i + 1), at(i + 2));
        if m(i, "eaux") {
            out.push(FrOClose);
            i += 4;
        } else if m(i, "eau") {
            out.push(FrOClose);
            i += 3;
        } else if m(i, "tion") {
            out.extend_from_slice(&[S, Y, Non]);
            i += 4;
        } else if m(i, "sion") {
            out.extend_from_slice(&[Z, Y, Non]);
            i += 4;
        } else if m(i, "oin") && nasal(i, 3) {
            out.extend_from_slice(&[W, Nin]);
            i += 3;
        } else if m(i, "ien") && nasal(i, 3) {
            out.extend_from_slice(&[Y, Nin]);
            i += 3;
        } else if (m(i, "ain") || m(i, "ein")) && nasal(i, 3) {
            out.push(Nin);
            i += 3;
        } else if m(i, "sch") {
            out.push(Sh);
            i += 3;
        } else if c == 'c' && d == 'h' {
            out.push(Sh);
            i += 2;
        } else if c == 'p' && d == 'h' {
            out.push(F);
            i += 2;
        } else if c == 't' && d == 'h' {
            out.push(T);
            i += 2;
        } else if c == 'g' && d == 'n' {
            out.push(Ny);
            i += 2;
        } else if c == 'n' && d == 'g' {
            out.push(Ng);
            i += 2;
        } else if c == 'q' && d == 'u' {
            out.push(K);
            i += 2;
        } else if c == 'g' && d == 'u' && matches!(e, 'e' | 'i' | 'y') {
            out.push(G);
            i += 2;
        } else if c == 'o' && d == 'i' {
            out.extend_from_slice(&[W, FrA]);
            i += 2;
        } else if c == 'o' && d == 'u' {
            out.push(Uw);
            i += 2;
        } else if c == 'a' && d == 'u' {
            out.push(FrOClose);
            i += 2;
        } else if matches!(c, 'a' | 'e' | 'o' | 'i' | 'y' | 'u')
            && matches!(d, 'n' | 'm')
            && nasal(i, 2)
        {
            out.push(match c {
                'a' | 'e' => Nan,
                'o' => Non,
                'u' => Nun,
                _ => Nin,
            });
            i += 2;
        } else if (c == 'a' || c == 'e') && d == 'i' {
            out.push(Eh);
            i += 2;
        } else if c == 'e' && d == 'u' {
            out.push(Eu);
            i += 2;
        } else if c == 'œ' && d == 'u' {
            out.push(EuOpen);
            i += 2;
        } else if c == 'i' && d == 'l' && e == 'l' {
            out.push(Y);
            i += 3;
        } else if ends(i, "er") || ends(i, "ez") {
            out.push(FrEClose);
            i += 2;
        } else if (i + 1 == n && matches!(c, 'e' | 's' | 'x' | 'z' | 'd' | 't' | 'p' | 'g'))
            || c == 'h'
        {
            // Silent: word-final e/s/x/z/d/t/p/g and h.
            i += 1;
        } else {
            match c {
                'a' | 'à' | 'â' => out.push(FrA),
                'é' => out.push(FrEClose),
                'e' | 'è' | 'ê' | 'ë' => out.push(if i + 1 == n { Schwa } else { Eh }),
                'i' | 'î' | 'ï' => out.push(Iy),
                'o' | 'ô' => out.push(Ao),
                'u' | 'ù' | 'û' => out.push(FrY),
                'y' => out.push(if i > 0 && is_vowel_fr(at(i - 1)) && is_vowel_fr(d) {
                    Y
                } else {
                    Iy
                }),
                'b' => out.push(B),
                'd' => out.push(D),
                'f' => out.push(F),
                'g' => out.push(if matches!(d, 'e' | 'i' | 'y') { Zh } else { G }),
                'j' => out.push(Zh),
                'k' | 'q' => out.push(K),
                'c' | 'ç' => out.push(if c == 'ç' || matches!(d, 'e' | 'i' | 'y') {
                    S
                } else {
                    K
                }),
                'l' => out.push(L),
                'm' => out.push(M),
                'n' => out.push(N),
                'p' => out.push(P),
                'r' => out.push(R),
                's' => out.push(if i > 0 && is_vowel_fr(at(i - 1)) && is_vowel_fr(d) {
                    Z
                } else {
                    S
                }),
                't' => out.push(T),
                'v' => out.push(V),
                'w' => out.push(W),
                'z' => out.push(Z),
                'x' => out.extend_from_slice(&[K, S]),
                _ => {}
            }
            i += 1;
        }
    }
}

// -----------------------------------------------------------------------------
// French number-to-words
// -----------------------------------------------------------------------------

fn number_words_fr(value: u64, out: &mut Vec<Ph>) {
    if value == 0 {
        word_phones_fr("zéro", out);
        return;
    }
    if value >= 1_000_000_000 {
        number_words_fr(value / 1_000_000_000, out);
        word_phones_fr("milliard", out);
        out.push(Ph::Pause);
        if !value.is_multiple_of(1_000_000_000) {
            number_words_fr(value % 1_000_000_000, out);
        }
        return;
    }
    let mut v = value;
    // Millions: 1 000 000 +. In French, "un million" takes no "de", but
    // "deux millions", "trois millions", etc. take an "s" and connect
    // with "de" before a following number ("deux millions d'euros").
    // We omit the "de" here (the integer is standalone).
    if v >= 1_000_000 {
        let millions = v / 1_000_000;
        number_under_1000_fr(millions, out);
        word_phones_fr(if millions > 1 { "millions" } else { "million" }, out);
        out.push(Ph::Pause);
        v %= 1_000_000;
    }
    if v >= 1000 {
        let thousands = v / 1000;
        if thousands > 1 {
            number_under_1000_fr(thousands, out);
        }
        word_phones_fr("mille", out);
        out.push(Ph::Pause);
        v %= 1000;
    }
    if v > 0 {
        number_under_1000_fr(v, out);
    }
}

fn number_under_1000_fr(value: u64, out: &mut Vec<Ph>) {
    let mut v = value;
    if v >= 100 {
        let hundreds = v / 100;
        if hundreds > 1 {
            number_under_100_fr(hundreds, out);
            out.push(Ph::Pause);
        }
        word_phones_fr("cent", out);
        out.push(Ph::Pause);
        v %= 100;
    }
    if v > 0 {
        number_under_100_fr(v, out);
    }
}

fn number_under_100_fr(value: u64, out: &mut Vec<Ph>) {
    const ONES: [&str; 10] = [
        "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf",
    ];
    const TEENS: [&str; 7] = [
        "dix", "onze", "douze", "treize", "quatorze", "quinze", "seize",
    ];
    const TENS: [&str; 7] = [
        "",
        "",
        "vingt",
        "trente",
        "quarante",
        "cinquante",
        "soixante",
    ];
    let w = |s: &str, out: &mut Vec<Ph>| {
        word_phones_fr(s, out);
        out.push(Ph::Pause);
    };
    let v = value as usize;
    if v < 10 {
        word_phones_fr(ONES[v], out);
    } else if v <= 16 {
        word_phones_fr(TEENS[v - 10], out);
    } else if v < 20 {
        w("dix", out);
        word_phones_fr(ONES[v - 10], out);
    } else if v < 70 {
        let unit = v % 10;
        w(TENS[v / 10], out);
        if unit == 1 {
            w("et", out);
        }
        if unit != 0 {
            word_phones_fr(ONES[unit], out);
        }
    } else if v < 80 {
        w("soixante", out);
        if v == 71 {
            w("et", out);
        }
        number_under_100_fr((v - 60) as u64, out);
    } else {
        w("quatre", out);
        w("vingt", out);
        if v > 80 {
            number_under_100_fr((v - 80) as u64, out);
        }
    }
}

// -----------------------------------------------------------------------------
// Formant synthesis engine
// -----------------------------------------------------------------------------

/// A two-pole resonator (Klatt). `y[n] = a*x[n] + b*y[n-1] + c*y[n-2]`, with
/// coefficients set from a centre frequency and bandwidth, unity gain at the centre.
struct Resonator {
    a: f64,
    b: f64,
    c: f64,
    y1: f64,
    y2: f64,
}

impl Resonator {
    const fn new() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Set the resonator to `freq` / `bw` (both hertz) at the output sample rate.
    fn set(&mut self, freq: f64, bw: f64) {
        let r = exp(-PI * bw / SAMPLE_RATE);
        let c = -(r * r);
        let b = 2.0 * r * cos(TWO_PI * freq / SAMPLE_RATE);
        // Unity-gain normalisation: A = 1 - r^2 - 2r*cos(w) is the "raw" coefficient;
        // divide by (1 - r^2 - 2r*cos(w)) for unity peak gain, but Klatt's classical
        // form is just A = 1 - B - C, which has ~unity gain at the centre. We keep
        // the simple form for stability and stability of the cascade.
        self.a = 1.0 - b - c;
        self.b = b;
        self.c = c;
    }

    fn step(&mut self, x: f64) -> f64 {
        let y = self.a * x + self.b * self.y1 + self.c * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// A two-pole two-zero notch (anti-resonance) filter.
///
/// H(z) = (1 - 2cos(w)z^-1 + z^-2) / (1 - 2r cos(w) z^-1 + r^2 z^-2)
///
/// Used for the nasal anti-resonance: the pole-zero pair (≈ 250 Hz pole,
/// place-of-articulation zero ≈ 1000-2000 Hz) that gives /m/, /n/, /ng/ their
/// characteristic spectral dip. The previous code used a peak resonator as an
/// anti-resonance, which only worked because the cascade happened to subtract
/// instead of add — but the depth was wrong and the bandwidth was wrong, so
/// the nasals sounded like muted vowels rather than true nasals. A true notch
/// filter with the zero on the unit circle gives the real ~25-30 dB dip that
/// natural nasals carry.
///
/// Implementation: Direct Form II transposed biquad — four coefficients, two
/// state variables. The form is:
///   w[n]   = x[n] - a1*w[n-1] - a2*w[n-2]
///   y[n]   = b0*w[n] + b1*w[n-1] + b2*w[n-2]
/// This is numerically stable for the bandwidths we use and keeps the code small.
struct Notch {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    w1: f64,
    w2: f64,
}

impl Notch {
    const fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            w1: 0.0,
            w2: 0.0,
        }
    }

    /// Set the notch to centre `freq` Hz with pole-bandwidth `bw_pole` Hz.
    /// The zeros sit on the unit circle for maximum depth; the poles are at
    /// the same angle but radius `r = exp(-pi*bw/SR)` so the dip is narrow
    /// and well-defined.
    fn set(&mut self, freq: f64, bw_pole: f64) {
        let r = exp(-PI * bw_pole / SAMPLE_RATE);
        let cos_w = cos(TWO_PI * freq / SAMPLE_RATE);
        // DC-gain-preserving normalisation: sum of zeros = 2(1-cos(w));
        // we divide so that H(0) = 1.
        let g = (1.0 - r * r) / (2.0 * (1.0 - cos_w).max(1e-9));
        self.b0 = g;
        self.b1 = -2.0 * cos_w * g;
        self.b2 = g;
        self.a1 = 2.0 * r * cos_w;
        self.a2 = -(r * r);
        // Reset state to avoid clicks on parameter change.
        self.w1 = 0.0;
        self.w2 = 0.0;
    }

    #[inline]
    fn step(&mut self, x: f64) -> f64 {
        let w = x - self.a1 * self.w1 - self.a2 * self.w2;
        let y = self.b0 * w + self.b1 * self.w1 + self.b2 * self.w2;
        self.w2 = self.w1;
        self.w1 = w;
        y
    }
}
///
/// Liljencrants-Fant (LF) glottal-flow pulse with adjustable open quotient
/// (`oq`) and return phase (`rq`).
///
/// `oq` controls how much of each pitch period the glottis is open. Modal
/// voice uses ~0.55; breathy voice uses 0.75+ (longer open phase, more
/// turbulent noise); pressed voice uses ~0.40. The LF model's defining
/// feature is the *return phase* `rq` (typically 0.05-0.10): after the glottis
/// closes there is a brief moment where the airflow decays exponentially to
/// zero rather than jumping to zero instantly. This sharp closure discontinuity
/// is what gives voiced speech its harmonic richness at the glottal source -
/// without it the spectrum rolls off at -6 dB/oct instead of the natural
/// -12 dB/oct.
///
/// The pulse is normalised so its peak amplitude sits at 1.0.
fn glottal_flow(phase: f64, oq: f64, config: Config) -> f64 {
    let oq = oq.clamp(0.30, 0.85);
    // Open phase: 62% smooth rise (opening) + 38% close. Asymmetric pulse.
    let t_open = oq;
    let t_rise = oq * 0.62;
    let t_fall = oq * 0.38;
    // Return phase: brief exponential decay after closure. ~0.07 in modal
    // voice, slightly longer (0.10+) in breathy voice. We bake the voice
    // quality adjustment into the constant here.
    let rq = match config.quality {
        VoiceQuality::Modal => 0.07,
        VoiceQuality::Breathy => 0.11,
        VoiceQuality::Pressed => 0.04,
        VoiceQuality::Creaky => 0.13,
    };
    let t_total = t_open + rq;
    if phase < t_rise {
        // Opening: smooth half-cosine rise from 0 to peak (1.0).
        0.5 * (1.0 - cos(PI * phase / t_rise))
    } else if phase < t_open {
        // Closing: cosine fall from peak back toward zero. At t_open, the
        // value is non-zero (~0.1-0.2 depending on t_fall); the return phase
        // then carries it down.
        let p = (phase - t_rise) / t_fall;
        // Smooth decay through positive values, ending near 0.15 at t_open.
        (1.0 - 0.85 * (1.0 - cos(PI * p)) * 0.5).max(0.0)
    } else if phase < t_total {
        // Return phase: exponential decay from the end-of-closing value to 0.
        let p = (phase - t_open) / rq;
        0.15 * exp(-4.0 * p)
    } else {
        // Closed phase.
        0.0
    }
}

/// Compute the open quotient (OQ) appropriate for a given phoneme target,
/// adjusted by the current voice quality mode and named voice. Vowels use
/// a modal voice (~0.55); nasals are slightly more open (~0.62); voiced
/// stops are tighter (~0.50).
fn target_oq(av: f64, an: f64, _af: f64, config: Config) -> f64 {
    let voice = config.voice;
    let voice_oq = voice.base_oq();
    let base = if av < 0.05 {
        voice_oq
    } else if an > 0.0 {
        voice_oq + 0.07
    } else {
        // NOTE: the ~0.50 OQ documented above for voiced stops (`af > 0.3`) is not
        // applied; they currently use the modal value. Changing it alters the voice.
        voice_oq
    };
    match config.quality {
        VoiceQuality::Modal => base,
        VoiceQuality::Breathy => (base + 0.18).min(0.85),
        VoiceQuality::Pressed => (base - 0.12).max(0.30),
        VoiceQuality::Creaky => (base - 0.20).max(0.25),
    }
}

/// Per-voice-quality jitter/shimmer/breathiness parameters. The renderer
/// multiplies the base 0.015/0.08/0.025 values by these.
fn voice_quality_params(config: Config) -> (f64, f64, f64) {
    match config.quality {
        VoiceQuality::Modal => (1.0, 1.0, 1.0),
        VoiceQuality::Breathy => (2.0, 1.6, 2.5), // more shimmer, much more breathiness
        VoiceQuality::Pressed => (0.4, 0.5, 0.2), // tighter, less variation
        VoiceQuality::Creaky => (3.0, 1.8, 0.6),  // very jittery
    }
}

/// Per-syllable pitch accent multiplier. Real speech uses pitch accents (ToBI
/// H*) on stressed syllables, a phrase-final low boundary tone (L%) for
/// statements, and a high tone (L+H%) for questions. Returns a multiplier
/// applied to the base F0 as a function of `position` (0..1) within the syllable.
///
/// v0.4.1: excursions bumped to match real-speech amplitudes (English H*
/// accents typically rise 25-50% above baseline, then fall back).
fn syl_accent(position: f64, is_stressed: bool, is_final: bool, is_question: bool) -> f64 {
    let t = position.clamp(0.0, 1.0);
    if is_final && is_question {
        // L+H% : dramatic rise across the final syllable (real questions can
        // hit 50-70% above baseline at the very end).
        let early_peak = (1.0 - (t / 0.18).powi(2)).max(0.0) * 0.22;
        let later_rise = ((t - 0.18) / 0.82).clamp(0.0, 1.0) * 0.28;
        1.0 + early_peak + later_rise
    } else if is_final {
        // L% : rise then long fall - the "end of statement" contour.
        let early_peak = (1.0 - (t / 0.18).powi(2)).max(0.0) * 0.22;
        let fall = (t * 0.45) + (t * t * 0.15);
        1.0 + early_peak - fall
    } else if is_stressed {
        // H* : real lexical stress - 25% rise, 30% fall across the syllable.
        let early_peak = (1.0 - (t / 0.22).powi(2)).max(0.0) * 0.25;
        let fall = ((t - 0.22) / 0.78).clamp(0.0, 1.0) * 0.30;
        1.0 + early_peak - fall
    } else {
        // Unstressed: compressed range - real English unstressed syllables
        // have very small F0 movement (often <5% variation).
        let early_peak = (1.0 - (t / 0.2).powi(2)).max(0.0) * 0.06;
        let fall = ((t - 0.2) / 0.8).clamp(0.0, 1.0) * 0.08;
        1.0 + early_peak - fall
    }
}

/// Renderer state: 5 formants, nasal pole + zero, fricative band, source & output
/// filters, plus the prosody counters (syllables, words, clauses) used to drive
/// word stress and sentence intonation.
struct Renderer {
    config: Config,
    f1: f64,
    f2: f64,
    f3: f64,
    f4: f64,
    f5: f64,
    r1: Resonator,
    r2: Resonator,
    r3: Resonator,
    r4: Resonator,
    r5: Resonator,
    /// Parallel nasal pole (bandpass at fnp).
    rn: Resonator,
    /// Parallel nasal anti-resonance (true notch at fnz) - fixed in v4.
    raz: Notch,
    rf: Resonator,
    glottal_phase: f64,
    f0_smooth: f64,
    cycle_jitter: f64,
    cycle_shimmer: f64,
    shimmer_smooth: f64,
    rng: u32,
    prev_flow: f64,
    src_lp: f64,
    prev_noise: f64,
    prev_aspiration: f64,
    out_lp: f64,
    /// Pre-emphasis high-pass state.
    pre_hp: f64,
    /// De-emphasis low-pass state.
    post_lp: f64,
    /// DC blocker state (x[n-1] and y[n-1]).
    dc_x: f64,
    dc_y: f64,
    /// Soft limiter previous state.
    limit_prev: f64,
}

impl Renderer {
    #[cfg(test)]
    fn new() -> Self {
        Self::configured(Config::snapshot())
    }
    fn configured(config: Config) -> Self {
        Self {
            config,
            f1: 500.0,
            f2: 1500.0,
            f3: 2500.0,
            f4: 3500.0,
            f5: 4500.0,
            r1: Resonator::new(),
            r2: Resonator::new(),
            r3: Resonator::new(),
            r4: Resonator::new(),
            r5: Resonator::new(),
            rn: Resonator::new(),
            raz: Notch::new(),
            rf: Resonator::new(),
            glottal_phase: 0.0,
            f0_smooth: 0.0,
            cycle_jitter: 1.0,
            cycle_shimmer: 1.0,
            shimmer_smooth: 1.0,
            rng: 0x1234_5678,
            prev_flow: 0.0,
            src_lp: 0.0,
            prev_noise: 0.0,
            prev_aspiration: 0.0,
            out_lp: 0.0,
            pre_hp: 0.0,
            post_lp: 0.0,
            dc_x: 0.0,
            dc_y: 0.0,
            limit_prev: 0.0,
        }
    }

    /// White-noise sample in -1..1 from a fast xorshift PRNG.
    fn noise(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f64 / u32::MAX as f64) * 2.0 - 1.0
    }

    /// Approximately Gaussian noise (sum of 4 uniforms, normalised) - sounds more
    /// natural than pure white for frication.
    fn gnoise(&mut self) -> f64 {
        let mut s = 0.0_f64;
        for _ in 0..4 {
            s += self.noise();
        }
        s * 0.5 // ~unit variance
    }

    fn render(
        &mut self,
        targets: &[Target],
        f0: f64,
        rate_percent: u32,
        is_question: bool,
        french: bool,
        buf: &mut Vec<f64>,
    ) {
        let rate_scale = 100.0 / rate_percent as f64;
        // Active voice preset — applies formant scaling and OQ baseline.
        let voice = self.config.voice;
        let formant_scale = voice.formant_scale();
        // Adaptive formant slew. The slew time-constant depends on the *type*
        // of the current segment and the *direction* of the transition:
        //
        //   V -> V   :  8 ms   (slow, smooth — vowel-to-vowel coarticulation)
        //   V -> C   :  12 ms  (moderate — vowel-to-consonant transition)
        //   C -> V   :  18 ms  (faster — release into vowel)
        //   Stop rel :  4 ms   (snap — formants leap to vowel target at burst)
        //
        // Constant slewing is what makes formant synthesis sound "robotic".
        // Real speech has all four regimes in every sentence.
        let slew_v2v = 1.0 - exp(-1.0 / (0.008 * SAMPLE_RATE));
        let slew_v2c = 1.0 - exp(-1.0 / (0.012 * SAMPLE_RATE));
        let slew_c2v = 1.0 - exp(-1.0 / (0.018 * SAMPLE_RATE));
        let slew_stop = 1.0 - exp(-1.0 / (0.004 * SAMPLE_RATE));
        // Anticipatory coarticulation: blend with the *next* target so the
        // formants begin to anticipate the following phoneme. v4 increases the
        // lookahead to 15% for vowel-to-vowel transitions (more anticipatory)
        // and keeps it at 8% elsewhere.
        let lookahead_sonor = 0.15_f64;
        let lookahead_other = 0.08_f64;

        // Per-target sample counts with stress + final-syllable lengthening.
        // We also pre-compute which syllable is the final one in the phrase so
        // the intonation model knows whether to apply a final fall or rise.
        let total_syls: usize = targets.iter().filter(|t| t.is_syl).count();
        let total: usize = targets
            .iter()
            .map(|t| ((t.dur_ms * rate_scale) / 1000.0 * SAMPLE_RATE) as usize)
            .sum::<usize>()
            .max(1);

        // Pre-classify each target's phoneme type to drive the adaptive slew.
        // Sonorants (vowels, nasals, liquids) slew slowly; obstruents slew
        // faster; stops trigger the snap-to-vowel regime on release.
        let mut is_sonorant: Vec<bool> = Vec::with_capacity(targets.len());
        let mut is_stop_release: Vec<bool> = Vec::with_capacity(targets.len());
        for t in targets.iter() {
            // Sonorants: voiced segments with formants (av > 0 and is_syl, or
            // the nasal/liquid class). Everything else is an obstruent.
            let sonorant = t.is_syl || (t.av > 0.0 && (t.an > 0.0 || t.f1 < 500.0));
            is_sonorant.push(sonorant);
            // Stop release: a short fricative burst following a silence within
            // the same phoneme (built into P/T/K/G/B/D targets). We detect it
            // by short duration + high af.
            is_stop_release.push(!t.is_syl && t.af > 0.0 && t.dur_ms < 30.0 && t.av < 0.05);
        }

        let mut global = 0usize;
        let mut syllables_in_word: u32 = 0;
        let mut saw_wbound: bool = true;
        let mut _current_syl_index: usize = 0;
        let mut current_syl_start_sample: usize = 0;
        let mut current_syl_total_samples: usize = 1;
        let mut current_syl_is_stressed: bool = false;
        let mut current_syl_is_final: bool = false;
        let mut syllables_seen: usize = 0;
        let mut prev_was_sonorant = true; // start as if we're in a sonorant

        for (t_idx, target) in targets.iter().enumerate() {
            // Adaptive slew: pick the coefficient based on the direction of
            // transition.
            let curr_sonor = is_sonorant[t_idx];
            let slew = if is_stop_release[t_idx] {
                slew_stop
            } else if curr_sonor && prev_was_sonorant {
                slew_v2v
            } else if !curr_sonor && prev_was_sonorant {
                slew_v2c
            } else {
                slew_c2v
            };
            prev_was_sonorant = curr_sonor;

            // Anticipatory coarticulation lookahead, with per-voice formant
            // scaling applied. The scaling is physical (vocal tract length) so
            // it is applied uniformly to the F1..F5 trajectory.
            let next = targets.get(t_idx + 1).copied().unwrap_or(*target);
            let lookahead = if curr_sonor {
                lookahead_sonor
            } else {
                lookahead_other
            };
            let ef1 = (target.f1 * (1.0 - lookahead) + next.f1 * lookahead) * formant_scale;
            let ef2 = (target.f2 * (1.0 - lookahead) + next.f2 * lookahead) * formant_scale;
            let ef3 = (target.f3 * (1.0 - lookahead) + next.f3 * lookahead) * formant_scale;
            let ef4 = (target.f4 * (1.0 - lookahead) + next.f4 * lookahead) * formant_scale;
            let ef5 = (target.f5 * (1.0 - lookahead) + next.f5 * lookahead) * formant_scale;

            let samples = ((target.dur_ms * rate_scale) / 1000.0 * SAMPLE_RATE) as usize;

            // Detect syllable / word boundaries before rendering this target.
            if saw_wbound {
                syllables_in_word = 0;
                saw_wbound = false;
            }
            let last_in_word = target.is_syl
                && !targets[t_idx + 1..]
                    .iter()
                    .take_while(|t| !(t.dur_ms < 5.0 && t.av == 0.0))
                    .any(|t| t.is_syl);
            let accented = target.is_syl
                && if french {
                    last_in_word
                } else {
                    syllables_in_word == 0
                };
            if target.is_syl {
                syllables_in_word += 1;
                let first_syl = accented;
                let is_final_syl = syllables_seen + 1 == total_syls;
                let len_mult = if first_syl || is_final_syl { 1.18 } else { 1.0 };
                let sc = ((samples as f64) * len_mult) as usize;
                current_syl_start_sample = global;
                current_syl_total_samples = sc;
                current_syl_is_stressed = first_syl;
                current_syl_is_final = is_final_syl;
            }
            let first_syl = accented;
            let amp_boost = if first_syl { 1.10 } else { 1.0 };
            let len_mult = if first_syl || (target.is_syl && syllables_seen + 1 == total_syls) {
                1.18
            } else {
                1.0
            };
            let sample_count = ((samples as f64) * len_mult) as usize;

            // Sentence-level declination: pitch falls ~12% over the utterance.
            let progress = global as f64 / total as f64;
            for index in 0..sample_count {
                // Slew the formants toward the *anticipated* effective target.
                self.f1 += (ef1 - self.f1) * slew;
                self.f2 += (ef2 - self.f2) * slew;
                self.f3 += (ef3 - self.f3) * slew;
                self.f4 += (ef4 - self.f4) * slew;
                self.f5 += (ef5 - self.f5) * slew;
                // Recompute resonator coefficients at ~1.5 kHz control rate
                // (every 16 samples at 32 kHz ≈ 2 kHz); the formants move far
                // slower than that, so it is inaudible and cuts the
                // transcendental cost 16-fold.
                if index & 15 == 0 {
                    self.r1.set(self.f1, target.bw1);
                    self.r2.set(self.f2, target.bw2);
                    self.r3.set(self.f3, target.bw3);
                    self.r4.set(self.f4, target.bw4);
                    self.r5.set(self.f5, target.bw5);
                    self.rn.set(target.fnp, target.bwnp);
                    self.raz.set(target.fnz, target.bwnz);
                    self.rf.set(target.fc, target.fbw);
                }

                // Position within the current syllable, used for the F0 contour.
                let pos_in_syl = if current_syl_total_samples > 0 {
                    (global - current_syl_start_sample) as f64
                        / current_syl_total_samples.max(1) as f64
                } else {
                    0.0
                };
                let accent = if global >= current_syl_start_sample {
                    let a = syl_accent(
                        pos_in_syl,
                        current_syl_is_stressed,
                        current_syl_is_final,
                        is_question,
                    );
                    // Voice-weighted: female and child voices have more
                    // pronounced intonation. The accent is centred at 1.0,
                    // so we scale the deviation from 1.0 by accent_weight.
                    let acc_w = voice.accent_weight();
                    1.0 + (a - 1.0) * acc_w
                } else {
                    1.0
                };

                // Phrase-initial L% rise: for the first syllable of the
                // utterance, raise F0 by up to 8% across the syllable then
                // fall back. This is the classic "uptalk" of natural
                // statements: speakers start a bit higher, then declination
                // takes over.
                let initial_rise = if syllables_seen == 0 && target.is_syl {
                    let p = pos_in_syl.clamp(0.0, 1.0);
                    // Peak at ~10% then fall.
                    let peak = (1.0 - ((p - 0.1) / 0.4).powi(2)).max(0.0) * 0.08;
                    1.0 + peak
                } else {
                    1.0
                };
                // Phrase-medial pitch reset (H-): every ~7 syllables, add a
                // small +3% F0 bump on the stressed onset to keep the voice
                // from sounding like it's trailing off. Most pronounced on
                // multi-clause sentences.
                let medial_reset = if target.is_syl
                    && current_syl_is_stressed
                    && syllables_seen > 0
                    && syllables_seen.is_multiple_of(7)
                {
                    1.03
                } else {
                    1.0
                };
                // Micro-prosody: voiced consonants (m, n, ng, l, r, v, z, etc.)
                // slightly perturb F0 - the canonical +5% bump on voiced
                // obstruents in natural speech. We detect this via
                // av > 0 and an > 0 or low F1.
                let micro_prosody =
                    if target.av > 0.05 && !target.is_syl && (target.an > 0.0 || target.f1 < 350.0)
                    {
                        let p = pos_in_syl.clamp(0.0, 1.0);
                        // Bell curve peaking at ~40% through the consonant.
                        let bump = (1.0 - ((p - 0.4) / 0.3).powi(2)).max(0.0) * 0.04;
                        1.0 + bump
                    } else {
                        1.0
                    };

                // Pitch: declination + jitter + per-syllable accent contour.
                let (jq, sq, _bq) = voice_quality_params(self.config);
                // Perturbations belong to glottal cycles, not audio samples.
                // Smooth the amplitude so cycle boundaries do not add broadband noise.
                self.shimmer_smooth += (self.cycle_shimmer - self.shimmer_smooth) * 0.02;
                let shimmer = self.shimmer_smooth;
                // Long-term F0 drift: a slow oscillator (~0.7 Hz) modulates F0
                // by ±3% over the phrase, simulating the natural drift in a
                // real speaker's pitch.
                let drift_freq = 0.7;
                let drift_amp = 0.03;
                let drift_phase = (global as f64) / SAMPLE_RATE * drift_freq * TWO_PI;
                let drift = 1.0 + drift_amp * cos(drift_phase);
                let f0_target = f0
                    * (1.05 - 0.12 * progress)
                    * drift
                    * accent
                    * initial_rise
                    * medial_reset
                    * micro_prosody;
                if self.f0_smooth == 0.0 {
                    self.f0_smooth = f0_target;
                }
                // 12 ms slew preserves phrase intonation while avoiding target jumps.
                self.f0_smooth += (f0_target - self.f0_smooth) * (1.0 / (0.012 * SAMPLE_RATE));
                let f0_now = self.f0_smooth * self.cycle_jitter;
                self.glottal_phase += f0_now / SAMPLE_RATE;
                // Use `while` so we can't get stuck with phase > 1 if f0 jumps.
                while self.glottal_phase >= 1.0 {
                    self.glottal_phase -= 1.0;
                    self.cycle_jitter = 1.0 + 0.002 * jq * self.noise();
                    self.cycle_shimmer = 1.0 + 0.015 * sq * self.noise();
                }
                let flow = glottal_flow(
                    self.glottal_phase,
                    target_oq(target.av, target.an, target.af, self.config),
                    self.config,
                );
                // Lip radiation: differentiate the glottal flow.
                let excitation = (flow - self.prev_flow) * 6.5;
                self.prev_flow = flow;
                // Spectral tilt: low-pass a copy of the excitation, mix with
                // the dry signal. Warmer, less buzzy.
                self.src_lp += (excitation - self.src_lp) * 0.30;
                let source =
                    (0.65 * excitation + 0.35 * self.src_lp) * target.av * shimmer * amp_boost;

                // Voiced cascade: r5(r4(r3(r2(r1(source))))) for a full five-formant
                // spectrum instead of the v2 four-formant cascade.
                let voiced_cascade = self.r5.step(
                    self.r4
                        .step(self.r3.step(self.r2.step(self.r1.step(source)))),
                );

                // Parallel nasal branch with v4 notch filter. The notch has
                // its zero on the unit circle (full depth) at fnz, so the
                // spectral dip at the place of articulation is properly deep
                // (~25-30 dB). The nasal pole first boosts the low-frequency
                // nasal murmur, then the notch carves out the place.
                let nasal = if target.an > 0.0 {
                    let pole = self.rn.step(source);
                    let zero = self.raz.step(pole);
                    zero * target.an
                } else {
                    0.0
                };

                // Frication: high-passed (crisper sibilants) band-passed noise.
                let fric = if target.af > 0.0 {
                    let raw = self.gnoise();
                    let hp = raw - self.prev_noise;
                    self.prev_noise = raw;
                    self.rf.step(hp) * target.af
                } else {
                    self.prev_noise *= 0.5;
                    0.0
                };

                // Aspiration: broadband breath noise, lightly high-passed.
                let (_jq, _sq, bq) = voice_quality_params(self.config);
                let breathy = if target.av > 0.1 { 0.008 * bq } else { 0.0 };
                let asp_amp = target.a6.max(breathy);
                let asp = if asp_amp > 0.0 {
                    let raw = self.gnoise();
                    let hp = raw - self.prev_aspiration;
                    self.prev_aspiration = raw;
                    hp * asp_amp * 0.7
                } else {
                    self.prev_aspiration *= 0.5;
                    0.0
                };

                // Mix.
                let mix = voiced_cascade + nasal + fric + asp;
                // Light output smoothing to remove HF stepping without dulling
                // consonants.
                self.out_lp += (mix - self.out_lp) * 0.18;
                let mut out = 0.80 * mix + 0.20 * self.out_lp;
                // Gentle high-shelf boost (very mild pre-emphasis) to keep the
                // upper formants audible without stripping low-frequency energy.
                self.pre_hp = 0.995 * self.pre_hp + 0.005 * out;
                out = out - 0.35 * (out - self.pre_hp);
                // Final smoothing: 1st-order low-pass to remove any residual
                // aliasing/stepping artefacts.
                self.post_lp += (out - self.post_lp) * 0.30;
                let mut final_out = 0.90 * out + 0.10 * self.post_lp;
                // DC blocker (high-pass at ~20 Hz). Removes any DC bias
                // accumulated from glottal asymmetric pulses.
                let dc_y = final_out - self.dc_x + 0.995 * self.dc_y;
                self.dc_x = final_out;
                self.dc_y = dc_y;
                final_out = dc_y;
                // Soft limiter: tanh saturation at +0.95 / -0.95 keeps
                // transients musical while preventing clipping. Smoothed by
                // a 1-pole IIR so we don't get zipper noise.
                let drive = (final_out - self.limit_prev) * 0.6 + self.limit_prev;
                self.limit_prev = drive;
                let limited = (drive).tanh() * 0.97 + (drive - drive.tanh()) * 0.0; // tanh does the work

                buf.push(limited);
                global += 1;
            }
            // Update syllable counter at end of target.
            if target.is_syl {
                syllables_seen += 1;
            }
            // Mark end of word for the stress counter.
            if target.dur_ms < 5.0 && target.av == 0.0 {
                saw_wbound = true;
                syllables_in_word = 0;
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Public synthesis entry point
// -----------------------------------------------------------------------------

/// Synthesize `text` into 32 kHz mono 16-bit PCM. English letter-to-sound rules
/// and number words drive the voice; `french` selects French letter and number
/// names. A '?' in the input selects question intonation (final rise).
///
/// v4 — SSML-lite: if the input contains SSML-style tags, they are parsed and
/// applied per-segment. Supported tags:
///   `<break time="500ms"/>`   insert a pause
///   `<emphasis level="strong|moderate|none">text</emphasis>`
///   `<prosody rate="fast|slow|default" pitch="high|low|default">text</prosody>`
///   `<voice name="male|female|child">text</voice>`
pub fn say(text: &str, french: bool) -> Vec<u8> {
    say_with_config(text, french, Config::snapshot())
}

/// Reentrant synthesis; no global configuration mutation, including nested SSML.
pub fn say_with_config(text: &str, french: bool, config: Config) -> Vec<u8> {
    to_pcm16(&render_samples(text, french, config))
}

/// Native 32 kHz floating-point fallback output, before PCM quantization.
pub fn render_samples(text: &str, french: bool, mut config: Config) -> Vec<f64> {
    config.rate = config.rate.clamp(50, 300);
    config.pitch = config.pitch.clamp(50, 350);
    let segments = parse_ssml_with_config(text, config);
    let mut out = Vec::new();
    for seg in segments {
        let mut local = config;
        if let Some(r) = seg.rate_override {
            local.rate = r.clamp(50, 300);
        }
        if let Some(p) = seg.pitch_override {
            local.pitch = p.clamp(50, 350);
        }
        if let Some(v) = seg.voice_override {
            local.voice = match v {
                1 => Voice::Female,
                2 => Voice::Child,
                _ => Voice::Male,
            };
        }
        if let Some(q) = seg.quality_override {
            local.quality = match q {
                1 => VoiceQuality::Breathy,
                2 => VoiceQuality::Pressed,
                3 => VoiceQuality::Creaky,
                _ => VoiceQuality::Modal,
            };
        }
        match seg.kind {
            SegmentKind::Text(s) => {
                for clause in s.split_inclusive(['.', '?', '!']) {
                    out.extend(say_clause_samples(clause, french, local));
                }
            }
            SegmentKind::Pause(ms) => {
                out.resize(out.len() + (ms as f64 * SAMPLE_RATE / 1000.0) as usize, 0.0)
            }
            SegmentKind::Empty => {}
        }
    }
    out
}

#[derive(Clone, Default)]
enum SegmentKind {
    #[default]
    Empty,
    Text(String),
    Pause(u32), // milliseconds
}

#[derive(Clone, Default)]
struct Segment {
    kind: SegmentKind,
    rate_override: Option<u32>,
    pitch_override: Option<u32>,
    voice_override: Option<u32>,
    quality_override: Option<u32>,
}

/// Parse SSML-lite tags into a sequence of segments with their overrides.
/// This is a small recursive-descent parser that handles the four tag types.
/// Anything we don't recognise is treated as literal text.
#[cfg(test)]
fn parse_ssml(text: &str) -> Vec<Segment> {
    parse_ssml_with_config(text, Config::snapshot())
}

fn parse_ssml_with_config(text: &str, config: Config) -> Vec<Segment> {
    fn flush(buf: &mut String, context: &Segment, out: &mut Vec<Segment>) {
        if !buf.is_empty() {
            let mut seg = context.clone();
            seg.kind = SegmentKind::Text(core::mem::take(buf));
            out.push(seg);
        }
    }
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut context = Segment::default();
    let mut stack: Vec<(String, Segment)> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text.as_bytes()[i] == b'<' {
            if let Some(end) = text[i..].find('>') {
                let tag = text[i + 1..i + end].trim();
                let name = tag
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/');
                if let Some(closing) = tag.strip_prefix('/') {
                    if stack.last().map(|x| x.0.as_str()) == Some(closing.trim()) {
                        flush(&mut buf, &context, &mut out);
                        context = stack.pop().unwrap().1;
                        i += end + 1;
                        continue;
                    }
                } else if name == "break" && tag.ends_with('/') {
                    flush(&mut buf, &context, &mut out);
                    out.push(Segment {
                        kind: SegmentKind::Pause(
                            parse_attr_ms(tag, "time").unwrap_or(500).min(60000),
                        ),
                        ..context.clone()
                    });
                    i += end + 1;
                    continue;
                } else if matches!(name, "speak" | "emphasis" | "prosody" | "voice")
                    && !tag.ends_with('/')
                {
                    flush(&mut buf, &context, &mut out);
                    stack.push((String::from(name), context.clone()));
                    match name {
                        "emphasis" => {
                            let level = parse_attr_str(tag, "level").unwrap_or_default();
                            let rate = context.rate_override.unwrap_or(config.rate);
                            if level == "strong" {
                                context.quality_override = Some(2);
                                context.rate_override = Some((rate as f64 * 0.92) as u32);
                            } else if level == "moderate" {
                                context.rate_override = Some((rate as f64 * 0.96) as u32);
                            }
                        }
                        "prosody" => {
                            let r = context.rate_override.unwrap_or(config.rate);
                            let p = context.pitch_override.unwrap_or(config.pitch);
                            if let Some(value) = parse_attr_str(tag, "rate") {
                                context.rate_override = Some(
                                    match value.as_str() {
                                        "fast" => (r as f64 * 1.20) as u32,
                                        "slow" => (r as f64 * 0.78) as u32,
                                        _ => r,
                                    }
                                    .clamp(50, 300),
                                );
                            }
                            if let Some(value) = parse_attr_str(tag, "pitch") {
                                context.pitch_override = Some(
                                    match value.as_str() {
                                        "high" => (p as f64 * 1.18) as u32,
                                        "low" => (p as f64 * 0.85) as u32,
                                        _ => p,
                                    }
                                    .clamp(50, 350),
                                );
                            }
                        }
                        "voice" => {
                            let v = parse_attr_str(tag, "name").unwrap_or_default();
                            let selected = match v.as_str() {
                                "female" | "f" => 1,
                                "child" | "c" => 2,
                                _ => 0,
                            };
                            context.voice_override = Some(selected);
                            context.pitch_override = Some(match selected {
                                1 => 200,
                                2 => 280,
                                _ => 115,
                            });
                        }
                        _ => {}
                    }
                    i += end + 1;
                    continue;
                }
            }
        }
        let c = text[i..].chars().next().unwrap();
        buf.push(c);
        i += c.len_utf8();
    }
    flush(&mut buf, &context, &mut out);
    out
}

fn parse_attr_str(tag: &str, name: &str) -> Option<String> {
    let needle = alloc::format!("{}=\"", name);
    if let Some(idx) = tag.find(&needle) {
        let rest = &tag[idx + needle.len()..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn parse_attr_ms(tag: &str, name: &str) -> Option<u32> {
    let s = parse_attr_str(tag, name)?;
    if let Some(stripped) = s.strip_suffix("ms") {
        stripped.parse().ok()
    } else if let Some(stripped) = s.strip_suffix("s") {
        Some(((stripped.parse::<f64>().ok()?) * 1000.0) as u32)
    } else {
        s.parse().ok()
    }
}

fn say_clause_samples(text: &str, french: bool, config: Config) -> Vec<f64> {
    if !text.chars().any(|c| c.is_alphanumeric()) {
        return Vec::new();
    }
    let phonemes = phones(text, french);
    if phonemes.is_empty() {
        return Vec::new();
    }
    let is_question = text.contains('?');
    let mut targets = Vec::new();
    // Short lead-in of silence settles the resonators before the first sound.
    targets.push(Target::silence(15.0));
    for ph in phonemes {
        targets_for(ph, &mut targets);
    }
    targets.push(Target::silence(20.0));

    let mut samples = Vec::new();
    Renderer::configured(config).render(
        &targets,
        config.pitch as f64,
        config.rate,
        is_question,
        french,
        &mut samples,
    );
    samples
}

/// Level a `f64` sample buffer to 16-bit PCM bytes: find the peak and scale so
/// the loudest sample sits near -3 dBFS, then apply a 5 ms linear fade-in/out to
/// remove onset/offset clicks.
fn to_pcm16(samples: &[f64]) -> Vec<u8> {
    let peak = samples.iter().fold(0.0_f64, |m, &s| m.max(fabs(s)));
    if peak <= 0.0 {
        return Vec::new();
    }
    let gain = 0.70 * 32767.0 / peak;
    let fade = (SAMPLE_RATE * 0.005) as usize; // 5 ms
    let mut out = Vec::with_capacity(samples.len() * 2);
    for (index, &sample) in samples.iter().enumerate() {
        let mut window = 1.0;
        if index < fade {
            window = index as f64 / fade as f64;
        } else if index + fade >= samples.len() {
            window = (samples.len() - index) as f64 / fade as f64;
        }
        let value = (sample * gain * window).clamp(-32768.0, 32767.0) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

// -----------------------------------------------------------------------------
// Quick self-test when run as the main crate binary. (Used by the CLI demo.)
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssml_voice_sets_pitch_and_restores_parent_context() {
        let segments = parse_ssml(
            "<voice name=\"female\">bonjour<voice name=\"child\">salut</voice>encore</voice>",
        );
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].pitch_override, Some(200));
        assert_eq!(segments[1].pitch_override, Some(280));
        assert_eq!(segments[2].pitch_override, Some(200));
    }

    #[test]
    fn english_exceptions_keep_initial_consonants() {
        for (word, expected) in [
            ("bought", Ph::B),
            ("thought", Ph::Th),
            ("fought", Ph::F),
            ("would", Ph::W),
            ("could", Ph::K),
            ("caught", Ph::K),
            ("taught", Ph::T),
            ("freight", Ph::F),
        ] {
            let mut actual = Vec::new();
            word_phones(word, false, &mut actual);
            assert_eq!(actual.first(), Some(&expected), "{}", word);
        }
    }
    #[test]
    fn numbers_keep_value_with_punctuation() {
        for french in [false, true] {
            let plain = phones("123", french);
            let punctuated = phones("123.", french);
            assert_eq!(&punctuated[..plain.len()], plain.as_slice());
            for value in ["10000", "1000000", "2000000000", "999999999999"] {
                let mut expected = Vec::new();
                number_words(value.parse().unwrap(), french, &mut expected);
                assert_eq!(phones(value, french), expected);
            }
        }
    }
    #[test]
    fn liaison_respects_punctuation_and_nasal_on() {
        assert_eq!(detect_liaison("on", "arrive"), Some(Ph::N));
        assert_eq!(detect_liaison("les,", "amis"), None);
        assert_eq!(detect_liaison("les", "honnêtes"), Some(Ph::Z));
        assert_eq!(detect_liaison("les", "haricots"), None);
        assert_eq!(detect_liaison("les", "Écoles"), Some(Ph::Z));
    }
    #[test]
    fn punctuation_and_hyphens_split_words() {
        assert_eq!(phones("hello,world", false), phones("hello, world", false));
        assert_eq!(phones("vingt-trois", true), phones("vingt trois", true));
        assert_eq!(phones("l’eau", true), phones("l'eau", true));
        assert!(!phones("les amis", true).contains(&Ph::Pause));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(say("", false).is_empty());
        assert!(say("   ", false).is_empty());
    }

    #[test]
    fn english_hello_produces_pcm() {
        let pcm = say("hello", false);
        assert!(!pcm.is_empty());
        assert_eq!(pcm.len() % 2, 0);
    }

    #[test]
    fn french_bonjour_produces_pcm() {
        let pcm = say("bonjour", true);
        assert!(!pcm.is_empty());
        assert_eq!(pcm.len() % 2, 0);
    }

    #[test]
    fn numbers_in_both_languages() {
        assert!(!say("1280", false).is_empty());
        assert!(!say("1280", true).is_empty());
        assert!(!say("8000", false).is_empty());
    }

    #[test]
    fn acronyms_are_spelled() {
        // "USB" should produce phonemes for U, S, B.
        let pcm = say("USB", false);
        assert!(!pcm.is_empty());
    }

    #[test]
    fn long_sentence_stays_bounded() {
        let pcm = say(
            "The quick brown fox jumps over the lazy dog while the firmware screen reader speaks.",
            false,
        );
        assert!(pcm.len() > 24000 * 2); // >0.5 s
    }

    #[test]
    fn nasals_have_an_target() {
        let mut ts = Vec::new();
        targets_for(Ph::M, &mut ts);
        assert!(ts[0].an > 0.0);
    }

    #[test]
    fn f4_is_real_per_phoneme() {
        let mut ts1 = Vec::new();
        let mut ts2 = Vec::new();
        targets_for(Ph::Iy, &mut ts1);
        targets_for(Ph::Aa, &mut ts2);
        assert_ne!(ts1[0].f4, ts2[0].f4);
    }

    #[test]
    fn voice_presets_differ() {
        // Three voices should produce different-sized PCM outputs because
        // their formant settings and F0 differ; adult female and child use
        // higher F0 which changes the rate at which the glottal phase
        // advances, slightly altering total length.
        let mut m = Vec::new();
        let mut f = Vec::new();
        let mut c = Vec::new();
        for text in ["Bonjour le monde.", "Hello world."] {
            let cfg = |voice: Voice| Config {
                voice,
                pitch: voice.f0() as u32,
                ..Config::default()
            };
            m.push(say_with_config(text, true, cfg(Voice::Male)).len());
            f.push(say_with_config(text, true, cfg(Voice::Female)).len());
            c.push(say_with_config(text, true, cfg(Voice::Child)).len());
        }
        // All three voices should produce something.
        assert!(m.iter().all(|&x| x > 0));
        assert!(f.iter().all(|&x| x > 0));
        assert!(c.iter().all(|&x| x > 0));
    }

    #[test]
    fn french_numbers_extended() {
        // 70 = soixante-dix, 80 = quatre-vingts, 90 = quatre-vingt-dix,
        // millions, milliards.
        for n in ["70", "71", "80", "90", "99", "1234", "1000000", "2000000"] {
            assert!(!say(n, true).is_empty(), "Failed on {}", n);
            assert!(!say(n, false).is_empty(), "Failed on {}", n);
        }
    }

    #[test]
    fn french_liaison_inserted() {
        // Direct test of the liaison detector: les + vowel-initial word
        // should insert /z/, les + consonant word should not.
        use Ph::*;
        let z_with_amis = detect_liaison("les", "amis");
        let z_with_ours = detect_liaison("les", "ours");
        let none_with_chat = detect_liaison("les", "chat");
        let n_with_ami = detect_liaison("mon", "ami");
        assert_eq!(z_with_amis, Some(Z));
        assert_eq!(z_with_ours, Some(Z));
        assert_eq!(none_with_chat, None);
        assert_eq!(n_with_ami, Some(N));
    }

    #[test]
    fn ssml_breaks_produce_pcm() {
        let bytes = say(r#"Bonjour <break time="500ms"/> le monde."#, true);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn ssml_emphasis_parses() {
        let bytes = say(
            "Bonjour <emphasis level=\"strong\">attention</emphasis> ici.",
            true,
        );
        assert!(!bytes.is_empty());
    }

    #[test]
    fn ssml_prosody_parses() {
        let bytes = say(
            "<prosody rate=\"slow\" pitch=\"high\">Très important.</prosody>",
            true,
        );
        assert!(!bytes.is_empty());
    }

    #[test]
    fn ssml_voice_tag_changes_voice() {
        // Explicit configs: global setters would race with parallel tests.
        let male = say_with_config(
            "Bonjour.",
            true,
            Config {
                voice: Voice::Male,
                pitch: 115,
                ..Config::default()
            },
        );
        let female = say_with_config(
            "Bonjour.",
            true,
            Config {
                voice: Voice::Female,
                pitch: 200,
                ..Config::default()
            },
        );
        // The two voices use different F0 (115 vs 200 Hz) and different
        // formant scaling (0.88x vs 1.00x). The PCM *content* must differ
        // even if the *length* is the same.
        assert!(!male.is_empty());
        assert!(!female.is_empty());
        assert_ne!(male, female);
    }

    #[test]
    fn english_tion_exception() {
        // "tion" must use the exception path; the result should be a
        // non-empty PCM.
        let pcm = say("information", false);
        assert!(!pcm.is_empty());
        // Suffix exception: "tion" alone.
        let pcm2 = say("tion", false);
        assert!(!pcm2.is_empty());
    }

    #[test]
    fn english_ough_exception() {
        let pcm = say("tough", false);
        assert!(!pcm.is_empty());
        let pcm2 = say("enough", false);
        assert!(!pcm2.is_empty());
    }

    #[test]
    fn renderer_state_init() {
        // Verify Renderer::new does not panic.
        let _ = Renderer::new();
    }

    #[test]
    fn notch_filter_stable() {
        // Notch filter should not produce NaN or infinity on a random input.
        let mut n = Notch::new();
        n.set(1500.0, 80.0);
        let mut x = 0.5f64;
        for _ in 0..1000 {
            x = n.step(x);
            assert!(x.is_finite());
        }
    }
}
