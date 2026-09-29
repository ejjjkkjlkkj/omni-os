//! Native Recovery Core, loader side (`os/docs/RECOVERY-BOOT-ARCHITECTURE.md`).
//!
//! Before the kernel is loaded, the loader reads the redundant boot-state record from the ESP
//! (`\OMNI\BOOTST.A` / `\OMNI\BOOTST.B`, `aw-bootstate`), applies the trial-boot rules, and
//! verifies that the selected generation's kernel image matches the SHA-256 digest recorded
//! for it. When nothing trustworthy can be booted, it enters the Recovery Core: one typed
//! `RecoveryEvent` is delivered to diagnostics, speech and braille, and a keyboard-only menu
//! (`aw-recovery-contract`) offers rollback, retry, diagnostic export and safe power-off.
//!
//! Generation 1 is the image at `\KERNEL.BIN`; generation N > 1 lives at
//! `\OMNI\GEN\<N>\KERNEL.BIN`. Promotion of a trial generation needs the kernel's runtime
//! health proof, which is not wired yet: a trial generation is therefore never promoted and
//! falls back to the known-good one when its attempts run out - the fail-safe direction.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;

use aw_bootstate::{
    BootSelectionState, BootStateError, BootStateRecord, GenerationLocator, next_write_slot,
    select_from_disk,
};
use aw_generation::ObjectId;
use aw_recovery_contract::{
    RecoveryAction, RecoveryCapability, RecoveryDiagnosticCode, RecoveryEvent,
    RecoveryInteractionOutcome, RecoveryKeyboardCommand, RecoveryMenuState, RecoveryProbeReport,
    RecoverySeverity,
};
use aw_recovery_io::{BrailleSink, SpeechSink, StructuredDiagnosticSink, deliver_recovery_event};
use aw_sha256::sha256;
use uefi::proto::console::text::{Key, ScanCode};
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode};
use uefi::runtime::{self, ResetType};
use uefi::{CString16, Status, boot};

use crate::audio;
use crate::aw_mark;
use crate::setup::{Lang, read_key_raw, speak_dynamic};

const STATE_A: &str = "OMNI\\BOOTST.A";
const STATE_B: &str = "OMNI\\BOOTST.B";
const DIAGNOSTICS: &str = "OMNI\\DIAG.TXT";

/// Where a generation's kernel image lives on the ESP.
fn kernel_path(generation: u64) -> String {
    if generation == 1 {
        String::from("KERNEL.BIN")
    } else {
        format!("OMNI\\GEN\\{generation}\\KERNEL.BIN")
    }
}

fn path16(path: &str) -> Option<CString16> {
    CString16::try_from(path).ok()
}

/// Read a whole file, or `None` if it is absent or unreadable.
pub fn read_file(root: &mut Directory, path: &str) -> Option<Vec<u8>> {
    let name = path16(path)?;
    let handle = root
        .open(&name, FileMode::Read, FileAttribute::empty())
        .ok()?;
    let mut file = handle.into_regular_file()?;
    let size = usize::try_from(file.get_boxed_info::<FileInfo>().ok()?.file_size()).ok()?;
    let mut data = vec![0_u8; size];
    let mut offset = 0;
    while offset < size {
        match file.read(&mut data[offset..]) {
            Ok(0) | Err(_) => break,
            Ok(read) => offset += read,
        }
    }
    (offset == size).then_some(data)
}

/// Write a whole file (creating `\OMNI` if needed) and flush it to the medium.
fn write_file(root: &mut Directory, path: &str, data: &[u8]) -> bool {
    if let Some(dir) = path16("OMNI") {
        let _ = root.open(&dir, FileMode::CreateReadWrite, FileAttribute::DIRECTORY);
    }
    let Some(name) = path16(path) else {
        return false;
    };
    // Replace the whole file: delete any previous version, then create it afresh, so a shorter
    // new content never leaves old bytes behind.
    if let Ok(old) = root.open(&name, FileMode::ReadWrite, FileAttribute::empty()) {
        let _ = old.delete();
    }
    let Ok(handle) = root.open(&name, FileMode::CreateReadWrite, FileAttribute::empty()) else {
        return false;
    };
    let Some(mut file) = handle.into_regular_file() else {
        return false;
    };
    if file.write(data).is_err() {
        return false;
    }
    file.flush().is_ok()
}

