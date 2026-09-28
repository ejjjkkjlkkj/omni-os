//! A from-scratch formant speech synthesizer for the firmware stage - real spoken
//! words for *dynamic* text, before any operating system and with no voice assets.
//!
//! Everything the firmware screen reader says in fixed wording is a pre-recorded clip
//! ([`crate::hda`]). But the interesting text at this stage is dynamic and cannot be
//! recorded ahead of time: the enumerated boot-device names, the CPU brand string, memory
//! sizes, resolutions, firmware settings and their values. Until now those were only
//! *spelled* letter by letter. This turns them into speech.
//!
//! The design is a Klatt-style cascade formant synthesizer (D. Klatt, "Software for a
//! cascade/parallel formant synthesizer", JASA 67, 1980), small enough to run in soft-float
//! under UEFI and emitting the exact format the audio backends already play - 24 kHz mono
//! 16-bit PCM - so a synthesized utterance is spoken through the same HDA/AC'97 DMA path as a
//! recorded clip, with nothing new below it. Three stages:
//!
//! 1. **Grapheme to phoneme.** [`phones`] turns text into a phoneme sequence with a compact
//!    English letter-to-sound rule set, letter names for acronyms (all-caps tokens are spelled,
//!    so "USB" becomes U S B), and number-to-words in English and French (1280 becomes "one
//!    thousand two hundred eighty"), which is what most firmware values are.
//! 2. **Phoneme to formant targets.** Each phoneme maps to one or more [`Target`]s - the
//!    formant frequencies, bandwidths and source amplitudes that define it.
//! 3. **Formant synthesis.** [`render`] excites a four-formant cascade with a Rosenberg
//!    glottal pulse (differentiated for lip radiation and gently spectrally tilted) rather
//!    than a bare impulse, adds high-passed band-passed frication, slews the formants between
//!    targets so transitions coarticulate, and applies pitch declination, micro-jitter and
//!    output smoothing before auto-levelling to a clean 16-bit signal. The glottal pulse,
//!    tilt and smoothing are what make it a voice rather than a buzz.
//!
//! Honest scope: a compact formant synthesizer is intelligible, not natural - it sounds
//! robotic, like early DECtalk. That is the right trade at the firmware stage, where the goal
//! is that a blind user can *understand* a device name or value they otherwise could not hear
//! at all. English grapheme rules drive the word path in English; in French, a full French
//! grapheme-to-phoneme frontend and phoneme inventory - nasal vowels and all - ported from the
//! Sintaise UEFI TTS drive it instead, so the setup's French labels and values are pronounced,
//! not spelled.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use libm::{cos, exp};

/// Output sample rate, matching the 24 kHz mono PCM the audio backends stream.
const SAMPLE_RATE: f64 = 24000.0;

/// Speech rate as a percentage of the base rate (100 = normal). Higher is faster (shorter
/// phoneme durations). Adjusted from the setup so a practiced user can speed the voice up.
static RATE_PERCENT: AtomicU32 = AtomicU32::new(100);
/// Voice pitch (glottal fundamental) in hertz. A moderate male-range default.
static PITCH_HZ: AtomicU32 = AtomicU32::new(115);

/// Raise the speech rate one step, capped, and return the new percentage.
pub fn rate_up() -> u32 {
    let next = (RATE_PERCENT.load(Ordering::Relaxed) + 25).min(250);
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
    let next = (PITCH_HZ.load(Ordering::Relaxed) + 15).min(255);
    PITCH_HZ.store(next, Ordering::Relaxed);
    next
}

/// Lower the voice pitch one step and return the new fundamental in hertz.
pub fn pitch_down() -> u32 {
    let next = PITCH_HZ.load(Ordering::Relaxed).saturating_sub(15).max(70);
    PITCH_HZ.store(next, Ordering::Relaxed);
    next
}

// ---- Phonemes ------------------------------------------------------------------

