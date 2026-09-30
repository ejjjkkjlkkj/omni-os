//! Intel High Definition Audio at the firmware stage - real spoken words from the
//! UEFI screen reader, before any operating system.
//!
//! This is the payoff of the whole effort: on a thin laptop with no PC-speaker
//! buzzer, the machine's HDA codec is the only thing that can make a blind user
//! hear anything before the OS. The approach follows Machado & Vieira, "UEFI BIOS
//! Accessibility for the Visually Impaired" (arXiv:1712.03186) - reach the HDA
//! controller from the pre-OS environment and drive the codec - and takes it past
//! where that prototype stopped: where they left DMA as an open question and
//! validated only the codec's beep generator, this streams real PCM speech by DMA.
//!
//! The words are short clips of the boot screen's fixed lines, synthesized ahead
//! of time and embedded as raw 24 kHz mono PCM ([`CLIP_WELCOME`] and friends). At
//! boot the controller is brought up once ([`bring_up`]) and each clip is played
//! through it ([`Speaker::speak`]); the same DMA path also carries the runtime
//! formant synthesizer ([`crate::synth`]) for dynamic text. The firmware identity-maps all of memory
//! during boot services, so a `static`'s address is its physical address and no
//! page mapping is needed.

use core::sync::atomic::{AtomicBool, Ordering};

use uefi::boot;

use crate::aw_mark;

/// The boot screen's spoken lines, synthesized offline by omni-os's own ST voice to 24 kHz
/// 16-bit mono PCM. Texts live in `tools/voice/firmware-clips.json`; regenerate with `tools/voice/gen-firmware-speech.py`.
pub static CLIP_WELCOME: &[u8] = include_bytes!("speech/welcome.pcm");
pub static CLIP_ACTIVE: &[u8] = include_bytes!("speech/active.pcm");
pub static CLIP_STARTING: &[u8] = include_bytes!("speech/starting.pcm");
pub static CLIP_LOADING: &[u8] = include_bytes!("speech/loading.pcm");

/// The accessible firmware Setup Utility's fixed spoken lines (`boot/uefi/src/setup.rs`),
/// synthesized to the same 24 kHz mono PCM by the ST voice (`tools/voice/gen-firmware-speech.py`). The setup is
/// voiced and operated at the firmware stage, where the keyboard is the firmware's own -
/// so it works with a USB keyboard on every machine, before any kernel USB stack exists.
/// The fixed scaffolding - the intro, the interaction instructions, the five tab names,
/// the submenu titles and the fixed action labels - each carries a clip, so a blind user
/// hears the whole navigable structure. Dynamic lines (Main/Advanced/Security values and
/// the enumerated Boot#### device names, composed at runtime) carry no clip: they are
/// spoken as words by the runtime formant synthesizer ([`crate::synth`]) through this same
/// codec, and spelled character by character as a fallback.
pub static CLIP_SETUP_INTRO: &[u8] = include_bytes!("speech/menu_intro.pcm");
pub static CLIP_INSTRUCTIONS: &[u8] = include_bytes!("speech/instructions.pcm");
pub static CLIP_TAB_MAIN: &[u8] = include_bytes!("speech/tab_main.pcm");
pub static CLIP_TAB_ADVANCED: &[u8] = include_bytes!("speech/tab_advanced.pcm");
pub static CLIP_TAB_BOOT: &[u8] = include_bytes!("speech/tab_boot.pcm");
pub static CLIP_TAB_SECURITY: &[u8] = include_bytes!("speech/tab_security.pcm");
pub static CLIP_TAB_SAVEEXIT: &[u8] = include_bytes!("speech/tab_saveexit.pcm");
pub static CLIP_ACT_BOOT_NORMALLY: &[u8] = include_bytes!("speech/act_boot_normally.pcm");
pub static CLIP_ACT_ENTER_SETUP: &[u8] = include_bytes!("speech/act_enter_setup.pcm");
pub static CLIP_ACT_RESET: &[u8] = include_bytes!("speech/act_reset.pcm");
pub static CLIP_ACT_SHUTDOWN: &[u8] = include_bytes!("speech/act_shutdown.pcm");
pub static CLIP_SUB_CPU: &[u8] = include_bytes!("speech/sub_cpu.pcm");
pub static CLIP_SUB_BOOT_PRIO: &[u8] = include_bytes!("speech/sub_boot_prio.pcm");
pub static CLIP_SUB_SECURE_BOOT: &[u8] = include_bytes!("speech/sub_secure_boot.pcm");
pub static CLIP_ACT_BOOT_NOW: &[u8] = include_bytes!("speech/act_boot_now.pcm");
pub static CLIP_ACT_MAKE_DEFAULT: &[u8] = include_bytes!("speech/act_make_default.pcm");
pub static CLIP_ACT_MOVE_UP: &[u8] = include_bytes!("speech/act_move_up.pcm");
pub static CLIP_ACT_MOVE_DOWN: &[u8] = include_bytes!("speech/act_move_down.pcm");
pub static CLIP_ACT_BACK: &[u8] = include_bytes!("speech/act_back.pcm");
pub static CLIP_BOOT_DEVICE: &[u8] = include_bytes!("speech/boot_device.pcm");
pub static CLIP_CONFIRM_PROMPT: &[u8] = include_bytes!("speech/confirm_prompt.pcm");
pub static CLIP_CONFIRM_CANCEL: &[u8] = include_bytes!("speech/confirm_cancel.pcm");
pub static CLIP_CONFIRM_DONE: &[u8] = include_bytes!("speech/confirm_done.pcm");
pub static CLIP_ACT_LANGUAGE: &[u8] = include_bytes!("speech/act_language.pcm");