/// Persist `record` into the copy that does not hold the newest valid record (rule 5).
fn persist(root: &mut Directory, record: BootStateRecord) -> bool {
    let a = read_file(root, STATE_A);
    let b = read_file(root, STATE_B);
    let slot_b = next_write_slot(a.as_deref(), b.as_deref());
    let ok = write_file(
        root,
        if slot_b { STATE_B } else { STATE_A },
        &record.encode(),
    );
    aw_mark!(
        "AW_RECOVERY_STATE_WRITE slot={} sequence={} ok={ok}",
        if slot_b { 'B' } else { 'A' },
        record.sequence()
    );
    ok
}

fn state_name(state: BootSelectionState) -> &'static str {
    match state {
        BootSelectionState::Trial { .. } => "trial",
        BootSelectionState::TrialAttempt { .. } => "trial_attempt",
        BootSelectionState::Successful => "successful",
    }
}

/// Read a generation's kernel and check it against its recorded digest.
fn verified_kernel(root: &mut Directory, locator: GenerationLocator) -> Option<Vec<u8>> {
    let generation = locator.generation();
    let Some(image) = read_file(root, &kernel_path(generation)) else {
        aw_mark!("AW_RECOVERY_KERNEL_MISSING generation={generation}");
        return None;
    };
    if sha256(&image) != locator.manifest().bytes() {
        aw_mark!("AW_RECOVERY_INTEGRITY_FAIL generation={generation}");
        return None;
    }
    aw_mark!(
        "AW_RECOVERY_INTEGRITY_OK generation={generation} bytes={}",
        image.len()
    );
    Some(image)
}

/// Choose, verify and return the kernel image to boot, or enter the Recovery Core.
pub fn choose_kernel(root: &mut Directory) -> Result<Vec<u8>, Status> {
    let a = read_file(root, STATE_A);
    let b = read_file(root, STATE_B);
    let mut record = match select_from_disk(a.as_deref(), b.as_deref()) {
        Ok(record) => record,
        Err(BootStateError::NoUsableRecord) if a.is_none() && b.is_none() => {
            return first_boot(root);
        }
        Err(error) => {
            aw_mark!("AW_RECOVERY_STATE_REJECTED error={error:?}");
            return recovery_core(root, None, RecoveryDiagnosticCode::BootStateCorrupt);
        }
    };
    // A bounded number of transitions: each consumes a trial attempt or falls back.
    for _ in 0..=usize::from(aw_bootstate::MAX_TRIAL_BOOT_ATTEMPTS) + 1 {
        aw_mark!(
            "AW_RECOVERY_STATE sequence={} selected={} known_good={} state={}",
            record.sequence(),
            record.selected().generation(),
            record.previous_successful().generation(),
            state_name(record.state())
        );
        match record.state() {
            // The previous attempt was consumed and never promoted: it failed (power loss,
            // hang, crash). Count it before anything else.
            BootSelectionState::TrialAttempt { .. } => {
                let Ok(next) = record.after_interrupted_trial() else {
                    break;
                };
                aw_mark!(
                    "AW_RECOVERY_TRIAL_INTERRUPTED generation={}",
                    record.selected().generation()
                );
                if !persist(root, next) {
                    break;
                }
                record = next;
            }
            BootSelectionState::Trial { .. } => {
                let Ok(attempt) = record.prepare_trial_boot() else {
                    break;
                };
                // Rule 2: the consumed attempt is durable before control is transferred.
                if !persist(root, attempt) {
                    break;
                }
                if let Some(image) = verified_kernel(root, attempt.selected()) {
                    aw_mark!(
                        "AW_RECOVERY_BOOT generation={} state=trial_attempt",
                        attempt.selected().generation()
                    );
                    return Ok(image);
                }
                // The trial image itself is bad: record the failure and try again/fall back.
                let Ok(next) = attempt.after_failed_trial() else {
                    break;
                };
                if !persist(root, next) {
                    break;
                }
                record = next;
            }
            BootSelectionState::Successful => {
                if let Some(image) = verified_kernel(root, record.selected()) {
                    aw_mark!(
                        "AW_RECOVERY_BOOT generation={} state=successful",
                        record.selected().generation()
                    );
                    return Ok(image);
                }
                return recovery_core(
                    root,
                    Some(record),
                    RecoveryDiagnosticCode::ObjectVerificationFailed,
                );
            }
        }
    }
    recovery_core(
        root,
        Some(record),
        RecoveryDiagnosticCode::NoBootableGeneration,
    )
}

