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
//! `\OMNI\GEN\<N>\KERNEL.BIN`. A trial generation is promoted to known-good only on the boot
//! after its attempt, and only with the kernel's runtime-health record (`OmniHealth` UEFI
//! variable, `aw_bootstate::HealthRecord`) for that exact attempt, an accessible Recovery Core and
//! a verified rollback target. Without that record the attempt counts as failed and, when the
//! attempts run out, the known-good generation boots again - the fail-safe direction.
//!
//! Reinstall restores the known-good generation from a removable medium
//! (`\OMNI\REINST\KERNEL.BIN`): the image is accepted only when its SHA-256 equals the digest
//! of that generation in the boot-state record, the target disk is announced before the
//! confirmation, and the written file is read back and verified before it boots.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;

use aw_bootstate::{
    BootSelectionState, BootStateError, BootStateRecord, GenerationLocator, HEALTH_VARIABLE_NAME,
    HEALTH_VENDOR_GUID, HealthRecord, next_write_slot, promote_trial, select_from_disk,
};
use aw_generation::ObjectId;
use aw_recovery_contract::{
    AccessibleRecoveryReady, RecoveryAction, RecoveryCapability, RecoveryDiagnosticCode,
    RecoveryEvent, RecoveryInteractionOutcome, RecoveryKeyboardCommand, RecoveryMenuState,
    RecoveryProbeReport, RecoverySeverity,
};
use aw_recovery_io::{BrailleSink, SpeechSink, StructuredDiagnosticSink, deliver_recovery_event};
use aw_sha256::sha256;
use uefi::proto::console::text::{Key, ScanCode};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::runtime::{self, ResetType, VariableVendor};
use uefi::{CString16, Status, boot};

use crate::audio;
use crate::aw_mark;
use crate::setup::{Lang, read_key_raw, speak_dynamic};

const STATE_A: &str = "OMNI\\BOOTST.A";
const STATE_B: &str = "OMNI\\BOOTST.B";
const DIAGNOSTICS: &str = "OMNI\\DIAG.TXT";
/// Reinstall image on a removable medium.
const REINSTALL_IMAGE: &str = "OMNI\\REINST\\KERNEL.BIN";
const REINSTALL_SIGNATURE: &str = "OMNI\\REINST\\KERNEL.SIG";

/// The trial attempt being booted (generation, boot-state sequence), for the kernel handoff.
static TRIAL_GENERATION: AtomicU64 = AtomicU64::new(0);
static TRIAL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// `(generation, sequence)` when this boot is a trial attempt.
pub fn trial_attempt() -> Option<(u64, u64)> {
    let generation = TRIAL_GENERATION.load(Ordering::SeqCst);
    (generation != 0).then(|| (generation, TRIAL_SEQUENCE.load(Ordering::SeqCst)))
}

/// One-shot update request left on the ESP next to a new generation.
const UPDATE_REQUEST: &str = "OMNI\\UPDATE.REQ";
/// Attempts a staged generation gets before the known-good one boots again.
const UPDATE_TRIES: u8 = 2;