/// The French clip set: the setup can be operated in French (the default) or English, the
/// way a real ASUS/AMI BIOS offers a "System Language" option. These are spoken by the ST
/// voice in French. Regenerate with `tools/voice/gen-firmware-speech.py`.
pub static CLIP_FR_INTRO: &[u8] = include_bytes!("speech/fr_intro.pcm");
pub static CLIP_FR_INSTRUCTIONS: &[u8] = include_bytes!("speech/fr_instructions.pcm");
pub static CLIP_FR_TAB_MAIN: &[u8] = include_bytes!("speech/fr_tab_main.pcm");
pub static CLIP_FR_TAB_ADVANCED: &[u8] = include_bytes!("speech/fr_tab_advanced.pcm");
pub static CLIP_FR_TAB_BOOT: &[u8] = include_bytes!("speech/fr_tab_boot.pcm");
pub static CLIP_FR_TAB_SECURITY: &[u8] = include_bytes!("speech/fr_tab_security.pcm");
pub static CLIP_FR_TAB_SAVEEXIT: &[u8] = include_bytes!("speech/fr_tab_saveexit.pcm");
pub static CLIP_FR_ACT_BOOT_NORMALLY: &[u8] = include_bytes!("speech/fr_act_boot_normally.pcm");
pub static CLIP_FR_ACT_ENTER_SETUP: &[u8] = include_bytes!("speech/fr_act_enter_setup.pcm");
pub static CLIP_FR_ACT_RESET: &[u8] = include_bytes!("speech/fr_act_reset.pcm");
pub static CLIP_FR_ACT_SHUTDOWN: &[u8] = include_bytes!("speech/fr_act_shutdown.pcm");
pub static CLIP_FR_SUB_CPU: &[u8] = include_bytes!("speech/fr_sub_cpu.pcm");
pub static CLIP_FR_SUB_BOOT_PRIO: &[u8] = include_bytes!("speech/fr_sub_boot_prio.pcm");
pub static CLIP_FR_SUB_SECURE_BOOT: &[u8] = include_bytes!("speech/fr_sub_secure_boot.pcm");
pub static CLIP_FR_ACT_BOOT_NOW: &[u8] = include_bytes!("speech/fr_act_boot_now.pcm");
pub static CLIP_FR_ACT_MAKE_DEFAULT: &[u8] = include_bytes!("speech/fr_act_make_default.pcm");
pub static CLIP_FR_ACT_MOVE_UP: &[u8] = include_bytes!("speech/fr_act_move_up.pcm");
pub static CLIP_FR_ACT_MOVE_DOWN: &[u8] = include_bytes!("speech/fr_act_move_down.pcm");
pub static CLIP_FR_ACT_BACK: &[u8] = include_bytes!("speech/fr_act_back.pcm");
pub static CLIP_FR_BOOT_DEVICE: &[u8] = include_bytes!("speech/fr_boot_device.pcm");
pub static CLIP_FR_CONFIRM_PROMPT: &[u8] = include_bytes!("speech/fr_confirm_prompt.pcm");
pub static CLIP_FR_CONFIRM_CANCEL: &[u8] = include_bytes!("speech/fr_confirm_cancel.pcm");
pub static CLIP_FR_CONFIRM_DONE: &[u8] = include_bytes!("speech/fr_confirm_done.pcm");
pub static CLIP_FR_LANG: &[u8] = include_bytes!("speech/fr_lang.pcm");

/// The command agent's spoken replies. The agent lets a user TYPE a plain instruction
/// ("boot usb", "secure boot", "restart") instead of walking the tree, and speaks back
/// what it understood and did. Each reply is a `(english, french)` pair; the agent picks
/// the active language with [`agent_clip`]. Regenerate with `tools/voice/gen-firmware-speech.py`.
macro_rules! agent_pair {
    ($konst:ident, $name:literal) => {
        pub static $konst: (&[u8], &[u8]) = (
            include_bytes!(concat!("speech/agent_", $name, ".pcm")),
            include_bytes!(concat!("speech/fr_agent_", $name, ".pcm")),
        );
    };
}
agent_pair!(AGENT_PROMPT, "prompt");
agent_pair!(AGENT_HELP, "help");
agent_pair!(AGENT_UNKNOWN, "unknown");
agent_pair!(AGENT_FIRMWARE_ONLY, "firmware_only");
agent_pair!(AGENT_OPENING_SETUP, "opening_setup");
agent_pair!(AGENT_SETUP_DENIED, "setup_denied");
agent_pair!(AGENT_RESTARTING, "restarting");
agent_pair!(AGENT_SHUTTING_DOWN, "shutting_down");
agent_pair!(AGENT_SECURE_BOOT_IS, "secure_boot_is");
agent_pair!(AGENT_VALUE_IS, "value_is");
agent_pair!(AGENT_BOOTING, "booting");
agent_pair!(AGENT_SET_DEFAULT, "set_default");
agent_pair!(AGENT_NO_MATCH, "no_match");
agent_pair!(AGENT_BOOT_LIST, "boot_list");
agent_pair!(AGENT_DONE, "done");
agent_pair!(AGENT_FAILED, "failed");
agent_pair!(AGENT_TIME_IS, "time_is");
agent_pair!(AGENT_MEMORY_IS, "memory_is");
agent_pair!(AGENT_PROCESSOR_IS, "processor_is");
agent_pair!(AGENT_FIRMWARE_IS, "firmware_is");
agent_pair!(AGENT_TIMEOUT_SET, "timeout_set");

/// Pick the English or French half of an agent reply pair for the active language.
pub fn agent_clip(pair: (&'static [u8], &'static [u8]), french: bool) -> &'static [u8] {
    if french { pair.1 } else { pair.0 }
}

/// The spelling alphabet: one clip per letter and digit, so a dynamic line the setup
/// cannot pre-record whole - a boot-device name, a machine-state value - can still be
/// read aloud character by character (a screen reader's "read by character"), on the "S"
/// key. Regenerate with `tools/voice/gen-firmware-speech.py`.
static SPELL_LETTERS: [&[u8]; 26] = [
    include_bytes!("speech/spell_a.pcm"),
    include_bytes!("speech/spell_b.pcm"),
    include_bytes!("speech/spell_c.pcm"),
    include_bytes!("speech/spell_d.pcm"),
    include_bytes!("speech/spell_e.pcm"),
    include_bytes!("speech/spell_f.pcm"),
    include_bytes!("speech/spell_g.pcm"),
    include_bytes!("speech/spell_h.pcm"),
    include_bytes!("speech/spell_i.pcm"),
    include_bytes!("speech/spell_j.pcm"),
    include_bytes!("speech/spell_k.pcm"),
    include_bytes!("speech/spell_l.pcm"),
    include_bytes!("speech/spell_m.pcm"),
    include_bytes!("speech/spell_n.pcm"),
    include_bytes!("speech/spell_o.pcm"),
    include_bytes!("speech/spell_p.pcm"),
    include_bytes!("speech/spell_q.pcm"),
    include_bytes!("speech/spell_r.pcm"),
    include_bytes!("speech/spell_s.pcm"),
    include_bytes!("speech/spell_t.pcm"),
    include_bytes!("speech/spell_u.pcm"),
    include_bytes!("speech/spell_v.pcm"),
    include_bytes!("speech/spell_w.pcm"),
    include_bytes!("speech/spell_x.pcm"),
    include_bytes!("speech/spell_y.pcm"),
    include_bytes!("speech/spell_z.pcm"),
];
static SPELL_DIGITS: [&[u8]; 10] = [
    include_bytes!("speech/spell_0.pcm"),
    include_bytes!("speech/spell_1.pcm"),
    include_bytes!("speech/spell_2.pcm"),
    include_bytes!("speech/spell_3.pcm"),
    include_bytes!("speech/spell_4.pcm"),
    include_bytes!("speech/spell_5.pcm"),
    include_bytes!("speech/spell_6.pcm"),
    include_bytes!("speech/spell_7.pcm"),
    include_bytes!("speech/spell_8.pcm"),
    include_bytes!("speech/spell_9.pcm"),
];
static SPELL_SPACE: &[u8] = include_bytes!("speech/spell_space.pcm");