/// No record at all: this is the first boot. The image at `\KERNEL.BIN` becomes known-good
/// generation 1, bound to its SHA-256 digest.
fn first_boot(root: &mut Directory) -> Result<Vec<u8>, Status> {
    let Some(image) = read_file(root, &kernel_path(1)) else {
        aw_mark!("AW_RECOVERY_KERNEL_MISSING generation=1");
        return recovery_core(root, None, RecoveryDiagnosticCode::NoBootableGeneration);
    };
    let digest = ObjectId::new(sha256(&image)).ok_or(Status::LOAD_ERROR)?;
    let locator = GenerationLocator::new(1, digest).ok_or(Status::LOAD_ERROR)?;
    let record = BootStateRecord::new(1, locator, locator, 1, BootSelectionState::Successful)
        .map_err(|_| Status::LOAD_ERROR)?;
    let written = persist(root, record);
    aw_mark!("AW_RECOVERY_STATE_INIT generation=1 persisted={written}");
    aw_mark!("AW_RECOVERY_BOOT generation=1 state=successful");
    Ok(image)
}

// ---- Recovery Core -----------------------------------------------------------------------

fn code_name(code: RecoveryDiagnosticCode) -> &'static str {
    match code {
        RecoveryDiagnosticCode::BootStateCorrupt => "boot_state_corrupt",
        RecoveryDiagnosticCode::NoBootableGeneration => "no_bootable_generation",
        RecoveryDiagnosticCode::ManifestRejected => "manifest_rejected",
        RecoveryDiagnosticCode::ObjectVerificationFailed => "object_verification_failed",
        RecoveryDiagnosticCode::StorageReadFailed => "storage_read_failed",
        RecoveryDiagnosticCode::RollbackActivated => "rollback_activated",
        RecoveryDiagnosticCode::InputUnavailable => "input_unavailable",
        RecoveryDiagnosticCode::AudioUnavailable => "audio_unavailable",
        RecoveryDiagnosticCode::SpeechUnavailable => "speech_unavailable",
        RecoveryDiagnosticCode::AccessibilityBrokerUnavailable => "accessibility_unavailable",
        RecoveryDiagnosticCode::RecoveryIntegrityFailed => "recovery_integrity_failed",
    }
}

fn problem_text(code: RecoveryDiagnosticCode) -> &'static str {
    match code {
        RecoveryDiagnosticCode::BootStateCorrupt => {
            "Récupération. L'état de démarrage est illisible ou incohérent."
        }
        RecoveryDiagnosticCode::ObjectVerificationFailed => {
            "Récupération. Le noyau ne correspond pas à son empreinte : il a été modifié ou abîmé."
        }
        RecoveryDiagnosticCode::NoBootableGeneration => {
            "Récupération. Aucun système vérifié ne peut démarrer."
        }
        _ => "Récupération. Un problème empêche le démarrage.",
    }
}

fn action_text(action: RecoveryAction) -> &'static str {
    match action {
        RecoveryAction::RetryCurrentGeneration => "Réessayer le démarrage",
        RecoveryAction::BootPreviousGeneration => "Revenir au dernier système qui fonctionnait",
        RecoveryAction::EnterRecovery => "Démarrer la récupération externe, clé USB",
        RecoveryAction::ExportDiagnostics => "Enregistrer le diagnostic sur le disque",
        RecoveryAction::ReinstallSignedImage => "Réinstaller une image signée",
        RecoveryAction::PowerOffSafely => "Éteindre l'ordinateur",
    }
}

fn action_name(action: RecoveryAction) -> &'static str {
    match action {
        RecoveryAction::RetryCurrentGeneration => "retry",
        RecoveryAction::BootPreviousGeneration => "previous_generation",
        RecoveryAction::EnterRecovery => "external_recovery",
        RecoveryAction::ExportDiagnostics => "export_diagnostics",
        RecoveryAction::ReinstallSignedImage => "signed_reinstall",
        RecoveryAction::PowerOffSafely => "power_off",
    }
}

/// Structured diagnostics: the serial/debug marker, kept for the diagnostic export.
struct Diagnostics(Vec<String>);
/// Speech through whatever audio backend the machine has.
struct Voice(Option<audio::Speaker>);
/// A USB HID braille display, when one is connected.
struct BrailleOut(Option<crate::usb::Braille>);

/// The three outputs; each receives the same event (`aw-recovery-io`).
struct Channels {
    diagnostics: Diagnostics,
    voice: Voice,
    braille: BrailleOut,
    /// A key pressed while speech was playing (barge-in): handled next, never lost.
    pending: Option<Key>,
}