/// The phoneme inventory (a compact ARPABET subset). Diphthongs and affricates are their own
/// symbols and expand to two formant targets in [`targets_for`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ph {
    // Monophthong vowels.
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
    // Diphthongs.
    Ey,
    Ay,
    Oy,
    Aw,
    Ow,
    // Nasals.
    M,
    N,
    Ng,
    // Approximants / liquids.
    L,
    R,
    W,
    Y,
    // Fricatives.
    F,
    V,
    S,
    Z,
    Sh,
    Zh,
    Th,
    Dh,
    Hh,
    // Affricates.
    Ch,
    Jh,
    // Stops.
    P,
    B,
    T,
    D,
    K,
    G,
    // French-specific phonemes, so French dynamic text is pronounced rather than spelled. The
    // French frontend and this inventory are ported from the Sintaise UEFI TTS (a companion
    // French formant synthesizer): the oral vowels /a e o y ø œ ə/, the four nasal vowels, and
    // the palatal nasal. French /ɛ ɔ i u/ reuse the close English vowels above (Eh, Ao, Iy, Uw).
    FrA,      // /a/ - central, as in "patte"
    FrEClose, // /e/ - "été"
    FrOClose, // /o/ - "beau"
    FrY,      // /y/ - "tu" (front rounded)
    Eu,       // /ø/ - "deux"
    EuOpen,   // /œ/ - "neuf"
    Schwa,    // /ə/ - "le"
    Nan,      // /ɑ̃/ - "an"
    Non,      // /ɔ̃/ - "on"
    Nin,      // /ɛ̃/ - "in"
    Nun,      // /œ̃/ - "un"
    Ny,       // /ɲ/ - "gn"
    // A short pause (word/segment boundary).
    Pause,
}

/// One synthesis segment: the formant targets and source amplitudes held (and slewed toward)
/// for `dur_ms` milliseconds. `av` is the voiced (glottal) source level, `af` the frication
/// noise level, `fc`/`fbw` the frication band-pass centre and bandwidth.
#[derive(Clone, Copy)]
struct Target {
    f1: f64,
    f2: f64,
    f3: f64,
    bw1: f64,
    bw2: f64,
    bw3: f64,
    av: f64,
    af: f64,
    fc: f64,
    fbw: f64,
    dur_ms: f64,
}

impl Target {
    /// A voiced segment (vowel, nasal, approximant): glottal source through the three formants,
    /// no frication.
    const fn voiced(f1: f64, f2: f64, f3: f64, av: f64, dur_ms: f64) -> Self {
        Self {
            f1,
            f2,
            f3,
            bw1: 80.0,
            bw2: 90.0,
            bw3: 120.0,
            av,
            af: 0.0,
            fc: 0.0,
            fbw: 0.0,
            dur_ms,
        }
    }

    /// A frication segment: band-passed noise, optionally with a low voiced murmur (`av > 0`)
    /// for voiced fricatives. The vowel formants are held at neutral so a following vowel
    /// coarticulates from a sensible place.
    const fn fric(fc: f64, fbw: f64, av: f64, af: f64, dur_ms: f64) -> Self {
        Self {
            f1: 500.0,
            f2: 1500.0,
            f3: 2500.0,
            bw1: 100.0,
            bw2: 120.0,
            bw3: 150.0,
            av,
            af,
            fc,
            fbw,
            dur_ms,
        }
    }

    /// A silence (stop closure, pause): no source at all, formants held so a stop's release
    /// transitions cleanly into the next vowel.
    const fn silence(dur_ms: f64) -> Self {
        Self {
            f1: 500.0,
            f2: 1500.0,
            f3: 2500.0,
            bw1: 100.0,
            bw2: 120.0,
            bw3: 150.0,
            av: 0.0,
            af: 0.0,
            fc: 0.0,
            fbw: 0.0,
            dur_ms,
        }
    }
}

