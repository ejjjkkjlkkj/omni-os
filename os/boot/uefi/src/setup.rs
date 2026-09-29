//! Accessible UEFI Setup Utility - a complete, spoken, keyboard-operable firmware
//! setup with real menus *and submenus*, modeled on the ones a sighted user sees
//! (AMI Aptio, the ASUS UEFI BIOS Utility) but usable entirely without sight, before
//! the operating system exists.
//!
//! A real firmware setup organizes everything into a tree: top tabs - Main, Advanced,
//! Boot, Security, Save and Exit - and, under them, submenus (CPU Configuration, Boot
//! Option Priorities, Secure Boot, ...) you descend into and back out of. None of that
//! is reachable by a blind user, because the firmware's own setup is silent. This
//! rebuilds the whole tree at the pre-OS stage the project controls and makes every
//! screen, every item and every help line *spoken* - through the one `aw-screen-reader`
//! engine for the console/marker wording and pre-recorded clips for the fixed
//! scaffolding through the real HDA codec - and operable on the firmware's own keyboard
//! (so a USB keyboard works before any kernel USB stack exists). Left/Right move across
//! the top tabs, Up/Down move within a screen, Enter opens a submenu or activates an
//! item, Escape steps back out (and, at the top level, boots normally).
//!
//! Honest scope: a loaded UEFI application cannot rewrite chipset or CPU straps the way
//! the firmware's own setup can, so Main, Advanced and Security here *read and speak*
//! real machine state (firmware identity, RTC time, memory, display, CPU, virtualization
//! support, Secure Boot) rather than pretending to change it, and offer "Enter firmware
//! setup" for the settings only the firmware itself owns. The Boot tab is fully
//! actionable through architected UEFI services: the real boot entries are enumerated
//! from `BootOrder`/`Boot####`, and for any of them a blind user can **boot it now**
//! (via `BootNext` + restart) or **make it the persistent default** (by rewriting
//! `BootOrder`) - things a silent firmware never lets them do. Save and Exit requests
//! the firmware UI through `OsIndications`, or resets / powers off through runtime
//! `ResetSystem`.
//!
//! Determinism and the unattended contract: with nobody at the keyboard a countdown
//! takes the safe default ("Boot normally") and continues, so a headless machine - and
//! the timed proof harness, which presses no key - never hangs. Every marker is emitted
//! through [`crate::aw_mark`], so it lands on the 0xE9 debug console (QEMU) and the COM1
//! mirror (VMware, hardware) alike.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use core::time::Duration;

use aw_accessibility::{NodeId, Rect, Role, SemanticNode, State, validate_node};
use aw_screen_reader::{FocusContext, announce_focus};
use uefi::mem::memory_map::MemoryMap;
use uefi::proto::console::text::{Key, ScanCode};
use uefi::proto::hii::config_routing::HiiConfigRouting;
use uefi::proto::hii::config_str::MultiConfigurationStringIter;
use uefi::proto::tcg::v2::Tcg as Tcg2;
use uefi::runtime::{self, VariableAttributes, VariableVendor};
use uefi::table::cfg::ConfigTableEntry;
use uefi::{CStr16, CString16, Guid, Status, boot, cstr16, guid, system};

use crate::audio;
use crate::aw_mark;
use crate::hda;
use crate::sound;

/// Pitch of the cue when the highlight moves within a screen.
const CUE_MOVE_HZ: u32 = 740;
/// Pitch of the cue when a top tab changes or a submenu is entered.
const CUE_TAB_HZ: u32 = 622;
/// Pitch of the cue when stepping back out of a submenu.
const CUE_BACK_HZ: u32 = 466;
/// Pitch of the cue confirming a persistent change was written.
const CUE_APPLIED_HZ: u32 = 988;
/// Pitch of the cue confirming the boot is continuing.
const CUE_CONTINUE_HZ: u32 = 523;
/// Pitch of the cue when the setup is ready and waiting for the user.
const CUE_READY_HZ: u32 = 880;

/// How long an unattended boot waits for a key before it continues on its own.
/// Kept short: it is pure latency on every boot where nobody reviews, and it is in the
/// critical path of the timed boot proofs, which press no key and so always wait this
/// out before booting normally.
const REVIEW_WINDOW: Duration = Duration::from_secs(2);
/// How often the review window polls for a keystroke.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// `EFI_OS_INDICATIONS_BOOT_TO_FW_UI`: asks the firmware to enter its own setup UI on
/// the next boot. Advertised in `OsIndicationsSupported` when honored.
const OS_INDICATIONS_BOOT_TO_FW_UI: u64 = 0x0000_0000_0000_0001;
/// `LOAD_OPTION_ACTIVE`: a `Boot####` option the firmware would actually try.
const LOAD_OPTION_ACTIVE: u32 = 0x0000_0001;

/// The language the setup speaks and shows. French is the default; a Language item on the
/// Main tab switches to English, the way a real ASUS/AMI BIOS offers a "System Language"
/// option. Only the fixed scaffolding is translated (labels, help, clips); dynamic values
/// (device names, SMBIOS strings) are the machine's own text in either language.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    Fr,
    En,
}

impl Lang {
    /// The other language, for the toggle.
    fn toggled(self) -> Self {
        match self {
            Lang::Fr => Lang::En,
            Lang::En => Lang::Fr,
        }
    }
}

/// Pick the French or English form of a fixed string for the current language.
fn tx(lang: Lang, fr: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Fr => fr,
        Lang::En => en,
    }
}

/// Pick the French or English clip for the current language.
fn clip(lang: Lang, fr: &'static [u8], en: &'static [u8]) -> &'static [u8] {
    match lang {
        Lang::Fr => fr,
        Lang::En => en,
    }
}

/// Read a little-endian `u32` at `at` from `data`, or 0 if it would run off the end.
fn u32le(data: &[u8], at: usize) -> u32 {
    if at + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// The variable attributes a boot-control global carries (non-volatile, visible to boot
/// services and to the runtime), matching how firmware stores `BootOrder`/`BootNext`.
fn boot_var_attributes() -> VariableAttributes {
    VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS
}

/// What activating an item does.
#[derive(Clone, Copy)]
enum Action {
    /// No action: a read-only line of machine state.
    Info,
    /// Open the submenu at this screen index.
    SubMenu(usize),
    /// Step back out of the current submenu (Escape does the same).
    Back,
    /// Continue booting omni-os - the safe default.
    BootNormally,
    /// Set this `Boot####` id as `BootNext` and restart so the firmware boots it now.
    BootNow(u16),
    /// Rewrite `BootOrder` so this `Boot####` id is first - the persistent default.
    MakeDefault(u16),
    /// Move this `Boot####` id one place earlier in `BootOrder`.
    MoveUp(u16),
    /// Move this `Boot####` id one place later in `BootOrder`.
    MoveDown(u16),
    /// Request the firmware's own setup UI through `OsIndications`, then restart.
    EnterSetup,
    /// Cold-reset the machine now.
    Reset,
    /// Power the machine off now.
    Shutdown,
    /// Switch the setup's language (French <-> English) and rebuild.
    ToggleLang,
    /// Change the firmware HII setting at this index in [`Tree::settings`]: cycle a one-of to its
    /// next choice, toggle a checkbox, or step a numeric - browsable and changeable in the menu
    /// tree, not only from the typed agent.
    ChangeSetting(usize),
}

impl Action {
    /// The accessibility role used to announce an item: read-only lines are static text
    /// (no role word); everything selectable is a menu item.
    fn role(self) -> Role {
        match self {
            Action::Info => Role::StaticText,
            _ => Role::MenuItem,
        }
    }
}

/// One row in a screen: what it says, its help line, its spoken clip (when the label is
/// fixed and pre-recorded), and what selecting it does.
struct Item {
    text: String,
    help: String,
    clip: Option<&'static [u8]>,
    action: Action,
}

impl Item {
    fn info(text: String, help: &str) -> Self {
        Self {
            text,
            help: String::from(help),
            clip: None,
            action: Action::Info,
        }
    }

    fn action(text: &str, help: &str, clip: Option<&'static [u8]>, action: Action) -> Self {
        Self {
            text: String::from(text),
            help: String::from(help),
            clip,
            action,
        }
    }

    fn dynamic(text: String, help: &str, clip: Option<&'static [u8]>, action: Action) -> Self {
        Self {
            text,
            help: String::from(help),
            clip,
            action,
        }
    }
}

/// One screen in the tree: a top tab or a submenu. The five top tabs are the entries of
/// [`Tree::tabs`] and are announced as tabs with a 1..5 position; every other screen is a
/// submenu, reached from a parent and announced by title. Depth in the nav stack, not a
/// field here, is what tells the two apart at render/announce time.
struct Screen {
    title: String,
    title_clip: Option<&'static [u8]>,
    items: Vec<Item>,
}

/// The whole setup tree plus the indices of the five top tabs and which tab is Boot (the
/// safe-default landing tab, located by index so it is language-independent).
struct Tree {
    screens: Vec<Screen>,
    tabs: Vec<usize>,
    boot_tab: usize,
    /// The firmware's own HII settings, parsed from its IFR, in the order the settings submenu
    /// lists them - so a menu item's `ChangeSetting(index)` names the right one.
    settings: Vec<crate::hii_ifr::Setting>,
}

// ---- Machine-state gathering (read, never change) ------------------------------

/// The CPUID vendor string, e.g. "GenuineIntel" or "AuthenticAMD".
fn cpu_vendor() -> String {
    // CPUID leaf 0 is always available and side-effect free.
    let leaf = core::arch::x86_64::__cpuid(0);
    let mut bytes = [0u8; 12];
    bytes[0..4].copy_from_slice(&leaf.ebx.to_le_bytes());
    bytes[4..8].copy_from_slice(&leaf.edx.to_le_bytes());
    bytes[8..12].copy_from_slice(&leaf.ecx.to_le_bytes());
    String::from_utf8_lossy(&bytes).trim().into()
}

/// The processor brand string from extended CPUID leaves, when the CPU provides it.
fn cpu_brand() -> String {
    // Leaf 0x80000000 reports the highest extended leaf; reads are pure.
    let max = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    if max < 0x8000_0004 {
        return cpu_vendor();
    }
    let mut bytes = [0u8; 48];
    for (block, leaf) in (0x8000_0002u32..=0x8000_0004).enumerate() {
        // Guarded by the max-leaf check above; reads are pure.
        let r = core::arch::x86_64::__cpuid(leaf);
        let base = block * 16;
        bytes[base..base + 4].copy_from_slice(&r.eax.to_le_bytes());
        bytes[base + 4..base + 8].copy_from_slice(&r.ebx.to_le_bytes());
        bytes[base + 8..base + 12].copy_from_slice(&r.ecx.to_le_bytes());
        bytes[base + 12..base + 16].copy_from_slice(&r.edx.to_le_bytes());
    }
    let brand: String = String::from_utf8_lossy(&bytes).into_owned();
    let trimmed = brand.trim();
    if trimmed.is_empty() {
        cpu_vendor()
    } else {
        String::from(trimmed)
    }
}

/// Read an MSR. Only ever called after a CPUID feature check proves the MSR exists on
/// this CPU, so it cannot fault the firmware.
///
/// # Safety
/// CPL0, and `msr` must be a register this CPU implements.
unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: `rdmsr` reads model-specific register `ecx`; the caller guarantees it
    // exists. No memory is touched.
    unsafe {
        core::arch::asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi,
            options(nomem, nostack, preserves_flags));
    }
    (u64::from(hi) << 32) | u64::from(lo)
}

/// Describe hardware virtualization support - the setting a blind user could never reach
/// in a silent firmware. Intel VT-x (VMX) exposes a lock/enable state in
/// `IA32_FEATURE_CONTROL`; AMD-V (SVM) is reported from CPUID.
fn virtualization_status(lang: Lang) -> String {
    let vendor = cpu_vendor();
    // CPUID leaf 1 is always present; reads are pure.
    let vmx = core::arch::x86_64::__cpuid(1).ecx & (1 << 5) != 0;
    if vendor == "GenuineIntel" && vmx {
        // IA32_FEATURE_CONTROL (0x3A) exists on every VMX-capable Intel part, so the
        // CPUID check above makes this rdmsr safe. Bit 0 locks the register; bit 2
        // enables VMX outside SMX. Locked-but-disabled is the "off in firmware" case.
        // SAFETY: guarded by the Intel + VMX check.
        let feature_control = unsafe { rdmsr(0x3A) };
        let locked = feature_control & 0b001 != 0;
        let enabled = feature_control & 0b100 != 0;
        return String::from(match (locked, enabled) {
            (_, true) => tx(lang, "Intel VT-x, activé", "Intel VT-x, enabled"),
            (true, false) => tx(
                lang,
                "Intel VT-x, pris en charge mais désactivé dans le firmware",
                "Intel VT-x, supported but disabled in firmware",
            ),
            (false, false) => tx(lang, "Intel VT-x, pris en charge", "Intel VT-x, supported"),
        });
    }
    // Leaf 0x80000001 is present on all long-mode CPUs; reads are pure.
    let svm = core::arch::x86_64::__cpuid(0x8000_0001).ecx & (1 << 2) != 0;
    if vendor == "AuthenticAMD" && svm {
        return String::from(tx(lang, "AMD-V, pris en charge", "AMD-V, supported"));
    }
    String::from(tx(lang, "non pris en charge", "not supported"))
}

/// Total usable RAM in mebibytes, summed from the UEFI memory map. Best effort: a map
/// failure reports 0 rather than aborting the setup.
fn installed_memory_mib() -> u64 {
    match boot::memory_map(uefi::mem::memory_map::MemoryType::LOADER_DATA) {
        Ok(map) => {
            let pages: u64 = map.entries().map(|entry| entry.page_count).sum();
            pages * 4096 / (1024 * 1024)
        }
        Err(_) => 0,
    }
}

/// Read a small global UEFI variable by name into an owned buffer, or `None` if it is
/// absent or unreadable. Uses the boxed reader so a large `Boot####` option (long device
/// paths) is never truncated.
fn read_global(name: &CStr16) -> Option<Vec<u8>> {
    match runtime::get_variable_boxed(name, &VariableVendor::GLOBAL_VARIABLE) {
        Ok((data, _)) => Some(data.into_vec()),
        Err(_) => None,
    }
}