/// Consume `\OMNI\UPDATE.REQ` (`generation=<n>`): verify generation n's kernel (publisher
/// signature when this loader carries a key) and stage it for a bounded trial. The request is
/// deleted before anything else, so it never repeats; the known-good generation stays the
/// fallback until the new one proves its health.
fn take_update_request(root: &mut Directory, record: BootStateRecord) -> Option<BootStateRecord> {
    let request = read_file(root, UPDATE_REQUEST)?;
    let consumed = remove_file(root, UPDATE_REQUEST);
    aw_mark!("AW_UPDATE_REQUEST consumed={consumed}");
    if !consumed {
        return None;
    }
    let Some(generation) = core::str::from_utf8(&request)
        .ok()
        .and_then(|text| text.trim().strip_prefix("generation="))
        .and_then(|n| n.trim().parse::<u64>().ok())
    else {
        aw_mark!("AW_UPDATE_REFUSED reason=malformed_request");
        return None;
    };
    let Some(image) = read_file(root, &kernel_path(generation)) else {
        aw_mark!("AW_UPDATE_REFUSED reason=kernel_missing generation={generation}");
        return None;
    };
    let signature = read_file(root, &signature_path(generation));
    let verdict = crate::publisher::check(&image, signature.as_deref());
    if !matches!(
        verdict,
        crate::publisher::Verdict::Valid | crate::publisher::Verdict::NoKey
    ) {
        aw_mark!(
            "AW_UPDATE_REFUSED reason=publisher_signature_{} generation={generation}",
            verdict.name()
        );
        return None;
    }
    let locator =
        ObjectId::new(sha256(&image)).and_then(|id| GenerationLocator::new(generation, id))?;
    match record.stage_trial(locator, UPDATE_TRIES) {
        Ok(staged) if persist(root, staged) => {
            aw_mark!(
                "AW_UPDATE_STAGED generation={generation} tries={UPDATE_TRIES} signature={}",
                verdict.name()
            );
            Some(staged)
        }
        Ok(_) => {
            aw_mark!("AW_UPDATE_REFUSED reason=state_not_persisted");
            None
        }
        Err(error) => {
            aw_mark!("AW_UPDATE_REFUSED reason={error:?}");
            None
        }
    }
}

/// Read and delete the kernel's health record left by the previous boot (if any). The variable
/// is always deleted: a record is used at most once.
fn take_health_record() -> Option<Result<HealthRecord, aw_bootstate::HealthDecodeError>> {
    let name = path16(HEALTH_VARIABLE_NAME)?;
    let vendor = VariableVendor(uefi::Guid::from_bytes(HEALTH_VENDOR_GUID));
    let (data, _) = runtime::get_variable_boxed(&name, &vendor).ok()?;
    let deleted = runtime::delete_variable(&name, &vendor).is_ok();
    aw_mark!(
        "AW_RECOVERY_HEALTH_RECORD bytes={} deleted={deleted}",
        data.len()
    );
    Some(HealthRecord::decode(&data))
}

/// What the Recovery Core can do without sight, measured now.
fn recovery_probe(speech: bool, braille: bool) -> RecoveryProbeReport {
    let mut probe = RecoveryProbeReport::new();
    for capability in [
        RecoveryCapability::KeyboardInput,
        RecoveryCapability::StructuredDiagnostics,
        RecoveryCapability::RollbackSelection,
        RecoveryCapability::SignedReinstall,
        RecoveryCapability::DiagnosticExport,
    ] {
        probe.mark_passed(capability);
    }
    if speech {
        probe.mark_passed(RecoveryCapability::SpeechOutput);
    }
    if braille {
        probe.mark_passed(RecoveryCapability::BrailleOutput);
    }
    probe
}

/// Promote the persisted trial attempt with the kernel's health record, or say why not.
fn promote(
    root: &mut Directory,
    record: BootStateRecord,
    health: Option<Result<HealthRecord, aw_bootstate::HealthDecodeError>>,
) -> Option<BootStateRecord> {
    let generation = record.selected().generation();
    let health = match health {
        None => {
            aw_mark!(
                "AW_RECOVERY_PROMOTION_REFUSED generation={generation} reason=no_health_record"
            );
            return None;
        }
        Some(Err(error)) => {
            aw_mark!("AW_RECOVERY_PROMOTION_REFUSED generation={generation} reason={error:?}");
            return None;
        }
        Some(Ok(health)) => health,
    };
    let speech = audio::bring_up().is_some();
    let braille = crate::usb::find_braille().is_some();
    let ready: AccessibleRecoveryReady = match recovery_probe(speech, braille).accessible_ready() {
        Ok(ready) => ready,
        Err(error) => {
            aw_mark!("AW_RECOVERY_PROMOTION_REFUSED generation={generation} reason={error:?}");
            return None;
        }
    };
    let rollback = verified_kernel(root, record.previous_successful()).is_some();
    match promote_trial(record, health, ready, rollback) {
        Ok(promoted) => {
            aw_mark!(
                "AW_RECOVERY_PROMOTED generation={generation} sequence={} health_mask={:#x}",
                health.sequence(),
                health.passed_mask()
            );
            Some(promoted)
        }
        Err(error) => {
            aw_mark!("AW_RECOVERY_PROMOTION_REFUSED generation={generation} reason={error:?}");
            None
        }
    }
}

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

