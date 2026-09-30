//! The graphical administration session: the system screen a person operates after boot.
//!
//! Drawn on the framebuffer, driven by the keyboard only, and spoken with the native voice
//! ([`crate::speech`]) at every landing - nothing needs sight. Panels show what the kernel
//! measured on this machine (health, security, network and VPN, storage, processor and memory)
//! and hold the administrative actions (restart, power off), each confirmed by a second Enter.
//!
//! Keys: Up/Down move, Right or Enter open a panel, Left or Escape go back, Space repeats.
//! The navigation model is pure ([`step`]) so the live session and the boot proof share it.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use aw_bootstate::KERNEL_HEALTH_CHECKS;
use aw_generation::RuntimeHealthCheck;

use crate::facts::{self, Vpn};
use crate::framebuffer;
use crate::ps2_keyboard::{self, Key};
use crate::{debug_write, speech};

const TITLE: &str = "omni-os - Session d'administration";
const HINT: &str =
    "Fleches : naviguer. Entree : ouvrir ou valider. Echap : revenir. Espace : repeter.";
const PANELS: [&str; 6] = [
    "Santé du système",
    "Sécurité",
    "Réseau et VPN",
    "Stockage",
    "Processeur et mémoire",
    "Alimentation",
];
const POWER_PANEL: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Restart,
    PowerOff,
}

/// Where the focus is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct State {
    panel: usize,
    /// `None`: on the panel list; `Some(i)`: on line `i` of the open panel.
    line: Option<usize>,
    /// An action waiting for its confirming Enter.
    armed: Option<Action>,
}

impl State {
    pub const START: Self = Self {
        panel: 0,
        line: None,
        armed: None,
    };
}

/// What a key did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Moved,
    Repeat,
    ConfirmRequired(Action),
    Cancelled,
    Run(Action),
    Nothing,
}

fn yes(on: bool, word_on: &str, word_off: &str) -> String {
    String::from(if on { word_on } else { word_off })
}

/// The lines of a panel, from the facts measured on this machine.
fn panel_lines(panel: usize) -> Vec<String> {
    let mut lines = Vec::new();
    match panel {
        0 => {
            let passed = crate::firmware_runtime::passed();
            for check in KERNEL_HEALTH_CHECKS {
                lines.push(format!(
                    "{} : {}",
                    check_label(check),
                    yes(passed & check.bit() != 0, "réussi", "en échec")
                ));
            }
        }
        1 => {
            // SAFETY: CPL0; reads control registers and EFER only.
            let state = unsafe { crate::security_baseline::runtime_state() };
            lines.push(format!(
                "Pages mémoire écriture ou exécution, jamais les deux : {}",
                yes(
                    facts::KERNEL_MAP_WX.load(core::sync::atomic::Ordering::Acquire),
                    "actif",
                    "inactif"
                )
            ));
            lines.push(format!(
                "Protection non exécutable NX : {}",
                yes(state.efer_nx_enable, "active", "inactive")
            ));
            lines.push(format!(
                "SMEP : {}",
                yes(state.cr4_smep, "actif", "inactif")
            ));
            lines.push(format!(
                "SMAP : {}",
                yes(state.cr4_smap, "actif", "inactif")
            ));
            lines.push(format!(
                "UMIP : {}",
                yes(state.cr4_umip, "actif", "inactif")
            ));
            lines.push(format!(
                "Protection en écriture du noyau : {}",
                yes(state.cr0_write_protect, "active", "inactive")
            ));
            lines.push(format!(
                "Services UEFI : {}",
                crate::firmware_runtime::mode_label()
            ));
            lines.push(String::from(
                "Démarrage : noyau vérifié par empreinte SHA-256 avant exécution",
            ));
            lines.push(String::from("Réseau : fermé par défaut"));
        }
        2 => {
            if facts::NET_PRESENT.load(core::sync::atomic::Ordering::Acquire) {
                let mac = facts::NET_MAC
                    .load(core::sync::atomic::Ordering::Acquire)
                    .to_be_bytes();
                lines.push(format!(
                    "Carte réseau : présente, adresse {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[2], mac[3], mac[4], mac[5], mac[6], mac[7]
                ));
            } else {
                lines.push(String::from("Carte réseau : absente"));
            }
            lines.push(String::from(match facts::vpn() {
                Vpn::NotConfigured => "VPN local : non configuré",
                Vpn::Established => "VPN local : tunnel chiffré établi",
                Vpn::Failed => "VPN local : échec de l'établissement",
            }));
        }
        3 => {
            let mut model = [0_u8; 40];
            let model = facts::NVME_MODEL.get(&mut model);
            lines.push(if model.is_empty() {
                String::from("NVMe : absent")
            } else {
                format!("NVMe : {model}")
            });
            lines.push(yes(
                facts::SATA_DISK.load(core::sync::atomic::Ordering::Acquire),
                "Disque SATA : présent et lisible",
                "Disque SATA : absent",
            ));
        }
        4 => {
            let mut brand = [0_u8; 48];
            let brand = facts::CPU_BRAND.get(&mut brand);
            lines.push(format!(
                "Processeur : {}",
                if brand.is_empty() { "inconnu" } else { brand }
            ));
            let online = (0..256)
                .filter(|cpu| crate::smp::ap_summary(*cpu).is_some())
                .count()
                + 1;
            lines.push(format!("Processeurs actifs : {online}"));
            lines.push(format!(
                "Mémoire : {} mégaoctets",
                facts::MEMORY_MIB.load(core::sync::atomic::Ordering::Acquire)
            ));
        }
        _ => {
            lines.push(String::from("Redémarrer"));
            lines.push(String::from("Éteindre"));
        }
    }
    lines
}