/// Decode the human description of a `Boot####` load option, or `None` if the variable
/// is malformed or the option is inactive. The layout is a `u32` attributes, a `u16`
/// device-path length, then a NUL-terminated UCS-2 description.
fn decode_boot_option(raw: &[u8]) -> Option<String> {
    if raw.len() < 6 {
        return None;
    }
    let attributes = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    if attributes & LOAD_OPTION_ACTIVE == 0 {
        return None;
    }
    // The description is UCS-2 starting at offset 6, terminated by a 0x0000 unit.
    let mut units = Vec::new();
    let mut offset = 6;
    while offset + 1 < raw.len() {
        let unit = u16::from_le_bytes([raw[offset], raw[offset + 1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
        offset += 2;
    }
    if units.is_empty() {
        return None;
    }
    Some(String::from_utf16_lossy(&units))
}

/// The `Boot####` variable name for `id`, built into `buffer` (nine UCS-2 units: `B o o
/// t` then four upper-hex digits then a NUL). Returns it as a `CStr16`.
fn boot_var_name(id: u16, buffer: &mut [u16; 9]) -> &CStr16 {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    *buffer = [
        b'B' as u16,
        b'o' as u16,
        b'o' as u16,
        b't' as u16,
        HEX[((id >> 12) & 0xf) as usize] as u16,
        HEX[((id >> 8) & 0xf) as usize] as u16,
        HEX[((id >> 4) & 0xf) as usize] as u16,
        HEX[(id & 0xf) as usize] as u16,
        0,
    ];
    // The buffer is always a valid NUL-terminated UCS-2 string by construction.
    CStr16::from_u16_with_nul(buffer).unwrap_or(cstr16!("Boot"))
}

/// One enumerated boot option: its `Boot####` id and its decoded human label.
struct BootOption {
    id: u16,
    label: String,
}

/// Enumerate the machine's active boot options in firmware priority order, exactly as
/// the AMI/ASUS "Boot Option Priorities" list does: read `BootOrder`, then decode each
/// referenced `Boot####`. Skips missing or malformed entries with a marker.
fn enumerate_boot_options() -> Vec<BootOption> {
    let mut options = Vec::new();
    let Some(order) = read_global(cstr16!("BootOrder")) else {
        aw_mark!("AW_UEFI_BOOT_ENUM count=0 reason=no_boot_order");
        return options;
    };
    let mut index = 0u32;
    for pair in order.as_chunks::<2>().0 {
        let id = u16::from_le_bytes(*pair);
        let mut name_buffer = [0u16; 9];
        let name = boot_var_name(id, &mut name_buffer);
        let Some(raw) = read_global(name) else {
            aw_mark!("AW_UEFI_BOOT_SKIP id=0x{:04x} reason=unreadable", id);
            continue;
        };
        let Some(label) = decode_boot_option(&raw) else {
            aw_mark!(
                "AW_UEFI_BOOT_SKIP id=0x{:04x} reason=inactive_or_malformed",
                id
            );
            continue;
        };
        aw_mark!(
            "AW_UEFI_BOOT_ENTRY index={} id=0x{:04x} \"{}\"",
            index,
            id,
            label
        );
        options.push(BootOption { id, label });
        index += 1;
    }
    aw_mark!("AW_UEFI_BOOT_ENUM count={}", options.len());
    options
}

/// Read the one-byte `SecureBoot`/`SetupMode` state into a spoken word (caller supplies
/// the words, so they can be French or English).
fn one_byte_state(name: &CStr16, one: &str, zero: &str, unknown: &str) -> String {
    match read_global(name) {
        Some(bytes) if !bytes.is_empty() => String::from(if bytes[0] == 1 { one } else { zero }),
        _ => String::from(unknown),
    }
}

// ---- TPM and Secure Boot key state (read, never change) ------------------------

/// The vendor GUID of the UEFI image-security database, where `db` and `dbx` live (`PK`
/// and `KEK` live under the global-variable GUID instead). `EFI_IMAGE_SECURITY_DATABASE_GUID`.
const IMAGE_SECURITY_DATABASE: VariableVendor =
    VariableVendor(guid!("d719b2cb-3d3a-4596-a3bc-dad00e67656f"));

/// Describe the machine's TPM through the TCG2 protocol: present or not, and - when present -
/// which spec family and how many PCR banks are active. The setting a blind user could never
/// otherwise hear. Best effort: no TCG2 protocol means the firmware exposes no measured-boot
/// TPM interface, which is itself the honest answer ("no TPM 2.0 interface").
fn tpm_status(lang: Lang) -> String {
    let Ok(handle) = boot::get_handle_for_protocol::<Tcg2>() else {
        return String::from(tx(lang, "aucune interface TPM 2.0", "no TPM 2.0 interface"));
    };
    let Ok(mut tcg2) = boot::open_protocol_exclusive::<Tcg2>(handle) else {
        return String::from(tx(lang, "TPM inaccessible", "TPM not accessible"));
    };
    match tcg2.get_capability() {
        Ok(cap) if cap.tpm_present() => {
            let banks = cap.number_of_pcr_banks;
            format!(
                "{}, {} {}",
                tx(lang, "TPM 2.0 présent", "TPM 2.0 present"),
                banks,
                tx(lang, "banques PCR actives", "active PCR banks"),
            )
        }
        Ok(_) => String::from(tx(lang, "TPM absent", "TPM not present")),
        Err(_) => String::from(tx(lang, "état TPM inconnu", "TPM state unknown")),
    }
}

/// Describe the network interfaces for speech: how many, and each one's link state. The
/// loader never opens the network by itself (deny by default), so this reads, never connects.
fn network_status(lang: Lang) -> String {
    let nics = crate::net::interfaces();
    if nics.is_empty() {
        return String::from(tx(lang, "aucune carte réseau", "no network interface"));
    }
    let mut out = format!(
        "{} {}",
        nics.len(),
        if nics.len() == 1 {
            tx(lang, "carte réseau", "network interface")
        } else {
            tx(lang, "cartes réseau", "network interfaces")
        }
    );
    for (index, nic) in nics.iter().enumerate() {
        let link = match nic.link {
            crate::net::Link::Up => tx(lang, "câble connecté", "link up"),
            crate::net::Link::Down => tx(lang, "câble débranché", "link down"),
            crate::net::Link::Unknown => tx(lang, "état du lien inconnu", "link state unknown"),
        };
        out.push_str(&format!(
            ", {} {}, {}",
            tx(lang, "carte", "interface"),
            index + 1,
            link
        ));
    }
    out.push_str(tx(
        lang,
        ", réseau fermé par défaut",
        ", network closed by default",
    ));
    out
}

/// Read a variable under a chosen vendor GUID into an owned buffer, or `None` if absent.
fn read_var(name: &CStr16, vendor: &VariableVendor) -> Option<Vec<u8>> {
    runtime::get_variable_boxed(name, vendor)
        .ok()
        .map(|(data, _)| data.into_vec())
}

/// Count the certificates/hashes in an `EFI_SIGNATURE_LIST` series (as `KEK`, `db` and `dbx`
/// are stored): walk the lists, each a 28-byte header (type GUID, list size, header size,
/// signature size) followed by fixed-size signatures, and sum how many signatures they hold.
/// Best effort - a malformed list ends the walk rather than misreading it.
fn signature_count(raw: &[u8]) -> usize {
    let mut total = 0usize;
    let mut pos = 0usize;
    while pos + 28 <= raw.len() {
        let list_size = u32le(raw, pos + 16) as usize;
        let header_size = u32le(raw, pos + 20) as usize;
        let sig_size = u32le(raw, pos + 24) as usize;
        if sig_size == 0 || list_size < 28 + header_size || pos + list_size > raw.len() {
            break;
        }
        let sig_area = list_size - 28 - header_size;
        total += sig_area / sig_size;
        pos += list_size;
    }
    total
}

/// The state of one Secure Boot key store (`PK`, `KEK`, `db`, `dbx`): the number of
/// certificates/hashes it holds, or "not provisioned" when the variable is absent. This is the
/// richer key state a real firmware shows and a silent one hides.
fn key_store_state(name: &CStr16, vendor: &VariableVendor, lang: Lang) -> String {
    match read_var(name, vendor) {
        Some(raw) if !raw.is_empty() => {
            let count = signature_count(&raw);
            if count == 0 {
                // A PK is a single certificate stored as one signature list; report it as
                // provisioned even when the counter cannot resolve an entry.
                format!("{} {}", raw.len(), tx(lang, "octets", "bytes"))
            } else {
                format!(
                    "{count} {}",
                    tx(lang, "certificats ou empreintes", "certificates or hashes"),
                )
            }
        }
        _ => String::from(tx(lang, "non provisionné", "not provisioned")),
    }
}

/// A one-line summary of the whole Secure Boot key hierarchy - PK, KEK, db, dbx - for the
/// command agent to speak in answer to "secure boot keys".
fn secure_boot_key_summary(lang: Lang) -> String {
    format!(
        "PK {}, KEK {}, db {}, dbx {}",
        key_store_state(cstr16!("PK"), &VariableVendor::GLOBAL_VARIABLE, lang),
        key_store_state(cstr16!("KEK"), &VariableVendor::GLOBAL_VARIABLE, lang),
        key_store_state(cstr16!("db"), &IMAGE_SECURITY_DATABASE, lang),
        key_store_state(cstr16!("dbx"), &IMAGE_SECURITY_DATABASE, lang),
    )
}

/// The `Boot####` id the firmware selected for the current boot (`BootCurrent`), as a
/// label, or `None` if the variable is absent.
fn boot_current_label() -> Option<String> {
    read_global(cstr16!("BootCurrent"))
        .filter(|bytes| bytes.len() >= 2)
        .map(|bytes| format!("Boot{:04X}", u16::from_le_bytes([bytes[0], bytes[1]])))
}

/// The firmware boot-manager timeout (`Timeout`, in seconds) as a label, or `None`.
fn boot_timeout_label(lang: Lang) -> Option<String> {
    read_global(cstr16!("Timeout"))
        .filter(|bytes| bytes.len() >= 2)
        .map(|bytes| {
            format!(
                "{} {}",
                u16::from_le_bytes([bytes[0], bytes[1]]),
                tx(lang, "secondes", "seconds")
            )
        })
}

// ---- SMBIOS (system identity) --------------------------------------------------

/// The machine identity a real BIOS shows on its Main page, read from the SMBIOS tables
/// the firmware publishes. Without this a blind user has no way to hear the machine's
/// model, its firmware version, or its serial number.
struct SystemIdentity {
    bios: Option<String>,
    system: Option<String>,
    serial: Option<String>,
}

/// Pull one string from an SMBIOS structure's string-set by its 1-based `index` (0 means
/// "no string"). The set starts right after the `formatted_len`-byte formatted area.
fn smbios_string(
    data: &[u8],
    struct_start: usize,
    formatted_len: usize,
    index: u8,
) -> Option<String> {
    if index == 0 {
        return None;
    }
    let mut position = struct_start + formatted_len;
    let mut current = 1u8;
    while position < data.len() {
        if data[position] == 0 {
            return None; // set terminator reached before the wanted index
        }
        let start = position;
        while position < data.len() && data[position] != 0 {
            position += 1;
        }
        if current == index {
            let text = String::from_utf8_lossy(&data[start..position]);
            let trimmed = text.trim();
            return (!trimmed.is_empty()).then(|| String::from(trimmed));
        }
        position += 1;
        current += 1;
    }
    None
}

/// Read Type 0 (BIOS) and Type 1 (System) from the firmware's SMBIOS structure table:
/// BIOS vendor/version/date, and the machine's manufacturer, product name and serial.
/// The table is copied out of physical memory (bounded) and parsed as bytes, all before
/// ExitBootServices while the firmware still identity-maps memory.
fn read_system_identity() -> SystemIdentity {
    let mut identity = SystemIdentity {
        bios: None,
        system: None,
        serial: None,
    };

    let entry = system::with_config_table(|tables| {
        tables
            .iter()
            .find(|entry| entry.guid == ConfigTableEntry::SMBIOS3_GUID)
            .map(|entry| (entry.address as usize, true))
            .or_else(|| {
                tables
                    .iter()
                    .find(|entry| entry.guid == ConfigTableEntry::SMBIOS_GUID)
                    .map(|entry| (entry.address as usize, false))
            })
    });
    let Some((entry_addr, is_v3)) = entry else {
        return identity;
    };
    if entry_addr == 0 {
        return identity;
    }

    // SAFETY: the entry-point address is the firmware's own SMBIOS config-table entry; 32
    // bytes cover both the 32-bit ("_SM_") and 64-bit ("_SM3_") anchor layouts and are
    // identity-mapped during boot services.
    let header = unsafe { core::slice::from_raw_parts(entry_addr as *const u8, 32) };
    let (table_addr, table_len) = if is_v3 {
        let len =
            u32::from_le_bytes([header[0x0C], header[0x0D], header[0x0E], header[0x0F]]) as usize;
        let addr = u64::from_le_bytes([
            header[0x10],
            header[0x11],
            header[0x12],
            header[0x13],
            header[0x14],
            header[0x15],
            header[0x16],
            header[0x17],
        ]) as usize;
        (addr, len)
    } else {
        let len = u16::from_le_bytes([header[0x16], header[0x17]]) as usize;
        let addr =
            u32::from_le_bytes([header[0x18], header[0x19], header[0x1A], header[0x1B]]) as usize;
        (addr, len)
    };
    // Cap the copy so a bad length cannot request a huge or wrapping read.
    let table_len = table_len.min(64 * 1024);
    if table_addr == 0 || table_len < 4 {
        return identity;
    }
    // SAFETY: address and bounded length name the firmware's SMBIOS structure table,
    // identity-mapped and consumed here before ExitBootServices.
    let data = unsafe { core::slice::from_raw_parts(table_addr as *const u8, table_len) }.to_vec();

    let mut offset = 0usize;
    let mut guard = 0u32;
    while offset + 4 <= data.len() && guard < 1024 {
        guard += 1;
        let structure_type = data[offset];
        let formatted_len = data[offset + 1] as usize;
        if formatted_len < 4 {
            break;
        }
        let field = |relative: usize| -> Option<String> {
            data.get(offset + relative)
                .and_then(|&index| smbios_string(&data, offset, formatted_len, index))
        };
        match structure_type {
            0 => {
                let vendor = field(0x04);
                let version = field(0x05);
                let date = field(0x08);
                identity.bios = match (vendor, version) {
                    (Some(vendor), Some(version)) => Some(match date {
                        Some(date) => format!("{vendor} {version} ({date})"),
                        None => format!("{vendor} {version}"),
                    }),
                    (Some(vendor), None) => Some(vendor),
                    (None, Some(version)) => Some(version),
                    (None, None) => None,
                };
            }
            1 => {
                let manufacturer = field(0x04);
                let product = field(0x05);
                identity.serial = field(0x07);
                identity.system = match (manufacturer, product) {
                    (Some(manufacturer), Some(product)) => {
                        Some(format!("{manufacturer} {product}"))
                    }
                    (Some(manufacturer), None) => Some(manufacturer),
                    (None, Some(product)) => Some(product),
                    (None, None) => None,
                };
            }
            127 => break,
            _ => {}
        }
        // Advance past the formatted area and the string-set (ends at a double NUL).
        let mut end = offset + formatted_len;
        while end + 1 < data.len() && !(data[end] == 0 && data[end + 1] == 0) {
            end += 1;
        }
        offset = end + 2;
    }
    identity
}

// ---- Tree construction ---------------------------------------------------------

/// Build the whole setup tree - top tabs and every submenu - from real machine state, in
/// the chosen language (French or English), so the labels, help and clips baked into each
/// item are already in that language and the render/announce path needs no language logic.
/// Child screens are pushed first so their parents can reference them by index.
fn build_tree(lang: Lang, width: usize, height: usize) -> Tree {
    let mut screens: Vec<Screen> = Vec::new();

    // Submenu: CPU Configuration (child of Advanced).
    let cpu_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Configuration du processeur", "CPU Configuration")),
        title_clip: Some(clip(lang, hda::CLIP_FR_SUB_CPU, hda::CLIP_SUB_CPU)),
        items: alloc::vec![
            Item::info(
                format!(
                    "{}, {}",
                    tx(lang, "Technologie de virtualisation", "Virtualization technology"),
                    virtualization_status(lang)
                ),
                tx(
                    lang,
                    "Prise en charge Intel VT-x ou AMD-V et état du firmware ; se modifie dans la configuration du firmware.",
                    "Intel VT-x or AMD-V support and firmware state; change it in firmware setup.",
                ),
            ),
            Item::info(
                format!("{}, {}", tx(lang, "Processeur", "Processor"), cpu_brand()),
                tx(
                    lang,
                    "La chaîne de marque du processeur.",
                    "The processor brand string reported by the CPU.",
                ),
            ),
            Item::info(
                format!(
                    "{}, {}",
                    tx(lang, "Fournisseur du processeur", "Processor vendor"),
                    cpu_vendor()
                ),
                tx(
                    lang,
                    "L'identifiant du fournisseur du processeur.",
                    "The CPU vendor identification string.",
                ),
            ),
            Item::action(
                tx(lang, "Revenir", "Go back"),
                tx(lang, "Revenir à l'onglet Avancé.", "Return to the Advanced tab."),
                Some(clip(lang, hda::CLIP_FR_ACT_BACK, hda::CLIP_ACT_BACK)),
                Action::Back
            ),
        ],
    });

    // Submenu: Secure Boot (child of Security).
    let secure_screen = screens.len();
    screens.push(Screen {
        title: String::from("Secure Boot"),
        title_clip: Some(clip(lang, hda::CLIP_FR_SUB_SECURE_BOOT, hda::CLIP_SUB_SECURE_BOOT)),
        items: alloc::vec![
            Item::info(
                format!(
                    "Secure Boot, {}",
                    one_byte_state(
                        cstr16!("SecureBoot"),
                        tx(lang, "activé", "enabled"),
                        tx(lang, "désactivé", "disabled"),
                        tx(lang, "inconnu", "unknown"),
                    )
                ),
                tx(
                    lang,
                    "Si le firmware applique la vérification des signatures Secure Boot.",
                    "Whether the firmware is enforcing Secure Boot signature checks.",
                ),
            ),
            Item::info(
                format!(
                    "{}, {}",
                    tx(lang, "Mode de configuration", "Setup Mode"),
                    one_byte_state(
                        cstr16!("SetupMode"),
                        tx(lang, "mode configuration", "setup mode"),
                        tx(lang, "mode utilisateur", "user mode"),
                        tx(lang, "inconnu", "unknown"),
                    )
                ),
                tx(
                    lang,
                    "Si les clés Secure Boot sont provisionnées (mode utilisateur) ou ouvertes (mode configuration).",
                    "Whether Secure Boot keys are provisioned (user mode) or open (setup mode).",
                ),
            ),
            Item::info(
                format!(
                    "{}, {}",
                    tx(lang, "Clé de plateforme (PK)", "Platform Key (PK)"),
                    key_store_state(cstr16!("PK"), &VariableVendor::GLOBAL_VARIABLE, lang),
                ),
                tx(
                    lang,
                    "La clé de plateforme, racine de confiance de Secure Boot.",
                    "The Platform Key, the root of trust for Secure Boot.",
                ),
            ),
            Item::info(
                format!(
                    "{}, {}",
                    tx(lang, "Clés d'échange (KEK)", "Key Exchange Keys (KEK)"),
                    key_store_state(cstr16!("KEK"), &VariableVendor::GLOBAL_VARIABLE, lang),
                ),
                tx(
                    lang,
                    "Les clés autorisées à mettre à jour les bases de signatures.",
                    "The keys allowed to update the signature databases.",
                ),
            ),
            Item::info(
                format!(
                    "{}, {}",
                    tx(lang, "Base autorisée (db)", "Allowed database (db)"),
                    key_store_state(cstr16!("db"), &IMAGE_SECURITY_DATABASE, lang),
                ),
                tx(
                    lang,
                    "Les signatures autorisées à démarrer.",
                    "The signatures allowed to boot.",
                ),
            ),
            Item::info(
                format!(
                    "{}, {}",
                    tx(lang, "Base interdite (dbx)", "Forbidden database (dbx)"),
                    key_store_state(cstr16!("dbx"), &IMAGE_SECURITY_DATABASE, lang),
                ),
                tx(
                    lang,
                    "Les signatures révoquées, interdites de démarrage.",
                    "The revoked signatures, forbidden from booting.",
                ),
            ),
            Item::info(
                format!("{}, {}", tx(lang, "Module TPM", "TPM"), tpm_status(lang)),
                tx(
                    lang,
                    "L'état du module de plateforme sécurisée, d'après l'interface TCG2.",
                    "The Trusted Platform Module state, from the TCG2 interface.",
                ),
            ),
            Item::action(
                tx(lang, "Revenir", "Go back"),
                tx(lang, "Revenir à l'onglet Sécurité.", "Return to the Security tab."),
                Some(clip(lang, hda::CLIP_FR_ACT_BACK, hda::CLIP_ACT_BACK)),
                Action::Back
            ),
        ],
    });

    // Submenu: Boot Option Priorities (child of Boot), with one submenu per real boot
    // device. Build each device's action screen first, then the priorities list.
    let options = enumerate_boot_options();
    let mut priority_items: Vec<Item> = Vec::new();
    for option in &options {
        let device_screen = screens.len();
        screens.push(Screen {
            title: option.label.clone(),
            title_clip: Some(clip(lang, hda::CLIP_FR_BOOT_DEVICE, hda::CLIP_BOOT_DEVICE)),
            items: alloc::vec![
                Item::action(
                    tx(lang, "Démarrer ce périphérique maintenant", "Boot this device now"),
                    tx(
                        lang,
                        "Démarrer le périphérique sélectionné au prochain redémarrage.",
                        "Boot the selected device on the next restart.",
                    ),
                    Some(clip(lang, hda::CLIP_FR_ACT_BOOT_NOW, hda::CLIP_ACT_BOOT_NOW)),
                    Action::BootNow(option.id),
                ),
                Item::action(
                    tx(
                        lang,
                        "Définir comme périphérique de démarrage par défaut",
                        "Make this the default boot device",
                    ),
                    tx(
                        lang,
                        "Placer ce périphérique en tête de l'ordre de démarrage, de façon permanente.",
                        "Put this device first in the firmware boot order, permanently.",
                    ),
                    Some(clip(lang, hda::CLIP_FR_ACT_MAKE_DEFAULT, hda::CLIP_ACT_MAKE_DEFAULT)),
                    Action::MakeDefault(option.id),
                ),
                Item::action(
                    tx(lang, "Monter dans l'ordre de démarrage", "Move up in boot order"),
                    tx(
                        lang,
                        "Monter ce périphérique d'une place dans l'ordre de démarrage, de façon permanente.",
                        "Move this device one place earlier in the boot order, permanently.",
                    ),
                    Some(clip(lang, hda::CLIP_FR_ACT_MOVE_UP, hda::CLIP_ACT_MOVE_UP)),
                    Action::MoveUp(option.id),
                ),
                Item::action(
                    tx(lang, "Descendre dans l'ordre de démarrage", "Move down in boot order"),
                    tx(
                        lang,
                        "Descendre ce périphérique d'une place dans l'ordre de démarrage, de façon permanente.",
                        "Move this device one place later in the boot order, permanently.",
                    ),
                    Some(clip(lang, hda::CLIP_FR_ACT_MOVE_DOWN, hda::CLIP_ACT_MOVE_DOWN)),
                    Action::MoveDown(option.id),
                ),
                Item::action(
                    tx(lang, "Revenir", "Go back"),
                    tx(lang, "Revenir à la liste des périphériques.", "Return to the boot device list."),
                    Some(clip(lang, hda::CLIP_FR_ACT_BACK, hda::CLIP_ACT_BACK)),
                    Action::Back,
                ),
            ],
        });
        priority_items.push(Item::dynamic(
            option.label.clone(),
            tx(
                lang,
                "Ouvrir ce périphérique pour le démarrer ou le définir par défaut.",
                "Open this boot device to boot it now or make it the default.",
            ),
            Some(clip(lang, hda::CLIP_FR_BOOT_DEVICE, hda::CLIP_BOOT_DEVICE)),
            Action::SubMenu(device_screen),
        ));
    }
    priority_items.push(Item::action(
        tx(lang, "Revenir", "Go back"),
        tx(
            lang,
            "Revenir à l'onglet Démarrage.",
            "Return to the Boot tab.",
        ),
        Some(clip(lang, hda::CLIP_FR_ACT_BACK, hda::CLIP_ACT_BACK)),
        Action::Back,
    ));
    let priorities_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Priorités de démarrage", "Boot Option Priorities")),
        title_clip: Some(clip(
            lang,
            hda::CLIP_FR_SUB_BOOT_PRIO,
            hda::CLIP_SUB_BOOT_PRIO,
        )),
        items: priority_items,
    });

    // Top tab: Main. Lead with the Language selector (a real ASUS/AMI BIOS puts "System
    // Language" on the Main page), then the SMBIOS system identity a real BIOS shows - the
    // machine's model, serial and BIOS version - which a blind user otherwise cannot hear.
    let identity = read_system_identity();
    let mut main_items: Vec<Item> = Vec::new();
    main_items.push(Item::action(
        tx(lang, "Langue : Français", "Language: English"),
        tx(
            lang,
            "Choisir la langue de cet utilitaire ; appuyez sur Entrée pour passer en anglais.",
            "Choose this utility's language; press Enter to switch to French.",
        ),
        Some(clip(lang, hda::CLIP_FR_LANG, hda::CLIP_ACT_LANGUAGE)),
        Action::ToggleLang,
    ));
    if let Some(system_name) = identity.system {
        main_items.push(Item::info(
            format!("{}, {system_name}", tx(lang, "Système", "System")),
            tx(
                lang,
                "Le fabricant et le modèle de la machine, d'après SMBIOS.",
                "The machine's manufacturer and model, from SMBIOS.",
            ),
        ));
    }
    if let Some(serial) = identity.serial {
        main_items.push(Item::info(
            format!("{}, {serial}", tx(lang, "Numéro de série", "Serial number")),
            tx(
                lang,
                "Le numéro de série de la machine, d'après SMBIOS.",
                "The machine's serial number, from SMBIOS.",
            ),
        ));
    }
    if let Some(bios) = identity.bios {
        main_items.push(Item::info(
            format!("BIOS, {bios}"),
            tx(
                lang,
                "Le fournisseur, la version et la date du BIOS, d'après SMBIOS.",
                "The BIOS vendor, version and release date, from SMBIOS.",
            ),
        ));
    }
    main_items.push(Item::info(
        format!(
            "{}, {}",
            tx(lang, "Fournisseur du firmware", "Firmware vendor"),
            system::firmware_vendor()
        ),
        tx(
            lang,
            "Le firmware UEFI qui a démarré cette machine.",
            "The UEFI firmware that started this machine.",
        ),
    ));
    let revision = system::firmware_revision();
    main_items.push(Item::info(
        format!(
            "{}, {}.{}",
            tx(lang, "Version du firmware", "Firmware version"),
            revision >> 16,
            revision & 0xffff
        ),
        tx(
            lang,
            "Le numéro de version du firmware.",
            "The firmware's own version number.",
        ),
    ));
    let uefi = system::uefi_revision();
    main_items.push(Item::info(
        format!(
            "{}, {}.{}",
            tx(lang, "Version UEFI", "UEFI version"),
            uefi.major(),
            uefi.minor()
        ),
        tx(
            lang,
            "La révision de la spécification UEFI implémentée par le firmware.",
            "The UEFI specification revision the firmware implements.",
        ),
    ));
    if let Ok(time) = runtime::get_time() {
        main_items.push(Item::info(
            format!(
                "{}, {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                tx(lang, "Heure système", "System time"),
                time.year(),
                time.month(),
                time.day(),
                time.hour(),
                time.minute(),
                time.second()
            ),
            tx(
                lang,
                "La date et l'heure de l'horloge temps réel.",
                "The real-time clock's current date and time.",
            ),
        ));
    }
    main_items.push(Item::info(
        format!(
            "{}, {} {}",
            tx(lang, "Mémoire installée", "Installed memory"),
            installed_memory_mib(),
            tx(lang, "Mio", "mebibytes")
        ),
        tx(
            lang,
            "La mémoire totale rapportée par le firmware dans sa carte mémoire.",
            "Total memory the firmware reported in its memory map.",
        ),
    ));
    main_items.push(Item::info(
        format!(
            "{}, {} {} {}",
            tx(lang, "Résolution d'affichage", "Display resolution"),
            width,
            tx(lang, "par", "by"),
            height
        ),
        tx(
            lang,
            "Le mode graphique fourni par le firmware à omni-os.",
            "The graphics mode the firmware handed to omni-os.",
        ),
    ));
    let main_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Principal", "Main")),
        title_clip: Some(clip(lang, hda::CLIP_FR_TAB_MAIN, hda::CLIP_TAB_MAIN)),
        items: main_items,
    });

    // Submenu: Firmware Settings - the firmware's own HII settings, browsable by arrows and
    // changeable in place (Enter cycles the value), so reaching them no longer means typing an
    // agent command. This is where "cover everything" lives: every varstore-bound question the
    // firmware publishes, in one navigable list.
    let all_settings = crate::hii_ifr::enumerate_settings();
    let config = firmware_config_values();
    let mut settings_items: Vec<Item> = Vec::new();
    for (index, setting) in all_settings.iter().enumerate() {
        let value = setting_current_value(setting, &config);
        settings_items.push(Item::dynamic(
            format!(
                "{}, {}",
                setting.name,
                setting_value_text(setting, value, lang)
            ),
            tx(
                lang,
                "Appuyez sur Entrée pour changer ce réglage du firmware.",
                "Press Enter to change this firmware setting.",
            ),
            None,
            Action::ChangeSetting(index),
        ));
    }
    settings_items.push(Item::action(
        tx(lang, "Revenir", "Go back"),
        tx(
            lang,
            "Revenir à l'onglet Avancé.",
            "Return to the Advanced tab.",
        ),
        Some(clip(lang, hda::CLIP_FR_ACT_BACK, hda::CLIP_ACT_BACK)),
        Action::Back,
    ));
    let settings_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Réglages du firmware", "Firmware settings")),
        title_clip: None,
        items: settings_items,
    });

    // Top tab: Advanced (CPU Configuration, and the browsable Firmware Settings when the firmware
    // publishes any).
    let mut advanced_items = alloc::vec![Item::action(
        tx(lang, "Configuration du processeur", "CPU Configuration"),
        tx(
            lang,
            "Détails du processeur et état de la virtualisation.",
            "Processor details and virtualization state.",
        ),
        Some(clip(lang, hda::CLIP_FR_SUB_CPU, hda::CLIP_SUB_CPU)),
        Action::SubMenu(cpu_screen),
    )];
    if !all_settings.is_empty() {
        advanced_items.push(Item::dynamic(
            format!(
                "{} ({})",
                tx(lang, "Réglages du firmware", "Firmware settings"),
                all_settings.len()
            ),
            tx(
                lang,
                "Parcourir et changer les réglages publiés par le firmware.",
                "Browse and change the settings the firmware publishes.",
            ),
            None,
            Action::SubMenu(settings_screen),
        ));
    }
    let advanced_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Avancé", "Advanced")),
        title_clip: Some(clip(
            lang,
            hda::CLIP_FR_TAB_ADVANCED,
            hda::CLIP_TAB_ADVANCED,
        )),
        items: advanced_items,
    });

    // Top tab: Boot (Boot normally + the priorities submenu, then the boot-manager state
    // a real BIOS shows: which entry booted this time, and the boot-menu timeout).
    let mut boot_items = alloc::vec![
        Item::action(
            tx(lang, "Démarrer normalement", "Boot normally"),
            tx(
                lang,
                "Continuer et charger omni-os.",
                "Continue and load omni-os now."
            ),
            Some(clip(
                lang,
                hda::CLIP_FR_ACT_BOOT_NORMALLY,
                hda::CLIP_ACT_BOOT_NORMALLY
            )),
            Action::BootNormally,
        ),
        Item::action(
            tx(lang, "Priorités de démarrage", "Boot Option Priorities"),
            tx(
                lang,
                "Les périphériques de démarrage de la machine, à démarrer ou réordonner.",
                "The machine's boot devices, to boot now or reorder.",
            ),
            Some(clip(
                lang,
                hda::CLIP_FR_SUB_BOOT_PRIO,
                hda::CLIP_SUB_BOOT_PRIO
            )),
            Action::SubMenu(priorities_screen),
        ),
    ];
    if let Some(current) = boot_current_label() {
        boot_items.push(Item::info(
            format!(
                "{}, {current}",
                tx(lang, "Démarrage actuel", "Boot current")
            ),
            tx(
                lang,
                "L'entrée de démarrage sélectionnée par le firmware pour ce démarrage.",
                "The boot entry the firmware selected for the current boot.",
            ),
        ));
    }
    if let Some(timeout) = boot_timeout_label(lang) {
        boot_items.push(Item::info(
            format!(
                "{}, {timeout}",
                tx(lang, "Délai du menu de démarrage", "Boot menu timeout")
            ),
            tx(
                lang,
                "Le temps d'attente du menu de démarrage du firmware.",
                "How long the firmware's own boot menu waits before booting.",
            ),
        ));
    }
    let boot_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Démarrage", "Boot")),
        title_clip: Some(clip(lang, hda::CLIP_FR_TAB_BOOT, hda::CLIP_TAB_BOOT)),
        items: boot_items,
    });

    // Top tab: Security (Secure Boot submenu).
    let security_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Sécurité", "Security")),
        title_clip: Some(clip(
            lang,
            hda::CLIP_FR_TAB_SECURITY,
            hda::CLIP_TAB_SECURITY,
        )),
        items: alloc::vec![Item::action(
            "Secure Boot",
            tx(
                lang,
                "État de Secure Boot et du mode de configuration.",
                "Secure Boot and Setup Mode state."
            ),
            Some(clip(
                lang,
                hda::CLIP_FR_SUB_SECURE_BOOT,
                hda::CLIP_SUB_SECURE_BOOT
            )),
            Action::SubMenu(secure_screen),
        ),],
    });

    // Top tab: Save and Exit (the actions).
    let save_exit_screen = screens.len();
    screens.push(Screen {
        title: String::from(tx(lang, "Enregistrer et quitter", "Save and Exit")),
        title_clip: Some(clip(
            lang,
            hda::CLIP_FR_TAB_SAVEEXIT,
            hda::CLIP_TAB_SAVEEXIT,
        )),
        items: alloc::vec![
            Item::action(
                tx(lang, "Démarrer normalement", "Boot normally"),
                tx(
                    lang,
                    "Continuer et charger omni-os.",
                    "Continue and load omni-os now."
                ),
                Some(clip(
                    lang,
                    hda::CLIP_FR_ACT_BOOT_NORMALLY,
                    hda::CLIP_ACT_BOOT_NORMALLY
                )),
                Action::BootNormally,
            ),
            Item::action(
                tx(
                    lang,
                    "Entrer dans la configuration du firmware",
                    "Enter firmware setup"
                ),
                tx(
                    lang,
                    "Redémarrer dans l'écran de configuration du firmware.",
                    "Restart into the firmware's own setup screen.",
                ),
                Some(clip(
                    lang,
                    hda::CLIP_FR_ACT_ENTER_SETUP,
                    hda::CLIP_ACT_ENTER_SETUP
                )),
                Action::EnterSetup,
            ),
            Item::action(
                tx(lang, "Redémarrer le système", "Reset the system"),
                tx(
                    lang,
                    "Redémarrer la machine maintenant.",
                    "Restart the machine now."
                ),
                Some(clip(lang, hda::CLIP_FR_ACT_RESET, hda::CLIP_ACT_RESET)),
                Action::Reset,
            ),
            Item::action(
                tx(lang, "Éteindre le système", "Shut down the system"),
                tx(
                    lang,
                    "Éteindre la machine maintenant.",
                    "Power the machine off now."
                ),
                Some(clip(
                    lang,
                    hda::CLIP_FR_ACT_SHUTDOWN,
                    hda::CLIP_ACT_SHUTDOWN
                )),
                Action::Shutdown,
            ),
        ],
    });

    Tree {
        screens,
        tabs: alloc::vec![
            main_screen,
            advanced_screen,
            boot_screen,
            security_screen,
            save_exit_screen,
        ],
        boot_tab: 2,
        settings: all_settings,
    }
}