/// Delete a file; `true` if it existed and is gone.
pub fn remove_file(root: &mut Directory, path: &str) -> bool {
    let Some(name) = path16(path) else {
        return false;
    };
    root.open(&name, FileMode::ReadWrite, FileAttribute::empty())
        .is_ok_and(|file| file.delete().is_ok())
}

/// Write a whole file (creating `\OMNI` if needed) and flush it to the medium.
pub(crate) fn write_file(root: &mut Directory, path: &str, data: &[u8]) -> bool {
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
    // One-shot update request (recovery rules 3 and 4): stage a new generation for a trial.
    if let Some(staged) = take_update_request(root, record) {
        record = staged;
    }
    let mut health = take_health_record();
    // A bounded number of transitions: each consumes a trial attempt or falls back.
    for _ in 0..=usize::from(aw_bootstate::MAX_TRIAL_BOOT_ATTEMPTS) + 2 {
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
                // The attempt reached a healthy kernel: promote it (the record is used once).
                if let Some(promoted) = promote(root, record, health.take())
                    && persist(root, promoted)
                {
                    record = promoted;
                    continue;
                }
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
                    TRIAL_GENERATION.store(attempt.selected().generation(), Ordering::SeqCst);
                    TRIAL_SEQUENCE.store(attempt.sequence(), Ordering::SeqCst);
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

    // Readiness (rule 6), measured: keyboard, diagnostics, rollback, verified reinstall and
    // export are built in; speech and braille depend on the hardware found now.
    let probe = recovery_probe(channels.voice.0.is_some(), channels.braille.0.is_some());
    aw_mark!(
        "AW_RECOVERY_READINESS ready={} speech={} braille={}",
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
                if action == RecoveryAction::ReinstallSignedImage {
                    // The target is named before the user confirms a destructive action.
                    let target = boot_disk_text();
                    aw_mark!("AW_RECOVERY_REINSTALL_TARGET disk=\"{target}\"");
                    say(
                        &mut channels,
                        &format!("Cible : le disque de démarrage, {target}. Il sera réécrit."),
                    );
                }
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
                    let generation = record.map_or(0, |r| match action {
                        RecoveryAction::RetryCurrentGeneration => r.selected().generation(),
                        _ => r.previous_successful().generation(),
                    });
                    aw_mark!(
                        "AW_RECOVERY_BOOT generation={generation} state=recovery action={}",
                        action_name(action)
                    );
                    return Ok(image);
                }
            }
        }
    }
}

/// The removable-media boot path every UEFI firmware honours (UEFI 2.11, 3.5.1.1).
const EXTERNAL_LOADER: &str = "EFI\\BOOT\\BOOTX64.EFI";

/// Text of the device path of the volume omni-os booted from.
fn boot_disk_text() -> String {
    use uefi::proto::device_path::DevicePath;
    use uefi::proto::device_path::text::{AllowShortcuts, DisplayOnly};
    let device = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .ok()
        .and_then(|image| image.device());
    let Some(device) = device else {
        return String::from("inconnu");
    };
    // SAFETY: shared GetProtocol open of the device path, read only.
    let path = unsafe {
        boot::open_protocol::<DevicePath>(
            boot::OpenProtocolParams {
                handle: device,
                agent: boot::image_handle(),
                controller: None,
            },
            boot::OpenProtocolAttributes::GetProtocol,
        )
    };
    path.ok()
        .and_then(|p| p.to_string16(DisplayOnly(true), AllowShortcuts(true)).ok())
        .map_or_else(|| String::from("inconnu"), |text| format!("{text}"))
}

