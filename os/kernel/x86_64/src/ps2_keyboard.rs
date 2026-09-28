//! PS/2 keyboard (i8042) input, proved by real IRQ1 delivery (roadmap Phase 3
//! "USB HID keyboard" - the 8042 path is the one that works before a USB stack).
//!
//! A blind user cannot operate an installer they cannot drive. The framebuffer
//! console gives the machine a voice on screen; this gives it ears. On a physical
//! machine with "USB legacy support" enabled - the firmware default on most
//! desktops and many laptops - a USB keyboard is presented through the same 8042
//! controller and the same IRQ1 as a real PS/2 keyboard, so this driver reads it
//! without a USB stack. QEMU emulates the 8042 directly.
//!
//! The proof holds to [`crate::irq_proof`]'s bar in spirit but drives it by hand,
//! because a keyboard is not free-running: the 8042's own `0xD2` command ("write
//! a byte to the output buffer as if the keyboard sent it") injects a scancode
//! that raises a real IRQ1 edge, the exact path a keypress takes (device ->
//! output buffer -> IRQ1 -> ISR -> port 0x60 read -> decode). A known scancode is
//! injected, the handler must receive it and decode it to the expected key, a
//! mask test proves the counter is driven by delivery and not polling, and
//! delivery must resume once unmasked. On hardware, real keys take the identical
//! path; nothing here is claimed from a status bit.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};

use aw_kernel_core::KernelHandoff;

use crate::acpi;
use crate::debug_write;
use crate::device_irq;
use crate::ioapic::IoApic;
use crate::local_apic::x2apic_eoi;

// 8042 controller ports.
const DATA: u16 = 0x60;
const STATUS_CMD: u16 = 0x64;

// Status register bits.
const STATUS_OUTPUT_FULL: u8 = 1 << 0; // OBF: a byte is waiting to be read
const STATUS_INPUT_FULL: u8 = 1 << 1; // IBF: the controller has not read our last write

// Controller commands (written to 0x64).
const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_DISABLE_MOUSE_PORT: u8 = 0xA7;
const CMD_DISABLE_KBD_PORT: u8 = 0xAD;
const CMD_ENABLE_KBD_PORT: u8 = 0xAE;
const CMD_SELF_TEST: u8 = 0xAA; // -> 0x55 on success
const CMD_KBD_IFACE_TEST: u8 = 0xAB; // -> 0x00 on success
const CMD_WRITE_OUTPUT_BUFFER: u8 = 0xD2; // next data byte appears as keyboard input

// Config byte bits.
const CONFIG_KBD_INTERRUPT: u8 = 1 << 0;
const CONFIG_MOUSE_INTERRUPT: u8 = 1 << 1;
const CONFIG_MOUSE_CLOCK_DISABLE: u8 = 1 << 5;
const CONFIG_TRANSLATE: u8 = 1 << 6; // present set-1 scancodes regardless of the device set

const SELF_TEST_OK: u8 = 0x55;
const IFACE_TEST_OK: u8 = 0x00;

/// ISA IRQ the keyboard raises.
const KBD_ISA_IRQ: u8 = 1;

/// A bounded wait so a wedged controller fails a proof in a moment instead of
/// hanging the boot.
const IO_SPIN_BUDGET: u32 = 500_000;

/// Set-1 scancode prefix introducing an extended key (arrows, etc.).
const EXTENDED_PREFIX: u8 = 0xE0;
/// Set-1 codes are made larger by this bit on release; we act on presses only.
const BREAK_BIT: u8 = 0x80;

// SAFETY wrappers around the two architected 8042 ports.
unsafe fn outb(port: u16, value: u8) {
    // SAFETY: caller names a valid byte-wide port.
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

/// A decoded key. Only the keys the accessible boot menu needs are named; any
/// other printable make maps to [`Key::Char`], and everything else is dropped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Tab,
    Enter,
    Space,
    Escape,
    Backspace,
    Char(u8),
}