// ---- Rendering and speech ------------------------------------------------------

/// Draw the current screen on the visible console: title, the tab bar (when at a top
/// tab), the items with the focus marked, the focused item's help line, and the key
/// legend. This is the sighted mirror of what is spoken; the spoken form is primary.
fn render(tree: &Tree, tab_index: usize, screen_index: usize, item_index: usize, depth: usize) {
    let _ = system::with_stdout(|stdout| stdout.clear());
    uefi::println!("omni-os Setup Utility");

    if depth == 0 {
        let mut bar = String::new();
        for (index, &screen) in tree.tabs.iter().enumerate() {
            if index == tab_index {
                bar.push_str(&format!("[{}]  ", tree.screens[screen].title));
            } else {
                bar.push_str(&format!(" {}   ", tree.screens[screen].title));
            }
        }
        uefi::println!("{bar}");
    } else {
        uefi::println!("{}", tree.screens[screen_index].title);
    }
    uefi::println!();

    let screen = &tree.screens[screen_index];
    for (index, item) in screen.items.iter().enumerate() {
        let marker = if index == item_index { ">" } else { " " };
        uefi::println!("  {marker} {}", item.text);
    }

    uefi::println!();
    if let Some(item) = screen.items.get(item_index) {
        uefi::println!("  {}", item.help);
    }
    uefi::println!();
    if depth == 0 {
        uefi::println!(
            "  Left/Right: tab.  Up/Down: item.  Enter: select.  Esc: boot normally.  Space: repeat.  A: read all.  S: spell.  C: command.  Plus/minus: volume.  Brackets: rate.  Comma/dot: pitch.  V: verbosity.  X: punctuation.  O: word.  N/B: review.  G: go to review.  M: mute.  P: phonetic.  H: help."
        );
    } else {
        uefi::println!(
            "  Up/Down: item.  Enter: select.  Esc: back.  Space: repeat.  A: read all.  S: spell.  C: command.  Plus/minus: volume.  Brackets: rate.  Comma/dot: pitch.  V: verbosity.  X: punctuation.  O: word.  N/B: review.  G: go to review.  M: mute.  P: phonetic.  H: help.  W: where."
        );
    }
}

