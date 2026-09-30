//! Firmware-stage screen reader: the first moment omni-os speaks, and
//! the first surface a user can operate - before the kernel is even loaded.
//!
//! The kernel's screen-reader proof voices the installer's welcome dialog, but
//! that runs after ExitBootServices, once the operating system is up. This runs
//! earlier still, inside the UEFI boot application, so the accessibility contract
//! holds from the first stage the project controls. It does three things a real
//! screen reader must do, this early:
//!
//! 1. **Speaks on a surface a person can actually perceive.** The utterances are
//!    written to the UEFI text console (`ConOut`), which is the physical screen on
//!    real hardware, not only to the QEMU debug port. Each is also emitted on the
//!    debug console as an `AW_UEFI_SR_*` marker, the machine-checkable nonvisual
//!    delivery evidence the boot proofs assert.
//! 2. **Describes the real machine.** The display line carries the resolution the
//!    firmware actually reported through GOP, not a placeholder, so what is spoken
//!    matches what is there.
//! 3. **Is operable by keyboard, with no pointer.** After reading the boot screen
//!    top to bottom it presents an accessible boot menu - Start omni-os,
//!    Reboot, Shut down - spoken through the same engine: the up and down arrows (or
//!    Tab) move between items and speak each landing with its position, Enter selects
//!    it, and Escape takes the safe default of starting the operating system. The
//!    keyboard here is the firmware's own, so a USB keyboard works before any kernel
//!    USB stack exists - the one interaction guaranteed on every machine. If nobody
//!    is there - an unattended or automated boot - a countdown starts the operating
//!    system on its own after a short window, so the machine never hangs waiting for
//!    a key that will not come; once a key is pressed the countdown stops and the
//!    menu waits for a deliberate choice.
//!
//! Every utterance comes from the one allocation-free announcement engine the
//! kernel and installer use ([`aw_screen_reader`]) over the same validated
//! semantics ([`aw_accessibility`]): the wording cannot drift between boot,
//! installer and desktop, because there is one engine, unit-tested once and proven
//! here to run unchanged this early.

extern crate alloc;

use alloc::string::String;

use aw_accessibility::{NodeId, Rect, Role, SemanticNode, State, validate_node};
use aw_screen_reader::{FocusContext, announce_focus};
use uefi::system;

use crate::audio;
use crate::aw_mark;
use crate::hda;
use crate::serial;
use crate::sound;

/// Build one node of the firmware boot screen. Every node hangs off an implicit
/// root (id 0), the way the kernel proof frames its dialog.
fn node<'a>(id: u64, role: Role, name: &'a str) -> SemanticNode<'a> {
    SemanticNode {
        id: NodeId(id),
        parent: Some(NodeId(0)),
        role,
        name,
        description: "",
        value: "",
        state: State::from_bits(0),
        // The firmware screen is text a user hears, not a laid-out control
        // surface, but a plausible non-zero rectangle keeps every node valid.
        bounds: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 32,
        },
    }
}

/// Compose the utterance for `node`, or `None` if it violates an accessibility
/// invariant (which for these compile-time-constant nodes means a code bug). The
/// utterance is written into the caller's buffer so nothing is allocated.
fn utterance<'b>(node: &SemanticNode<'_>, buffer: &'b mut [u8]) -> Option<&'b str> {
    if validate_node(node).is_err() {
        log::error!("AW_UEFI_SR_FAIL reason=invalid_node");
        return None;
    }
    let text = announce_focus(node, FocusContext::NONE, buffer);
    if text.is_empty() {
        log::error!("AW_UEFI_SR_FAIL reason=empty_utterance");
        return None;
    }
    Some(text)
}

/// Print one spoken line on the visible console: what a speech engine would say,
/// clean and without the logger's source-location noise.
fn show(text: &str) {
    uefi::println!("  {text}");
}

/// Play a line's pre-recorded speech clip through the HDA codec, when audio is
/// available and the line has one (the dynamic display line does not). This is
/// what a blind user actually hears - the words, on the machine's real speakers.
fn play_clip(clip: Option<&'static [u8]>, speaker: &mut Option<audio::Speaker>) {
    if let (Some(sp), Some(data)) = (speaker.as_mut(), clip)
        && sp.speak(data)
    {
        aw_mark!("AW_UEFI_AUDIO_SPEAK bytes={}", data.len());
    }
}