/// Spoken name of one punctuation symbol, in English and French. Firmware values carry
/// separators - `1280x800`, `USB 3.0`, dates, `85%`, boot paths - and dropping them when
/// spelling loses information ("3.0" heard as "three zero"). Each symbol's name differs by
/// language, so both are recorded: English with an English voice, French with a French one.
macro_rules! spell_symbol {
    ($name:literal) => {
        (
            include_bytes!(concat!("speech/spell_", $name, ".pcm")),
            include_bytes!(concat!("speech/fr_spell_", $name, ".pcm")),
        )
    };
}

/// A spelled symbol: its character and its `(english, french)` clips.
type SpellSymbol = (char, (&'static [u8], &'static [u8]));

/// `(character, (english_clip, french_clip))` for every spelled symbol. Kept in one table
/// so the code and the generated assets (`tools/voice/firmware-clips.json`) cannot drift.
static SPELL_SYMBOLS: &[SpellSymbol] = &[
    ('.', spell_symbol!("dot")),
    ('-', spell_symbol!("dash")),
    (':', spell_symbol!("colon")),
    ('/', spell_symbol!("slash")),
    ('\\', spell_symbol!("backslash")),
    ('%', spell_symbol!("percent")),
    (',', spell_symbol!("comma")),
    ('_', spell_symbol!("underscore")),
    ('(', spell_symbol!("lparen")),
    (')', spell_symbol!("rparen")),
    ('+', spell_symbol!("plus")),
    ('=', spell_symbol!("equals")),
    ('@', spell_symbol!("at")),
];

/// NATO phonetic names (Alpha, Bravo, Charlie...), one clip per letter. When phonetic
/// spelling is on, a letter is read as its NATO word so it cannot be confused with a
/// similar-sounding one (b/d/p, m/n) - the classic screen-reader "phonetic" mode.
static SPELL_NATO: [&[u8]; 26] = [
    include_bytes!("speech/spell_nato_a.pcm"),
    include_bytes!("speech/spell_nato_b.pcm"),
    include_bytes!("speech/spell_nato_c.pcm"),
    include_bytes!("speech/spell_nato_d.pcm"),
    include_bytes!("speech/spell_nato_e.pcm"),
    include_bytes!("speech/spell_nato_f.pcm"),
    include_bytes!("speech/spell_nato_g.pcm"),
    include_bytes!("speech/spell_nato_h.pcm"),
    include_bytes!("speech/spell_nato_i.pcm"),
    include_bytes!("speech/spell_nato_j.pcm"),
    include_bytes!("speech/spell_nato_k.pcm"),
    include_bytes!("speech/spell_nato_l.pcm"),
    include_bytes!("speech/spell_nato_m.pcm"),
    include_bytes!("speech/spell_nato_n.pcm"),
    include_bytes!("speech/spell_nato_o.pcm"),
    include_bytes!("speech/spell_nato_p.pcm"),
    include_bytes!("speech/spell_nato_q.pcm"),
    include_bytes!("speech/spell_nato_r.pcm"),
    include_bytes!("speech/spell_nato_s.pcm"),
    include_bytes!("speech/spell_nato_t.pcm"),
    include_bytes!("speech/spell_nato_u.pcm"),
    include_bytes!("speech/spell_nato_v.pcm"),
    include_bytes!("speech/spell_nato_w.pcm"),
    include_bytes!("speech/spell_nato_x.pcm"),
    include_bytes!("speech/spell_nato_y.pcm"),
    include_bytes!("speech/spell_nato_z.pcm"),
];

/// Whether spelling reads letters as their NATO phonetic word. Off by default, toggled from
/// the setup with the P key.
static PHONETIC: AtomicBool = AtomicBool::new(false);

/// Turn phonetic (NATO) spelling on or off; returns the new state.
pub fn toggle_phonetic() -> bool {
    let on = !PHONETIC.load(Ordering::Relaxed);
    PHONETIC.store(on, Ordering::Relaxed);
    on
}

/// The clip for one alphabetic index (0 = a), NATO word when phonetic spelling is on, plain
/// letter otherwise.
fn letter_clip(index: usize) -> &'static [u8] {
    if PHONETIC.load(Ordering::Relaxed) {
        SPELL_NATO[index]
    } else {
        SPELL_LETTERS[index]
    }
}

/// The spoken clip for one character when spelling a dynamic line: the letter's or digit's
/// name, "space", or a punctuation symbol's name in the active language (`french`). Letters
/// fold to lower case and become NATO words when phonetic spelling is on. A character with no
/// clip (an unlisted symbol) is skipped, but the meaningful separators in firmware values are
/// now spoken instead of silently lost.
pub fn spell_clip(character: char, french: bool) -> Option<&'static [u8]> {
    match character {
        'a'..='z' => Some(letter_clip(character as usize - 'a' as usize)),
        'A'..='Z' => Some(letter_clip(character as usize - 'A' as usize)),
        '0'..='9' => Some(SPELL_DIGITS[character as usize - '0' as usize]),
        ' ' => Some(SPELL_SPACE),
        _ => SPELL_SYMBOLS
            .iter()
            .find(|(c, _)| *c == character)
            .map(|(_, (en, fr))| if french { *fr } else { *en }),
    }
}

// ---- PCI mechanism #1 (CF8/CFC) and MMIO -------------------------------

unsafe fn outl(port: u16, value: u32) {
    // SAFETY: caller names a valid dword-wide port.
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: caller names a valid dword-wide port.
    unsafe {
        core::arch::asm!("in eax, dx", out("eax") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
}

const PCI_CONFIG_ADDRESS: u16 = 0x0cf8;
const PCI_CONFIG_DATA: u16 = 0x0cfc;

fn pci_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | (u32::from(bus) << 16)
        | (u32::from(device) << 11)
        | (u32::from(function) << 8)
        | u32::from(offset & 0xfc)
}

unsafe fn pci_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    // SAFETY: CF8/CFC are the architected PCI configuration ports.
    unsafe {
        outl(
            PCI_CONFIG_ADDRESS,
            pci_address(bus, device, function, offset),
        );
        inl(PCI_CONFIG_DATA)
    }
}