const fn check_label(check: RuntimeHealthCheck) -> &'static str {
    match check {
        RuntimeHealthCheck::Kernel => "Noyau",
        RuntimeHealthCheck::Storage => "Stockage",
        RuntimeHealthCheck::Input => "Clavier",
        RuntimeHealthCheck::Audio => "Audio",
        RuntimeHealthCheck::AccessibilityBroker => "Lecteur d'écran",
        RuntimeHealthCheck::Speech => "Voix",
        RuntimeHealthCheck::AccessibleRecovery => "Récupération accessible",
        RuntimeHealthCheck::Security => "Sécurité",
        RuntimeHealthCheck::Update => "Mise à jour",
    }
}

/// Apply one key. Pure: the live session and the proof share it.
pub fn step(state: State, key: Key) -> (State, Outcome) {
    let panels = PANELS.len();
    match state.line {
        None => match key {
            Key::Up => (
                State {
                    panel: (state.panel + panels - 1) % panels,
                    ..state
                },
                Outcome::Moved,
            ),
            Key::Down | Key::Tab => (
                State {
                    panel: (state.panel + 1) % panels,
                    ..state
                },
                Outcome::Moved,
            ),
            Key::Right | Key::Enter => (
                State {
                    line: Some(0),
                    armed: None,
                    ..state
                },
                Outcome::Moved,
            ),
            Key::Space => (state, Outcome::Repeat),
            _ => (state, Outcome::Nothing),
        },
        Some(line) => {
            let count = panel_lines(state.panel).len().max(1);
            match key {
                Key::Up => (
                    State {
                        line: Some((line + count - 1) % count),
                        armed: None,
                        ..state
                    },
                    Outcome::Moved,
                ),
                Key::Down | Key::Tab => (
                    State {
                        line: Some((line + 1) % count),
                        armed: None,
                        ..state
                    },
                    Outcome::Moved,
                ),
                Key::Left | Key::Escape => {
                    if state.armed.is_some() {
                        (
                            State {
                                armed: None,
                                ..state
                            },
                            Outcome::Cancelled,
                        )
                    } else {
                        (
                            State {
                                line: None,
                                ..state
                            },
                            Outcome::Moved,
                        )
                    }
                }
                Key::Space => (state, Outcome::Repeat),
                Key::Enter if state.panel == POWER_PANEL => {
                    let action = if line == 0 {
                        Action::Restart
                    } else {
                        Action::PowerOff
                    };
                    if state.armed == Some(action) {
                        (
                            State {
                                armed: None,
                                ..state
                            },
                            Outcome::Run(action),
                        )
                    } else {
                        (
                            State {
                                armed: Some(action),
                                ..state
                            },
                            Outcome::ConfirmRequired(action),
                        )
                    }
                }
                _ => (state, Outcome::Nothing),
            }
        }
    }
}

/// What is said for the current focus.
pub fn utterance(state: State) -> String {
    match state.line {
        None => format!(
            "{}, panneau, {} sur {}",
            PANELS[state.panel],
            state.panel + 1,
            PANELS.len()
        ),
        Some(line) => {
            let lines = panel_lines(state.panel);
            let text = lines.get(line).map_or("", String::as_str);
            if state.panel == POWER_PANEL {
                format!("{text}, bouton, {} sur {}", line + 1, lines.len())
            } else {
                format!("{text}, {} sur {}", line + 1, lines.len())
            }
        }
    }
}

/// The screen font covers ASCII only: fold French accents for display (speech keeps them).
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'É' | 'È' => 'E',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_ascii() => c,
            _ => '?',
        })
        .collect()
}

/// Draw the whole session for `state`.
pub fn render(state: State) {
    framebuffer::clear_screen();
    framebuffer::draw_menu_row(0, TITLE, false);
    for (index, panel) in PANELS.iter().enumerate() {
        let focused = state.line.is_none() && index == state.panel;
        let marker = if index == state.panel { "> " } else { "  " };
        framebuffer::draw_menu_row(
            2 + index as u32,
            &fold(&format!("{marker}{panel}")),
            focused,
        );
    }
    let top = 3 + PANELS.len() as u32;
    framebuffer::draw_menu_row(top, &fold(&format!("-- {} --", PANELS[state.panel])), false);
    for (index, line) in panel_lines(state.panel).iter().enumerate() {
        let focused = state.line == Some(index);
        framebuffer::draw_menu_row(top + 1 + index as u32, &fold(line), focused);
    }
    framebuffer::draw_menu_row(top + 12, HINT, false);
}