/// Speak one node for the first time: show it on the console, emit its
/// `AW_UEFI_SR_SPEAK` marker, and say it aloud through HDA. Returns false on an
/// invariant violation.
fn speak(
    node: &SemanticNode<'_>,
    clip: Option<&'static [u8]>,
    speaker: &mut Option<audio::Speaker>,
) -> bool {
    let mut buffer = [0u8; 128];
    let Some(text) = utterance(node, &mut buffer) else {
        return false;
    };
    show(text);
    aw_mark!("AW_UEFI_SR_SPEAK \"{text}\"");
    play_clip(clip, speaker);
    true
}

/// Voice the firmware boot screen through the native screen reader, on the visible
/// console, and let the user review it and continue by keyboard - all before the
/// kernel is loaded and boot services end. `width`/`height` are the display mode
/// the firmware reported, so the machine is described as it actually is.
///
/// Deterministic apart from the display line, and safe unattended: with nobody at
/// the keyboard it reads the screen and continues on its own. On an accessibility
/// invariant violation it emits `AW_UEFI_SR_FAIL` and withholds the proof marker
/// rather than claiming success, but it still lets the machine boot - stranding a
/// user at a dead firmware screen would be the worse failure.
pub fn run(width: usize, height: usize) {
    // Arm the COM1 mirror before the first marker, so the whole firmware-stage
    // screen reader is captured on machines with no 0xE9 debug port (VMware,
    // physical hardware). A no-op where COM1 is absent, so the QEMU proofs are
    // unaffected.
    serial::init();

    aw_mark!("AW_UEFI_SR_BEGIN");

    // Bring up the machine's real audio (HDA) once: each line is then spoken aloud
    // through it. On a thin laptop with no PC-speaker buzzer this codec is the only
    // thing that will actually sound; when there is no HDA controller, the PC
    // speaker plays a chime so at least the boot is audibly confirmed.
    let mut speaker = audio::bring_up();
    match &speaker {
        // A real codec (HDA or AC'97) came up: announce which backend is speaking, so the
        // boot proof and a field log record the active audio channel.
        Some(sp) => {
            sound::set_voice_present();
            aw_mark!("AW_UEFI_AUDIO_BACKEND channel={}", sp.backend());
        }
        // No codec at all: the PC speaker is the universal fallback.
        None => {
            aw_mark!("AW_UEFI_AUDIO_BACKEND channel=pc_speaker");
            sound::startup_chime();
        }
    }

    // A clean surface for the spoken screen: the boot log lives on the debug
    // console, so clearing here only affects what a person sees on the display.
    let _ = system::with_stdout(|stdout| stdout.clear());
    uefi::println!("omni-os");
    uefi::println!();

    // The display line is built from the real GOP mode. It is the one line that
    // is not compile-time constant, so it is the one line the proofs do not pin.
    let display: String = alloc::format!("Display {width} by {height}");

    // What a user hears the instant the firmware hands control to us: the system
    // names itself, confirms the screen reader is already live, states the real
    // display mode, and narrates the one thing this stage does - load the OS.
    let screen = [
        node(1, Role::Window, "omni-os"),
        node(
            2,
            Role::StaticText,
            "Screen reader active at firmware stage",
        ),
        node(3, Role::StaticText, "Starting omni-os"),
        node(4, Role::StaticText, &display),
        node(5, Role::StaticText, "Loading the operating system"),
    ];
    // Pre-recorded speech for each line, in order; the dynamic display line has no
    // clip and is spoken on the console (the runtime formant synthesizer in
    // `synth.rs` now voices dynamic values inside the setup that follows).
    let clips: [Option<&'static [u8]>; 5] = [
        Some(hda::CLIP_WELCOME),
        Some(hda::CLIP_ACTIVE),
        Some(hda::CLIP_STARTING),
        None,
        Some(hda::CLIP_LOADING),
    ];

    for (line, clip) in screen.iter().zip(clips.iter()) {
        if !speak(line, *clip, &mut speaker) {
            // A constant node failed to validate: a bug, not a runtime condition.
            // Report it and skip the success marker, but keep booting.
            return;
        }
    }

    // The boot screen has been read aloud; now present the complete accessible
    // firmware setup the user actually operates - a tabbed Setup Utility (Main,
    // Advanced, Boot, Security, Save and Exit) modeled on AMI Aptio and the ASUS
    // UEFI BIOS Utility, but spoken and driven from the firmware's own keyboard, so
    // it works on every machine before any kernel USB stack exists.
    crate::setup::run(width, height, &mut speaker);

    aw_mark!("AW_UEFI_SR_PROOF_OK");
}
