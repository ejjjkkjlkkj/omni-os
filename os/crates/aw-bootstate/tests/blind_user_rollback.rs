use aw_bootstate::{BootSelectionState, BootStateRecord, GenerationLocator};
use aw_generation::{
    GenerationPlan, ObjectId, REQUIRED_BOOT_COMPONENTS, REQUIRED_SUCCESS_HEALTH_CHECKS,
    RuntimeHealthCheck, RuntimeHealthError, RuntimeHealthReport,
};
use aw_recovery_contract::{
    AccessibleRecoveryReady, RecoveryAction, RecoveryCapability, RecoveryDiagnosticCode,
    RecoveryEvent, RecoveryProbeReport, RecoveryReadinessError, RecoverySeverity,
};
use aw_recovery_io::{BrailleSink, SpeechSink, StructuredDiagnosticSink, deliver_recovery_event};

fn object(seed: u8) -> ObjectId {
    ObjectId::new([seed; 32]).unwrap()
}

fn locator(generation: u64, seed: u8) -> GenerationLocator {
    GenerationLocator::new(generation, object(seed)).unwrap()
}

fn complete_plan() -> GenerationPlan<8> {
    let mut plan = GenerationPlan::new(42, 7).unwrap();
    for (index, kind) in REQUIRED_BOOT_COMPONENTS.into_iter().enumerate() {
        plan.push(kind, object((index + 1) as u8)).unwrap();
    }
    plan
}

fn recovery_ready_with(output: RecoveryCapability) -> AccessibleRecoveryReady {
    let mut report = RecoveryProbeReport::new();
    for capability in [
        RecoveryCapability::KeyboardInput,
        RecoveryCapability::StructuredDiagnostics,
        RecoveryCapability::RollbackSelection,
        RecoveryCapability::SignedReinstall,
        RecoveryCapability::DiagnosticExport,
        output,
    ] {
        report.mark_passed(capability);
    }
    report.accessible_ready().unwrap()
}

fn health_except(
    missing: RuntimeHealthCheck,
    recovery: Option<AccessibleRecoveryReady>,
) -> RuntimeHealthReport {
    let mut health = RuntimeHealthReport::new();
    for check in REQUIRED_SUCCESS_HEALTH_CHECKS {
        if check == missing {
            continue;
        }
        if check == RuntimeHealthCheck::AccessibleRecovery {
            if let Some(proof) = recovery {
                health.mark_accessible_recovery(proof);
            }
        } else {
            health.mark_passed(check).unwrap();
        }
    }
    health
}

fn final_trial() -> BootStateRecord {
    BootStateRecord::new(
        10,
        locator(42, 1),
        locator(41, 2),
        7,
        BootSelectionState::Trial { tries_remaining: 1 },
    )
    .unwrap()
}

fn consume_final_trial_and_roll_back() -> BootStateRecord {
    final_trial()
        .prepare_trial_boot()
        .unwrap()
        .after_failed_trial()
        .unwrap()
}

#[derive(Default)]
struct RecoveryRecorder {
    accepted: bool,
    seen: Option<RecoveryEvent>,
}

impl RecoveryRecorder {
    fn accepting() -> Self {
        Self {
            accepted: true,
            seen: None,
        }
    }
}

impl StructuredDiagnosticSink for RecoveryRecorder {
    fn emit(&mut self, event: RecoveryEvent) -> bool {
        self.seen = Some(event);
        self.accepted
    }
}

impl SpeechSink for RecoveryRecorder {
    fn speak(&mut self, event: RecoveryEvent) -> bool {
        self.seen = Some(event);
        self.accepted
    }
}

impl BrailleSink for RecoveryRecorder {
    fn present(&mut self, event: RecoveryEvent) -> bool {
        self.seen = Some(event);
        self.accepted
    }
}