unsafe fn pci_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    // SAFETY: CF8/CFC are the architected PCI configuration ports.
    unsafe {
        outl(
            PCI_CONFIG_ADDRESS,
            pci_address(bus, device, function, offset),
        );
        outl(PCI_CONFIG_DATA, value);
    }
}

unsafe fn read8(base: u64, offset: u64) -> u8 {
    // SAFETY: base+offset is inside the firmware-identity-mapped BAR0 window.
    unsafe { ((base + offset) as *const u8).read_volatile() }
}
unsafe fn write8(base: u64, offset: u64, value: u8) {
    // SAFETY: as read8.
    unsafe { ((base + offset) as *mut u8).write_volatile(value) };
}
unsafe fn read16(base: u64, offset: u64) -> u16 {
    // SAFETY: as read8.
    unsafe { ((base + offset) as *const u16).read_volatile() }
}
unsafe fn write16(base: u64, offset: u64, value: u16) {
    // SAFETY: as read8.
    unsafe { ((base + offset) as *mut u16).write_volatile(value) };
}
unsafe fn read32(base: u64, offset: u64) -> u32 {
    // SAFETY: as read8.
    unsafe { ((base + offset) as *const u32).read_volatile() }
}
unsafe fn write32(base: u64, offset: u64, value: u32) {
    // SAFETY: as read8.
    unsafe { ((base + offset) as *mut u32).write_volatile(value) };
}

// ---- HDA registers (see the kernel driver for the annotated set) ---------

const REG_GCAP: u64 = 0x00;
const REG_GCTL: u64 = 0x08;
const REG_STATESTS: u64 = 0x0e;
const REG_CORBLBASE: u64 = 0x40;
const REG_CORBUBASE: u64 = 0x44;
const REG_CORBWP: u64 = 0x48;
const REG_CORBRP: u64 = 0x4a;
const REG_CORBCTL: u64 = 0x4c;
const REG_CORBSIZE: u64 = 0x4e;
const REG_RIRBLBASE: u64 = 0x50;
const REG_RIRBUBASE: u64 = 0x54;
const REG_RIRBWP: u64 = 0x58;
const REG_RINTCNT: u64 = 0x5a;
const REG_RIRBCTL: u64 = 0x5c;
const REG_RIRBSTS: u64 = 0x5d;
const REG_RIRBSIZE: u64 = 0x5e;

const GCTL_CRST: u32 = 1 << 0;
const CORBRP_RST: u16 = 1 << 15;
const CORBCTL_RUN: u8 = 1 << 1;
const RIRBWP_RST: u16 = 1 << 15;
const RIRBCTL_DMAEN: u8 = 1 << 1;
const RIRBSTS_INTFL: u8 = 1 << 0;

const RING_ENTRIES: u16 = 256;
const RING_SIZE_256: u8 = 0x02;

const VERB_GET_PARAMETER: u32 = 0xf00;
const PARAM_VENDOR_ID: u32 = 0x00;
const PARAM_SUBNODE_COUNT: u32 = 0x04;
const PARAM_FUNCTION_GROUP_TYPE: u32 = 0x05;
const PARAM_WIDGET_CAP: u32 = 0x09;
const PARAM_PIN_CAP: u32 = 0x0c;
const PARAM_CONNECTION_LIST_LEN: u32 = 0x0e;

const WIDGET_AUDIO_OUTPUT: u32 = 0x0;
const WIDGET_AUDIO_MIXER: u32 = 0x2;
const WIDGET_AUDIO_SELECTOR: u32 = 0x3;
const WIDGET_PIN_COMPLEX: u32 = 0x4;

const VERB4_SET_FORMAT: u32 = 0x2;
const VERB4_SET_AMP: u32 = 0x3;
const VERB_GET_CONNECTION_ENTRY: u32 = 0xf02;
const VERB_GET_CONFIG_DEFAULT: u32 = 0xf1c;
const VERB_SET_CONNECTION_SELECT: u32 = 0x701;
const VERB_SET_POWER_STATE: u32 = 0x705;
const VERB_SET_STREAM_CHANNEL: u32 = 0x706;
const VERB_SET_PIN_CONTROL: u32 = 0x707;
const VERB_SET_EAPD: u32 = 0x70c;

const PIN_CONTROL_OUT_ENABLE: u32 = 1 << 6;
const EAPD_ENABLE: u32 = 1 << 1;

// Set Amplifier Gain/Mute (verb 0x3) payload bits. An output path is only audible
// when every stage on it - the DAC, any mixer or selector between, and the pin -
// has its amp unmuted. QEMU's codec is a bare DAC->pin, so unmuting the ends was
// enough; a real codec (VMware's, physical hardware's) routes DAC->mixer->pin, and
// the mixer's per-input amp is muted at reset, which silences everything.
const AMP_SET_OUTPUT: u16 = 1 << 15;
const AMP_SET_INPUT: u16 = 1 << 14;
const AMP_LEFT: u16 = 1 << 13;
const AMP_RIGHT: u16 = 1 << 12;
const AMP_INDEX_SHIFT: u16 = 8;
const AMP_GAIN: u16 = 0x2a;
const AMP_OUT_UNMUTE: u16 = AMP_SET_OUTPUT | AMP_LEFT | AMP_RIGHT | AMP_GAIN;

/// Clips are 24 kHz mono; played as 48 kHz 16-bit stereo, the one rate every HD Audio codec
/// must support (HDA 1.0a, 7.3.4.7): each sample is upsampled by two (linear interpolation)
/// and duplicated to both channels. A codec driven at 24 kHz produces noise where that rate
/// is not implemented (VMware's HD Audio, many real codecs).
const STREAM_FORMAT: u16 = 0x0011;
/// Bytes per second of the played stream (48000 frames * 2 channels * 2 bytes).
const BYTES_PER_SEC: u32 = 48000 * 2 * 2;
const STREAM_TAG: u8 = 1;
/// Silence appended after every clip: 250 ms at 48 kHz stereo 16-bit, longer than the
/// 20 ms polling step plus the stop margin.
const TAIL_SILENCE_BYTES: usize = 48_000 / 4 * 4;

const SD_CTL: u64 = 0x00;
const SD_LPIB: u64 = 0x04;
const SD_CBL: u64 = 0x08;
const SD_LVI: u64 = 0x0c;
const SD_FMT: u64 = 0x12;
const SD_BDPL: u64 = 0x18;
const SD_BDPU: u64 = 0x1c;
const SDCTL_SRST: u8 = 1 << 0;
const SDCTL_RUN: u8 = 1 << 1;
const STREAM_BASE: u64 = 0x80;
const STREAM_STRIDE: u64 = 0x20;

#[repr(C, align(4096))]
struct Page([u8; 4096]);