/// The formant targets for one phoneme. Vowel formant values are the classic measured
/// centres; diphthongs emit a glide (two targets); stops emit a closure then a burst.
fn targets_for(ph: Ph, out: &mut Vec<Target>) {
    // Standard vowel amplitude; nasals and approximants are a little quieter.
    const AV: f64 = 1.0;
    match ph {
        Ph::Aa => out.push(Target::voiced(730.0, 1090.0, 2440.0, AV, 140.0)),
        Ph::Ae => out.push(Target::voiced(660.0, 1720.0, 2410.0, AV, 150.0)),
        Ph::Ah => out.push(Target::voiced(640.0, 1190.0, 2390.0, AV, 110.0)),
        Ph::Ao => out.push(Target::voiced(570.0, 840.0, 2410.0, AV, 140.0)),
        Ph::Eh => out.push(Target::voiced(530.0, 1840.0, 2480.0, AV, 130.0)),
        Ph::Er => out.push(Target::voiced(490.0, 1350.0, 1690.0, AV, 150.0)),
        Ph::Ih => out.push(Target::voiced(390.0, 1990.0, 2550.0, AV, 110.0)),
        Ph::Iy => out.push(Target::voiced(270.0, 2290.0, 3010.0, AV, 130.0)),
        Ph::Uh => out.push(Target::voiced(440.0, 1020.0, 2240.0, AV, 110.0)),
        Ph::Uw => out.push(Target::voiced(300.0, 870.0, 2240.0, AV, 140.0)),
        // Diphthongs: start target then glide target.
        Ph::Ey => {
            out.push(Target::voiced(530.0, 1840.0, 2480.0, AV, 90.0));
            out.push(Target::voiced(270.0, 2290.0, 3010.0, AV, 90.0));
        }
        Ph::Ay => {
            out.push(Target::voiced(730.0, 1090.0, 2440.0, AV, 100.0));
            out.push(Target::voiced(270.0, 2290.0, 3010.0, AV, 90.0));
        }
        Ph::Oy => {
            out.push(Target::voiced(570.0, 840.0, 2410.0, AV, 110.0));
            out.push(Target::voiced(270.0, 2290.0, 3010.0, AV, 90.0));
        }
        Ph::Aw => {
            out.push(Target::voiced(730.0, 1090.0, 2440.0, AV, 100.0));
            out.push(Target::voiced(300.0, 870.0, 2240.0, AV, 90.0));
        }
        Ph::Ow => {
            out.push(Target::voiced(570.0, 840.0, 2410.0, AV, 100.0));
            out.push(Target::voiced(300.0, 870.0, 2240.0, AV, 90.0));
        }
        // Nasals: low first formant plus a nasal murmur; second formant sets the place.
        Ph::M => out.push(Target::voiced(250.0, 1100.0, 2300.0, 0.7, 90.0)),
        Ph::N => out.push(Target::voiced(250.0, 1700.0, 2600.0, 0.7, 90.0)),
        Ph::Ng => out.push(Target::voiced(250.0, 2300.0, 2900.0, 0.7, 90.0)),
        // Approximants / liquids.
        Ph::L => out.push(Target::voiced(360.0, 1300.0, 3000.0, 0.8, 80.0)),
        Ph::R => out.push(Target::voiced(490.0, 1350.0, 1690.0, 0.8, 80.0)),
        Ph::W => out.push(Target::voiced(300.0, 610.0, 2200.0, 0.8, 70.0)),
        Ph::Y => out.push(Target::voiced(270.0, 2290.0, 3010.0, 0.8, 60.0)),
        // Fricatives: band-passed noise. Voiced ones (V, Z, Dh, Zh) add a low murmur.
        Ph::F => out.push(Target::fric(1400.0, 1500.0, 0.0, 0.5, 90.0)),
        Ph::V => out.push(Target::fric(1400.0, 1500.0, 0.25, 0.4, 70.0)),
        Ph::Th => out.push(Target::fric(1600.0, 1400.0, 0.0, 0.4, 90.0)),
        Ph::Dh => out.push(Target::fric(1600.0, 1400.0, 0.25, 0.35, 70.0)),
        Ph::S => out.push(Target::fric(5500.0, 1800.0, 0.0, 0.7, 100.0)),
        Ph::Z => out.push(Target::fric(5500.0, 1800.0, 0.25, 0.55, 80.0)),
        Ph::Sh => out.push(Target::fric(2600.0, 1400.0, 0.0, 0.7, 100.0)),
        Ph::Zh => out.push(Target::fric(2600.0, 1400.0, 0.25, 0.55, 80.0)),
        Ph::Hh => out.push(Target::fric(1000.0, 2500.0, 0.0, 0.35, 70.0)),
        // Affricates: a brief stop closure then a fricative release.
        Ph::Ch => {
            out.push(Target::silence(50.0));
            out.push(Target::fric(2600.0, 1400.0, 0.0, 0.7, 70.0));
        }
        Ph::Jh => {
            out.push(Target::silence(40.0));
            out.push(Target::fric(2600.0, 1400.0, 0.25, 0.55, 70.0));
        }
        // Stops: a silent closure then a short noise burst at the place of articulation.
        // Voiced stops (B, D, G) carry a low voice bar through the closure.
        Ph::P => {
            out.push(Target::silence(60.0));
            out.push(Target::fric(1000.0, 900.0, 0.0, 0.5, 15.0));
        }
        Ph::B => {
            out.push(Target::voiced(200.0, 1000.0, 2300.0, 0.2, 50.0));
            out.push(Target::fric(1000.0, 900.0, 0.0, 0.4, 12.0));
        }
        Ph::T => {
            out.push(Target::silence(60.0));
            out.push(Target::fric(3800.0, 1600.0, 0.0, 0.55, 15.0));
        }
        Ph::D => {
            out.push(Target::voiced(200.0, 1700.0, 2600.0, 0.2, 45.0));
            out.push(Target::fric(3800.0, 1600.0, 0.0, 0.45, 12.0));
        }
        Ph::K => {
            out.push(Target::silence(60.0));
            out.push(Target::fric(2000.0, 1400.0, 0.0, 0.55, 18.0));
        }
        Ph::G => {
            out.push(Target::voiced(200.0, 2000.0, 2500.0, 0.2, 45.0));
            out.push(Target::fric(2000.0, 1400.0, 0.0, 0.45, 12.0));
        }
        // French oral vowels (classic French formant centres). Nasal vowels carry a slightly
        // lower amplitude to hint the nasal murmur, the same cue the nasal consonants use.
        Ph::FrA => out.push(Target::voiced(750.0, 1350.0, 2500.0, AV, 120.0)),
        Ph::FrEClose => out.push(Target::voiced(400.0, 2100.0, 2600.0, AV, 110.0)),
        Ph::FrOClose => out.push(Target::voiced(400.0, 800.0, 2600.0, AV, 120.0)),
        Ph::FrY => out.push(Target::voiced(300.0, 1800.0, 2200.0, AV, 120.0)),
        Ph::Eu => out.push(Target::voiced(400.0, 1500.0, 2300.0, AV, 120.0)),
        Ph::EuOpen => out.push(Target::voiced(560.0, 1500.0, 2400.0, AV, 110.0)),
        Ph::Schwa => out.push(Target::voiced(500.0, 1500.0, 2500.0, 0.9, 90.0)),
        Ph::Nan => out.push(Target::voiced(650.0, 1000.0, 2500.0, 0.85, 130.0)),
        Ph::Non => out.push(Target::voiced(450.0, 900.0, 2500.0, 0.85, 130.0)),
        Ph::Nin => out.push(Target::voiced(560.0, 1600.0, 2500.0, 0.85, 130.0)),
        Ph::Nun => out.push(Target::voiced(500.0, 1400.0, 2400.0, 0.85, 130.0)),
        Ph::Ny => out.push(Target::voiced(300.0, 1900.0, 2600.0, 0.7, 90.0)),
        Ph::Pause => out.push(Target::silence(70.0)),
    }
}