impl Key {
    /// A stable small integer for the atomic "last key" slot the proof reads.
    fn code(self) -> u32 {
        match self {
            Key::Up => 1,
            Key::Down => 2,
            Key::Left => 3,
            Key::Right => 4,
            Key::Tab => 5,
            Key::Enter => 6,
            Key::Space => 7,
            Key::Escape => 8,
            Key::Backspace => 9,
            Key::Char(c) => 0x1000 | u32::from(c),
        }
    }
}

/// Translate a set-1 make code (non-extended) into a [`Key`].
fn decode_basic(make: u8) -> Option<Key> {
    Some(match make {
        0x01 => Key::Escape,
        0x0E => Key::Backspace,
        0x0F => Key::Tab,
        0x1C => Key::Enter,
        0x39 => Key::Space,
        other => return decode_char(other).map(Key::Char),
    })
}

/// Translate a set-1 extended (0xE0-prefixed) make code into a [`Key`].
fn decode_extended(make: u8) -> Option<Key> {
    Some(match make {
        0x48 => Key::Up,
        0x50 => Key::Down,
        0x4B => Key::Left,
        0x4D => Key::Right,
        0x1C => Key::Enter, // keypad Enter
        _ => return None,
    })
}

/// Map a set-1 make code to its unshifted ASCII, for the letters and digits the
/// menu might use. Deliberately small; the menu is driven by the named keys.
fn decode_char(make: u8) -> Option<u8> {
    const ROW: &[(u8, u8)] = &[
        (0x02, b'1'), (0x03, b'2'), (0x04, b'3'), (0x05, b'4'), (0x06, b'5'),
        (0x07, b'6'), (0x08, b'7'), (0x09, b'8'), (0x0A, b'9'), (0x0B, b'0'),
        (0x10, b'q'), (0x11, b'w'), (0x12, b'e'), (0x13, b'r'), (0x14, b't'),
        (0x15, b'y'), (0x16, b'u'), (0x17, b'i'), (0x18, b'o'), (0x19, b'p'),
        (0x1E, b'a'), (0x1F, b's'), (0x20, b'd'), (0x21, b'f'), (0x22, b'g'),
        (0x23, b'h'), (0x24, b'j'), (0x25, b'k'), (0x26, b'l'),
        (0x2C, b'z'), (0x2D, b'x'), (0x2E, b'c'), (0x2F, b'v'), (0x30, b'b'),
        (0x31, b'n'), (0x32, b'm'),
    ];
    ROW.iter().find(|&&(sc, _)| sc == make).map(|&(_, ch)| ch)
}

/// Deliveries counted by the ISR, and the last key it decoded. The proof reads
/// both; the menu reads the ring buffer below.
static KBD_TICKS: AtomicU64 = AtomicU64::new(0);
static LAST_KEY: AtomicU32 = AtomicU32::new(0);
/// Whether the previous byte was the 0xE0 extended prefix.
static EXTENDED_PENDING: AtomicU8 = AtomicU8::new(0);

/// A tiny single-producer/single-consumer ring of decoded keys. The ISR is the
/// only producer and runs with interrupts disabled; the consumer masks interrupts
/// briefly around a pop, so on this single core no access overlaps.
const RING_LEN: usize = 32;
struct KeyRing {
    buf: UnsafeCell<[u32; RING_LEN]>,
    head: UnsafeCell<usize>,
    tail: UnsafeCell<usize>,
}
// SAFETY: all access is serialized on one core - the ISR runs in a cli context,
// the consumer brackets its access with cli/sti.
unsafe impl Sync for KeyRing {}
static RING: KeyRing = KeyRing {
    buf: UnsafeCell::new([0; RING_LEN]),
    head: UnsafeCell::new(0),
    tail: UnsafeCell::new(0),
};

fn ring_push(code: u32) {
    // SAFETY: called only from the ISR (interrupts already disabled).
    unsafe {
        let head = &mut *RING.head.get();
        let tail = *RING.tail.get();
        let next = (*head + 1) % RING_LEN;
        if next != tail {
            (*RING.buf.get())[*head] = code;
            *head = next;
        }
        // A full ring drops the oldest-unread policy in favour of dropping the
        // newest, which is fine for a boot menu: a held key cannot wedge it.
    }
}

crate::device_interrupt_stub!(aw_kbd_isr, kbd_dispatch);