static mut CORB: Page = Page([0; 4096]);
static mut RIRB: Page = Page([0; 4096]);
static mut BDL: Page = Page([0; 4096]);

/// Playback buffer, page-aligned and identity-mapped. Sized for the longest clip
/// as 48 kHz stereo (mono 24 kHz clip bytes * 4): 2 MiB holds ~11 s, enough for the
/// longest firmware line without truncation.
const AUDIO_BYTES: usize = 2 * 1024 * 1024;
#[repr(C, align(4096))]
struct AudioBuffer([u8; AUDIO_BYTES]);
static mut AUDIO: AudioBuffer = AudioBuffer([0; AUDIO_BYTES]);

#[derive(Clone, Copy)]
struct PciLocation {
    bus: u8,
    device: u8,
    function: u8,
}

fn is_hda(location: PciLocation) -> bool {
    // SAFETY: configuration reads have no side effects.
    let id = unsafe { pci_read32(location.bus, location.device, location.function, 0x00) };
    if id & 0xffff == 0xffff {
        return false;
    }
    let class = unsafe { pci_read32(location.bus, location.device, location.function, 0x08) };
    (class >> 24) & 0xff == 0x04 && (class >> 16) & 0xff == 0x03
}

/// Enable memory space + bus mastering and return BAR0. No page mapping: the
/// firmware identity-maps the BAR, so its physical address is directly usable.
fn enable_bar0(location: PciLocation) -> Option<u64> {
    let PciLocation {
        bus,
        device,
        function,
    } = location;
    // SAFETY: enable MMIO + bus mastering, then read the 64-bit BAR0.
    let base = unsafe {
        let command = pci_read32(bus, device, function, 0x04);
        pci_write32(bus, device, function, 0x04, command | 0b110);
        let low = pci_read32(bus, device, function, 0x10);
        let high = pci_read32(bus, device, function, 0x14);
        (u64::from(low & 0xffff_fff0)) | (u64::from(high) << 32)
    };
    (base != 0).then_some(base)
}

/// A brought-up HDA controller with a configured output path: ready to speak.
pub struct Speaker {
    base: u64,
    codec: u8,
    input_streams: u8,
    dac: u8,
    pin: u8,
    rirb_read: u16,
}

fn build_verb(codec: u8, nid: u8, verb: u32, payload: u32) -> u32 {
    (u32::from(codec) << 28) | (u32::from(nid) << 20) | ((verb & 0xfff) << 8) | (payload & 0xff)
}