#[test]
fn speech_failure_is_nonvisual_diagnostic_and_rolls_back_known_good() {
    let plan = complete_plan();
    let candidate = plan.boot_candidate(7).unwrap();
    let recovery = recovery_ready_with(RecoveryCapability::BrailleOutput);
    let health = health_except(RuntimeHealthCheck::Speech, Some(recovery));

    assert_eq!(
        candidate.successful_generation(health),
        Err(RuntimeHealthError::MissingRequiredCheck(
            RuntimeHealthCheck::Speech
        ))
    );

    let failure = RecoveryEvent::new(
        RecoveryDiagnosticCode::SpeechUnavailable,
        RecoverySeverity::Critical,
        RecoveryAction::BootPreviousGeneration,
        Some(42),
    );
    assert_eq!(failure.code().code(), 0x1403);
    assert_eq!(failure.action(), RecoveryAction::BootPreviousGeneration);
    assert_eq!(failure.generation(), Some(42));

    let rolled_back = consume_final_trial_and_roll_back();
    assert_eq!(rolled_back.selected().generation(), 41);
    assert_eq!(rolled_back.previous_successful().generation(), 41);
    assert_eq!(rolled_back.state(), BootSelectionState::Successful);

    let rollback = RecoveryEvent::new(
        RecoveryDiagnosticCode::RollbackActivated,
        RecoverySeverity::Warning,
        RecoveryAction::BootPreviousGeneration,
        Some(41),
    );
    assert_eq!(rollback.code().code(), 0x1301);
    assert_eq!(rollback.generation(), Some(41));
}

#[test]
fn delivered_braille_failure_event_precedes_known_good_rollback() {
    let failure = RecoveryEvent::new(
        RecoveryDiagnosticCode::SpeechUnavailable,
        RecoverySeverity::Critical,
        RecoveryAction::BootPreviousGeneration,
        Some(42),
    );
    let mut diagnostics = RecoveryRecorder::accepting();
    let mut speech = RecoveryRecorder::default();
    let mut braille = RecoveryRecorder::accepting();

    let evidence =
        deliver_recovery_event(failure, &mut diagnostics, &mut speech, &mut braille).unwrap();
    assert_eq!(evidence.event(), failure);
    assert!(!evidence.speech_delivered());
    assert!(evidence.braille_delivered());
    assert_eq!(diagnostics.seen, Some(failure));
    assert_eq!(speech.seen, Some(failure));
    assert_eq!(braille.seen, Some(failure));

    let delivered = evidence.event();
    assert_eq!(delivered.action(), RecoveryAction::BootPreviousGeneration);
    assert_eq!(delivered.generation(), Some(42));

    let rolled_back = consume_final_trial_and_roll_back();
    assert_eq!(rolled_back.selected().generation(), 41);
    assert_eq!(rolled_back.previous_successful().generation(), 41);
    assert_eq!(rolled_back.state(), BootSelectionState::Successful);
}

#[test]
fn power_loss_after_attempt_consumption_cannot_loop_broken_generation_forever() {
    let persisted_before_transfer = final_trial().prepare_trial_boot().unwrap();
    assert_eq!(
        persisted_before_transfer.state(),
        BootSelectionState::TrialAttempt { tries_remaining: 0 }
    );

    let recovered_after_power_loss = persisted_before_transfer.after_interrupted_trial().unwrap();
    assert_eq!(recovered_after_power_loss.selected().generation(), 41);
    assert_eq!(
        recovered_after_power_loss.state(),
        BootSelectionState::Successful
    );
}

#[test]
fn inaccessible_recovery_cannot_promote_trial_and_exhaustion_rolls_back() {
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
    assert_eq!(
        probe.accessible_ready(),
        Err(RecoveryReadinessError::NoDirectNonVisualOutput)
    );

    let plan = complete_plan();
    let candidate = plan.boot_candidate(7).unwrap();
    let health = health_except(RuntimeHealthCheck::AccessibleRecovery, None);
    assert_eq!(
        candidate.successful_generation(health),
        Err(RuntimeHealthError::MissingRequiredCheck(
            RuntimeHealthCheck::AccessibleRecovery
        ))
    );

    let failure = RecoveryEvent::new(
        RecoveryDiagnosticCode::RecoveryIntegrityFailed,
        RecoverySeverity::Critical,
        RecoveryAction::BootPreviousGeneration,
        Some(42),
    );
    assert_eq!(failure.code().code(), 0x1501);
    assert_eq!(failure.action(), RecoveryAction::BootPreviousGeneration);

    let rolled_back = consume_final_trial_and_roll_back();
    assert_eq!(rolled_back.selected().generation(), 41);
    assert_eq!(rolled_back.state(), BootSelectionState::Successful);
}

#[test]
fn graphical_only_boot_never_becomes_known_good() {
    let plan = complete_plan();
    let candidate = plan.boot_candidate(7).unwrap();
    let health = health_except(RuntimeHealthCheck::Speech, None);

    assert_eq!(
        candidate.successful_generation(health),
        Err(RuntimeHealthError::MissingRequiredCheck(
            RuntimeHealthCheck::Speech
        ))
    );

    let rolled_back = consume_final_trial_and_roll_back();
    assert_eq!(rolled_back.selected().generation(), 41);
    assert_eq!(rolled_back.state(), BootSelectionState::Successful);
}