// ---- Grapheme to phoneme -------------------------------------------------------

/// English phoneme spelling of each letter's *name*, for reading acronyms (all-caps tokens)
/// and any character that falls through the word rules.
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

/// French phoneme spelling of each letter's name, for the French voice.
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

/// English phonemes for a single decimal digit's name.
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

/// French phonemes for a single decimal digit's name.
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

/// Append the phonemes for a run of digits. Short runs (<= 4 digits, e.g. a memory size or a
/// resolution) are read as a whole number in words; longer runs (serial numbers) are read
/// digit by digit, which is how a person reads an identifier aloud.
fn number_phones(digits: &str, french: bool, out: &mut Vec<Ph>) {
    let value: u64 = digits.parse().unwrap_or(u64::MAX);
    if digits.len() <= 4 && value != u64::MAX {
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

/// Append the phonemes that name `value` (0..=9999) as words, in English or French - the form
/// a firmware size, count or resolution is naturally spoken in.
fn number_words(value: u64, french: bool, out: &mut Vec<Ph>) {
    if french {
        // French number formation is irregular enough (soixante-dix, quatre-vingts) that it has
        // its own path, ported from the Sintaise French frontend.
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
    let thousands = (value / 1000) % 10;
    let hundreds = (value / 100) % 10;
    let remainder = value % 100;
    if thousands > 0 {
        digit(thousands as u8, out);
        // "thousand" / "mille".
        out.extend_from_slice(if french {
            &[Ph::M, Ph::Iy, Ph::L]
        } else {
            &[Ph::Th, Ph::Aw, Ph::Z, Ph::Ah, Ph::N, Ph::D]
        });
        out.push(Ph::Pause);
    }
    if hundreds > 0 {
        digit(hundreds as u8, out);
        // "hundred" / "cent".
        out.extend_from_slice(if french {
            &[Ph::S, Ph::Aa, Ph::N]
        } else {
            &[Ph::Hh, Ph::Ah, Ph::N, Ph::D, Ph::R, Ph::Ah, Ph::D]
        });
        out.push(Ph::Pause);
    }
    tens_unit(remainder as u8, french, out);
}

/// Append the phonemes for a two-digit remainder (0..=99) as words.
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
    // The teens and tens are irregular; spell them from small word tables. To keep the tables
    // compact and the voice intelligible rather than perfectly idiomatic, 20..99 are read as
    // "<tens-word> <unit>" in English and digit-compounded in French for the awkward ranges.
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
        // French: read the two digits (e.g. "72" -> "sept deux") for the irregular ranges - not
        // idiomatic, but unambiguous for a firmware value and far smaller than the full rules.
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

/// Whether a token is an acronym to spell out: all its letters are upper-case and it is short.
/// Firmware strings are full of these ("USB", "EFI", "AHCI", "QM00001").
fn is_acronym(token: &str) -> bool {
    let letters: Vec<char> = token.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    !letters.is_empty() && letters.len() <= 5 && letters.iter().all(|c| c.is_ascii_uppercase())
}

/// Turn `text` into a phoneme sequence. Tokens are split on spaces; each is a number (read as
/// words or digits), an acronym (spelled by letter name), or a word (English letter-to-sound
/// rules, or French letter names when `french` and the word is not English-pronounceable is a
/// judgement we do not make - English rules are used for Latin words either way, since firmware
/// values are overwhelmingly English/Latin).
fn phones(text: &str, french: bool) -> Vec<Ph> {
    let mut out = Vec::new();
    for (index, token) in text.split_whitespace().enumerate() {
        if index > 0 {
            out.push(Ph::Pause);
        }
        if token.chars().all(|c| c.is_ascii_digit()) && !token.is_empty() {
            number_phones(token, french, &mut out);
        } else if is_acronym(token) {
            for c in token.chars() {
                if c.is_ascii_alphanumeric() {
                    spell_char(c, french, &mut out);
                    out.push(Ph::Pause);
                }
            }
        } else if french {
            word_phones_fr(token, &mut out);
        } else {
            word_phones(token, french, &mut out);
        }
    }
    out
}

/// Append the phonemes for one character read by name (letter or digit), in the active language.
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

/// Append the phonemes for one word using a compact English letter-to-sound rule set. It reads
/// left to right, consuming multi-letter graphemes (sh, ch, th, ph, ck, ng, qu, ...) and
/// applying the "magic e" long-vowel rule, then falls back to the letter's common sound. It is
/// deliberately small: enough to make English device and setting names intelligible, not a
/// complete pronunciation dictionary.
fn word_phones(word: &str, french: bool, out: &mut Vec<Ph>) {
    let chars: Vec<char> = word
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if chars.is_empty() {
        // Punctuation-only token: read any digits, otherwise skip.
        for c in word.chars() {
            if c.is_ascii_digit() {
                spell_char(c, french, out);
            }
        }
        return;
    }
    let n = chars.len();
    // A word with no vowel letter is almost certainly an abbreviation; spell it.
    if !chars.iter().any(|c| "aeiouy".contains(*c)) {
        for &c in &chars {
            spell_char(c, french, out);
            out.push(Ph::Pause);
        }
        return;
    }

    use Ph::*;
    let at = |i: usize| chars.get(i).copied().unwrap_or(' ');
    let is_vowel = |c: char| "aeiou".contains(c);
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let next = at(i + 1);
        let next2 = at(i + 2);
        match c {
            'a' => {
                // "magic e": a consonant then a final 'e' makes the vowel long ("gate").
                if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    out.push(Ey);
                } else if next == 'i' || next == 'y' {
                    out.push(Ey);
                    i += 1;
                } else if next == 'w' || (next == 'u' && !is_vowel(next2)) {
                    out.push(Ao);
                    i += 1;
                } else if next == 'r' {
                    out.push(Aa);
                } else {
                    out.push(Ae);
                }
            }
            'e' => {
                if i == n - 1 && n > 2 {
                    // Final silent 'e' after a consonant: usually silent.
                } else if next == 'e' || next == 'a' {
                    out.push(Iy);
                    i += 1;
                } else if next == 'i' || next == 'y' {
                    out.push(Ey);
                    i += 1;
                } else if next == 'r' {
                    out.push(Er);
                } else if next == 'w' {
                    out.push(Uw);
                    i += 1;
                } else {
                    out.push(Eh);
                }
            }
            'i' => {
                if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    out.push(Ay);
                } else if next == 'g' && next2 == 'h' {
                    out.push(Ay);
                    i += 2;
                } else if next == 'r' {
                    out.push(Er);
                } else {
                    out.push(Ih);
                }
            }
            'o' => {
                if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    out.push(Ow);
                } else if next == 'o' {
                    out.push(Uw);
                    i += 1;
                } else if next == 'w' || next == 'u' {
                    out.push(Aw);
                    i += 1;
                } else if next == 'i' || next == 'y' {
                    out.push(Oy);
                    i += 1;
                } else if next == 'r' {
                    out.push(Ao);
                } else {
                    out.push(Aa);
                }
            }
            'u' => {
                if i + 2 < n && !is_vowel(next) && next2 == 'e' && i + 3 == n {
                    out.push(Y);
                    out.push(Uw);
                } else if next == 'r' {
                    out.push(Er);
                } else {
                    out.push(Ah);
                }
            }
            'y' => {
                if i == 0 {
                    out.push(Y);
                } else if i == n - 1 {
                    out.push(Iy);
                } else {
                    out.push(Ih);
                }
            }
            // Consonant digraphs first, then single consonants.
            's' if next == 'h' => {
                out.push(Sh);
                i += 1;
            }
            'c' if next == 'h' => {
                out.push(Ch);
                i += 1;
            }
            't' if next == 'h' => {
                // Word-initial "th" is usually voiced (the, this, they); elsewhere unvoiced.
                out.push(if i == 0 { Dh } else { Th });
                i += 1;
            }
            'p' if next == 'h' => {
                out.push(F);
                i += 1;
            }
            'g' if next == 'h' => {
                // "gh" is usually silent (night) or /f/ (rough); treat as silent here.
                i += 1;
            }
            'c' if next == 'k' => {
                out.push(K);
                i += 1;
            }
            'n' if next == 'g' && i + 2 == n => {
                out.push(Ng);
                i += 1;
            }
            'q' => {
                out.push(K);
                if next == 'u' {
                    out.push(W);
                    i += 1;
                }
            }
            'c' => {
                // Soft c before e/i/y, else hard.
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
            'k' => out.push(K),
            'l' => out.push(L),
            'm' => out.push(M),
            'n' => out.push(N),
            'p' => out.push(P),
            'r' => out.push(R),
            's' => {
                let prev_vowel = i > 0 && "aeiouy".contains(at(i - 1));
                let rest: String = chars[i + 1..].iter().collect();
                if prev_vowel && (rest.starts_with("ion") || rest.starts_with("ure")) {
                    // "-sion"/"-sure": the /ʒ/ of "television", "measure".
                    out.push(Zh);
                } else if prev_vowel && is_vowel(next) {
                    // Intervocalic single "s" voices to /z/ ("laser", "reason").
                    out.push(Z);
                } else {
                    out.push(S);
                }
            }
            't' => out.push(T),
            'v' => out.push(V),
            'w' => out.push(W),
            'z' => out.push(Z),
            _ => {}
        }
        // Skip a doubled consonant so "ll", "ss", "tt" sound once.
        if i + 1 < n && chars[i + 1] == c && !is_vowel(c) {
            i += 1;
        }
        i += 1;
    }
}