impl Speaker {
    fn send_raw(&mut self, command: u32) -> Result<u32, &'static str> {
        let corb = core::ptr::addr_of_mut!(CORB) as *mut u32;
        // SAFETY: CORB is an identity-mapped ring; advance the write pointer and
        // place the verb at the new slot.
        unsafe {
            let next = (read16(self.base, REG_CORBWP) + 1) % RING_ENTRIES;
            corb.add(next as usize).write_volatile(command);
            write16(self.base, REG_CORBWP, next);
            let mut budget = 10_000_000u32;
            loop {
                let write = read16(self.base, REG_RIRBWP) & (RING_ENTRIES - 1);
                if write != self.rirb_read {
                    break;
                }
                budget -= 1;
                if budget == 0 {
                    return Err("no_response");
                }
                core::hint::spin_loop();
            }
            self.rirb_read = (self.rirb_read + 1) % RING_ENTRIES;
            let rirb = core::ptr::addr_of!(RIRB) as *const u32;
            let response = rirb.add(self.rirb_read as usize * 2).read_volatile();
            write8(self.base, REG_RIRBSTS, RIRBSTS_INTFL);
            Ok(response)
        }
    }

    fn command(&mut self, nid: u8, verb: u32, payload: u32) -> Result<u32, &'static str> {
        self.send_raw(build_verb(self.codec, nid, verb, payload))
    }

    fn command16(&mut self, nid: u8, verb4: u32, payload: u16) -> Result<(), &'static str> {
        let value = (u32::from(self.codec) << 28)
            | (u32::from(nid) << 20)
            | ((verb4 & 0xf) << 16)
            | u32::from(payload);
        self.send_raw(value).map(|_| ())
    }

    fn set(&mut self, nid: u8, verb: u32, payload: u32) -> Result<(), &'static str> {
        self.command(nid, verb, payload).map(|_| ())
    }

    fn get_parameter(&mut self, nid: u8, parameter: u32) -> Result<u32, &'static str> {
        self.command(nid, VERB_GET_PARAMETER, parameter)
    }

    fn widget_type(&mut self, nid: u8) -> Result<u32, &'static str> {
        Ok((self.get_parameter(nid, PARAM_WIDGET_CAP)? >> 20) & 0xf)
    }

    /// Number of entries in a widget's connection list (short form; the long-form
    /// bit is ignored, which is safe for the small graphs at this stage).
    fn connection_len(&mut self, nid: u8) -> u8 {
        (self
            .get_parameter(nid, PARAM_CONNECTION_LIST_LEN)
            .unwrap_or(0)
            & 0x7f) as u8
    }

    /// The source node id at `index` in a widget's connection list. Short form:
    /// one response carries four one-byte entries, so read the aligned group and
    /// pick the byte. Returns 0 on error, which is never a valid widget id here.
    fn connection_entry(&mut self, nid: u8, index: u8) -> u8 {
        let group = self
            .command(nid, VERB_GET_CONNECTION_ENTRY, u32::from(index & 0xfc))
            .unwrap_or(0);
        let shift = (index & 0x3) * 8;
        ((group >> shift) & 0xff) as u8
    }

    /// Read a widget's connection list into `out`, returning how many entries were
    /// written (capped by the buffer).
    fn connections(&mut self, nid: u8, out: &mut [u8]) -> usize {
        let len = (self.connection_len(nid) as usize).min(out.len());
        for (index, slot) in out.iter_mut().enumerate().take(len) {
            *slot = self.connection_entry(nid, index as u8);
        }
        len
    }

    /// Unmute and set a moderate gain on a widget's output amplifier.
    fn unmute_output(&mut self, nid: u8) -> Result<(), &'static str> {
        self.command16(nid, VERB4_SET_AMP, AMP_OUT_UNMUTE)
    }

    /// Unmute and set a moderate gain on a widget's input amplifier for one input
    /// index - needed on a mixer, whose per-input amps are muted at reset.
    fn unmute_input(&mut self, nid: u8, index: u8) -> Result<(), &'static str> {
        let payload = AMP_SET_INPUT
            | AMP_LEFT
            | AMP_RIGHT
            | ((u16::from(index) & 0xf) << AMP_INDEX_SHIFT)
            | AMP_GAIN;
        self.command16(nid, VERB4_SET_AMP, payload)
    }

    fn output_stream_base(&self) -> u64 {
        self.base + STREAM_BASE + u64::from(self.input_streams) * STREAM_STRIDE
    }

    /// Play one 24 kHz mono PCM clip through the codec, blocking until it has finished
    /// or `interrupted` returns true. The interrupt is barge-in: a screen-reader user
    /// who has heard enough presses a key, the caller's `interrupted` closure sees it,
    /// and the clip is cut short instead of talking over the next keystroke. Returns
    /// true when the link position advanced (the controller streamed samples), which on
    /// real hardware is audible speech.
    pub fn speak_until(&mut self, clip: &[u8], mut interrupted: impl FnMut() -> bool) -> bool {
        // Upsample each 24 kHz mono sample to two 48 kHz frames (the second interpolated
        // with the next sample) and duplicate each frame to both channels, clamped to the
        // aligned DMA buffer's capacity.
        let mono_samples = (clip.len() / 2).min((AUDIO_BYTES - TAIL_SILENCE_BYTES) / 8);
        let audio = core::ptr::addr_of_mut!(AUDIO) as *mut i16;
        let sample = |index: usize| -> i32 {
            let index = index.min(mono_samples.saturating_sub(1));
            i32::from(crate::audio::scale(i16::from_le_bytes([
                clip[index * 2],
                clip[index * 2 + 1],
            ])))
        };
        for index in 0..mono_samples {
            let first = sample(index) as i16;
            let middle = ((sample(index) + sample(index + 1)) / 2) as i16;
            // SAFETY: index*4+3 < AUDIO_BYTES/2, inside the buffer.
            unsafe {
                audio.add(index * 4).write_volatile(first);
                audio.add(index * 4 + 1).write_volatile(first);
                audio.add(index * 4 + 2).write_volatile(middle);
                audio.add(index * 4 + 3).write_volatile(middle);
            }
        }
        let stereo_bytes = (mono_samples * 8) as u32;
        if stereo_bytes == 0 {
            return false;
        }
        // Silence after the speech: the stream is cyclic, so whatever plays between the end of
        // the speech and the stop is silence, never the start of the clip again.
        // SAFETY: the tail stays inside AUDIO_BYTES by the bound on `mono_samples`.
        unsafe {
            core::ptr::write_bytes(
                (audio as *mut u8).add(stereo_bytes as usize),
                0,
                TAIL_SILENCE_BYTES,
            );
        }
        let dma_bytes = stereo_bytes + TAIL_SILENCE_BYTES as u32;

        // Per-clip: set the converter format (all clips share it here).
        if self
            .command16(self.dac, VERB4_SET_FORMAT, STREAM_FORMAT)
            .is_err()
        {
            return false;
        }

        let stream = self.output_stream_base();
        let bdl_phys = core::ptr::addr_of!(BDL) as u64;
        let audio_phys = core::ptr::addr_of!(AUDIO) as u64;
        let bdl = core::ptr::addr_of_mut!(BDL) as *mut u32;
        // Two descriptor entries, as the HD Audio specification requires (1.0a, 3.6.2): a
        // one-entry list is tolerated by QEMU but streams garbage on other controllers.
        let first = (dma_bytes / 2) & !127;
        // SAFETY: BDL is an identity-mapped descriptor page; two 16-byte entries fit.
        unsafe {
            for (index, (address, bytes)) in [
                (audio_phys, first),
                (audio_phys + u64::from(first), dma_bytes - first),
            ]
            .into_iter()
            .enumerate()
            {
                let entry = bdl.add(index * 4);
                entry.write_volatile(address as u32);
                entry.add(1).write_volatile((address >> 32) as u32);
                entry.add(2).write_volatile(bytes);
                entry.add(3).write_volatile(u32::from(index == 1));
            }
        }
        // SAFETY: `stream` is inside the identity-mapped BAR0 register file.
        unsafe {
            write8(stream, SD_CTL, SDCTL_SRST);
            let mut budget = 1_000_000u32;
            while read8(stream, SD_CTL) & SDCTL_SRST == 0 && budget > 0 {
                budget -= 1;
                core::hint::spin_loop();
            }
            write8(stream, SD_CTL, 0);
            let mut budget = 1_000_000u32;
            while read8(stream, SD_CTL) & SDCTL_SRST != 0 && budget > 0 {
                budget -= 1;
                core::hint::spin_loop();
            }
            write32(stream, SD_CBL, dma_bytes);
            write16(stream, SD_LVI, 1);
            write16(stream, SD_FMT, STREAM_FORMAT);
            write32(stream, SD_BDPL, bdl_phys as u32);
            write32(stream, SD_BDPU, (bdl_phys >> 32) as u32);
            write8(stream, SD_CTL + 2, STREAM_TAG << 4);
            write8(stream, SD_CTL, read8(stream, SD_CTL) | SDCTL_RUN);
        }

        // Wait out the clip: its length plus a small margin, checking that the
        // position moved so a stuck stream is not reported as spoken.
        let duration_ms = stereo_bytes / (BYTES_PER_SEC / 1000);
        let mut moved = 0u32;
        let mut waited = 0u32;
        while waited < duration_ms + 150 {
            if interrupted() {
                break;
            }
            boot::stall(core::time::Duration::from_millis(20));
            waited += 20;
            // SAFETY: reading LPIB is side-effect-free.
            let position = unsafe { read32(stream, SD_LPIB) };
            moved = moved.max(position);
            if position >= stereo_bytes {
                break;
            }
        }
        // SAFETY: clearing RUN on our own stream descriptor.
        unsafe {
            write8(stream, SD_CTL, read8(stream, SD_CTL) & !SDCTL_RUN);
        }
        moved > 0
    }
}

fn reset(base: u64) -> bool {
    // SAFETY: BAR0 is the identity-mapped MMIO window.
    unsafe {
        write32(base, REG_GCTL, read32(base, REG_GCTL) & !GCTL_CRST);
        let mut budget = 10_000_000u32;
        while read32(base, REG_GCTL) & GCTL_CRST != 0 {
            budget -= 1;
            if budget == 0 {
                return false;
            }
            core::hint::spin_loop();
        }
        write32(base, REG_GCTL, read32(base, REG_GCTL) | GCTL_CRST);
        let mut budget = 10_000_000u32;
        while read32(base, REG_GCTL) & GCTL_CRST == 0 {
            budget -= 1;
            if budget == 0 {
                return false;
            }
            core::hint::spin_loop();
        }
    }
    true
}

