//! The accessible boot menu: the first screen a user drives themselves.
//!
//! Everything before this proves the machine *can* do something; this is where a
//! person decides *what* it does. It draws a keyboard-navigable menu on the
//! framebuffer console, highlighting the focused item as a solid bar (visible
//! without colour vision), and voices each landing through the same
//! [`aw_screen_reader`] engine the installer and desktop will use, so the wording
//! never drifts between boot and the rest of the system. Input comes from the
//! proven PS/2 keyboard: Up/Down or Tab move focus, Enter selects.
//!
//! The navigation logic is proved deterministically ([`prove`]) by driving the
//! same handler the live loop uses with a fixed key sequence and checking every
//! landing - the keyboard's own IRQ delivery is proved separately in
//! [`crate::ps2_keyboard`], so together they cover the whole key-to-action path.

use aw_accessibility::{NodeId, Rect, Role, SemanticNode, State, validate_node};
use aw_screen_reader::{FocusContext, announce_focus};

use crate::debug_write;
use crate::framebuffer;
use crate::ps2_keyboard::{self, Key};

/// What selecting an item does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    /// Leave the menu and let the kernel idle.
    Continue,
    /// Show a read-only system information screen.
    SystemInfo,
    /// Restart the machine.
    Reboot,
    /// Turn the machine off (ACPI S5).
    PowerOff,
}

struct Item {
    label: &'static str,
    action: MenuAction,
}

const ITEMS: &[Item] = &[
    Item {
        label: "Continue and idle",
        action: MenuAction::Continue,
    },
    Item {
        label: "System information",
        action: MenuAction::SystemInfo,
    },
    Item {
        label: "Reboot",
        action: MenuAction::Reboot,
    },
    Item {
        label: "Power off",
        action: MenuAction::PowerOff,
    },
];

const TITLE: &str = "Accessible Windows - boot menu";
const HINT: &str = "Up/Down or Tab to move. Enter to select.";

/// Pre-recorded speech for the menu title, played when the menu opens. Voiced by
/// the HDA codec on real hardware (`scripts/gen-menu-speech.ps1`); ignored when
/// the machine has no audio output.
static CLIP_TITLE: &[u8] = include_bytes!("speech/menu_title.pcm");

/// Pre-recorded speech per item, in the same order as [`ITEMS`], played on the
/// focused item as the selection moves.
static ITEM_CLIPS: [&[u8]; 4] = [
    include_bytes!("speech/item_continue.pcm"),
    include_bytes!("speech/item_sysinfo.pcm"),
    include_bytes!("speech/item_reboot.pcm"),
    include_bytes!("speech/item_poweroff.pcm"),
];

/// The title clip, for the HDA speech proof to play during bring-up.
#[must_use]
pub fn title_clip() -> &'static [u8] {
    CLIP_TITLE
}

/// Row index (in the framebuffer's text grid) the first menu item sits on.
const FIRST_ITEM_ROW: u32 = 2;

/// Build the accessible node for item `index`, so its spoken form is produced by
/// the same engine as every other control.
fn item_node(index: usize) -> SemanticNode<'static> {
    SemanticNode {
        id: NodeId(100 + index as u64),
        parent: Some(NodeId(1)),
        role: Role::MenuItem,
        name: ITEMS[index].label,
        description: "",
        value: "",
        state: State::from_bits(State::FOCUSABLE),
        bounds: Rect {
            x: 8,
            y: 40 + index as i32 * 16,
            width: 480,
            height: 16,
        },
    }
}

/// The spoken utterance for landing on item `index`, e.g.
/// "Reboot, menu item, 3 of 4". Written into `buffer`; empty on failure.
fn announce_item(index: usize, buffer: &mut [u8]) -> &str {
    let node = item_node(index);
    if validate_node(&node).is_err() {
        return "";
    }
    let context = FocusContext::in_set(index as u32 + 1, ITEMS.len() as u32);
    announce_focus(&node, context, buffer)
}

/// Speak (to the debug channel now, HDA later) and mirror to screen the landing
/// on `index`.
fn speak_item(index: usize) {
    let mut buffer = [0u8; 128];
    let text = announce_item(index, &mut buffer);
    if !text.is_empty() {
        debug_write("AW_MENU_SPEAK \"");
        debug_write(text);
        debug_write("\"\n");
    }
    // Voice the landing aloud through the HDA codec. A no-op on a machine with no
    // audio output, so the on-screen focus bar still stands alone there.
    if let Some(clip) = ITEM_CLIPS.get(index) {
        crate::hda::speak(clip);
    }
}

/// Repaint the whole menu with `selected` highlighted.
fn render(selected: usize) {
    framebuffer::clear_screen();
    framebuffer::draw_menu_row(0, TITLE, false);
    for (index, item) in ITEMS.iter().enumerate() {
        framebuffer::draw_menu_row(FIRST_ITEM_ROW + index as u32, item.label, index == selected);
    }
    framebuffer::draw_menu_row(FIRST_ITEM_ROW + ITEMS.len() as u32 + 1, HINT, false);
}

/// Apply one key to the current selection. Returns the new selection and, when a
/// key activates an item, the action to run. Pure: the live loop and the proof
/// share it.
fn handle_key(selected: usize, key: Key) -> (usize, Option<MenuAction>) {
    let last = ITEMS.len() - 1;
    match key {
        Key::Up | Key::Left => (if selected == 0 { last } else { selected - 1 }, None),
        Key::Down | Key::Tab | Key::Right => {
            (if selected == last { 0 } else { selected + 1 }, None)
        }
        Key::Enter | Key::Space => (selected, Some(ITEMS[selected].action)),
        _ => (selected, None),
    }
}

