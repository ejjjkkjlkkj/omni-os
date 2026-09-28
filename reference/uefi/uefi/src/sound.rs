//! Pre-boot audible feedback through the PC speaker - the first sound a blind
//! user hears from Accessible Windows, before the kernel exists.
//!
//! A screen reader that only writes text to the console is not usable by a blind
//! person on its own: nothing is perceivable without sight. Speaking the boot
//! screen aloud is the goal, but real speech needs an audio codec driver (Intel
//! HDA) and a synthesizer, which is a large body of work. This is the first,
//! universally available audible layer underneath that: the PC speaker, driven
//! the classic way through PIT channel 2 and port 0x61 (the same timer/port the
//! kernel's clock calibration already uses). It cannot speak words, but it can
//! tell a blind user, audibly and immediately, that the accessible boot has come
//! up and respond to their keys - so the fixed boot flow is operable by ear.
//!
//! Honest limitation: some thin laptops have no PC-speaker buzzer at all, and on
//! those this is silent even though the interface is driven correctly (the port
//! bits still latch). Audible output on every machine is what the later HDA +
//! speech-synthesis work delivers; this is the provable foundation and works on
//! QEMU, desktops and most systems with a beeper.

use core::time::Duration;

use uefi::boot;

/// PIT mode/command register.
const PIT_MODE_PORT: u16 = 0x43;
/// PIT channel 2 data register (the channel wired to the speaker).
const PIT_CH2_PORT: u16 = 0x42;
/// Port B of the legacy keyboard controller: bit 0 gates PIT channel 2, bit 1
/// connects it to the speaker; bit 5 reflects channel 2's output.
const PORT_61: u16 = 0x61;
/// PIT input frequency in hertz; a tone's divisor is this over the tone's pitch.
const PIT_INPUT_HZ: u32 = 1_193_182;
/// Bits 0 and 1 of port 0x61: channel-2 gate on, speaker data connected.
const SPEAKER_ENABLE: u8 = 0b11;

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: caller names a valid byte-wide port; these are the legacy PIT and
    // keyboard-controller ports, driven before ExitBootServices.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value,
            options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: caller names a valid byte-wide port.
    unsafe {
        core::arch::asm!("in al, dx", out("al") value, in("dx") port,
            options(nomem, nostack, preserves_flags));
    }
    value
}

/// Program PIT channel 2 to `frequency` hertz and connect it to the speaker.
/// Returns true when port 0x61 reads back with the speaker enabled, i.e. the
/// interface was actually driven (audibility then depends on the hardware).
fn start_tone(frequency: u32) -> bool {
    if frequency == 0 {
        return false;
    }
    let divisor = PIT_INPUT_HZ / frequency;
    if divisor == 0 || divisor > u32::from(u16::MAX) {
        return false;
    }
    let divisor = divisor as u16;

    // SAFETY: legacy PIT/port-0x61 access before ExitBootServices; channel 2 is
    // the speaker and is otherwise unused here, mirroring the kernel's clock code.
    unsafe {
        // Channel 2, access lo then hi byte, mode 3 (square wave), binary.
        outb(PIT_MODE_PORT, 0xB6);
        outb(PIT_CH2_PORT, (divisor & 0xFF) as u8);
        outb(PIT_CH2_PORT, (divisor >> 8) as u8);
        let prior = inb(PORT_61);
        outb(PORT_61, prior | SPEAKER_ENABLE);
        inb(PORT_61) & SPEAKER_ENABLE == SPEAKER_ENABLE
    }
}

/// Disconnect the speaker. Returns true when port 0x61 reads back silenced.
fn stop_tone() -> bool {
    // SAFETY: as `start_tone`.
    unsafe {
        let prior = inb(PORT_61);
        outb(PORT_61, prior & !SPEAKER_ENABLE);
        inb(PORT_61) & SPEAKER_ENABLE == 0
    }
}

/// Play one short tone as operator feedback, best effort: no markers, and a
/// machine with no beeper simply stays quiet. Used for keyboard cues, which only
/// happen when someone is actually pressing keys.
pub fn cue(frequency: u32, duration: Duration) {
    if start_tone(frequency) {
        boot::stall(duration);
    }
    let _ = stop_tone();
}

/// Sound the startup chime and prove the speaker interface was driven: two
/// ascending tones, with a read-back of port 0x61 confirming the speaker was
/// connected (`AW_UEFI_SND_GATED`) and then disconnected (`AW_UEFI_SND_SILENCED`).
/// This is the audible "the accessible boot is up" signal a blind user hears.
/// Returns false, and emits `AW_UEFI_SND_FAIL`, only if the interface did not
/// read back as driven - which a working PC speaker or emulator never does.
pub fn startup_chime() -> bool {
    log::info!("AW_UEFI_SND_BEGIN");

    if !start_tone(660) {
        log::error!("AW_UEFI_SND_FAIL reason=gate freq=660");
        return false;
    }
    log::info!("AW_UEFI_SND_TONE freq=660");
    log::info!("AW_UEFI_SND_GATED");
    boot::stall(Duration::from_millis(140));

    if !start_tone(990) {
        log::error!("AW_UEFI_SND_FAIL reason=gate freq=990");
        let _ = stop_tone();
        return false;
    }
    log::info!("AW_UEFI_SND_TONE freq=990");
    boot::stall(Duration::from_millis(160));

    if !stop_tone() {
        log::error!("AW_UEFI_SND_FAIL reason=silence");
        return false;
    }
    log::info!("AW_UEFI_SND_SILENCED");

    log::info!("AW_UEFI_SND_PROOF_OK");
    true
}