/// Announce a screen's title through the one screen-reader engine and speak its clip:
/// a top tab as "<Name>, tab, <n> of 5" (`AW_UEFI_SETUP_TAB`), a submenu by title
/// (`AW_UEFI_SETUP_MENU`).
fn announce_screen(
    tree: &Tree,
    tab_index: usize,
    screen_index: usize,
    depth: usize,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    let screen = &tree.screens[screen_index];
    let (name, context) = if depth == 0 {
        (
            format!("{}, tab", screen.title),
            FocusContext::in_set(tab_index as u32 + 1, tree.tabs.len() as u32),
        )
    } else {
        (screen.title.clone(), FocusContext::NONE)
    };
    let node = SemanticNode {
        id: NodeId(300),
        parent: Some(NodeId(0)),
        role: Role::StaticText,
        name: &name,
        description: "",
        value: "",
        state: State::from_bits(0),
        bounds: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 32,
        },
    };
    if validate_node(&node).is_err() {
        log::error!("AW_UEFI_MENU_FAIL reason=invalid_screen");
        return;
    }
    let mut buffer = [0u8; 192];
    let text = announce_focus(&node, context, &mut buffer);
    if depth == 0 {
        aw_mark!("AW_UEFI_SETUP_TAB \"{text}\"");
    } else {
        aw_mark!("AW_UEFI_SETUP_MENU \"{text}\"");
    }
    if let Some(clip) = screen.title_clip {
        play(clip, speaker, pending);
    }
}

/// Announce the focused item through the engine and speak its clip. A selectable row is
/// a "menu item" (`AW_UEFI_MENU_ITEM`), a read-only row is static text
/// (`AW_UEFI_SETUP_ITEM`). Returns false only on an invariant violation (a bug).
fn announce_item(
    screen: &Screen,
    item_index: usize,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) -> bool {
    let Some(item) = screen.items.get(item_index) else {
        return true;
    };
    let node = SemanticNode {
        id: NodeId(400 + item_index as u64),
        parent: Some(NodeId(0)),
        role: item.action.role(),
        name: &item.text,
        description: "",
        value: "",
        state: match item.action {
            Action::Info => State::from_bits(0),
            _ => State::from_bits(State::FOCUSABLE),
        },
        bounds: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 32,
        },
    };
    if validate_node(&node).is_err() {
        log::error!("AW_UEFI_MENU_FAIL reason=invalid_item");
        return false;
    }
    let mut buffer = [0u8; 192];
    // Verbosity: at the "low" level the "n of N" position is dropped for a terser announcement;
    // "medium" and "high" keep it.
    let verbosity = VERBOSITY.load(core::sync::atomic::Ordering::Relaxed);
    let context = if verbosity == 0 {
        FocusContext::NONE
    } else {
        FocusContext::in_set(item_index as u32 + 1, screen.items.len() as u32)
    };
    let text = announce_focus(&node, context, &mut buffer);
    match item.action {
        Action::Info => aw_mark!("AW_UEFI_SETUP_ITEM \"{text}\""),
        _ => aw_mark!("AW_UEFI_MENU_ITEM \"{text}\""),
    }
    let lang = if CURRENT_FRENCH.load(core::sync::atomic::Ordering::Relaxed) {
        Lang::Fr
    } else {
        Lang::En
    };
    if let Some(clip) = item.clip {
        play(clip, speaker, pending);
    } else {
        // No pre-recorded clip - a read-only value line (CPU, memory, Secure Boot) or a dynamic
        // firmware-setting row. Speak the text itself on arrival (it is already "label, value",
        // the VoiceOver order), so the value is heard without pressing the spell key.
        speak_dynamic(&item.text, lang, speaker, pending);
    }
    // A VoiceOver-style hint on an actionable item: a short "what this does", spoken after the
    // label so the interaction teaches itself. An adjustable firmware setting says "adjustable,
    // Enter to change"; a submenu says "Enter to open". Dropped at the terse (low) verbosity.
    if verbosity >= 1 {
        let hint = match item.action {
            Action::ChangeSetting(_) => Some(tx(
                lang,
                "réglable, Entrée pour changer",
                "adjustable, Enter to change",
            )),
            Action::SubMenu(_) => Some(tx(lang, "Entrée pour ouvrir", "Enter to open")),
            _ => None,
        };
        if let Some(hint) = hint {
            aw_mark!("AW_UEFI_SETUP_HINT \"{hint}\"");
            speak_dynamic(hint, lang, speaker, pending);
        }
    }
    // High verbosity: also read the full help line, so a new user hears what each item does
    // without pressing H.
    if verbosity == 2 && !item.help.is_empty() {
        aw_mark!("AW_UEFI_SETUP_HELP \"{}\"", item.help);
        speak_dynamic(&item.help, lang, speaker, pending);
    }
    true
}

// ---- Actions -------------------------------------------------------------------

/// Set `BootNext` to `id` so the firmware boots that option on the next restart, then
/// cold-reset. Returns without resetting only if the variable write fails, so a failure
/// never strands the user.
fn boot_now(id: u16) {
    let data = id.to_le_bytes();
    match runtime::set_variable(
        cstr16!("BootNext"),
        &VariableVendor::GLOBAL_VARIABLE,
        boot_var_attributes(),
        &data,
    ) {
        Ok(()) => {
            aw_mark!(
                "AW_UEFI_MENU_SELECT name=\"boot_now\" boot_next=0x{:04x}",
                id
            );
            runtime::reset(runtime::ResetType::COLD, Status::SUCCESS, None);
        }
        Err(error) => {
            log::error!(
                "AW_UEFI_MENU_FAIL reason=boot_next status={:?}",
                error.status()
            );
        }
    }
}

/// Rewrite `BootOrder` so `id` is first, making it the persistent default boot device -
/// the reorder a blind user cannot do in a silent firmware. Reads the current order,
/// moves `id` to the front, and writes it back; stays in the menu on success (no reset),
/// so the user can keep reviewing. Returns whether the order was changed.
fn make_default(id: u16) -> bool {
    let Some(order) = read_global(cstr16!("BootOrder")) else {
        log::error!("AW_UEFI_MENU_FAIL reason=make_default_no_order");
        return false;
    };
    let mut ids: Vec<u16> = order
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_le_bytes(*p))
        .collect();
    ids.retain(|&existing| existing != id);
    ids.insert(0, id);
    let mut bytes = Vec::with_capacity(ids.len() * 2);
    for value in &ids {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    match runtime::set_variable(
        cstr16!("BootOrder"),
        &VariableVendor::GLOBAL_VARIABLE,
        boot_var_attributes(),
        &bytes,
    ) {
        Ok(()) => {
            aw_mark!(
                "AW_UEFI_MENU_SELECT name=\"make_default\" boot_first=0x{:04x}",
                id
            );
            true
        }
        Err(error) => {
            log::error!(
                "AW_UEFI_MENU_FAIL reason=boot_order status={:?}",
                error.status()
            );
            false
        }
    }
}

/// Move `id` one position earlier (`up`) or later in `BootOrder` - a finer, persistent
/// reorder than "make default", the boot priority a blind user could not otherwise change.
/// A no-op (returns false) if `id` is absent or already at the end it is moving toward.
fn move_in_boot_order(id: u16, up: bool) -> bool {
    let Some(order) = read_global(cstr16!("BootOrder")) else {
        log::error!("AW_UEFI_MENU_FAIL reason=move_no_order");
        return false;
    };
    let mut ids: Vec<u16> = order
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_le_bytes(*p))
        .collect();
    let Some(pos) = ids.iter().position(|&existing| existing == id) else {
        return false;
    };
    let target = if up {
        if pos == 0 {
            return false;
        }
        pos - 1
    } else {
        if pos + 1 >= ids.len() {
            return false;
        }
        pos + 1
    };
    ids.swap(pos, target);
    let mut bytes = Vec::with_capacity(ids.len() * 2);
    for value in &ids {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    match runtime::set_variable(
        cstr16!("BootOrder"),
        &VariableVendor::GLOBAL_VARIABLE,
        boot_var_attributes(),
        &bytes,
    ) {
        Ok(()) => {
            aw_mark!(
                "AW_UEFI_MENU_SELECT name=\"move_{}\" boot=0x{:04x} position={}",
                if up { "up" } else { "down" },
                id,
                target
            );
            true
        }
        Err(error) => {
            log::error!(
                "AW_UEFI_MENU_FAIL reason=boot_order status={:?}",
                error.status()
            );
            false
        }
    }
}

/// Request the firmware's own setup UI on the next boot through `OsIndications`, then
/// cold-reset - but only when the firmware advertises support in `OsIndicationsSupported`.
/// Otherwise it is a no-op with a marker, never a lie.
fn enter_firmware_setup() {
    let read_u64 = |name| {
        read_global(name)
            .filter(|bytes| bytes.len() >= 8)
            .map(|bytes| {
                u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            })
            .unwrap_or(0)
    };
    if read_u64(cstr16!("OsIndicationsSupported")) & OS_INDICATIONS_BOOT_TO_FW_UI == 0 {
        aw_mark!("AW_UEFI_MENU_SETUP_UNSUPPORTED");
        return;
    }
    let current = read_u64(cstr16!("OsIndications")) | OS_INDICATIONS_BOOT_TO_FW_UI;
    match runtime::set_variable(
        cstr16!("OsIndications"),
        &VariableVendor::GLOBAL_VARIABLE,
        boot_var_attributes(),
        &current.to_le_bytes(),
    ) {
        Ok(()) => {
            aw_mark!("AW_UEFI_MENU_SELECT name=\"enter_setup\"");
            runtime::reset(runtime::ResetType::COLD, Status::SUCCESS, None);
        }
        Err(error) => {
            log::error!(
                "AW_UEFI_MENU_FAIL reason=os_indications status={:?}",
                error.status()
            );
        }
    }
}

// ---- Input and loop ------------------------------------------------------------

/// Read one key from the firmware console without blocking. Any read error is treated as
/// "no key", so a flaky console cannot wedge the boot.
pub(crate) fn read_key_raw() -> Option<Key> {
    system::with_stdin(|stdin| stdin.read_key().unwrap_or(None))
}

/// Play a clip with barge-in - the screen-reader behaviour of stopping speech the moment
/// the user acts. If a key is already queued in `pending`, the clip is skipped entirely;
/// if a key arrives while it plays, the clip is cut short and that key stashed in
/// `pending`, so the caller handles it next instead of the setup talking over it. A no-op
/// when there is no audio codec.
fn play(clip: &'static [u8], speaker: &mut Option<audio::Speaker>, pending: &mut Option<Key>) {
    if pending.is_some() {
        return;
    }
    if let Some(sp) = speaker.as_mut() {
        let mut hit: Option<Key> = None;
        // `speak_until` returns whether the DMA link position actually advanced - i.e.
        // whether the codec really streamed the samples. A codec can be present, brought
        // up and unmuted yet still stream nothing (a topology this driver mis-routed, a
        // disconnected jack): the classic way "the audio works" turns out unreliable.
        let advanced = sp.speak_until(clip, || {
            if hit.is_none() {
                hit = read_key_raw();
            }
            hit.is_some()
        });
        if hit.is_some() {
            *pending = hit;
        } else if !advanced {
            // The words did not sound. Guarantee audible feedback with a PC-speaker cue,
            // so a blind user is never left with silence they cannot tell from a hang.
            aw_mark!("AW_UEFI_AUDIO_FALLBACK reason=stream_silent");
            sound::cue(CUE_READY_HZ, Duration::from_millis(60));
        }
    }
}

/// Error prevention: before an irreversible action - one that reboots or powers the
/// machine off - require a second, deliberate keystroke. The prompt is spoken and shown;
/// Enter confirms, anything else cancels. Returns whether to proceed. The user has already
/// pressed a key to get here, so this deliberately blocks; the timed proof, which presses
/// no key, never selects such an action and so never reaches it.
fn confirm(lang: Lang, speaker: &mut Option<audio::Speaker>, pending: &mut Option<Key>) -> bool {
    uefi::println!(
        "  {}",
        tx(
            lang,
            "Appuyez à nouveau sur Entrée pour confirmer, ou Échap pour annuler.",
            "Press Enter again to confirm, or Escape to cancel.",
        )
    );
    aw_mark!("AW_UEFI_SETUP_CONFIRM");
    sound::cue(CUE_READY_HZ, Duration::from_millis(90));
    play(
        clip(lang, hda::CLIP_FR_CONFIRM_PROMPT, hda::CLIP_CONFIRM_PROMPT),
        speaker,
        pending,
    );
    loop {
        if let Some(key) = pending.take().or_else(read_key_raw) {
            if matches!(classify(key), Nav::Select) {
                return true;
            }
            aw_mark!("AW_UEFI_SETUP_CONFIRM_CANCEL");
            play(
                clip(lang, hda::CLIP_FR_CONFIRM_CANCEL, hda::CLIP_CONFIRM_CANCEL),
                speaker,
                pending,
            );
            return false;
        }
        boot::stall(POLL_INTERVAL);
    }
}

/// A keystroke translated into a setup command. Beyond navigation, the screen-reader
/// affordances a blind user expects: repeat the current item, read its help, say where
/// they are, and jump to the first or last item.
enum Nav {
    NextTab,
    PreviousTab,
    NextItem,
    PreviousItem,
    First,
    Last,
    Select,
    /// Escape: step back out, or boot normally at the top level.
    Back,
    /// Re-read the focused item (Space).
    Repeat,
    /// Read the focused item's help line (H or F1).
    Help,
    /// Say where we are: the screen title and the item's position (W).
    Where,
    /// Spell the focused item character by character (S), for dynamic names.
    Spell,
    /// Read every item on the current screen top to bottom (A), interruptibly.
    SayAll,
    /// Open the command agent (C): type a plain instruction instead of walking the tree.
    Command,
    /// Raise the speech volume (+ or =).
    VolumeUp,
    /// Lower the speech volume (-).
    VolumeDown,
    /// Toggle mute (M).
    Mute,
    /// Toggle NATO phonetic spelling (P).
    Phonetic,
    /// Speak faster (`]`) or slower (`[`): the synthesizer's speech rate.
    RateUp,
    RateDown,
    /// Raise (`.`) or lower (`,`) the synthesized voice pitch.
    PitchUp,
    PitchDown,
    /// Cycle the verbosity level (V).
    Verbosity,
    /// Cycle the punctuation level (X).
    Punctuation,
    /// Read the focused line word by word (O), synthesized.
    ReadByWord,
    /// Move an independent review cursor to the next (N) or previous (B) line and read it,
    /// without changing which item is selected - the classic screen-reader review cursor.
    ReviewNext,
    ReviewPrev,
    /// Move the real selection to the review cursor's line (G).
    FocusToReview,
    Ignore,
}

/// Map a keystroke to a command: Left/Right change tab, Up/Down (or Tab) move the
/// highlight, Home/End jump to the ends, Enter opens/selects, Escape steps back (or boots
/// normally at the top). The screen-reader keys - Space (repeat), H or F1 (help), W
/// (where am I) - work on every screen and never change what is focused.
fn classify(key: Key) -> Nav {
    match key {
        Key::Special(ScanCode::RIGHT) => Nav::NextTab,
        Key::Special(ScanCode::LEFT) => Nav::PreviousTab,
        Key::Special(ScanCode::DOWN) => Nav::NextItem,
        Key::Special(ScanCode::UP) => Nav::PreviousItem,
        Key::Special(ScanCode::HOME) => Nav::First,
        Key::Special(ScanCode::END) => Nav::Last,
        Key::Special(ScanCode::ESCAPE) => Nav::Back,
        Key::Special(ScanCode::FUNCTION_1) => Nav::Help,
        Key::Printable(character) => match char::from(character) {
            '\r' => Nav::Select,
            '\t' => Nav::NextItem,
            ' ' => Nav::Repeat,
            'h' | 'H' => Nav::Help,
            'w' | 'W' => Nav::Where,
            's' | 'S' => Nav::Spell,
            'a' | 'A' => Nav::SayAll,
            'c' | 'C' => Nav::Command,
            '+' | '=' => Nav::VolumeUp,
            '-' | '_' => Nav::VolumeDown,
            'm' | 'M' => Nav::Mute,
            'p' | 'P' => Nav::Phonetic,
            ']' => Nav::RateUp,
            '[' => Nav::RateDown,
            '.' | '>' => Nav::PitchUp,
            ',' | '<' => Nav::PitchDown,
            'v' | 'V' => Nav::Verbosity,
            'x' | 'X' => Nav::Punctuation,
            'o' | 'O' => Nav::ReadByWord,
            'n' | 'N' => Nav::ReviewNext,
            'b' | 'B' => Nav::ReviewPrev,
            'g' | 'G' => Nav::FocusToReview,
            _ => Nav::Ignore,
        },
        Key::Special(_) => Nav::Ignore,
    }
}