fn setup_rings(base: u64) {
    let corb_phys = core::ptr::addr_of!(CORB) as u64;
    let rirb_phys = core::ptr::addr_of!(RIRB) as u64;
    // SAFETY: MMIO on a controller that exists; ring bases are identity-mapped.
    unsafe {
        write8(base, REG_CORBCTL, 0);
        write8(base, REG_RIRBCTL, 0);
        write8(base, REG_CORBSIZE, RING_SIZE_256);
        write32(base, REG_CORBLBASE, corb_phys as u32);
        write32(base, REG_CORBUBASE, (corb_phys >> 32) as u32);
        write16(base, REG_CORBRP, CORBRP_RST);
        let mut budget = 1_000_000u32;
        while read16(base, REG_CORBRP) & CORBRP_RST == 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
        }
        write16(base, REG_CORBRP, 0);
        write16(base, REG_CORBWP, 0);
        write8(base, REG_RIRBSIZE, RING_SIZE_256);
        write32(base, REG_RIRBLBASE, rirb_phys as u32);
        write32(base, REG_RIRBUBASE, (rirb_phys >> 32) as u32);
        write16(base, REG_RIRBWP, RIRBWP_RST);
        // A high response-interrupt count: a threshold of 1 stalls the ring after
        // the first response (the bug the kernel driver hit and this avoids).
        write16(base, REG_RINTCNT, 0xff);
        write8(base, REG_RIRBCTL, RIRBCTL_DMAEN);
        write8(base, REG_CORBCTL, CORBCTL_RUN);
    }
}

/// Walk the codec's audio function group and log every widget - node id, type,
/// and connection list, plus each pin's capabilities and configuration default -
/// to the serial mirror. This is diagnostic: it is how the real topology of an
/// unfamiliar codec (VMware's, a physical machine's) is read, so the output path
/// can be routed from data rather than guessed. Widget types: 0 audio output
/// (DAC), 1 audio input, 2 mixer, 3 selector, 4 pin complex.
fn dump_graph(speaker: &mut Speaker) {
    let Ok(root) = speaker.get_parameter(0, PARAM_SUBNODE_COUNT) else {
        return;
    };
    let first_group = ((root >> 16) & 0xff) as u8;
    let group_count = (root & 0xff) as u8;
    for group in 0..group_count {
        let nid = first_group + group;
        if speaker
            .get_parameter(nid, PARAM_FUNCTION_GROUP_TYPE)
            .unwrap_or(0)
            & 0xff
            != 0x01
        {
            continue;
        }
        let Ok(widgets) = speaker.get_parameter(nid, PARAM_SUBNODE_COUNT) else {
            continue;
        };
        let first_widget = ((widgets >> 16) & 0xff) as u8;
        let widget_count = (widgets & 0xff) as u8;
        for index in 0..widget_count {
            let widget = first_widget + index;
            let Ok(wtype) = speaker.widget_type(widget) else {
                continue;
            };
            let len = speaker.connection_len(widget).min(8);
            let mut conns = [0u8; 8];
            for entry in 0..len {
                conns[entry as usize] = speaker.connection_entry(widget, entry);
            }
            let conns = &conns[..len as usize];
            if wtype == WIDGET_PIN_COMPLEX {
                let pincap = speaker.get_parameter(widget, PARAM_PIN_CAP).unwrap_or(0);
                // Get Configuration Default (verb 0xf1c): its "default device"
                // nibble says whether a pin is a line-out/speaker/headphone sink.
                let cfg = speaker
                    .command(widget, VERB_GET_CONFIG_DEFAULT, 0)
                    .unwrap_or(0);
                aw_mark!(
                    "AW_UEFI_HDA_NODE nid={widget} type={wtype} pin pincap=0x{pincap:x} cfg=0x{cfg:08x} conns={conns:?}"
                );
            } else {
                aw_mark!("AW_UEFI_HDA_NODE nid={widget} type={wtype} conns={conns:?}");
            }
        }
    }
}

/// A complete, routable output path from a stream-carrying DAC to a connected
/// output pin, with any single mixer or selector stage between them. Every stage
/// on it must be unmuted for sound to reach the pin.
#[derive(Clone, Copy)]
struct OutputPath {
    dac: u8,
    pin: u8,
    /// The pin's connection index that reaches the DAC (its input selector).
    pin_conn_index: u8,
    /// An intermediate mixer/selector, if the DAC is not on the pin's own list.
    node: Option<u8>,
    /// The DAC's input index on that intermediate node.
    node_input_index: u8,
    /// Whether the intermediate node is a selector (needs a connection-select) or
    /// a mixer (needs its per-input amp unmuted).
    node_is_selector: bool,
}

/// Rank a pin as an output sink, or `None` if it is not one: it must be output
/// capable, physically connected (config default not "no connection"), and its
/// default device an output. Lower rank is preferred: speaker, then line-out,
/// then headphone - the order most likely to reach a machine's actual speakers.
fn output_pin_rank(speaker: &mut Speaker, pin: u8) -> Option<u8> {
    if speaker.get_parameter(pin, PARAM_PIN_CAP).unwrap_or(0) & (1 << 4) == 0 {
        return None;
    }
    let cfg = speaker
        .command(pin, VERB_GET_CONFIG_DEFAULT, 0)
        .unwrap_or(0);
    if (cfg >> 30) & 0x3 == 0x1 {
        return None; // "no physical connection"
    }
    match (cfg >> 20) & 0xf {
        0x1 => Some(0), // speaker
        0x0 => Some(1), // line out
        0x2 => Some(2), // headphone out
        _ => None,
    }
}

/// Trace a route from `pin` back to a DAC: directly if the DAC is on the pin's
/// connection list, otherwise through one mixer or selector level. Returns the
/// full path, or `None` if the pin does not reach a DAC.
fn route_pin(speaker: &mut Speaker, pin: u8) -> Option<OutputPath> {
    let mut pins = [0u8; 16];
    let n = speaker.connections(pin, &mut pins);
    for (i, &src) in pins.iter().enumerate().take(n) {
        if speaker.widget_type(src).ok()? == WIDGET_AUDIO_OUTPUT {
            return Some(OutputPath {
                dac: src,
                pin,
                pin_conn_index: i as u8,
                node: None,
                node_input_index: 0,
                node_is_selector: false,
            });
        }
    }
    for (i, &mid) in pins.iter().enumerate().take(n) {
        let mid_type = speaker.widget_type(mid).ok()?;
        if mid_type != WIDGET_AUDIO_MIXER && mid_type != WIDGET_AUDIO_SELECTOR {
            continue;
        }
        let mut mids = [0u8; 16];
        let m = speaker.connections(mid, &mut mids);
        for (j, &src) in mids.iter().enumerate().take(m) {
            if speaker.widget_type(src).ok()? == WIDGET_AUDIO_OUTPUT {
                return Some(OutputPath {
                    dac: src,
                    pin,
                    pin_conn_index: i as u8,
                    node: Some(mid),
                    node_input_index: j as u8,
                    node_is_selector: mid_type == WIDGET_AUDIO_SELECTOR,
                });
            }
        }
    }
    None
}