fn announce(state: State, outcome: Outcome, live: bool) {
    let text = match outcome {
        Outcome::ConfirmRequired(Action::Restart) => String::from(
            "Redémarrer l'ordinateur ? Appuyez de nouveau sur Entrée pour confirmer, Échap pour annuler.",
        ),
        Outcome::ConfirmRequired(Action::PowerOff) => String::from(
            "Éteindre l'ordinateur ? Appuyez de nouveau sur Entrée pour confirmer, Échap pour annuler.",
        ),
        Outcome::Cancelled => String::from("Annulé."),
        _ => utterance(state),
    };
    if live {
        speech::say(&text);
    } else {
        speech::trace(&text);
    }
}

fn wait_for_key() -> Key {
    loop {
        if let Some(key) = ps2_keyboard::poll_key() {
            return key;
        }
        // SAFETY: CPL0; interrupts are enabled, so the keyboard IRQ can wake HLT.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// Run the session for real keys. Never returns.
///
/// # Safety
/// CPL0 on the bootstrap processor, after the keyboard was routed. Enables interrupts.
pub unsafe fn run() -> ! {
    debug_write("AW_ADMIN_SESSION_BEGIN\n");
    // SAFETY: caller's contract.
    unsafe { ps2_keyboard::arm_for_input() };
    let mut state = State::START;
    render(state);
    speech::say("Session d'administration d'omni-os.");
    announce(state, Outcome::Moved, true);
    loop {
        let key = wait_for_key();
        let (next, outcome) = step(state, key);
        state = next;
        match outcome {
            Outcome::Nothing => continue,
            Outcome::Run(Action::Restart) => {
                debug_write("AW_ADMIN_RESTART\n");
                speech::say("Redémarrage.");
                // SAFETY: CPL0; the FADT reset register, then the 8042, then a triple fault.
                unsafe { crate::power::reset_machine() };
            }
            Outcome::Run(Action::PowerOff) => {
                debug_write("AW_ADMIN_POWER_OFF\n");
                speech::say("Arrêt.");
                // SAFETY: CPL0; enters ACPI S5.
                unsafe { crate::power::power_off_machine() };
            }
            _ => {
                render(state);
                announce(state, outcome, true);
            }
        }
    }
}

/// Prove the session: navigate every panel with a fixed key script through the shared [`step`],
/// render and voice each landing (traced), check a confirmation is required and cancellable,
/// then render one utterance to real speech through HDA.
pub fn prove() {
    debug_write("AW_ADMIN_BEGIN\n");
    let script: &[(Key, usize, Option<usize>, Outcome)] = &[
        (Key::Right, 0, Some(0), Outcome::Moved),
        (Key::Down, 0, Some(1), Outcome::Moved),
        (Key::Escape, 0, None, Outcome::Moved),
        (Key::Down, 1, None, Outcome::Moved),
        (Key::Enter, 1, Some(0), Outcome::Moved),
        (Key::Left, 1, None, Outcome::Moved),
        (Key::Down, 2, None, Outcome::Moved),
        (Key::Down, 3, None, Outcome::Moved),
        (Key::Down, 4, None, Outcome::Moved),
        (Key::Down, 5, None, Outcome::Moved),
        (Key::Enter, 5, Some(0), Outcome::Moved),
        (Key::Down, 5, Some(1), Outcome::Moved),
        (
            Key::Enter,
            5,
            Some(1),
            Outcome::ConfirmRequired(Action::PowerOff),
        ),
        (Key::Escape, 5, Some(1), Outcome::Cancelled),
        (Key::Down, 5, Some(0), Outcome::Moved),
        (Key::Up, 5, Some(1), Outcome::Moved),
        (Key::Left, 5, None, Outcome::Moved),
        (Key::Down, 0, None, Outcome::Moved),
    ];
    let mut state = State::START;
    render(state);
    for &(key, panel, line, expected) in script {
        let (next, outcome) = step(state, key);
        state = next;
        if state.panel != panel || state.line != line || outcome != expected {
            debug_write("AW_ADMIN_FAIL reason=transition\n");
            return;
        }
        render(state);
        announce(state, outcome, false);
    }
    // Every panel says something measured.
    for panel in 0..PANELS.len() {
        if panel_lines(panel).is_empty() {
            debug_write("AW_ADMIN_FAIL reason=empty_panel\n");
            return;
        }
    }
    if !speech::prove("Session d'administration d'omni-os.") {
        return;
    }
    debug_write("AW_ADMIN_PROOF_OK panels=6\n");
}