extern "C" fn kbd_dispatch() {
    // SAFETY: interrupt context; reading the 8042 data port acknowledges the byte.
    let byte = unsafe { inb(DATA) };

    if byte == EXTENDED_PREFIX {
        EXTENDED_PENDING.store(1, Ordering::Relaxed);
    } else {
        let extended = EXTENDED_PENDING.swap(0, Ordering::Relaxed) != 0;
        if byte & BREAK_BIT == 0 {
            let key = if extended {
                decode_extended(byte)
            } else {
                decode_basic(byte)
            };
            if let Some(key) = key {
                LAST_KEY.store(key.code(), Ordering::Relaxed);
                KBD_TICKS.fetch_add(1, Ordering::Relaxed);
                ring_push(key.code());
            }
        }
    }

    // SAFETY: CPL0 interrupt context on a CPU whose x2APIC is enabled; EOI before
    // iretq or the local APIC keeps this priority level blocked.
    unsafe { x2apic_eoi() };
}

#[must_use]
fn ticks() -> u64 {
    KBD_TICKS.load(Ordering::Acquire)
}

/// Pop one decoded key, or `None` if the buffer is empty. Safe to call with
/// interrupts enabled; it masks them briefly to fence out the ISR.
#[must_use]
pub fn poll_key() -> Option<Key> {
    // SAFETY: CPL0. Masking interrupts around the read fences out the ISR on this
    // single core; the flags are restored to whatever they were.
    let flags = unsafe { save_and_disable_interrupts() };
    let code = unsafe {
        let tail = &mut *RING.tail.get();
        let head = *RING.head.get();
        if *tail == head {
            None
        } else {
            let code = (*RING.buf.get())[*tail];
            *tail = (*tail + 1) % RING_LEN;
            Some(code)
        }
    };
    // SAFETY: restore the caller's interrupt flag.
    unsafe { restore_interrupts(flags) };
    code.map(key_from_code)
}

fn key_from_code(code: u32) -> Key {
    match code {
        1 => Key::Up,
        2 => Key::Down,
        3 => Key::Left,
        4 => Key::Right,
        5 => Key::Tab,
        6 => Key::Enter,
        7 => Key::Space,
        8 => Key::Escape,
        9 => Key::Backspace,
        c if c & 0x1000 != 0 => Key::Char((c & 0xff) as u8),
        _ => Key::Escape,
    }
}

/// # Safety
/// CPL0. Returns the prior RFLAGS so [`restore_interrupts`] can reinstate IF.
unsafe fn save_and_disable_interrupts() -> u64 {
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}; cli", out(reg) flags,
            options(nomem, preserves_flags));
    }
    flags
}

/// # Safety
/// CPL0. `flags` must come from [`save_and_disable_interrupts`].
unsafe fn restore_interrupts(flags: u64) {
    if flags & (1 << 9) != 0 {
        unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
    }
}