fn find_output(speaker: &mut Speaker) -> Result<OutputPath, &'static str> {
    let root = speaker.get_parameter(0, PARAM_SUBNODE_COUNT)?;
    let first_group = ((root >> 16) & 0xff) as u8;
    let group_count = (root & 0xff) as u8;
    for group in 0..group_count {
        let fg = first_group + group;
        if speaker.get_parameter(fg, PARAM_FUNCTION_GROUP_TYPE)? & 0xff != 0x01 {
            continue;
        }
        speaker.set(fg, VERB_SET_POWER_STATE, 0)?;
        let widgets = speaker.get_parameter(fg, PARAM_SUBNODE_COUNT)?;
        let first_widget = ((widgets >> 16) & 0xff) as u8;
        let widget_count = (widgets & 0xff) as u8;

        // Prefer the best-ranked connected output pin (speaker, then line-out,
        // then headphone) and route it to a DAC through any intermediate stage.
        for rank_target in 0u8..=2 {
            for index in 0..widget_count {
                let widget = first_widget + index;
                if speaker.widget_type(widget)? != WIDGET_PIN_COMPLEX {
                    continue;
                }
                if output_pin_rank(speaker, widget) == Some(rank_target)
                    && let Some(path) = route_pin(speaker, widget)
                {
                    return Ok(path);
                }
            }
        }

        // Fallback for a codec with blank config defaults (QEMU's): the first
        // output-capable pin routed to the first DAC, directly or through a stage.
        for index in 0..widget_count {
            let widget = first_widget + index;
            if speaker.widget_type(widget)? == WIDGET_PIN_COMPLEX
                && speaker.get_parameter(widget, PARAM_PIN_CAP)? & (1 << 4) != 0
                && let Some(path) = route_pin(speaker, widget)
            {
                return Ok(path);
            }
        }
    }
    Err("no_output_path")
}

/// Find and bring up an HDA controller with a codec and an output path, ready to
/// speak. Returns `None` (and logs why) when there is no usable audio, so the
/// caller can fall back to the PC speaker.
pub fn bring_up() -> Option<Speaker> {
    let mut found: Option<PciLocation> = None;
    'scan: for bus in 0..=255u16 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let location = PciLocation {
                    bus: bus as u8,
                    device,
                    function,
                };
                if is_hda(location) {
                    found = Some(location);
                    break 'scan;
                }
            }
        }
    }
    let location = found?;
    let base = enable_bar0(location)?;
    if !reset(base) {
        log::error!("AW_UEFI_HDA_FAIL reason=reset");
        return None;
    }
    // SAFETY: BAR0 is the identity-mapped MMIO window.
    let gcap = unsafe { read16(base, REG_GCAP) };
    setup_rings(base);

    // SAFETY: reading STATESTS has no side effects.
    let statests = unsafe {
        let mut budget = 1_000_000u32;
        let mut bits = read16(base, REG_STATESTS);
        while bits == 0 && budget > 0 {
            budget -= 1;
            core::hint::spin_loop();
            bits = read16(base, REG_STATESTS);
        }
        bits
    };
    if statests == 0 {
        log::info!("AW_UEFI_HDA_UNAVAILABLE");
        return None;
    }

    let mut speaker = Speaker {
        base,
        codec: statests.trailing_zeros() as u8,
        input_streams: ((gcap >> 8) & 0xf) as u8,
        dac: 0,
        pin: 0,
        rirb_read: 0,
    };

    let vendor = match speaker.get_parameter(0, PARAM_VENDOR_ID) {
        Ok(vendor) if vendor != 0 && vendor != 0xffff_ffff => vendor,
        _ => {
            log::error!("AW_UEFI_HDA_FAIL reason=codec");
            return None;
        }
    };
    aw_mark!("AW_UEFI_HDA_CODEC_ID vendor_device=0x{vendor:08x}");

    // Diagnostic: log the real codec graph so an unfamiliar codec's output path
    // can be routed from data. Cheap, one-time, and only on the serial mirror.
    dump_graph(&mut speaker);

    let path = match find_output(&mut speaker) {
        Ok(path) => path,
        Err(reason) => {
            log::error!("AW_UEFI_HDA_FAIL reason={reason}");
            return None;
        }
    };
    speaker.dac = path.dac;
    speaker.pin = path.pin;

    // Configure the whole path once (the per-clip format is set in `speak`). Every
    // stage must be powered and unmuted: the DAC, the pin, and - the piece a real
    // codec needs that a trivial one does not - the mixer or selector between them,
    // whose amp is muted at reset and silences the output until unmuted/selected.
    let stage_ok = if let Some(node) = path.node {
        let base = speaker.set(node, VERB_SET_POWER_STATE, 0).is_ok()
            && speaker.unmute_output(node).is_ok();
        base && if path.node_is_selector {
            speaker
                .set(
                    node,
                    VERB_SET_CONNECTION_SELECT,
                    u32::from(path.node_input_index),
                )
                .is_ok()
        } else {
            speaker.unmute_input(node, path.node_input_index).is_ok()
        }
    } else {
        true
    };

    let configured = speaker.set(path.dac, VERB_SET_POWER_STATE, 0).is_ok()
        && speaker
            .set(
                path.dac,
                VERB_SET_STREAM_CHANNEL,
                u32::from(STREAM_TAG) << 4,
            )
            .is_ok()
        && speaker.unmute_output(path.dac).is_ok()
        && stage_ok
        && speaker
            .set(
                path.pin,
                VERB_SET_CONNECTION_SELECT,
                u32::from(path.pin_conn_index),
            )
            .is_ok()
        && speaker.set(path.pin, VERB_SET_POWER_STATE, 0).is_ok()
        && speaker
            .set(path.pin, VERB_SET_PIN_CONTROL, PIN_CONTROL_OUT_ENABLE)
            .is_ok()
        && speaker.set(path.pin, VERB_SET_EAPD, EAPD_ENABLE).is_ok()
        && speaker.unmute_output(path.pin).is_ok();
    if !configured {
        log::error!("AW_UEFI_HDA_FAIL reason=configure");
        return None;
    }
    match path.node {
        Some(node) => aw_mark!(
            "AW_UEFI_HDA_READY dac={} pin={} via={} conn={}",
            path.dac,
            path.pin,
            node,
            path.pin_conn_index
        ),
        None => aw_mark!("AW_UEFI_HDA_READY dac={} pin={}", path.dac, path.pin),
    }
    Some(speaker)
}