impl StructuredDiagnosticSink for Diagnostics {
    fn emit(&mut self, event: RecoveryEvent) -> bool {
        let line = format!(
            "AW_RECOVERY_EVENT code=0x{:04x} name={} severity={:?} action={} generation={:?}",
            event.code().code(),
            code_name(event.code()),
            event.severity(),
            action_name(event.action()),
            event.generation()
        );
        aw_mark!("{line}");
        self.0.push(line);
        true
    }
}

impl SpeechSink for Voice {
    fn speak(&mut self, event: RecoveryEvent) -> bool {
        if self.0.is_none() {
            return false;
        }
        let mut pending = None;
        speak_dynamic(
            problem_text(event.code()),
            Lang::Fr,
            &mut self.0,
            &mut pending,
        );
        true
    }
}

impl BrailleSink for BrailleOut {
    fn present(&mut self, event: RecoveryEvent) -> bool {
        self.0
            .as_ref()
            .is_some_and(|display| display.show(problem_text(event.code())))
    }
}

fn say(channels: &mut Channels, text: &str) {
    aw_mark!("AW_RECOVERY_SPEAK \"{text}\"");
    uefi::println!("{text}");
    speak_dynamic(text, Lang::Fr, &mut channels.voice.0, &mut channels.pending);
    if let Some(display) = &channels.braille.0 {
        display.show(text);
    }
}

fn next_command(
    pending_key: &mut Option<Key>,
    pending_confirmation: bool,
) -> RecoveryKeyboardCommand {
    // Timeout means no action (rule 8): the menu re-announces itself, it never decides.
    aw_mark!("AW_RECOVERY_AWAITING_INPUT confirmation={pending_confirmation}");
    for _ in 0..600 {
        let barged = pending_key.is_some();
        if let Some(key) = pending_key.take().or_else(read_key_raw) {
            aw_mark!("AW_RECOVERY_KEY key={key:?} during_speech={barged}");
            return match key {
                Key::Special(ScanCode::UP) => RecoveryKeyboardCommand::Previous,
                Key::Special(ScanCode::DOWN) => RecoveryKeyboardCommand::Next,
                Key::Special(ScanCode::ESCAPE) => RecoveryKeyboardCommand::Cancel,
                Key::Printable(c) if char::from(c) == '\r' => {
                    if pending_confirmation {
                        RecoveryKeyboardCommand::Confirm
                    } else {
                        RecoveryKeyboardCommand::Activate
                    }
                }
                _ => continue,
            };
        }
        boot::stall(Duration::from_millis(100));
    }
    RecoveryKeyboardCommand::Timeout
}

/// The Recovery Core: announce the problem, then let the user choose, keyboard only.
fn recovery_core(
    root: &mut Directory,
    record: Option<BootStateRecord>,
    code: RecoveryDiagnosticCode,
) -> Result<Vec<u8>, Status> {
    let mut channels = Channels {
        diagnostics: Diagnostics(Vec::new()),
        voice: Voice(audio::bring_up()),
        braille: BrailleOut(crate::usb::find_braille()),
        pending: None,
    };
    let generation = record.map(|r| r.selected().generation());
    let event = RecoveryEvent::new(
        code,
        RecoverySeverity::Critical,
        RecoveryAction::EnterRecovery,
        generation,
    );
    // One event, every output (rule 7); the proof is bound to this exact event.
    match deliver_recovery_event(
        event,
        &mut channels.diagnostics,
        &mut channels.voice,
        &mut channels.braille,
    ) {
        Ok(evidence) => aw_mark!(
            "AW_RECOVERY_EVENT_DELIVERED speech={} braille={}",
            evidence.speech_delivered(),
            evidence.braille_delivered()
        ),
        Err(error) => aw_mark!("AW_RECOVERY_EVENT_UNDELIVERED error={error:?}"),
    }

    // Readiness (rule 6), stated honestly: signed reinstall is not built yet.
    let mut probe = RecoveryProbeReport::new();
    probe.mark_passed(RecoveryCapability::KeyboardInput);
    probe.mark_passed(RecoveryCapability::StructuredDiagnostics);
    probe.mark_passed(RecoveryCapability::RollbackSelection);
    probe.mark_passed(RecoveryCapability::DiagnosticExport);
    if channels.voice.0.is_some() {
        probe.mark_passed(RecoveryCapability::SpeechOutput);
    }
    if channels.braille.0.is_some() {
        probe.mark_passed(RecoveryCapability::BrailleOutput);
    }
    aw_mark!(
        "AW_RECOVERY_READINESS ready={} speech={} braille={} missing=signed_reinstall",
        probe.accessible_ready().is_ok(),
        channels.voice.0.is_some(),
        channels.braille.0.is_some()
    );
    aw_mark!("AW_RECOVERY_CORE_READY code={}", code_name(code));

    let mut menu = RecoveryMenuState::new();
    say(&mut channels, problem_text(code));
    say(
        &mut channels,
        "Flèches pour choisir, Entrée pour valider, Échap pour annuler.",
    );
    say(&mut channels, action_text(menu.selected_action()));
    loop {
        let command = next_command(&mut channels.pending, menu.pending_confirmation().is_some());
        match menu.apply(command) {
            RecoveryInteractionOutcome::SelectionChanged(action) => {
                aw_mark!("AW_RECOVERY_FOCUS action={}", action_name(action));
                say(&mut channels, action_text(action));
            }
            RecoveryInteractionOutcome::ConfirmationRequired(action) => {
                aw_mark!(
                    "AW_RECOVERY_CONFIRM_REQUIRED action={}",
                    action_name(action)
                );
                say(
                    &mut channels,
                    "Appuyez de nouveau sur Entrée pour confirmer, ou Échap pour annuler.",
                );
            }
            RecoveryInteractionOutcome::ConfirmationCancelled => say(&mut channels, "Annulé."),
            RecoveryInteractionOutcome::NoAction => {
                if command == RecoveryKeyboardCommand::Timeout {
                    say(&mut channels, action_text(menu.selected_action()));
                }
            }
            RecoveryInteractionOutcome::ActionReady(action) => {
                aw_mark!("AW_RECOVERY_ACTION action={}", action_name(action));
                if let Some(image) = perform(root, record, action, &mut channels) {
                    return Ok(image);
                }
            }
        }
    }
}