/// Whether `c` is a French vowel letter (including the accented forms).
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

/// French grapheme-to-phoneme, ported from the Sintaise UEFI TTS French frontend. It handles the
/// French digraphs and trigraphs (eau, oi, ou, au, ai, eu, tion, sion, gn, ill, ...), nasal
/// vowels in context, the accented letters, and a small exceptions lexicon for very frequent
/// words, so French dynamic text is pronounced as words rather than spelled. Prosody and
/// cross-word liaison from the original are omitted (this synth voices one token at a time).
fn word_phones_fr(word: &str, out: &mut Vec<Ph>) {
    use Ph::*;
    let chars: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    let n = chars.len();
    if n == 0 {
        return;
    }
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    // Match the ASCII pattern `s` at position `i`.
    let m = |i: usize, s: &str| s.bytes().enumerate().all(|(k, b)| at(i + k) == b as char);
    let ends = |i: usize, s: &str| m(i, s) && i + s.len() == n;
    // A following consonant (or end of word) makes a preceding vowel+n/m nasal; a following
    // vowel or a doubled n/m does not.
    let nasal = |i: usize, consumed: usize| -> bool {
        let j = i + consumed;
        if j >= n {
            return true;
        }
        let c = chars[j];
        c != 'n' && c != 'm' && !is_vowel_fr(c)
    };

    // A small lexicon of very frequent words the graphical rules get wrong.
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
        } else if i + 1 == n && matches!(c, 'e' | 's' | 'x' | 'z' | 'd' | 't' | 'p' | 'g') {
            // A silent final consonant (or mute e): common in French.
            i += 1;
        } else if c == 'h' {
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

/// French number-to-words (0..=9999), ported from the Sintaise frontend, then voiced through the
/// French grapheme rules so the irregular forms (soixante-dix, quatre-vingts) come out right.
fn number_words_fr(value: u64, out: &mut Vec<Ph>) {
    if value == 0 {
        word_phones_fr("zéro", out);
        return;
    }
    let mut v = value;
    if v >= 1000 {
        let thousands = v / 1000;
        if thousands > 1 {
            number_under_1000_fr(thousands, out);
            out.push(Ph::Pause);
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

// ---- Formant synthesis ---------------------------------------------------------

/// A two-pole formant resonator (Klatt): `y[n] = a*x[n] + b*y[n-1] + c*y[n-2]`, with
/// coefficients set from a centre frequency and bandwidth. Unity gain at the centre.
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

    /// Set the resonator to `freq`/`bw` (both hertz) at the output sample rate.
    fn set(&mut self, freq: f64, bw: f64) {
        let c = -exp(-2.0 * core::f64::consts::PI * bw / SAMPLE_RATE);
        let b = 2.0
            * exp(-core::f64::consts::PI * bw / SAMPLE_RATE)
            * cos(2.0 * core::f64::consts::PI * freq / SAMPLE_RATE);
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

/// A Rosenberg glottal-flow pulse over one pitch period `phase` in `0.0..1.0`: a smooth rise
/// (opening), a fall (closing), then a closed rest. Feeding its *derivative* through the
/// formants, rather than a bare impulse, is what turns a buzzy robot into a voice - the pulse
/// has the natural -12 dB/octave spectral rolloff a vocal fold produces.
fn glottal_flow(phase: f64) -> f64 {
    // Open quotient ~0.6: opening then closing fractions of the period.
    const TP: f64 = 0.44;
    const TN: f64 = 0.16;
    if phase < TP {
        0.5 * (1.0 - cos(core::f64::consts::PI * phase / TP))
    } else if phase < TP + TN {
        cos(core::f64::consts::PI * (phase - TP) / (2.0 * TN))
    } else {
        0.0
    }
}

/// Render a phoneme's targets into PCM samples appended to `buf` (as `f64`, levelled later).
/// Formants slew toward each target so segments coarticulate. The voiced source is a Rosenberg
/// glottal pulse differentiated for lip radiation and gently spectrally tilted; frication is
/// high-passed band-passed noise. A four-formant cascade, output smoothing, pitch declination
/// and micro-jitter give a voice rather than a buzz.
struct Renderer {
    f1: f64,
    f2: f64,
    f3: f64,
    r1: Resonator,
    r2: Resonator,
    r3: Resonator,
    /// A fixed high fourth formant that fills in the upper spectrum for a fuller timbre.
    r4: Resonator,
    rf: Resonator,
    glottal_phase: f64,
    rng: u32,
    /// Previous glottal-flow value, for the radiation differentiator.
    prev_flow: f64,
    /// Source spectral-tilt low-pass state.
    src_lp: f64,
    /// Previous raw noise value, for the fricative high-pass.
    prev_noise: f64,
    /// Output smoothing low-pass state.
    out_lp: f64,
}

impl Renderer {
    fn new() -> Self {
        let mut r4 = Resonator::new();
        r4.set(3300.0, 250.0);
        Self {
            f1: 500.0,
            f2: 1500.0,
            f3: 2500.0,
            r1: Resonator::new(),
            r2: Resonator::new(),
            r3: Resonator::new(),
            r4,
            rf: Resonator::new(),
            glottal_phase: 0.0,
            rng: 0x1234_5678,
            prev_flow: 0.0,
            src_lp: 0.0,
            prev_noise: 0.0,
            out_lp: 0.0,
        }
    }

    /// One white-noise sample in `-1.0..1.0` from a fast xorshift PRNG.
    fn noise(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f64 / u32::MAX as f64) * 2.0 - 1.0
    }

    fn render(&mut self, targets: &[Target], f0: f64, rate_percent: u32, buf: &mut Vec<f64>) {
        let rate_scale = 100.0 / rate_percent as f64;
        // Slew coefficient: reach a new formant target in ~35 ms (natural transition speed).
        let slew = 1.0 - exp(-1.0 / (0.035 * SAMPLE_RATE));
        // Total voiced length, for a gentle pitch declination across the utterance.
        let total: usize = targets
            .iter()
            .map(|t| ((t.dur_ms * rate_scale) / 1000.0 * SAMPLE_RATE) as usize)
            .sum::<usize>()
            .max(1);
        let mut global = 0usize;
        for target in targets {
            let samples = ((target.dur_ms * rate_scale) / 1000.0 * SAMPLE_RATE) as usize;
            for index in 0..samples {
                self.f1 += (target.f1 - self.f1) * slew;
                self.f2 += (target.f2 - self.f2) * slew;
                self.f3 += (target.f3 - self.f3) * slew;
                // Update the (expensive, soft-float) resonator coefficients at a ~1.5 kHz
                // control rate rather than every sample: formants move far slower than that, so
                // it is inaudible and cuts the transcendental cost 16-fold.
                if index % 16 == 0 {
                    self.r1.set(self.f1, target.bw1);
                    self.r2.set(self.f2, target.bw2);
                    self.r3.set(self.f3, target.bw3);
                }

                // Pitch: a natural downward declination over the phrase plus a little jitter, so
                // the voice is not a dead monotone.
                let progress = global as f64 / total as f64;
                let jitter = 1.0 + 0.004 * self.noise();
                let f0_now = f0 * (1.05 - 0.15 * progress) * jitter;
                self.glottal_phase += f0_now / SAMPLE_RATE;
                if self.glottal_phase >= 1.0 {
                    self.glottal_phase -= 1.0;
                }
                // Glottal flow, then differentiate for lip radiation (the excitation), then a
                // light spectral tilt so it is warm rather than harsh.
                let flow = glottal_flow(self.glottal_phase);
                let excitation = (flow - self.prev_flow) * 6.0;
                self.prev_flow = flow;
                self.src_lp += (excitation - self.src_lp) * 0.28;
                let source = (0.7 * excitation + 0.3 * self.src_lp) * target.av;

                // Voiced path: a four-formant cascade.
                let voiced = self
                    .r4
                    .step(self.r3.step(self.r2.step(self.r1.step(source))));

                // Frication: high-passed (crisper sibilants) band-passed noise.
                let fric = if target.af > 0.0 {
                    if index % 16 == 0 {
                        self.rf.set(target.fc, target.fbw);
                    }
                    let raw = self.noise();
                    let hp = raw - self.prev_noise;
                    self.prev_noise = raw;
                    self.rf.step(hp) * target.af
                } else {
                    self.prev_noise = 0.0;
                    0.0
                };

                // Gentle output smoothing removes high-frequency stepping without dulling
                // consonants (mostly the raw signal, a little low-passed).
                let mix = voiced + fric;
                self.out_lp += (mix - self.out_lp) * 0.16;
                buf.push(0.82 * mix + 0.18 * self.out_lp);
                global += 1;
            }
        }
    }
}

// ---- Public API ----------------------------------------------------------------

/// Synthesize `text` into 24 kHz mono 16-bit PCM, ready to hand straight to
/// [`crate::audio::Speaker::speak`]. English letter-to-sound rules and number words drive the
/// voice; `french` selects French letter and number names. Returns an empty vector for empty
/// or unpronounceable input, which the caller can fall back on (e.g. to spelling).
pub fn say(text: &str, french: bool) -> Vec<u8> {
    let phonemes = phones(text, french);
    if phonemes.is_empty() {
        return Vec::new();
    }
    let mut targets = Vec::new();
    // A short lead-in of silence settles the resonators before the first sound.
    targets.push(Target::silence(15.0));
    for ph in phonemes {
        targets_for(ph, &mut targets);
    }
    targets.push(Target::silence(20.0));

    let f0 = PITCH_HZ.load(Ordering::Relaxed) as f64;
    let rate = RATE_PERCENT.load(Ordering::Relaxed);
    let mut samples = Vec::new();
    Renderer::new().render(&targets, f0, rate, &mut samples);

    to_pcm16(&samples)
}

/// Level a `f64` sample buffer to 16-bit PCM bytes: find the peak and scale so the loudest
/// sample sits near -3 dBFS, keeping every utterance clearly audible without clipping. A short
/// linear fade at each end removes onset/offset clicks.
fn to_pcm16(samples: &[f64]) -> Vec<u8> {
    let peak = samples.iter().fold(0.0_f64, |m, &s| m.max(libm::fabs(s)));
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
