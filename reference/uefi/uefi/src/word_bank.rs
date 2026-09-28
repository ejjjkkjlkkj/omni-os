//! The premium word bank: real-voice clips for the words that dynamic text is built from.
//!
//! The fixed setup scaffolding is spoken by pre-recorded clips of a real installed voice, so it
//! sounds native rather than synthetic. This extends that real voice to *dynamic* text: every
//! number word and a curated set of common firmware label/value words, in French and English,
//! each spoken by the installed premium voice ([`scripts/gen-word-bank.ps1`]). Dynamic text is
//! spoken word by word - a bank clip where the word is known (premium, real voice), the runtime
//! formant synthesizer ([`crate::synth`]) only for the rare word that is not. Numbers are read by
//! decomposing them into their word atoms ("mille deux cent quatre-vingts"), each a bank clip.
//!
//! The clips are the same 24 kHz mono PCM the audio backends stream, packed per language into one
//! blob with a generated `(word, offset, length)` index, so a word plays through the same DMA
//! path as a fixed clip.

extern crate alloc;

#[path = "word_bank_gen.rs"]
mod bank_index;

/// The packed real-voice PCM blobs, one per language.
static FR_BLOB: &[u8] = include_bytes!("speech/word_bank_fr.bin");
static EN_BLOB: &[u8] = include_bytes!("speech/word_bank_en.bin");

/// The real-voice clip for `word` (already lower-cased) in the active language, or `None` when
/// the word is not in the bank.
pub fn clip_for(word: &str, french: bool) -> Option<&'static [u8]> {
    let (index, blob) = if french {
        (bank_index::FR_INDEX, FR_BLOB)
    } else {
        (bank_index::EN_INDEX, EN_BLOB)
    };
    index
        .iter()
        .find(|(name, _, _)| *name == word)
        .map(|(_, offset, length)| &blob[*offset as usize..(*offset + *length) as usize])
}

/// Decompose a run of decimal digits into the sequence of number-word atoms to speak, in the
/// active language - each atom a word the bank holds ("1280" -> "mille","deux","cent","quatre",
/// "vingts"... as atoms). Long runs (identifiers) are read digit by digit. Mirrors the
/// synthesizer's own number reading so the two agree.
pub fn number_atoms(digits: &str, french: bool) -> alloc::vec::Vec<&'static str> {
    let mut out = alloc::vec::Vec::new();
    let value: u64 = digits.parse().unwrap_or(u64::MAX);
    if digits.len() <= 4 && value != u64::MAX {
        number_words(value, french, &mut out);
    } else {
        for c in digits.chars() {
            if let Some(d) = c.to_digit(10) {
                out.push(digit_word(d as usize, french));
            }
        }
    }
    out
}

fn digit_word(d: usize, french: bool) -> &'static str {
    const FR: [&str; 10] = [
        "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf",
    ];
    const EN: [&str; 10] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    if french { FR[d] } else { EN[d] }
}

fn number_words(value: u64, french: bool, out: &mut alloc::vec::Vec<&'static str>) {
    if french {
        number_words_fr(value, out);
    } else {
        number_words_en(value, out);
    }
}

fn number_words_fr(value: u64, out: &mut alloc::vec::Vec<&'static str>) {
    if value == 0 {
        out.push("zéro");
        return;
    }
    let mut v = value;
    if v >= 1000 {
        let th = v / 1000;
        if th > 1 {
            under_1000_fr(th, out);
        }
        out.push("mille");
        v %= 1000;
    }
    if v > 0 {
        under_1000_fr(v, out);
    }
}

fn under_1000_fr(value: u64, out: &mut alloc::vec::Vec<&'static str>) {
    let mut v = value;
    if v >= 100 {
        let h = v / 100;
        if h > 1 {
            under_100_fr(h, out);
        }
        out.push("cent");
        v %= 100;
    }
    if v > 0 {
        under_100_fr(v, out);
    }
}

fn under_100_fr(value: u64, out: &mut alloc::vec::Vec<&'static str>) {
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
    let v = value as usize;
    if v < 10 {
        out.push(ONES[v]);
    } else if v <= 16 {
        out.push(TEENS[v - 10]);
    } else if v < 20 {
        out.push("dix");
        out.push(ONES[v - 10]);
    } else if v < 70 {
        let unit = v % 10;
        out.push(TENS[v / 10]);
        if unit == 1 {
            out.push("et");
        }
        if unit != 0 {
            out.push(ONES[unit]);
        }
    } else if v < 80 {
        out.push("soixante");
        if v == 71 {
            out.push("et");
        }
        under_100_fr((v - 60) as u64, out);
    } else {
        out.push("quatre");
        out.push("vingt");
        if v > 80 {
            under_100_fr((v - 80) as u64, out);
        }
    }
}

fn number_words_en(value: u64, out: &mut alloc::vec::Vec<&'static str>) {
    if value == 0 {
        out.push("zero");
        return;
    }
    let mut v = value;
    if v >= 1000 {
        under_100_en(v / 1000, out);
        out.push("thousand");
        v %= 1000;
    }
    if v >= 100 {
        under_100_en(v / 100, out);
        out.push("hundred");
        v %= 100;
    }
    if v > 0 {
        under_100_en(v, out);
    }
}

fn under_100_en(value: u64, out: &mut alloc::vec::Vec<&'static str>) {
    const ONES: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    let v = value as usize;
    if v < 20 {
        out.push(ONES[v]);
    } else {
        let unit = v % 10;
        out.push(TENS[v / 10]);
        if unit != 0 {
            out.push(ONES[unit]);
        }
    }
}