/// Run one chosen action. Returns a verified kernel image when the action leads to a boot.
fn perform(
    root: &mut Directory,
    record: Option<BootStateRecord>,
    action: RecoveryAction,
    channels: &mut Channels,
) -> Option<Vec<u8>> {
    match action {
        RecoveryAction::RetryCurrentGeneration => {
            let image = record.and_then(|r| verified_kernel(root, r.selected()));
            if image.is_none() {
                say(
                    channels,
                    "Impossible : ce système ne passe pas la vérification.",
                );
            }
            image
        }
        RecoveryAction::BootPreviousGeneration => {
            let record = record?;
            let known = record.previous_successful();
            let image = verified_kernel(root, known);
            match (&image, record.sequence().checked_add(1)) {
                (Some(_), Some(sequence)) => {
                    if let Ok(rollback) = BootStateRecord::new(
                        sequence,
                        known,
                        known,
                        record.rollback_floor(),
                        BootSelectionState::Successful,
                    ) {
                        persist(root, rollback);
                        aw_mark!("AW_RECOVERY_ROLLBACK generation={}", known.generation());
                    }
                }
                _ => say(
                    channels,
                    "Impossible : le dernier système connu ne passe pas la vérification.",
                ),
            }
            image
        }
        RecoveryAction::EnterRecovery => {
            say(
                channels,
                "Aucune clé de récupération externe n'est détectée.",
            );
            None
        }
        RecoveryAction::ExportDiagnostics => {
            let mut text = String::new();
            for line in &channels.diagnostics.0 {
                text.push_str(line);
                text.push_str("\r\n");
            }
            if let Some(record) = record {
                text.push_str(&format!(
                    "state sequence={} selected={} known_good={} state={}\r\n",
                    record.sequence(),
                    record.selected().generation(),
                    record.previous_successful().generation(),
                    state_name(record.state())
                ));
            }
            let ok = write_file(root, DIAGNOSTICS, text.as_bytes());
            aw_mark!(
                "AW_RECOVERY_DIAGNOSTICS_EXPORTED ok={ok} bytes={}",
                text.len()
            );
            say(
                channels,
                if ok {
                    "Diagnostic enregistré dans OMNI, DIAG point TXT."
                } else {
                    "Le diagnostic n'a pas pu être enregistré."
                },
            );
            None
        }
        RecoveryAction::ReinstallSignedImage => {
            say(
                channels,
                "La réinstallation signée n'est pas encore disponible dans cette version.",
            );
            None
        }
        RecoveryAction::PowerOffSafely => {
            say(channels, "Arrêt de l'ordinateur.");
            aw_mark!("AW_RECOVERY_POWER_OFF");
            runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
        }
    }
}