/// Draw a read-only system information screen, then wait for one key to return.
fn show_system_info() {
    framebuffer::clear_screen();
    framebuffer::draw_menu_row(0, "System information", false);
    framebuffer::draw_menu_row(2, "Accessible Windows - native x86-64 kernel", false);
    framebuffer::draw_menu_row(3, "Firmware: UEFI. Boot services exited.", false);
    framebuffer::draw_menu_row(4, "Framebuffer console: online.", false);
    framebuffer::draw_menu_row(5, "Keyboard: PS/2 (8042), IRQ1 proven.", false);
    framebuffer::draw_menu_row(7, "Press any key to return to the menu.", false);
    debug_write("AW_MENU_INFO_SHOWN\n");
    wait_for_key();
}

/// Block until one key is available, sleeping under `hlt` between checks so the
/// CPU is idle while waiting. Interrupts must already be enabled.
fn wait_for_key() -> Key {
    loop {
        if let Some(key) = ps2_keyboard::poll_key() {
            return key;
        }
        // SAFETY: CPL0; interrupts are enabled, so the keyboard IRQ can wake HLT.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// Run the menu for real keys. Never returns: it either idles (Continue) or
/// reboots. Under headless boot no keys arrive, so it simply idles under `hlt`,
/// which is why the boot proof's idle marker is emitted before this is entered.
///
/// # Safety
/// CPL0 on the bootstrap processor, after [`crate::ps2_keyboard::prove`] has
/// routed the keyboard. Enables interrupts.
pub unsafe fn run_interactive() -> ! {
    debug_write("AW_MENU_INTERACTIVE_BEGIN\n");
    // SAFETY: caller's contract; unmasks IRQ1 and sets IF so keys can arrive.
    unsafe { ps2_keyboard::arm_for_input() };

    let mut selected = 0usize;
    render(selected);
    // Announce the menu itself, then the first item, so a blind user knows where
    // they are before touching a key.
    crate::hda::speak(CLIP_TITLE);
    speak_item(selected);

    loop {
        let key = wait_for_key();
        let (next, action) = handle_key(selected, key);
        if next != selected {
            selected = next;
            render(selected);
            speak_item(selected);
        }
        match action {
            Some(MenuAction::Continue) => {
                framebuffer::write_line("");
                framebuffer::write_line("Idle. The system is running; nothing else to do yet.");
                debug_write("AW_MENU_CONTINUE\n");
                idle_forever();
            }
            Some(MenuAction::SystemInfo) => {
                show_system_info();
                render(selected);
                speak_item(selected);
            }
            Some(MenuAction::Reboot) => {
                framebuffer::clear_screen();
                framebuffer::draw_menu_row(0, "Rebooting...", false);
                debug_write("AW_MENU_REBOOT\n");
                // SAFETY: CPL0; the FADT reset register, then the 8042, then a triple fault.
                unsafe { crate::power::reset_machine() };
            }
            Some(MenuAction::PowerOff) => {
                framebuffer::clear_screen();
                framebuffer::draw_menu_row(0, "Powering off...", false);
                debug_write("AW_MENU_POWEROFF\n");
                // SAFETY: CPL0; enters ACPI S5.
                unsafe { crate::power::power_off_machine() };
            }
            None => {}
        }
    }
}

/// Idle with interrupts enabled so the keyboard still wakes the CPU, but ignore
/// further keys - used after Continue.
fn idle_forever() -> ! {
    loop {
        // SAFETY: CPL0; HLT with interrupts enabled parks the CPU until an IRQ.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// Prove the menu's navigation and selection logic deterministically: drive the
/// shared [`handle_key`] with a fixed key sequence and check every landing and
/// the activated action. Renders each state too, so the framebuffer path is
/// exercised. No side-effecting action is run (the Enter lands on a benign item).
///
/// The keyboard's real IRQ-to-key path is proved in [`crate::ps2_keyboard`]; this
/// proves what those keys then do.
pub fn prove() {
    debug_write("AW_MENU_BEGIN\n");

    let mut selected = 0usize;
    render(selected);

    // (key, expected selection after it, expected action).
    let script: &[(Key, usize, Option<MenuAction>)] = &[
        (Key::Down, 1, None),
        (Key::Down, 2, None),
        (Key::Up, 1, None),
        (Key::Tab, 2, None),
        (Key::Up, 1, None),
        // Enter on "System information" (index 1) - a benign, non-rebooting action.
        (Key::Enter, 1, Some(MenuAction::SystemInfo)),
    ];

    for &(key, expect_sel, expect_action) in script {
        let (next, action) = handle_key(selected, key);
        selected = next;
        render(selected);
        if selected != expect_sel || action != expect_action {
            debug_write("AW_MENU_FAIL reason=transition\n");
            return;
        }
        if action.is_none() {
            debug_write("AW_MENU_FOCUS index=");
            crate::debug_write_u64(selected as u64);
            debug_write(" name=\"");
            debug_write(ITEMS[selected].label);
            debug_write("\"\n");
            speak_item(selected);
        } else {
            debug_write("AW_MENU_SELECT name=\"");
            debug_write(ITEMS[selected].label);
            debug_write("\"\n");
        }
    }

    // Wrap-around check: Up from the first item lands on the last.
    let (wrapped, _) = handle_key(0, Key::Up);
    if wrapped != ITEMS.len() - 1 {
        debug_write("AW_MENU_FAIL reason=no_wrap\n");
        return;
    }
    debug_write("AW_MENU_WRAP_OK\n");

    debug_write("AW_MENU_PROOF_OK\n");
}