/// Other volumes than the one omni-os booted from that carry a bootable recovery loader, with
/// that loader's bytes. Read-only.
fn external_media() -> Vec<(uefi::Handle, Vec<u8>)> {
    external_files(EXTERNAL_LOADER)
}

/// `path` read from every volume except the one omni-os booted from. Read-only.
fn external_files(path: &str) -> Vec<(uefi::Handle, Vec<u8>)> {
    // A recovery key is often plugged in after boot, and the firmware only connects the devices
    // of its boot order: connect every controller recursively (UEFI 2.11, 7.3 ConnectController)
    // so its volume appears.
    let all = boot::locate_handle_buffer(boot::SearchType::AllHandles)
        .map(|handles| handles.to_vec())
        .unwrap_or_default();
    let connected = all
        .iter()
        .filter(|handle| boot::connect_controller(**handle, &[], None, true).is_ok())
        .count();
    aw_mark!(
        "AW_RECOVERY_CONNECT_ALL handles={} connected={connected}",
        all.len()
    );
    let own = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .ok()
        .and_then(|image| image.device());
    let mut found = Vec::new();
    for handle in boot::find_handles::<SimpleFileSystem>().unwrap_or_default() {
        if Some(handle) == own {
            continue;
        }
        // SAFETY: GetProtocol does not take ownership; the volume is only read, then closed.
        let fs = unsafe {
            boot::open_protocol::<SimpleFileSystem>(
                boot::OpenProtocolParams {
                    handle,
                    agent: boot::image_handle(),
                    controller: None,
                },
                boot::OpenProtocolAttributes::GetProtocol,
            )
        };
        let Ok(mut fs) = fs else { continue };
        let Ok(mut volume) = fs.open_volume() else {
            continue;
        };
        if let Some(image) = read_file(&mut volume, path) {
            aw_mark!(
                "AW_RECOVERY_EXTERNAL_MEDIUM device={handle:?} bytes={}",
                image.len()
            );
            found.push((handle, image));
        }
    }
    found
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
            let media = external_media();
            aw_mark!("AW_RECOVERY_EXTERNAL media={}", media.len());
            let Some((handle, image)) = media.into_iter().next() else {
                say(
                    channels,
                    "Aucune clé de récupération externe n'est détectée.",
                );
                return None;
            };
            say(channels, "Support de récupération trouvé. Démarrage.");
            // The firmware's LoadImage applies the Secure Boot policy to this image as it would
            // to any boot option: an unsigned or revoked recovery medium is refused here.
            let loaded = boot::load_image(
                boot::image_handle(),
                boot::LoadImageSource::FromBuffer {
                    buffer: &image,
                    file_path: None,
                },
            );
            match loaded {
                Ok(child) => {
                    aw_mark!(
                        "AW_RECOVERY_EXTERNAL_START bytes={} device={handle:?}",
                        image.len()
                    );
                    let status = boot::start_image(child).err().map(|e| e.status());
                    aw_mark!("AW_RECOVERY_EXTERNAL_RETURNED status={:?}", status);
                    say(
                        channels,
                        "La récupération externe est terminée. Retour au menu.",
                    );
                }
                Err(error) => {
                    aw_mark!("AW_RECOVERY_EXTERNAL_REFUSED status={:?}", error.status());
                    say(
                        channels,
                        "Le support de récupération a été refusé par le micrologiciel.",
                    );
                }
            }
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
        RecoveryAction::ReinstallSignedImage => reinstall(root, record, channels),
        RecoveryAction::PowerOffSafely => {
            say(channels, "Arrêt de l'ordinateur.");
            aw_mark!("AW_RECOVERY_POWER_OFF");
            runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
        }
    }
}