/// Read the focused item's help line aloud: on the console and as an
/// `AW_UEFI_SETUP_HELP` marker. Help text is composed at runtime, so it has no clip;
/// this is the "what is this?" a screen-reader user presses H for.
fn announce_help(screen: &Screen, item_index: usize) {
    if let Some(item) = screen.items.get(item_index) {
        uefi::println!("  {}", item.help);
        aw_mark!("AW_UEFI_SETUP_HELP \"{}\"", item.help);
    }
}

/// Spell the focused line character by character through the HDA codec - a screen
/// reader's "read by character", and the answer to the one line a blind user cannot
/// otherwise hear by name: the runtime-composed device names and machine-state values
/// that have no whole-line clip. Letters, digits and spaces are spoken from the spelling
/// alphabet, and meaningful punctuation is spoken by name in the active language so a
/// value's separators are not silently lost. The full text is also emitted as a marker and
/// shown on the console.
fn spell_current(
    text: &str,
    lang: Lang,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    uefi::println!("  Spelling: {text}");
    aw_mark!("AW_UEFI_SETUP_SPELL \"{text}\"");
    spell_chars(text, matches!(lang, Lang::Fr), speaker, pending);
}

/// Speak a dynamic line as *words* through the runtime formant synthesizer - the boot-device
/// names, machine-state values and setting values that have no pre-recorded clip and were, until
/// now, only spellable. Synthesizes 24 kHz PCM and plays it on the real codec with barge-in;
/// falls back to spelling character by character when there is no codec, the text does not
/// synthesize, or synthesis yields nothing. The full text is emitted as a marker so the boot
/// proofs can assert what was spoken.
pub(crate) fn speak_dynamic(
    text: &str,
    lang: Lang,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    let french = matches!(lang, Lang::Fr);
    // No codec or a queued key: spell it (the always-available fallback).
    if speaker.is_none() || pending.is_some() {
        spell_chars(text, french, speaker, pending);
        return;
    }
    aw_mark!("AW_UEFI_SPEAK \"{text}\"");
    // Word by word: a pre-recorded ST-voice clip from the word bank where the word is known, the
    // formant synthesizer only where it is not. This is what makes dynamic values sound native.
    for token in text.split_whitespace() {
        if pending.is_some() {
            break;
        }
        speak_token(token, french, speaker, pending);
    }
}

/// Play a raw PCM buffer (a bank clip or a synthesized word) with barge-in: a key pressed while
/// it plays is stashed in `pending` so the caller acts on it instead of talking over it.
fn play_bytes(pcm: &[u8], speaker: &mut Option<audio::Speaker>, pending: &mut Option<Key>) {
    if pending.is_some() || pcm.is_empty() {
        return;
    }
    if let Some(sp) = speaker.as_mut() {
        let mut hit: Option<Key> = None;
        sp.speak_until(pcm, || {
            if hit.is_none() {
                hit = read_key_raw();
            }
            hit.is_some()
        });
        if let Some(key) = hit {
            aw_mark!("AW_UEFI_BARGE_IN key={key:?}");
            *pending = Some(key);
        }
    }
}

/// Speak one whitespace-delimited token: a number as its word atoms (each a bank clip), or a word
/// as its bank clip, falling back to the formant synthesizer and then to spelling.
fn speak_token(
    token: &str,
    french: bool,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    let core = token.trim_matches(|c: char| !c.is_alphanumeric());
    if !core.is_empty() && core.chars().all(|c| c.is_ascii_digit()) {
        for word in crate::word_bank::number_atoms(core, french) {
            if pending.is_some() {
                break;
            }
            speak_word(word, french, speaker, pending);
        }
        return;
    }
    let key = core.to_lowercase();
    if let Some(clip) = crate::word_bank::clip_for(&key, french) {
        play_bytes(clip, speaker, pending);
    } else if is_acronym(core) {
        // An acronym (USB, EFI, QEMU): spell it with the pre-recorded letter clips rather
        // than the formant synthesizer, so it stays in the same recorded voice.
        spell_chars(core, french, speaker, pending);
    } else {
        // An unknown word: synthesize the original token (it keeps any internal punctuation),
        // and spell it only if synthesis yields nothing.
        let pcm = crate::synth::say(token, french);
        if pcm.is_empty() {
            spell_chars(token, french, speaker, pending);
        } else {
            play_bytes(&pcm, speaker, pending);
        }
    }
}

/// Whether a token is a short all-upper-case acronym worth spelling in the real letter voice.
fn is_acronym(token: &str) -> bool {
    let letters = token.chars().filter(|c| c.is_ascii_alphabetic()).count();
    (2..=6).contains(&letters)
        && token
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .all(|c| c.is_ascii_uppercase())
}

/// Speak one already-clean word (a number atom): its bank clip, or the synthesizer if absent.
fn speak_word(
    word: &str,
    french: bool,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    if let Some(clip) = crate::word_bank::clip_for(word, french) {
        play_bytes(clip, speaker, pending);
    } else {
        let pcm = crate::synth::say(word, french);
        play_bytes(&pcm, speaker, pending);
    }
}

/// Play one clip per character of `text`, honouring phonetic mode and language, and
/// stopping at once on any key (barge-in). The shared core of both the explicit "spell"
/// command and the automatic reading of a focused value.
fn spell_chars(
    text: &str,
    french: bool,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    for character in text.chars() {
        if pending.is_some() {
            break;
        }
        // Honour the punctuation level: at "none" or "some" the quieter symbols are skipped.
        if !punctuation_spoken(character) {
            continue;
        }
        if let Some(clip) = hda::spell_clip(character, french) {
            play(clip, speaker, pending);
        }
    }
}

/// The active setup language, mirrored into a flag so [`announce_item`] can read a focused
/// value aloud without threading the language through every call site. Set when the setup
/// starts and whenever the language is toggled.
static CURRENT_FRENCH: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// Record the active language for [`announce_item`]'s automatic value reading.
fn set_current_lang(lang: Lang) {
    CURRENT_FRENCH.store(
        matches!(lang, Lang::Fr),
        core::sync::atomic::Ordering::Relaxed,
    );
}

/// How much context is spoken around a focused item - the screen-reader "verbosity" a user
/// tunes to taste. `Low` says the label alone; `Medium` (default) adds the "n of N" position;
/// `High` also reads the help line automatically on focus.
static VERBOSITY: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(1);
/// How much punctuation is spoken when spelling a value - the screen-reader "punctuation level".
/// 0 = none (letters and digits only), 1 = some (the meaningful separators: dot, dash, colon,
/// slash, percent), 2 = all (every symbol named). Default 1.
static PUNCTUATION: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(1);

/// Cycle the verbosity level (Low -> Medium -> High -> Low) and return the new level as a word.
fn cycle_verbosity(lang: Lang) -> &'static str {
    let next = (VERBOSITY.load(core::sync::atomic::Ordering::Relaxed) + 1) % 3;
    VERBOSITY.store(next, core::sync::atomic::Ordering::Relaxed);
    match next {
        0 => tx(lang, "concis", "low"),
        2 => tx(lang, "détaillé", "high"),
        _ => tx(lang, "moyen", "medium"),
    }
}

/// Cycle the punctuation level (none -> some -> all -> none) and return the new level as a word.
fn cycle_punctuation(lang: Lang) -> &'static str {
    let next = (PUNCTUATION.load(core::sync::atomic::Ordering::Relaxed) + 1) % 3;
    PUNCTUATION.store(next, core::sync::atomic::Ordering::Relaxed);
    match next {
        0 => tx(lang, "aucune", "none"),
        2 => tx(lang, "toute", "all"),
        _ => tx(lang, "partielle", "some"),
    }
}

/// Whether the character `c` should be spoken when spelling, at the current punctuation level.
/// Letters, digits and space are always spoken; punctuation depends on the level.
fn punctuation_spoken(c: char) -> bool {
    if c.is_ascii_alphanumeric() || c == ' ' {
        return true;
    }
    match PUNCTUATION.load(core::sync::atomic::Ordering::Relaxed) {
        0 => false,
        2 => true,
        // "some": only the separators that carry meaning in firmware values.
        _ => matches!(c, '.' | '-' | ':' | '/' | '%'),
    }
}

/// The command agent: type a plain instruction ("boot usb", "secure boot", "restart")
/// and it speaks back what it understood and carries it out - one flat command surface
/// instead of walking the whole tree, which a screen-reader user often finds faster. It
/// does the things a loaded UEFI application is allowed to do (boot a device now, set the
/// default boot device, open the firmware's own setup, restart, shut down). Firmware-owned
/// settings a loaded app cannot change - Secure Boot (immutable by the UEFI spec), the
/// virtualization straps - are read aloud and routed to the firmware's own setup instead of
/// pretending to toggle them. Typed characters are echoed and spoken; Enter runs, Escape
/// cancels, Backspace edits.
fn run_agent(lang: Lang, speaker: &mut Option<audio::Speaker>, pending: &mut Option<Key>) {
    let french = matches!(lang, Lang::Fr);
    uefi::println!();
    uefi::println!("Command >");
    aw_mark!("AW_UEFI_AGENT_OPEN");
    play(hda::agent_clip(hda::AGENT_PROMPT, french), speaker, pending);

    let mut buffer = String::new();
    loop {
        let Some(key) = pending.take().or_else(read_key_raw) else {
            boot::stall(POLL_INTERVAL);
            continue;
        };
        match key {
            Key::Special(ScanCode::ESCAPE) => {
                uefi::println!();
                aw_mark!("AW_UEFI_AGENT_CANCEL");
                return;
            }
            Key::Printable(character) => match char::from(character) {
                '\r' => break,
                // Backspace: drop the last character and redraw the line.
                '\u{8}' => {
                    buffer.pop();
                    uefi::print!("\rCommand > {buffer} \r");
                    uefi::print!("Command > {buffer}");
                }
                ch => {
                    buffer.push(ch);
                    uefi::print!("{ch}");
                    // Echo the typed character aloud, so a blind user hears what they enter.
                    if let Some(clip) = hda::spell_clip(ch, french) {
                        play(clip, speaker, pending);
                    }
                }
            },
            Key::Special(_) => {}
        }
    }
    uefi::println!();
    let command = buffer.trim().to_ascii_lowercase();
    aw_mark!("AW_UEFI_AGENT_COMMAND \"{command}\"");
    dispatch_agent(&command, lang, french, speaker, pending);
}