/// Wait for the controller's input buffer to drain before writing to it.
fn wait_input_clear() -> bool {
    let mut budget = IO_SPIN_BUDGET;
    // SAFETY: reading the status port has no side effects.
    while unsafe { inb(STATUS_CMD) } & STATUS_INPUT_FULL != 0 {
        budget -= 1;
        if budget == 0 {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

/// Wait for a byte to be available in the output buffer.
fn wait_output_full() -> bool {
    let mut budget = IO_SPIN_BUDGET;
    // SAFETY: reading the status port has no side effects.
    while unsafe { inb(STATUS_CMD) } & STATUS_OUTPUT_FULL == 0 {
        budget -= 1;
        if budget == 0 {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

/// Send a command byte to the controller (port 0x64).
fn command(cmd: u8) -> bool {
    if !wait_input_clear() {
        return false;
    }
    // SAFETY: 0x64 is the architected 8042 command port.
    unsafe { outb(STATUS_CMD, cmd) };
    true
}

/// Send a data byte to the controller (port 0x60).
fn write_data(value: u8) -> bool {
    if !wait_input_clear() {
        return false;
    }
    // SAFETY: 0x60 is the architected 8042 data port.
    unsafe { outb(DATA, value) };
    true
}

/// Read one byte from the output buffer if one is waiting; used to flush.
fn drain_output() {
    // SAFETY: reading status then data acknowledges any pending byte.
    while unsafe { inb(STATUS_CMD) } & STATUS_OUTPUT_FULL != 0 {
        let _ = unsafe { inb(DATA) };
    }
}

/// Inject one scancode through the controller so it appears as keyboard input
/// and raises a real IRQ1 - the mechanism the proof and, on hardware, nothing
/// else uses (real keys come from the keyboard itself).
fn inject_scancode(scancode: u8) -> bool {
    command(CMD_WRITE_OUTPUT_BUFFER) && write_data(scancode)
}

/// Bring the 8042 up: quiesce both ports, self-test the controller and the
/// keyboard interface, then program the config byte to raise IRQ1 on keypress
/// and hand us translated set-1 scancodes. Returns false with a marker if any
/// step's readback does not confirm.
///
/// # Safety
/// CPL0, single core, during bring-up.
unsafe fn init_controller() -> bool {
    unsafe {
        // Quiesce: disable both device ports so nothing arrives mid-setup, and
        // flush anything already latched.
        command(CMD_DISABLE_KBD_PORT);
        command(CMD_DISABLE_MOUSE_PORT);
        drain_output();

        // Controller self-test. Some controllers reset their config here, so the
        // config byte is written afterwards.
        if !command(CMD_SELF_TEST) || !wait_output_full() {
            debug_write("AW_KBD_FAIL reason=self_test_no_reply\n");
            return false;
        }
        let st = inb(DATA);
        if st != SELF_TEST_OK {
            debug_write("AW_KBD_FAIL reason=self_test\n");
            return false;
        }

        // Keyboard interface (clock/data line) test.
        if !command(CMD_KBD_IFACE_TEST) || !wait_output_full() {
            debug_write("AW_KBD_FAIL reason=iface_no_reply\n");
            return false;
        }
        if inb(DATA) != IFACE_TEST_OK {
            debug_write("AW_KBD_FAIL reason=iface_test\n");
            return false;
        }

        // Enable the keyboard port and program the config byte: keyboard IRQ on,
        // mouse IRQ off, mouse clock disabled, translation on (set-1 codes).
        command(CMD_ENABLE_KBD_PORT);
        let config = (CONFIG_KBD_INTERRUPT | CONFIG_TRANSLATE | CONFIG_MOUSE_CLOCK_DISABLE)
            & !CONFIG_MOUSE_INTERRUPT;
        if !command(CMD_WRITE_CONFIG) || !write_data(config) {
            debug_write("AW_KBD_FAIL reason=write_config\n");
            return false;
        }
        // Read it back so a controller that ignored the write is caught.
        if !command(CMD_READ_CONFIG) || !wait_output_full() {
            debug_write("AW_KBD_FAIL reason=read_config\n");
            return false;
        }
        let readback = inb(DATA);
        if readback & CONFIG_KBD_INTERRUPT == 0 || readback & CONFIG_TRANSLATE == 0 {
            debug_write("AW_KBD_FAIL reason=config_not_applied\n");
            return false;
        }

        // Tell the keyboard itself to start scanning, so real keypresses arrive on
        // hardware. Best-effort: QEMU and the injection proof do not need it, and a
        // controller with no physical keyboard must not fail bring-up over a
        // missing ACK.
        let _ = write_data(0xF4); // enable scanning; keyboard replies 0xFA
        drain_output();
    }
    debug_write("AW_KBD_CONTROLLER_OK\n");
    true
}

/// A programmed IRQ1 route the driver can mask/unmask. Stored so the menu can
/// re-arm the keyboard after the proof leaves it masked.
#[derive(Clone, Copy)]
struct KbdRoute {
    io_apic: IoApic,
    redirection_index: u32,
}

struct RouteCell(UnsafeCell<Option<KbdRoute>>);
// SAFETY: written once during single-core bring-up, read afterwards on the BSP.
unsafe impl Sync for RouteCell {}
static ROUTE: RouteCell = RouteCell(UnsafeCell::new(None));

fn store_route(route: KbdRoute) {
    // SAFETY: single-core bring-up on the BSP.
    unsafe { *ROUTE.0.get() = Some(route) };
}

fn route() -> Option<KbdRoute> {
    // SAFETY: single-core; the option is Copy.
    unsafe { *ROUTE.0.get() }
}

fn set_masked(masked: bool) {
    if let Some(route) = route() {
        // SAFETY: CPL0; touches only the mask bit of the entry this route owns.
        let _ = unsafe {
            route
                .io_apic
                .set_masked(route.redirection_index, masked)
        };
    }
}

/// Discard any pending input: the controller's output buffer and the decoded-key
/// ring. Called before handing control to a user so keys injected by the proof
/// (or held during boot) do not auto-select anything.
pub fn flush_input() {
    drain_output();
    // SAFETY: single-core, called with interrupts disabled during bring-up.
    unsafe {
        *RING.head.get() = 0;
        *RING.tail.get() = 0;
    }
    EXTENDED_PENDING.store(0, Ordering::Relaxed);
    LAST_KEY.store(0, Ordering::Relaxed);
}

/// Flush stale input, then unmask IRQ1 and enable interrupts, so the ring buffer
/// fills only from real keys the user presses. Used by the accessible menu once
/// the proofs are done.
///
/// # Safety
/// CPL0, after [`prove`] has routed and proved the keyboard.
pub unsafe fn arm_for_input() {
    flush_input();
    set_masked(false);
    unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
}

/// Restart the machine by pulsing the 8042 controller's reset line (command
/// 0xFE), the most portable software reset on a PC. If the controller does not
/// reset the CPU, fall back to a triple fault via a null IDT so the request
/// never silently does nothing.
///
/// # Safety
/// CPL0. Does not return.
pub unsafe fn reboot() -> ! {
    // SAFETY: 0xFE on the command port asserts the CPU reset line on a PC 8042.
    unsafe {
        // Wait for the input buffer to drain, then pulse reset.
        let _ = wait_input_clear();
        outb(STATUS_CMD, 0xFE);
    }
    // If reset did not take, force a triple fault: load a zero-length IDT and
    // raise an interrupt so there is no gate to dispatch, then a fault on the
    // fault, then reset.
    let idtr: [u8; 10] = [0; 10];
    // SAFETY: CPL0; deliberately installing an empty IDT to force a triple fault.
    unsafe {
        core::arch::asm!("lidt [{}]", in(reg) idtr.as_ptr(), options(readonly, nostack, preserves_flags));
        core::arch::asm!("int3", options(nomem, nostack));
    }
    // Unreachable in practice; satisfy the never-return contract.
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// Number of decoded keys the injection proof drives through the real IRQ path.
const PROOF_KEYS: &[(&[u8], Key)] = &[
    (&[0x39], Key::Space),
    (&[0x1C], Key::Enter),
    (&[EXTENDED_PREFIX, 0x48], Key::Up),
    (&[EXTENDED_PREFIX, 0x50], Key::Down),
];

/// Bring the keyboard up and prove the real IRQ1 delivery-and-decode path, plus
/// a mask/resume negative test. Routed through the same I/O APIC the device-IRQ
/// proof used. Deterministic and safe on any machine: injection uses only the
/// controller's own command, and the route is left masked with interrupts
/// disabled when the proof returns.
///
/// # Safety
/// CPL0, single core, after the IDT is installed, x2APIC is enabled, and the
/// I/O APIC is reachable (i.e. after the device-IRQ routing proof).
pub unsafe fn prove(handoff: &KernelHandoff) {
    debug_write("AW_KBD_BEGIN\n");

    // SAFETY: the identity map is active and the RSDP comes from the validated
    // handoff, exactly as the device-IRQ routing proof resolves it.
    let madt = match unsafe { acpi::find_madt(handoff.acpi_rsdp) } {
        Ok(madt) => madt,
        Err(error) => {
            debug_write("AW_KBD_UNAVAILABLE reason=madt_");
            debug_write(error.name());
            debug_write("\n");
            return;
        }
    };

    // SAFETY: CPL0 bring-up.
    if !unsafe { init_controller() } {
        return;
    }

    // Route IRQ1 through the I/O APIC to a freshly allocated vector, masked.
    let stub = aw_kbd_isr as *const () as u64;
    // SAFETY: CPL0, single core, after the IDT and x2APIC are up.
    let (io_apic, routing) = match unsafe { device_irq::route_isa_irq(madt, KBD_ISA_IRQ, stub) } {
        Ok(pair) => pair,
        Err(reason) => {
            debug_write("AW_KBD_UNAVAILABLE reason=");
            debug_write(reason);
            debug_write("\n");
            return;
        }
    };
    store_route(KbdRoute {
        io_apic,
        redirection_index: routing.redirection_index,
    });
    debug_write("AW_KBD_ROUTED gsi=");
    crate::debug_write_u64(u64::from(routing.global_system_interrupt));
    debug_write(" vector=");
    crate::debug_write_hex_u64(u64::from(routing.vector));
    debug_write("\n");

    // Positive path: inject each known scancode (sequence) and require the ISR to
    // receive it and decode it to the expected key.
    drain_output();
    set_masked(false);
    // SAFETY: CPL0; the vector is installed and the rest of bring-up is ready.
    unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };

    for (scancodes, expected) in PROOF_KEYS {
        let before = ticks();
        for &sc in *scancodes {
            if !inject_scancode(sc) {
                cli();
                set_masked(true);
                debug_write("AW_KBD_FAIL reason=inject\n");
                return;
            }
        }
        if !spin_until_tick(before + 1) {
            cli();
            set_masked(true);
            debug_write("AW_KBD_FAIL reason=not_delivered\n");
            return;
        }
        let got = key_from_code(LAST_KEY.load(Ordering::Acquire));
        if got != *expected {
            cli();
            set_masked(true);
            debug_write("AW_KBD_FAIL reason=wrong_key\n");
            return;
        }
        debug_write("AW_KBD_KEY name=");
        debug_write(key_name(*expected));
        debug_write("\n");
    }

    // Negative test: mask IRQ1, flush any pending byte, inject, and require the
    // counter to stay frozen - delivery, not polling, drives it.
    set_masked(true);
    drain_output();
    let masked_before = ticks();
    let _ = inject_scancode(0x39);
    // Give a real interrupt time to (not) arrive.
    let mut spins = 0u32;
    while spins < 2_000_000 {
        spins += 1;
        core::hint::spin_loop();
    }
    let masked_after = ticks();
    if masked_after != masked_before {
        cli();
        debug_write("AW_KBD_FAIL reason=mask_ineffective\n");
        return;
    }
    debug_write("AW_KBD_MASKED_STOPPED\n");
    // Clear the byte the masked injection left latched, so a fresh edge follows.
    drain_output();

    // Resume: unmask and require delivery to come back.
    set_masked(false);
    let resume_before = ticks();
    let _ = inject_scancode(0x1C);
    let resumed = spin_until_tick(resume_before + 1);
    cli();
    set_masked(true);
    if !resumed {
        debug_write("AW_KBD_FAIL reason=did_not_resume\n");
        return;
    }
    debug_write("AW_KBD_UNMASKED_RESUMED\n");
    debug_write("AW_KBD_PROOF_OK\n");
}

fn key_name(key: Key) -> &'static str {
    match key {
        Key::Up => "up",
        Key::Down => "down",
        Key::Left => "left",
        Key::Right => "right",
        Key::Tab => "tab",
        Key::Enter => "enter",
        Key::Space => "space",
        Key::Escape => "escape",
        Key::Backspace => "backspace",
        Key::Char(_) => "char",
    }
}

/// Spin, interrupts enabled, until the tick counter reaches `target` or a bounded
/// budget runs out.
fn spin_until_tick(target: u64) -> bool {
    let mut budget = 50_000_000u32;
    while budget > 0 {
        if ticks() >= target {
            return true;
        }
        budget -= 1;
        core::hint::spin_loop();
    }
    ticks() >= target
}

/// # Safety
/// CPL0.
fn cli() {
    // SAFETY: CPL0; disabling interrupts is always sound here.
    unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) };
}