/// Signature file next to a generation's kernel image.
fn signature_path(generation: u64) -> String {
    if generation == 1 {
        String::from("KERNEL.SIG")
    } else {
        format!("OMNI\\GEN\\{generation}\\KERNEL.SIG")
    }
}

/// Reinstall from a removable medium. Accepted: an image signed by the embedded publisher key
/// (installed as known-good, a new generation when it differs from the recorded one), or an
/// unsigned image identical to the known-good generation. A signature that does not verify is
/// always refused.
fn reinstall(
    root: &mut Directory,
    record: Option<BootStateRecord>,
    channels: &mut Channels,
) -> Option<Vec<u8>> {
    let Some(record) = record else {
        say(
            channels,
            "Impossible : aucun état de démarrage ne désigne le système à réinstaller.",
        );
        aw_mark!("AW_RECOVERY_REINSTALL_REFUSED reason=no_state");
        return None;
    };
    let known = record.previous_successful();
    let wanted = known.manifest().bytes();
    let candidates = external_files(REINSTALL_IMAGE);
    let signatures = external_files(REINSTALL_SIGNATURE);
    let found = candidates.len();
    let mut chosen = None;
    for (handle, image) in candidates {
        let signature = signatures
            .iter()
            .find(|(h, _)| *h == handle)
            .map(|(_, s)| s.clone());
        let verdict = crate::publisher::check(&image, signature.as_deref());
        let digest = sha256(&image);
        aw_mark!(
            "AW_RECOVERY_REINSTALL_CANDIDATE signature={} matches_known_good={}",
            verdict.name(),
            digest == wanted
        );
        let accepted = match verdict {
            crate::publisher::Verdict::Valid => true,
            crate::publisher::Verdict::Invalid => false,
            _ => digest == wanted,
        };
        if accepted {
            chosen = Some((image, signature, verdict, digest));
            break;
        }
    }
    let Some((image, signature, verdict, digest)) = chosen else {
        aw_mark!("AW_RECOVERY_REINSTALL_REFUSED reason=no_verified_image media={found}");
        say(
            channels,
            if found == 0 {
                "Aucun support de réinstallation n'est détecté."
            } else {
                "Le support de réinstallation n'est ni signé par l'éditeur, ni identique au système connu. Refusé."
            },
        );
        return None;
    };
    // Same bytes: the known-good generation. A different signed release: the next generation.
    let target = if digest == wanted {
        known
    } else {
        let generation = known.generation().checked_add(1)?;
        GenerationLocator::new(generation, ObjectId::new(digest)?)?
    };
    let written = write_file(root, &kernel_path(target.generation()), &image)
        && signature
            .as_ref()
            .is_none_or(|sig| write_file(root, &signature_path(target.generation()), sig));
    // Read back from the disk: only what is really on it may boot.
    let verified = written && verified_kernel(root, target).is_some();
    if !verified {
        aw_mark!("AW_RECOVERY_REINSTALL_FAIL written={written}");
        say(
            channels,
            "L'écriture sur le disque a échoué. Le système n'a pas été modifié.",
        );
        return None;
    }
    let sequence = record.sequence().checked_add(1)?;
    let restored = BootStateRecord::new(
        sequence,
        target,
        target,
        record.rollback_floor(),
        BootSelectionState::Successful,
    )
    .ok()?;
    let persisted = persist(root, restored);
    aw_mark!(
        "AW_RECOVERY_REINSTALLED generation={} bytes={} signature={} state_persisted={persisted}",
        target.generation(),
        image.len(),
        verdict.name()
    );
    say(
        channels,
        if verdict == crate::publisher::Verdict::Valid {
            "Réinstallation vérifiée : signature de l'éditeur valide. Démarrage du système."
        } else {
            "Réinstallation vérifiée. Démarrage du système."
        },
    );
    Some(image)
}