/// Match one typed command to an intent and carry it out. Keyword matching accepts both
/// languages, so "boot usb" and "demarrer usb" both work. Ordered from most specific to
/// least, and terminal actions (boot, restart, shut down, open firmware setup) never
/// return because they reset the machine.
fn dispatch_agent(
    cmd: &str,
    lang: Lang,
    french: bool,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    let ag = |pair| hda::agent_clip(pair, french);
    let has = |needle: &str| cmd.contains(needle);

    if cmd.is_empty() {
        return;
    }

    // Help: speak the list of commands.
    if has("help") || has("aide") || cmd == "?" {
        play(ag(hda::AGENT_HELP), speaker, pending);
        return;
    }

    // Restart / reboot - checked before the boot commands, so "reboot" is not mistaken for a
    // boot-device request. Confirmed first, like the menu.
    if has("restart") || has("reboot") || has("redemarr") {
        if confirm(lang, speaker, pending) {
            play(ag(hda::AGENT_RESTARTING), speaker, pending);
            runtime::reset(runtime::ResetType::COLD, Status::SUCCESS, None);
        }
        return;
    }

    // Shut down / power off - confirmed first.
    if has("shut") || has("eteind") || has("arret") || has("power off") || has("poweroff") {
        if confirm(lang, speaker, pending) {
            play(ag(hda::AGENT_SHUTTING_DOWN), speaker, pending);
            runtime::reset(runtime::ResetType::SHUTDOWN, Status::SUCCESS, None);
        }
        return;
    }

    // Set the firmware boot-manager timeout - the architected global `Timeout` variable a
    // loaded application is allowed to write ("set timeout 5", "delai 5").
    if has("timeout") || has("delai") {
        match first_number(cmd) {
            Some(secs) if set_boot_timeout(secs as u16) => {
                play(ag(hda::AGENT_TIMEOUT_SET), speaker, pending);
                speak_dynamic(&format!("{secs}"), lang, speaker, pending);
            }
            Some(_) => play(ag(hda::AGENT_FAILED), speaker, pending),
            None => play(ag(hda::AGENT_UNKNOWN), speaker, pending),
        }
        return;
    }

    // Reorder a boot entry, earlier ("move up") or later ("move down") in BootOrder.
    if has("move") || has("monter") || has("descendre") || has("priorit") {
        let up = has("up") || has("monter") || has("haut");
        let options = enumerate_boot_options();
        match find_boot_target(cmd, &options) {
            Some(opt) if move_in_boot_order(opt.id, up) => {
                play(ag(hda::AGENT_DONE), speaker, pending)
            }
            Some(_) => play(ag(hda::AGENT_FAILED), speaker, pending),
            None => play(ag(hda::AGENT_NO_MATCH), speaker, pending),
        }
        return;
    }

    // Connect: DHCP through the firmware stack, only because the user asked for it.
    if has("dhcp") || has("connect") || has("connecter") {
        aw_mark!("AW_UEFI_AGENT_NETWORK_CONNECT");
        let text = match crate::net::dhcp("agent_command") {
            Ok(lease) => format!(
                "{} {}",
                tx(lang, "connecté, adresse", "connected, address"),
                crate::net::dotted(lease.address)
            ),
            Err(_) => String::from(tx(lang, "connexion impossible", "connection failed")),
        };
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        speak_dynamic(&text, lang, speaker, pending);
        return;
    }

    // Network: which interfaces exist and whether their link is up. Read-only by policy.
    if has("network") || has("reseau") || has("réseau") || has("ethernet") {
        let state = network_status(lang);
        aw_mark!("AW_UEFI_AGENT_NETWORK");
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        speak_dynamic(&state, lang, speaker, pending);
        return;
    }

    // TPM: read the measured-boot module's presence and PCR-bank state through TCG2.
    if has("tpm") || has("trusted platform") {
        let state = tpm_status(lang);
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        speak_dynamic(&state, lang, speaker, pending);
        return;
    }

    // Secure Boot key hierarchy: the PK/KEK/db/dbx provisioning a silent firmware hides.
    // Checked before the generic "secure" branch so "secure boot keys" reports the key state.
    if has("key") || has("cle") || has("pk") || has("kek") || has("dbx") || has("db ") {
        let summary = secure_boot_key_summary(lang);
        uefi::println!("  {summary}");
        aw_mark!("AW_UEFI_AGENT_SECUREBOOT_KEYS");
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        speak_dynamic(&summary, lang, speaker, pending);
        return;
    }

    // Secure Boot: always speak its state. Only when the user asks to CHANGE it do we explain
    // a loaded app cannot (the spec makes SecureBoot immutable) and route to firmware setup -
    // so simply asking the status never reboots the machine.
    if has("secure") {
        let state = one_byte_state(
            cstr16!("SecureBoot"),
            tx(lang, "active", "enabled"),
            tx(lang, "desactive", "disabled"),
            tx(lang, "inconnu", "unknown"),
        );
        play(ag(hda::AGENT_SECURE_BOOT_IS), speaker, pending);
        speak_dynamic(&state, lang, speaker, pending);
        if wants_change(cmd) {
            play(ag(hda::AGENT_FIRMWARE_ONLY), speaker, pending);
            enter_firmware_setup();
            play(ag(hda::AGENT_SETUP_DENIED), speaker, pending);
        }
        return;
    }

    // Virtualization: read the live CPU state; route to firmware setup only on a change ask.
    if has("virtu") || has("vt-x") || has("vmx") || has("svm") {
        let state = virtualization_status(lang);
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        speak_dynamic(&state, lang, speaker, pending);
        if wants_change(cmd) {
            play(ag(hda::AGENT_FIRMWARE_ONLY), speaker, pending);
            enter_firmware_setup();
            play(ag(hda::AGENT_SETUP_DENIED), speaker, pending);
        }
        return;
    }

    // Set the real-time clock: "set time 14:30", "set date 2026-09-21", "regler l'heure".
    // Checked before the read branch so a change request is not swallowed as a query, and
    // confirmed first because it writes the hardware clock.
    if (has("set") || has("regl") || has("mettre") || has("change") || has("chang"))
        && (has("time") || has("heure") || has("date") || has("clock") || has("horloge"))
    {
        let (date, time) = parse_clock(cmd);
        if date.is_none() && time.is_none() {
            play(ag(hda::AGENT_UNKNOWN), speaker, pending);
            return;
        }
        let (y, mo, d) = date.map_or((None, None, None), |(y, mo, d)| {
            (Some(y), Some(mo), Some(d))
        });
        let (h, mi, s) = time.map_or((None, None, None), |(h, mi, s)| (Some(h), Some(mi), s));
        aw_mark!("AW_UEFI_AGENT_SETCLOCK");
        if confirm(lang, speaker, pending) {
            if set_rtc(y, mo, d, h, mi, s) {
                play(ag(hda::AGENT_DONE), speaker, pending);
                if let Ok(t) = runtime::get_time() {
                    speak_dynamic(
                        &format!(
                            "{:04}-{:02}-{:02} {:02}:{:02}",
                            t.year(),
                            t.month(),
                            t.day(),
                            t.hour(),
                            t.minute()
                        ),
                        lang,
                        speaker,
                        pending,
                    );
                }
            } else {
                play(ag(hda::AGENT_FAILED), speaker, pending);
            }
        }
        return;
    }

    // The firmware's optional driver and system-preparation load lists (Driver####/SysPrep####)
    // - decoded like boot entries and read aloud. Read-only; a silent firmware never lists them.
    if has("driver") || has("sysprep") || has("pilote") || has("prepar") {
        let (order, prefix, kind) = if has("sysprep") || has("prepar") {
            (cstr16!("SysPrepOrder"), 'S', "SysPrep")
        } else {
            (cstr16!("DriverOrder"), 'D', "Driver")
        };
        let options = enumerate_load_options(order, prefix);
        uefi::println!("  {} {} load options", options.len(), kind);
        aw_mark!("AW_UEFI_AGENT_LOADOPTS kind={kind} count={}", options.len());
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        speak_dynamic(&format!("{}", options.len()), lang, speaker, pending);
        for (index, opt) in options.iter().enumerate() {
            if pending.is_some() {
                break;
            }
            speak_dynamic(
                &format!("{}. {}", index + 1, opt.label),
                lang,
                speaker,
                pending,
            );
        }
        return;
    }

    // Show text on a connected HID braille display: "braille <text>" (or the machine summary
    // when no text is given). For a deaf-blind user, the one output that reaches them.
    if has("braille") {
        let text = cmd
            .split_once("braille")
            .map(|(_, rest)| rest.trim())
            .filter(|rest| !rest.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("{} MiB", installed_memory_mib()));
        match crate::usb::find_braille() {
            Some(display) if display.show(&text) => play(ag(hda::AGENT_DONE), speaker, pending),
            Some(_) => play(ag(hda::AGENT_FAILED), speaker, pending),
            None => play(ag(hda::AGENT_NO_MATCH), speaker, pending),
        }
        return;
    }

    // The firmware clock: time and date. ("timeout" was already handled above.)
    if has("time") || has("heure") || has("date") || has("clock") || has("horloge") {
        if let Ok(t) = runtime::get_time() {
            let text = format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                t.year(),
                t.month(),
                t.day(),
                t.hour(),
                t.minute()
            );
            play(ag(hda::AGENT_TIME_IS), speaker, pending);
            speak_dynamic(&text, lang, speaker, pending);
        } else {
            play(ag(hda::AGENT_FAILED), speaker, pending);
        }
        return;
    }

    // Installed memory.
    if has("memory") || has("memoire") || has("ram") {
        let mem = installed_memory_mib();
        play(ag(hda::AGENT_MEMORY_IS), speaker, pending);
        speak_dynamic(
            &format!("{mem} {}", tx(lang, "mega-octets", "megabytes")),
            lang,
            speaker,
            pending,
        );
        return;
    }

    // Processor.
    if has("cpu") || has("processor") || has("processeur") {
        play(ag(hda::AGENT_PROCESSOR_IS), speaker, pending);
        speak_dynamic(&cpu_brand(), lang, speaker, pending);
        return;
    }

    // Firmware identity (vendor and revision) - distinct from opening firmware setup below.
    if (has("firmware") || has("micrologiciel") || has("bios"))
        && (has("version") || has("revision") || has("vendor") || has("fabricant"))
    {
        let text = format!(
            "{}, {}",
            system::firmware_vendor(),
            system::firmware_revision()
        );
        play(ag(hda::AGENT_FIRMWARE_IS), speaker, pending);
        speak_dynamic(&text, lang, speaker, pending);
        return;
    }

    // Full system information: the long facts on the console, the short ones spoken.
    if has("info") || has("system") || has("systeme") || has("machine") {
        let mem = installed_memory_mib();
        let virt = virtualization_status(lang);
        uefi::println!("  {}: {}", tx(lang, "Processeur", "Processor"), cpu_brand());
        uefi::println!("  {}: {mem} MiB", tx(lang, "Memoire", "Memory"));
        uefi::println!("  {}: {virt}", tx(lang, "Virtualisation", "Virtualization"));
        aw_mark!("AW_UEFI_AGENT_INFO mem={mem}");
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        let summary = format!("{mem} {}, {virt}", tx(lang, "mega-octets", "megabytes"));
        speak_dynamic(&summary, lang, speaker, pending);
        return;
    }

    // Read the firmware's own HII configuration - the real settings the firmware publishes,
    // by GUID store and count. This is the foundation for changing them by name later; for
    // now it lets a blind user hear what the firmware actually exposes, not just our tree.
    if has("hii")
        || has("firmware config")
        || has("firmware options")
        || has("options firmware")
        || has("advanced options")
        || has("options avancees")
    {
        match read_firmware_config() {
            Some((groups, settings, names)) => {
                uefi::println!("  HII: {groups} config groups, {settings} settings");
                for name in &names {
                    uefi::println!("    {name}");
                }
                aw_mark!("AW_UEFI_AGENT_HII groups={groups} settings={settings}");
                play(ag(hda::AGENT_VALUE_IS), speaker, pending);
                let summary = format!(
                    "{groups} {}, {settings} {}",
                    tx(lang, "groupes", "groups"),
                    tx(lang, "reglages", "settings")
                );
                speak_dynamic(&summary, lang, speaker, pending);
                // Read the real store names aloud too, so the firmware's own configuration is
                // heard by ear, not just counted. Interruptible.
                for name in &names {
                    if pending.is_some() {
                        break;
                    }
                    speak_dynamic(name, lang, speaker, pending);
                }
            }
            None => play(ag(hda::AGENT_FAILED), speaker, pending),
        }
        return;
    }

    // Read the firmware's own named settings, parsed from its IFR - optionally filtered by a
    // word - with their current values. This is what a blind user cannot otherwise discover.
    if has("settings") || has("reglage") || has("parametre") || has("hidden") || has("cachee") {
        let all = crate::hii_ifr::enumerate_settings();
        let terms = descriptive_terms(
            cmd,
            &[
                "settings",
                "reglages",
                "reglage",
                "parametres",
                "parametre",
                "hidden",
                "cachee",
                "cachees",
                "list",
                "liste",
                "show",
                "read",
                "lire",
                "les",
                "the",
            ],
        );
        let shown: Vec<&crate::hii_ifr::Setting> = all
            .iter()
            .filter(|s| {
                terms.is_empty() || {
                    let name = s.name.to_ascii_lowercase();
                    terms.iter().any(|term| name.contains(term))
                }
            })
            .take(12)
            .collect();
        uefi::println!("  {} firmware settings, {} shown", all.len(), shown.len());
        play(ag(hda::AGENT_VALUE_IS), speaker, pending);
        speak_dynamic(&format!("{}", all.len()), lang, speaker, pending);
        let config = firmware_config_values();
        for setting in &shown {
            if pending.is_some() {
                break;
            }
            // Current value: a real NVRAM variable if there is one, else the HII config export
            // (which covers driver varstores too). Spoken by meaning for a one-of ("Enabled").
            let raw = get_setting_value(setting)
                .or_else(|| config.get(&(setting.guid, setting.offset as u64)).copied());
            let value = match raw {
                Some(v) => setting
                    .label_for(v)
                    .map(String::from)
                    .unwrap_or_else(|| format!("{v}")),
                None => String::new(),
            };
            uefi::println!(
                "    {} = {} [{}:{:#06x}/{}]",
                setting.name,
                value,
                setting.store,
                setting.offset,
                setting.width
            );
            speak_dynamic(
                &format!("{}, {value}", setting.name),
                lang,
                speaker,
                pending,
            );
        }
        return;
    }

    // Change a firmware setting by name - the setup_var method (enable / disable / set to N).
    // Confirmed first, because a wrong write can soft-brick the firmware configuration.
    if has("enable") || has("disable") || has("activ") || has("desactiv") || has("set ") {
        let all = crate::hii_ifr::enumerate_settings();
        let terms = descriptive_terms(
            cmd,
            &[
                "enable",
                "disable",
                "activer",
                "activ",
                "desactiver",
                "desactiv",
                "set",
                "to",
                "the",
                "les",
                "regler",
                "mettre",
            ],
        );
        let target = all.iter().find(|s| {
            let name = s.name.to_ascii_lowercase();
            terms
                .iter()
                .any(|term| term.len() >= 3 && name.contains(term))
        });
        match target {
            Some(setting) => {
                let value = if has("disable") || has("desactiv") {
                    0
                } else if let Some(number) = first_number(cmd) {
                    number as u64
                } else {
                    // Accept an option label too: "set iSCSI mode to enabled" -> that option's
                    // value; otherwise default to 1 (enable).
                    setting
                        .options
                        .iter()
                        .find(|(_, label)| {
                            let label = label.to_ascii_lowercase();
                            terms
                                .iter()
                                .any(|term| term.len() >= 3 && label.contains(term))
                        })
                        .map(|(value, _)| *value)
                        .unwrap_or(1)
                };
                uefi::println!(
                    "  Set {} = {} [{}:{:#06x}/{}]",
                    setting.name,
                    value,
                    setting.store,
                    setting.offset,
                    setting.width
                );
                aw_mark!(
                    "AW_UEFI_AGENT_SETVAR store={} offset={}",
                    setting.store,
                    setting.offset
                );
                if confirm(lang, speaker, pending) {
                    if set_setting_value(setting, value) {
                        play(ag(hda::AGENT_DONE), speaker, pending);
                    } else {
                        play(ag(hda::AGENT_FAILED), speaker, pending);
                    }
                }
            }
            None => play(ag(hda::AGENT_NO_MATCH), speaker, pending),
        }
        return;
    }

    // Open the firmware's own setup (for everything a loaded app cannot reach).
    if has("firmware") || has("setup") || has("bios") || has("config") {
        play(ag(hda::AGENT_OPENING_SETUP), speaker, pending);
        enter_firmware_setup();
        play(ag(hda::AGENT_SETUP_DENIED), speaker, pending);
        return;
    }

    // Boot management: list the entries, or boot / set-default one by name or position.
    if has("boot")
        || has("demarr")
        || has("default")
        || has("defaut")
        || has("par def")
        || has("list")
        || has("liste")
    {
        let options = enumerate_boot_options();
        if has("list") || has("liste") {
            play(ag(hda::AGENT_BOOT_LIST), speaker, pending);
            for (index, opt) in options.iter().enumerate() {
                if pending.is_some() {
                    break;
                }
                speak_dynamic(
                    &format!("{}. {}", index + 1, opt.label),
                    lang,
                    speaker,
                    pending,
                );
            }
            return;
        }
        let want_default = has("default") || has("defaut") || has("par def");
        match find_boot_target(cmd, &options) {
            None => play(ag(hda::AGENT_NO_MATCH), speaker, pending),
            Some(opt) if want_default => {
                if make_default(opt.id) {
                    play(ag(hda::AGENT_SET_DEFAULT), speaker, pending);
                }
            }
            Some(opt) => {
                play(ag(hda::AGENT_BOOTING), speaker, pending);
                boot_now(opt.id); // sets BootNext and resets; does not return
            }
        }
        return;
    }

    // Nothing matched.
    play(ag(hda::AGENT_UNKNOWN), speaker, pending);
}

/// Whether a command asks to change a setting rather than just read it - the verbs that turn
/// a "secure boot" query into a request to open firmware setup.
fn wants_change(cmd: &str) -> bool {
    [
        "change", "chang", "enable", "enabl", "disable", "activ", "desactiv", "turn", "modif",
        "set", "off", "on ",
    ]
    .iter()
    .any(|verb| cmd.contains(verb))
}

/// Read the firmware's own HII configuration through the Config Routing protocol: the real
/// settings the firmware publishes. Returns `(config groups, total settings, up to eight
/// store names)`, or `None` if the firmware exposes no HII routing (e.g. minimal firmware).
/// Read-only; a first step toward letting a blind user change these by name.
fn read_firmware_config() -> Option<(usize, usize, Vec<String>)> {
    let handle = boot::get_handle_for_protocol::<HiiConfigRouting>().ok()?;
    let routing = boot::open_protocol_exclusive::<HiiConfigRouting>(handle).ok()?;
    let export = routing.export().ok()?;
    let mut groups = 0usize;
    let mut settings = 0usize;
    let mut names = Vec::new();
    for entry in MultiConfigurationStringIter::new(&export).flatten() {
        groups += 1;
        settings += entry.elements.len();
        if !entry.name.is_empty() && names.len() < 8 {
            names.push(entry.name);
        }
    }
    Some((groups, settings, names))
}

/// Write the firmware boot-manager timeout (`Timeout`, seconds). Returns whether it stuck.
fn set_boot_timeout(seconds: u16) -> bool {
    runtime::set_variable(
        cstr16!("Timeout"),
        &VariableVendor::GLOBAL_VARIABLE,
        boot_var_attributes(),
        &seconds.to_le_bytes(),
    )
    .is_ok()
}

/// Set the real-time clock. The `uefi` crate wraps `GetTime` but not `SetTime`, so this reaches
/// the raw runtime-services `set_time` through the global system table. It reads the current
/// time first and overrides only the fields the caller supplies (`None` keeps the current one),
/// so "set time 14:30" changes the clock without disturbing the date, and preserves the RTC's
/// own time-zone and daylight fields. Returns whether the firmware accepted the write - the one
/// clock change a loaded application is architected to make.
#[allow(clippy::too_many_arguments)]
fn set_rtc(
    year: Option<u16>,
    month: Option<u8>,
    day: Option<u8>,
    hour: Option<u8>,
    minute: Option<u8>,
    second: Option<u8>,
) -> bool {
    let Ok(current) = runtime::get_time() else {
        return false;
    };
    let time = uefi_raw::time::Time {
        year: year.unwrap_or(current.year()),
        month: month.unwrap_or(current.month()),
        day: day.unwrap_or(current.day()),
        hour: hour.unwrap_or(current.hour()),
        minute: minute.unwrap_or(current.minute()),
        second: second.unwrap_or(0),
        pad1: 0,
        nanosecond: 0,
        time_zone: current
            .time_zone()
            .unwrap_or(uefi_raw::time::Time::UNSPECIFIED_TIMEZONE),
        // The RTC's own time-zone and daylight fields are preserved unchanged.
        daylight: current.daylight(),
        pad2: 0,
    };

    let Some(system_table) = uefi::table::system_table_raw() else {
        return false;
    };
    // SAFETY: `system_table` is the firmware's live system table; its `runtime_services`
    // pointer is valid before ExitBootServices, and `set_time` takes a pointer to a `Time` we
    // own for the duration of the call.
    unsafe {
        let rt = (*system_table.as_ptr()).runtime_services;
        if rt.is_null() {
            return false;
        }
        ((*rt).set_time)(&time) == uefi_raw::Status::SUCCESS
    }
}

/// One enumerated load option list (`Driver####` or `SysPrep####`): the firmware's optional
/// drivers to load, and its system-preparation applications - lists a real BIOS exposes and a
/// silent one hides. Both are stored exactly like `Boot####`, so they decode the same way.
fn enumerate_load_options(order: &CStr16, prefix: char) -> Vec<BootOption> {
    let mut options = Vec::new();
    let Some(order_bytes) = read_global(order) else {
        return options;
    };
    for pair in order_bytes.as_chunks::<2>().0 {
        let id = u16::from_le_bytes(*pair);
        // The variable name is <prefix><four hex digits>, e.g. Driver0001 / SysPrep0002.
        let name = format!("{prefix}{id:04X}");
        let Ok(name16) = CString16::try_from(name.as_str()) else {
            continue;
        };
        let Some(raw) = read_global(&name16) else {
            continue;
        };
        if let Some(label) = decode_boot_option(&raw) {
            options.push(BootOption { id, label });
        }
    }
    options
}

/// Read the whole variable that backs a firmware setting's varstore, or `None`.
fn read_varstore(store: &str, guid: [u8; 16]) -> Option<Vec<u8>> {
    let name = CString16::try_from(store).ok()?;
    let vendor = VariableVendor(Guid::from_bytes(guid));
    runtime::get_variable_boxed(&name, &vendor)
        .ok()
        .map(|(data, _)| data.into_vec())
}

/// Current values of every firmware setting, keyed by `(varstore guid, offset)`, read from
/// the HII Config Routing export. This works even for driver-internal varstores that are not
/// plain NVRAM variables (so values can be spoken where `GetVariable` alone returns nothing).
fn firmware_config_values() -> BTreeMap<([u8; 16], u64), u64> {
    let mut map = BTreeMap::new();
    let Ok(handle) = boot::get_handle_for_protocol::<HiiConfigRouting>() else {
        return map;
    };
    let Ok(routing) = boot::open_protocol_exclusive::<HiiConfigRouting>(handle) else {
        return map;
    };
    let Ok(export) = routing.export() else {
        return map;
    };
    for cfg in MultiConfigurationStringIter::new(&export).flatten() {
        let guid = cfg.guid.to_bytes();
        for element in &cfg.elements {
            let mut value = [0u8; 8];
            let width = element.value.len().min(8);
            value[..width].copy_from_slice(&element.value[..width]);
            map.insert((guid, element.offset), u64::from_le_bytes(value));
        }
    }
    map
}

/// The current value of one firmware setting, read from its varstore at the parsed offset.
fn get_setting_value(setting: &crate::hii_ifr::Setting) -> Option<u64> {
    let buffer = read_varstore(&setting.store, setting.guid)?;
    let (offset, width) = (setting.offset as usize, setting.width as usize);
    if width == 0 || width > 8 || offset + width > buffer.len() {
        return None;
    }
    let mut value = [0u8; 8];
    value[..width].copy_from_slice(&buffer[offset..offset + width]);
    Some(u64::from_le_bytes(value))
}

/// The current value of a firmware setting - a real NVRAM variable if there is one, else the HII
/// config export (which reaches driver-internal varstores too).
fn setting_current_value(
    setting: &crate::hii_ifr::Setting,
    config: &BTreeMap<([u8; 16], u64), u64>,
) -> Option<u64> {
    get_setting_value(setting)
        .or_else(|| config.get(&(setting.guid, setting.offset as u64)).copied())
}

/// Speak-and-show a firmware setting's value by meaning: a one-of's choice label, a checkbox's
/// enabled/disabled, or the raw number.
fn setting_value_text(setting: &crate::hii_ifr::Setting, value: Option<u64>, lang: Lang) -> String {
    match value {
        Some(v) => setting.label_for(v).map(String::from).unwrap_or_else(|| {
            if setting.width == 1 && setting.options.is_empty() {
                String::from(if v != 0 {
                    tx(lang, "activé", "enabled")
                } else {
                    tx(lang, "désactivé", "disabled")
                })
            } else {
                format!("{v}")
            }
        }),
        None => String::from(tx(lang, "inconnu", "unknown")),
    }
}

/// The next value to cycle a setting to: for a one-of, the next choice (wrapping); for a
/// checkbox, the toggle; for a plain numeric, one more (wrapping within its byte width).
fn next_setting_value(setting: &crate::hii_ifr::Setting, current: Option<u64>) -> u64 {
    let cur = current.unwrap_or(0);
    if !setting.options.is_empty() {
        let position = setting.options.iter().position(|(v, _)| *v == cur);
        let next = position
            .map(|p| (p + 1) % setting.options.len())
            .unwrap_or(0);
        setting.options[next].0
    } else if setting.width == 1 {
        u64::from(cur == 0)
    } else {
        let max = if setting.width >= 8 {
            u64::MAX
        } else {
            (1u64 << (setting.width as u32 * 8)) - 1
        };
        cur.checked_add(1).filter(|v| *v <= max).unwrap_or(0)
    }
}

/// Change one firmware setting, trying both methods so it works on any firmware. First the
/// `setup_var` method (read the varstore's NVRAM variable, overwrite the bytes at the offset,
/// write it back) which real BIOSes back with a plain variable; if that varstore is not a
/// reachable variable (driver-internal, as under QEMU/OVMF), fall back to HII RouteConfig,
/// which reaches any varstore through its driver. The caller confirms first, since a wrong
/// value can soft-brick the firmware configuration.
fn set_setting_value(setting: &crate::hii_ifr::Setting, value: u64) -> bool {
    set_setting_via_variable(setting, value) || route_config_write(setting, value)
}

/// Write a setting through its NVRAM varstore variable (the `setup_var` method). Returns false
/// when the varstore is not a reachable EFI variable.
fn set_setting_via_variable(setting: &crate::hii_ifr::Setting, value: u64) -> bool {
    let Ok(name) = CString16::try_from(setting.store.as_str()) else {
        return false;
    };
    let vendor = VariableVendor(Guid::from_bytes(setting.guid));
    let Ok((data, attributes)) = runtime::get_variable_boxed(&name, &vendor) else {
        return false;
    };
    let mut buffer = data.into_vec();
    let (offset, width) = (setting.offset as usize, setting.width as usize);
    if width == 0 || width > 8 || offset + width > buffer.len() {
        return false;
    }
    let bytes = value.to_le_bytes();
    buffer[offset..offset + width].copy_from_slice(&bytes[..width]);
    runtime::set_variable(&name, &vendor, attributes, &buffer).is_ok()
}

/// Write a setting through HII RouteConfig - the driver-agnostic path that also reaches
/// varstores which are not plain NVRAM variables. Builds `<ConfigHdr>&OFFSET&WIDTH&VALUE` by
/// reusing the ConfigHdr the firmware itself exports for that varstore, then routes it. Returns
/// whether the firmware accepted the change.
fn route_config_write(setting: &crate::hii_ifr::Setting, value: u64) -> bool {
    use uefi_raw::protocol::hii::config::HiiConfigRoutingProtocol;

    let width = setting.width as usize;
    if width == 0 || width > 8 {
        return false;
    }
    let Ok(handle) = boot::get_handle_for_protocol::<HiiConfigRouting>() else {
        return false;
    };
    let Ok(routing) = boot::open_protocol_exclusive::<HiiConfigRouting>(handle) else {
        return false;
    };
    let Ok(export) = routing.export() else {
        return false;
    };

    // Locate this varstore's ConfigResp by its GUID, then take its ConfigHdr (everything up to
    // the first block element) verbatim, so the routing header matches the firmware exactly.
    let guid_hex: String = setting
        .guid
        .iter()
        .map(|b| alloc::format!("{b:02x}"))
        .collect();
    let lower = export.to_ascii_lowercase();
    let key = alloc::format!("guid={guid_hex}");
    let Some(start) = lower.find(&key) else {
        return false;
    };
    let resp_end = lower[start + 1..]
        .find("guid=")
        .map(|i| start + 1 + i)
        .unwrap_or(export.len());
    let resp = &export[start..resp_end];
    let Some(hdr_len) = resp.to_ascii_lowercase().find("&offset=") else {
        return false;
    };
    let config_hdr = &resp[..hdr_len];

    // VALUE is the value's bytes most-significant first (big-endian hex of the little-endian
    // field), which is how the firmware's own export encodes it.
    let mut value_hex = String::new();
    for i in (0..width).rev() {
        let byte = ((value >> (i * 8)) & 0xff) as u8;
        value_hex.push_str(&alloc::format!("{byte:02X}"));
    }
    let request = alloc::format!(
        "{config_hdr}&OFFSET={:04X}&WIDTH={:04X}&VALUE={value_hex}",
        setting.offset,
        width
    );
    let Ok(request16) = CString16::try_from(request.as_str()) else {
        return false;
    };

    // SAFETY: `routing` is an open HiiConfigRouting whose wrapper is repr(transparent) over the
    // raw protocol, so the cast yields a valid protocol pointer; `request16` is a live
    // NUL-terminated UCS-2 string for the duration of the call, and `progress` is a scratch
    // out-pointer the firmware fills.
    let raw = (&*routing) as *const HiiConfigRouting as *const HiiConfigRoutingProtocol;
    let mut progress: *const uefi_raw::Char16 = core::ptr::null();
    let status = unsafe {
        ((*raw).route_config)(
            raw,
            request16.as_ptr() as *const uefi_raw::Char16,
            &mut progress,
        )
    };
    if status != uefi_raw::Status::SUCCESS {
        log::error!("AW_UEFI_ROUTECONFIG_FAIL status={status:?}");
    }
    status == uefi_raw::Status::SUCCESS
}

/// The descriptive words of a command, with the given verbs and small filler words removed -
/// so "enable intel vt-d" searches firmware settings for "intel" and "vt-d", not "enable".
fn descriptive_terms<'a>(cmd: &'a str, stop: &[&str]) -> Vec<&'a str> {
    cmd.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 2)
        .filter(|word| !stop.contains(word))
        .collect()
}

/// Resolve which boot entry a command refers to: first by a position number ("boot 2"),
/// then by a descriptive word the user typed that appears in a device's name ("boot usb",
/// "boot windows"). The command verbs are ignored so they cannot match a label word like
/// "Boot" in "Windows Boot Manager".
fn find_boot_target<'a>(cmd: &str, options: &'a [BootOption]) -> Option<&'a BootOption> {
    if let Some(position) = first_number(cmd)
        && (1..=options.len()).contains(&position)
    {
        return options.get(position - 1);
    }
    let terms: Vec<&str> = cmd
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 2)
        .filter(|word| {
            !matches!(
                *word,
                "boot"
                    | "default"
                    | "defaut"
                    | "demarrer"
                    | "demarre"
                    | "demarrage"
                    | "par"
                    | "def"
                    | "sur"
                    | "move"
                    | "up"
                    | "down"
                    | "monter"
                    | "descendre"
                    | "haut"
                    | "bas"
                    | "list"
                    | "liste"
                    | "now"
                    | "maintenant"
            )
        })
        .collect();
    options.iter().find(|opt| {
        let label = opt.label.to_ascii_lowercase();
        terms.iter().any(|term| label.contains(term))
    })
}

/// Parse a date (`YYYY-MM-DD`) and/or a time (`HH:MM` or `HH:MM:SS`) out of a command, each
/// validated against its calendar range. Either may be absent, so "set time 14:30" sets only
/// the time and "set date 2026-09-21" only the date.
#[allow(clippy::type_complexity)]
fn parse_clock(cmd: &str) -> (Option<(u16, u8, u8)>, Option<(u8, u8, Option<u8>)>) {
    let mut date = None;
    let mut time = None;
    for token in cmd.split_whitespace() {
        if date.is_none() && token.contains('-') {
            let parts: Vec<&str> = token.split('-').collect();
            if let [y, mo, d] = parts[..]
                && let (Ok(y), Ok(mo), Ok(d)) =
                    (y.parse::<u16>(), mo.parse::<u8>(), d.parse::<u8>())
                && (1900..=9999).contains(&y)
                && (1..=12).contains(&mo)
                && (1..=31).contains(&d)
            {
                date = Some((y, mo, d));
            }
        }
        if time.is_none() && token.contains(':') {
            let parts: Vec<&str> = token.split(':').collect();
            if parts.len() >= 2
                && let (Ok(h), Ok(mi)) = (parts[0].parse::<u8>(), parts[1].parse::<u8>())
                && h < 24
                && mi < 60
            {
                let s = parts
                    .get(2)
                    .and_then(|p| p.parse::<u8>().ok())
                    .filter(|s| *s < 60);
                time = Some((h, mi, s));
            }
        }
    }
    (date, time)
}

/// The first run of decimal digits in `s`, parsed as a 1-based position, or `None`.
fn first_number(s: &str) -> Option<usize> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// One level of the descent: which screen we are in and which item is focused.
#[derive(Clone, Copy)]
struct Frame {
    screen: usize,
    item: usize,
}

/// Present the accessible setup tree and act on the user's choices. Voiced on the
/// console and through markers and HDA clips, with audible cues; operated on the
/// firmware's own keyboard. With no key pressed, a countdown boots normally, so an
/// unattended boot - and the timed proof harness - always proceeds.
pub fn run(width: usize, height: usize, speaker: &mut Option<audio::Speaker>) {
    // French is the default language; the Language item on the Main tab switches it.
    let mut lang = Lang::Fr;
    set_current_lang(lang);

    // Prove, at boot, that the firmware's own settings were parsed out of its HII database -
    // the evidence the `list settings` / `enable <name>` commands rest on. Zero on firmware
    // (or QEMU/OVMF) that publishes no IFR; non-zero on a real BIOS.
    {
        // Evidence at boot: how many named settings the firmware's IFR yields, and how many are
        // reachable as real NVRAM variables (`readable`). On a real BIOS whose Setup varstore is
        // NVRAM-backed, `readable` is non-zero and those settings can be changed by name; on
        // firmware whose varstores are driver-internal buffers (e.g. QEMU/OVMF) it is zero and
        // changing them would need the HII RouteConfig path instead.
        let all = crate::hii_ifr::enumerate_settings();
        let readable = all
            .iter()
            .filter(|s| get_setting_value(s).is_some())
            .count();
        let config = firmware_config_values();
        let via_config = all
            .iter()
            .filter(|s| config.contains_key(&(s.guid, s.offset as u64)))
            .count();
        aw_mark!(
            "AW_UEFI_HII_SETTINGS count={} readable={} via_config={} db_bytes={}",
            all.len(),
            readable,
            via_config,
            crate::hii_ifr::database_len()
        );
    }

    // Evidence at boot for the real security state a silent firmware never speaks: whether a
    // TPM 2.0 interface answers, and how many entries each Secure Boot key store holds. Proven
    // headless, so the boot proofs assert it without a keypress.
    {
        let tpm_present = boot::get_handle_for_protocol::<Tcg2>()
            .ok()
            .and_then(|handle| boot::open_protocol_exclusive::<Tcg2>(handle).ok())
            .and_then(|mut tcg2| tcg2.get_capability().ok())
            .map(|cap| cap.tpm_present())
            .unwrap_or(false);
        let count = |name: &CStr16, vendor: &VariableVendor| {
            read_var(name, vendor)
                .map(|raw| signature_count(&raw))
                .unwrap_or(0)
        };
        aw_mark!(
            "AW_UEFI_SECURITY tpm2={} pk={} kek={} db={} dbx={}",
            tpm_present,
            read_var(cstr16!("PK"), &VariableVendor::GLOBAL_VARIABLE).is_some(),
            count(cstr16!("KEK"), &VariableVendor::GLOBAL_VARIABLE),
            count(cstr16!("db"), &IMAGE_SECURITY_DATABASE),
            count(cstr16!("dbx"), &IMAGE_SECURITY_DATABASE),
        );
    }

    // Evidence at boot for the network, deny by default: which interfaces the firmware exposes
    // and whether their link is up - read-only, nothing is sent or received.
    let _ = crate::net::report();

    // Evidence at boot for the firmware's optional driver and system-preparation load lists -
    // read-only, so proven headless. Both are decoded like boot entries; a real BIOS may
    // populate them, OVMF usually leaves them empty.
    aw_mark!(
        "AW_UEFI_LOADOPTS drivers={} sysprep={}",
        enumerate_load_options(cstr16!("DriverOrder"), 'D').len(),
        enumerate_load_options(cstr16!("SysPrepOrder"), 'S').len(),
    );

    // Enumerate USB devices through the firmware's own host stack and report what is present -
    // proving USB accessibility hardware is reachable pre-OS. If a HID braille display is found,
    // mirror the setup's opening line to it, so a deaf-blind user feels that the firmware is up.
    let braille = crate::usb::find_braille();
    if crate::usb::report_devices()
        && let Some(display) = &braille
    {
        display.show("omni-os firmware setup");
    }

    // Close the last audio gap: if a USB Audio Class device is present, drive it directly through
    // a from-scratch XHCI isochronous driver (the firmware's own UsbIo cannot carry isochronous
    // transfers). Gated on a USB Audio device being present, so a keyboard-only controller is
    // never touched. Best effort, fully instrumented, and it always returns to continue the boot.
    crate::usb_audio::self_test();

    // Prove the runtime formant synthesizer runs on this firmware: synthesize a fixed phrase
    // (in soft-float, before any OS) and report the PCM it produced. A non-zero byte count is
    // headless evidence that arbitrary dynamic text - device names, values - can now be spoken
    // as words, not just spelled. The clip is not played here, so it adds no boot latency.
    {
        let pcm = crate::synth::say("boot device one two eight zero", false);
        aw_mark!(
            "AW_UEFI_SYNTH_SELFTEST bytes={} rate={} pitch={}",
            pcm.len(),
            crate::synth::rate(),
            crate::synth::pitch(),
        );
    }

    let mut tree = build_tree(lang, width, height);

    // Open on the Boot tab with "Boot normally" focused: the safe default a user or the
    // countdown takes, and the first thing announced. The tab is located by index, so it
    // is the same whatever the language.
    let mut tab_index = tree.boot_tab;
    // `stack` always holds at least the current top-tab frame; deeper frames are
    // submenus. Its length minus one is the current depth.
    let mut stack: Vec<Frame> = alloc::vec![Frame {
        screen: tree.tabs[tab_index],
        item: 0
    }];

    // A keystroke captured while a clip was playing (barge-in). It is processed before a
    // fresh key is read, so acting on the setup always interrupts what it was saying.
    let mut pending: Option<Key> = None;

    aw_mark!("AW_UEFI_SR_READY");
    {
        let top = *stack.last().unwrap();
        render(&tree, tab_index, top.screen, top.item, stack.len() - 1);
    }
    sound::cue(CUE_READY_HZ, Duration::from_millis(90));
    // Name the setup, then teach the interaction model up front, so a blind user knows how
    // to drive it before anything else is said - and can press a key to skip straight in.
    play(
        clip(lang, hda::CLIP_FR_INTRO, hda::CLIP_SETUP_INTRO),
        speaker,
        &mut pending,
    );
    play(
        clip(lang, hda::CLIP_FR_INSTRUCTIONS, hda::CLIP_INSTRUCTIONS),
        speaker,
        &mut pending,
    );
    announce_screen(
        &tree,
        tab_index,
        tree.tabs[tab_index],
        0,
        speaker,
        &mut pending,
    );
    announce_item(
        &tree.screens[tree.tabs[tab_index]],
        0,
        speaker,
        &mut pending,
    );

    let mut interacted = false;
    let mut waited = Duration::ZERO;

    // The independent review cursor: a line index within the current screen, and the focus it
    // was last based on. When focus moves, the next review step re-bases to the new focus, so
    // the review cursor follows the selection until the user drives it away from there.
    let mut review = 0usize;
    let mut review_focus = (usize::MAX, usize::MAX);

    loop {
        let next_key = pending.take().or_else(read_key_raw);
        if let Some(key) = next_key {
            interacted = true;
            let depth = stack.len() - 1;
            match classify(key) {
                Nav::NextTab | Nav::PreviousTab if depth == 0 => {
                    tab_index = match classify(key) {
                        Nav::NextTab => (tab_index + 1) % tree.tabs.len(),
                        _ => (tab_index + tree.tabs.len() - 1) % tree.tabs.len(),
                    };
                    stack = alloc::vec![Frame {
                        screen: tree.tabs[tab_index],
                        item: 0
                    }];
                    sound::cue(CUE_TAB_HZ, Duration::from_millis(45));
                    render(&tree, tab_index, tree.tabs[tab_index], 0, 0);
                    announce_screen(
                        &tree,
                        tab_index,
                        tree.tabs[tab_index],
                        0,
                        speaker,
                        &mut pending,
                    );
                    announce_item(
                        &tree.screens[tree.tabs[tab_index]],
                        0,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::NextTab | Nav::PreviousTab => {} // ignored inside a submenu
                Nav::NextItem | Nav::PreviousItem => {
                    let frame = stack.last_mut().unwrap();
                    let count = tree.screens[frame.screen].items.len().max(1);
                    frame.item = match classify(key) {
                        Nav::NextItem => (frame.item + 1) % count,
                        _ => (frame.item + count - 1) % count,
                    };
                    let (screen, item) = (frame.screen, frame.item);
                    sound::cue(CUE_MOVE_HZ, Duration::from_millis(35));
                    render(&tree, tab_index, screen, item, stack.len() - 1);
                    announce_item(&tree.screens[screen], item, speaker, &mut pending);
                }
                Nav::First | Nav::Last => {
                    let frame = stack.last_mut().unwrap();
                    let count = tree.screens[frame.screen].items.len().max(1);
                    frame.item = if matches!(classify(key), Nav::First) {
                        0
                    } else {
                        count - 1
                    };
                    let (screen, item) = (frame.screen, frame.item);
                    sound::cue(CUE_MOVE_HZ, Duration::from_millis(35));
                    render(&tree, tab_index, screen, item, stack.len() - 1);
                    announce_item(&tree.screens[screen], item, speaker, &mut pending);
                }
                Nav::Select => {
                    let frame = *stack.last().unwrap();
                    let action = tree.screens[frame.screen].items[frame.item].action;
                    match action {
                        Action::Info => {}
                        Action::SubMenu(child) => {
                            stack.push(Frame {
                                screen: child,
                                item: 0,
                            });
                            sound::cue(CUE_TAB_HZ, Duration::from_millis(45));
                            render(&tree, tab_index, child, 0, stack.len() - 1);
                            announce_screen(
                                &tree,
                                tab_index,
                                child,
                                stack.len() - 1,
                                speaker,
                                &mut pending,
                            );
                            announce_item(&tree.screens[child], 0, speaker, &mut pending);
                        }
                        Action::Back => {
                            back_out(&tree, tab_index, &mut stack, speaker, &mut pending);
                        }
                        Action::BootNormally => {
                            sound::cue(CUE_CONTINUE_HZ, Duration::from_millis(150));
                            aw_mark!("AW_UEFI_MENU_SELECT name=\"boot_normally\"");
                            aw_mark!("AW_UEFI_SR_CONTINUE reason=selected");
                            return;
                        }
                        // Booting a chosen device restarts the machine, so confirm first.
                        Action::BootNow(id) => {
                            if confirm(lang, speaker, &mut pending) {
                                boot_now(id);
                            } else {
                                announce_item(
                                    &tree.screens[frame.screen],
                                    frame.item,
                                    speaker,
                                    &mut pending,
                                );
                            }
                        }
                        // Persistent but reversible: apply at once, and say "Done" so the
                        // user knows it worked - not just a tone.
                        Action::MakeDefault(id) => {
                            if make_default(id) {
                                sound::cue(CUE_APPLIED_HZ, Duration::from_millis(120));
                                play(
                                    clip(lang, hda::CLIP_FR_CONFIRM_DONE, hda::CLIP_CONFIRM_DONE),
                                    speaker,
                                    &mut pending,
                                );
                            }
                        }
                        Action::MoveUp(id) => {
                            if move_in_boot_order(id, true) {
                                sound::cue(CUE_APPLIED_HZ, Duration::from_millis(120));
                                play(
                                    clip(lang, hda::CLIP_FR_CONFIRM_DONE, hda::CLIP_CONFIRM_DONE),
                                    speaker,
                                    &mut pending,
                                );
                            }
                        }
                        Action::MoveDown(id) => {
                            if move_in_boot_order(id, false) {
                                sound::cue(CUE_APPLIED_HZ, Duration::from_millis(120));
                                play(
                                    clip(lang, hda::CLIP_FR_CONFIRM_DONE, hda::CLIP_CONFIRM_DONE),
                                    speaker,
                                    &mut pending,
                                );
                            }
                        }
                        // Each of these reboots or powers off, so confirm first.
                        Action::EnterSetup => {
                            if confirm(lang, speaker, &mut pending) {
                                enter_firmware_setup();
                            } else {
                                announce_item(
                                    &tree.screens[frame.screen],
                                    frame.item,
                                    speaker,
                                    &mut pending,
                                );
                            }
                        }
                        Action::Reset => {
                            if confirm(lang, speaker, &mut pending) {
                                aw_mark!("AW_UEFI_MENU_SELECT name=\"reset\"");
                                runtime::reset(runtime::ResetType::COLD, Status::SUCCESS, None);
                            } else {
                                announce_item(
                                    &tree.screens[frame.screen],
                                    frame.item,
                                    speaker,
                                    &mut pending,
                                );
                            }
                        }
                        Action::Shutdown => {
                            if confirm(lang, speaker, &mut pending) {
                                aw_mark!("AW_UEFI_MENU_SELECT name=\"shutdown\"");
                                runtime::reset(runtime::ResetType::SHUTDOWN, Status::SUCCESS, None);
                            } else {
                                announce_item(
                                    &tree.screens[frame.screen],
                                    frame.item,
                                    speaker,
                                    &mut pending,
                                );
                            }
                        }
                        // Switch language and rebuild the whole tree in the new one, then
                        // land back on the same tab so the change is heard immediately.
                        Action::ToggleLang => {
                            lang = lang.toggled();
                            set_current_lang(lang);
                            aw_mark!(
                                "AW_UEFI_SETUP_LANG lang={}",
                                match lang {
                                    Lang::Fr => "fr",
                                    Lang::En => "en",
                                }
                            );
                            tree = build_tree(lang, width, height);
                            // Land on the Main tab (index 0), where the Language item is,
                            // so its first announcement is the new language.
                            tab_index = 0;
                            stack = alloc::vec![Frame {
                                screen: tree.tabs[tab_index],
                                item: 0
                            }];
                            sound::cue(CUE_TAB_HZ, Duration::from_millis(45));
                            render(&tree, tab_index, tree.tabs[tab_index], 0, 0);
                            announce_screen(
                                &tree,
                                tab_index,
                                tree.tabs[tab_index],
                                0,
                                speaker,
                                &mut pending,
                            );
                            announce_item(
                                &tree.screens[tree.tabs[tab_index]],
                                0,
                                speaker,
                                &mut pending,
                            );
                        }
                        // Change a firmware HII setting in place: cycle it to its next value,
                        // confirmed (a wrong write can soft-brick the firmware config), then
                        // update the item's text and re-announce it in the new value.
                        Action::ChangeSetting(idx) => {
                            let done = if let Some(setting) = tree.settings.get(idx) {
                                let config = firmware_config_values();
                                let current = setting_current_value(setting, &config);
                                let next = next_setting_value(setting, current);
                                uefi::println!(
                                    "  {} -> {}",
                                    setting.name,
                                    setting_value_text(setting, Some(next), lang)
                                );
                                aw_mark!(
                                    "AW_UEFI_SETUP_SETTING name=\"{}\" next={}",
                                    setting.name,
                                    next
                                );
                                if confirm(lang, speaker, &mut pending) {
                                    let ok = set_setting_value(setting, next);
                                    let value = if ok { Some(next) } else { current };
                                    Some((
                                        setting.name.clone(),
                                        setting_value_text(setting, value, lang),
                                        ok,
                                    ))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if let Some((name, vtext, ok)) = done {
                                if ok {
                                    sound::cue(CUE_APPLIED_HZ, Duration::from_millis(120));
                                }
                                let frame = *stack.last().unwrap();
                                if let Some(item) =
                                    tree.screens[frame.screen].items.get_mut(frame.item)
                                {
                                    item.text = format!("{name}, {vtext}");
                                }
                                play(
                                    clip(lang, hda::CLIP_FR_CONFIRM_DONE, hda::CLIP_CONFIRM_DONE),
                                    speaker,
                                    &mut pending,
                                );
                            }
                            let frame = *stack.last().unwrap();
                            announce_item(
                                &tree.screens[frame.screen],
                                frame.item,
                                speaker,
                                &mut pending,
                            );
                        }
                    }
                }
                Nav::Back => {
                    if stack.len() > 1 {
                        back_out(&tree, tab_index, &mut stack, speaker, &mut pending);
                    } else {
                        sound::cue(CUE_CONTINUE_HZ, Duration::from_millis(150));
                        aw_mark!("AW_UEFI_SR_CONTINUE reason=escape");
                        return;
                    }
                }
                Nav::Repeat => {
                    let frame = *stack.last().unwrap();
                    announce_item(
                        &tree.screens[frame.screen],
                        frame.item,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::Help => {
                    let frame = *stack.last().unwrap();
                    announce_help(&tree.screens[frame.screen], frame.item);
                }
                Nav::Where => {
                    let frame = *stack.last().unwrap();
                    let depth = stack.len() - 1;
                    announce_screen(&tree, tab_index, frame.screen, depth, speaker, &mut pending);
                    announce_item(
                        &tree.screens[frame.screen],
                        frame.item,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::Spell => {
                    let frame = *stack.last().unwrap();
                    spell_current(
                        &tree.screens[frame.screen].items[frame.item].text,
                        lang,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::SayAll => {
                    let frame = *stack.last().unwrap();
                    let count = tree.screens[frame.screen].items.len();
                    for index in 0..count {
                        // Barge-in stops the read-through at once.
                        if pending.is_some() {
                            break;
                        }
                        announce_item(&tree.screens[frame.screen], index, speaker, &mut pending);
                    }
                }
                Nav::Command => {
                    run_agent(lang, speaker, &mut pending);
                    // Re-announce where we are, so the user is oriented after the agent.
                    let frame = *stack.last().unwrap();
                    announce_item(
                        &tree.screens[frame.screen],
                        frame.item,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::VolumeUp => {
                    let level = audio::volume_up();
                    aw_mark!("AW_UEFI_VOLUME level={level} muted=false");
                    // A PC-speaker cue whose pitch rises with the level gives instant,
                    // always-audible feedback; re-announcing the item lets the user hear the
                    // new speech volume on the real channel.
                    sound::cue(300 + level * 2, Duration::from_millis(90));
                    let frame = *stack.last().unwrap();
                    announce_item(
                        &tree.screens[frame.screen],
                        frame.item,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::VolumeDown => {
                    let level = audio::volume_down();
                    aw_mark!("AW_UEFI_VOLUME level={level} muted=false");
                    sound::cue(300 + level * 2, Duration::from_millis(90));
                    let frame = *stack.last().unwrap();
                    announce_item(
                        &tree.screens[frame.screen],
                        frame.item,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::Mute => {
                    let muted = audio::toggle_mute();
                    aw_mark!("AW_UEFI_VOLUME muted={muted}");
                    // The cue is on the PC speaker, so it is heard even while speech is muted.
                    sound::cue(if muted { 240 } else { 660 }, Duration::from_millis(120));
                    if !muted {
                        let frame = *stack.last().unwrap();
                        announce_item(
                            &tree.screens[frame.screen],
                            frame.item,
                            speaker,
                            &mut pending,
                        );
                    }
                }
                Nav::Phonetic => {
                    let on = hda::toggle_phonetic();
                    aw_mark!("AW_UEFI_PHONETIC on={on}");
                    sound::cue(if on { 660 } else { 440 }, Duration::from_millis(90));
                    // Spell the letter A at once, so the user hears the new mode: the NATO
                    // word "Alpha" when on, the plain letter when off.
                    if let Some(clip) = hda::spell_clip('a', matches!(lang, Lang::Fr)) {
                        play(clip, speaker, &mut pending);
                    }
                }
                // Speech rate and pitch drive the runtime synthesizer. After each change,
                // speak the new value through the synthesizer itself, so the user hears the
                // effect on the real voice at once.
                Nav::RateUp | Nav::RateDown => {
                    let percent = if matches!(classify(key), Nav::RateUp) {
                        crate::synth::rate_up()
                    } else {
                        crate::synth::rate_down()
                    };
                    aw_mark!("AW_UEFI_SYNTH_RATE percent={percent}");
                    sound::cue(CUE_MOVE_HZ, Duration::from_millis(40));
                    speak_dynamic(&format!("{percent}"), lang, speaker, &mut pending);
                }
                Nav::PitchUp | Nav::PitchDown => {
                    let hz = if matches!(classify(key), Nav::PitchUp) {
                        crate::synth::pitch_up()
                    } else {
                        crate::synth::pitch_down()
                    };
                    aw_mark!("AW_UEFI_SYNTH_PITCH hz={hz}");
                    sound::cue(CUE_MOVE_HZ, Duration::from_millis(40));
                    speak_dynamic(&format!("{hz}"), lang, speaker, &mut pending);
                }
                Nav::Verbosity => {
                    let level = cycle_verbosity(lang);
                    aw_mark!("AW_UEFI_VERBOSITY level=\"{level}\"");
                    sound::cue(CUE_MOVE_HZ, Duration::from_millis(40));
                    speak_dynamic(level, lang, speaker, &mut pending);
                }
                Nav::Punctuation => {
                    let level = cycle_punctuation(lang);
                    aw_mark!("AW_UEFI_PUNCTUATION level=\"{level}\"");
                    sound::cue(CUE_MOVE_HZ, Duration::from_millis(40));
                    speak_dynamic(level, lang, speaker, &mut pending);
                }
                Nav::ReadByWord => {
                    let frame = *stack.last().unwrap();
                    let text = tree.screens[frame.screen].items[frame.item].text.clone();
                    aw_mark!("AW_UEFI_SETUP_READWORD \"{text}\"");
                    // Read the focused line one word at a time, each synthesized, so a user can
                    // step a long value or device name word by word. Interruptible (barge-in).
                    for word in text.split_whitespace() {
                        if pending.is_some() {
                            break;
                        }
                        speak_dynamic(word, lang, speaker, &mut pending);
                    }
                }
                Nav::ReviewNext | Nav::ReviewPrev => {
                    let frame = *stack.last().unwrap();
                    let count = tree.screens[frame.screen].items.len();
                    // Re-base to the current focus if the selection has moved since last review.
                    if review_focus != (frame.screen, frame.item) {
                        review = frame.item;
                        review_focus = (frame.screen, frame.item);
                    }
                    if matches!(classify(key), Nav::ReviewNext) {
                        if review + 1 < count {
                            review += 1;
                        }
                    } else {
                        review = review.saturating_sub(1);
                    }
                    let text = tree.screens[frame.screen].items[review].text.clone();
                    aw_mark!("AW_UEFI_REVIEW line={} \"{text}\"", review + 1);
                    sound::cue(CUE_MOVE_HZ, Duration::from_millis(30));
                    speak_dynamic(
                        &format!("{}, {text}", review + 1),
                        lang,
                        speaker,
                        &mut pending,
                    );
                }
                Nav::FocusToReview => {
                    let frame = stack.last_mut().unwrap();
                    let count = tree.screens[frame.screen].items.len().max(1);
                    frame.item = review.min(count - 1);
                    let (screen, item) = (frame.screen, frame.item);
                    review_focus = (screen, item);
                    aw_mark!("AW_UEFI_REVIEW_FOCUS line={}", item + 1);
                    sound::cue(CUE_TAB_HZ, Duration::from_millis(40));
                    render(&tree, tab_index, screen, item, stack.len() - 1);
                    announce_item(&tree.screens[screen], item, speaker, &mut pending);
                }
                Nav::Ignore => {}
            }
            continue;
        }

        // No key waiting. An unattended boot counts down and then boots normally; once
        // someone has interacted, the countdown is abandoned and we wait.
        if !interacted {
            if waited >= REVIEW_WINDOW {
                sound::cue(CUE_CONTINUE_HZ, Duration::from_millis(150));
                aw_mark!("AW_UEFI_SR_CONTINUE reason=timeout");
                return;
            }
            waited += POLL_INTERVAL;
        }
        boot::stall(POLL_INTERVAL);
    }
}

/// Pop one submenu level and re-announce the screen and focused item we return to.
fn back_out(
    tree: &Tree,
    tab_index: usize,
    stack: &mut Vec<Frame>,
    speaker: &mut Option<audio::Speaker>,
    pending: &mut Option<Key>,
) {
    stack.pop();
    let frame = *stack.last().unwrap();
    let depth = stack.len() - 1;
    sound::cue(CUE_BACK_HZ, Duration::from_millis(45));
    render(tree, tab_index, frame.screen, frame.item, depth);
    announce_screen(tree, tab_index, frame.screen, depth, speaker, pending);
    announce_item(&tree.screens[frame.screen], frame.item, speaker, pending);
}
