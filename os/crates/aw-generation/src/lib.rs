#![no_std]
#![forbid(unsafe_code)]

use aw_recovery_contract::AccessibleRecoveryReady;

pub const OBJECT_DIGEST_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectId([u8; OBJECT_DIGEST_BYTES]);

impl ObjectId {
    #[must_use]
    pub fn new(bytes: [u8; OBJECT_DIGEST_BYTES]) -> Option<Self> {
        bytes.iter().any(|byte| *byte != 0).then_some(Self(bytes))
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; OBJECT_DIGEST_BYTES] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreComponentKind {
    Kernel,
    StorageService,
    InputService,
    AudioService,
    AccessibilityBroker,
    SpeechService,
    SecurityService,
    UpdateService,
}

pub const REQUIRED_BOOT_COMPONENTS: [CoreComponentKind; 8] = [
    CoreComponentKind::Kernel,
    CoreComponentKind::StorageService,
    CoreComponentKind::InputService,
    CoreComponentKind::AudioService,
    CoreComponentKind::AccessibilityBroker,
    CoreComponentKind::SpeechService,
    CoreComponentKind::SecurityService,
    CoreComponentKind::UpdateService,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenerationComponent {
    kind: CoreComponentKind,
    object: ObjectId,
}

impl GenerationComponent {
    #[must_use]
    pub const fn kind(self) -> CoreComponentKind {
        self.kind
    }

    #[must_use]
    pub const fn object(self) -> ObjectId {
        self.object
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationPlanError {
    ZeroGeneration,
    ZeroRollbackIndex,
    Capacity,
    DuplicateComponent(CoreComponentKind),
    MissingRequiredComponent(CoreComponentKind),
    RollbackRejected { declared: u64, minimum_allowed: u64 },
}

/// In-memory semantic plan for one immutable OS generation.
///
/// This type intentionally does not perform hashing, signature verification, serialization, or
/// disk I/O. Those security boundaries belong to dedicated layers. `ObjectId` values are
/// references to already content-addressed objects and this plan validates only composition and
/// boot-health policy.
pub struct GenerationPlan<const COMPONENTS: usize> {
    generation: u64,
    rollback_index: u64,
    components: [Option<GenerationComponent>; COMPONENTS],
    len: usize,
}

impl<const COMPONENTS: usize> GenerationPlan<COMPONENTS> {
    pub fn new(generation: u64, rollback_index: u64) -> Result<Self, GenerationPlanError> {
        if generation == 0 {
            return Err(GenerationPlanError::ZeroGeneration);
        }
        if rollback_index == 0 {
            return Err(GenerationPlanError::ZeroRollbackIndex);
        }

        Ok(Self {
            generation,
            rollback_index,
            components: [None; COMPONENTS],
            len: 0,
        })
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn rollback_index(&self) -> u64 {
        self.rollback_index
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn component(&self, index: usize) -> Option<GenerationComponent> {
        if index >= self.len {
            return None;
        }
        self.components[index]
    }

    pub fn push(
        &mut self,
        kind: CoreComponentKind,
        object: ObjectId,
    ) -> Result<(), GenerationPlanError> {
        if self.contains(kind) {
            return Err(GenerationPlanError::DuplicateComponent(kind));
        }
        if self.len >= COMPONENTS {
            return Err(GenerationPlanError::Capacity);
        }

        self.components[self.len] = Some(GenerationComponent { kind, object });
        self.len += 1;
        Ok(())
    }

    #[must_use]
    pub fn contains(&self, kind: CoreComponentKind) -> bool {
        let mut index = 0;
        while index < self.len {
            if self.components[index].is_some_and(|component| component.kind == kind) {
                return true;
            }
            index += 1;
        }
        false
    }

    /// Produces a private proof token only when the generation is boot-policy complete.
    ///
    /// Accessibility is deliberately part of the boot contract: input, audio, the accessibility
    /// broker and speech service are mandatory. A generation that boots graphically but strands a
    /// blind user therefore cannot be marked boot-ready by this API.
    pub fn boot_candidate(
        &self,
        minimum_rollback_index: u64,
    ) -> Result<BootCandidate<'_, COMPONENTS>, GenerationPlanError> {
        if self.rollback_index < minimum_rollback_index {
            return Err(GenerationPlanError::RollbackRejected {
                declared: self.rollback_index,
                minimum_allowed: minimum_rollback_index,
            });
        }

        for required in REQUIRED_BOOT_COMPONENTS {
            if !self.contains(required) {
                return Err(GenerationPlanError::MissingRequiredComponent(required));
            }
        }

        Ok(BootCandidate { plan: self })
    }
}

/// Proof that semantic generation composition and anti-rollback policy passed.
///
/// Cryptographic authentication remains a separate proof layer. A boot loader must never treat
/// this token alone as signature verification.
pub struct BootCandidate<'a, const COMPONENTS: usize> {
    plan: &'a GenerationPlan<COMPONENTS>,
}

impl<const COMPONENTS: usize> BootCandidate<'_, COMPONENTS> {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.plan.generation()
    }

    #[must_use]
    pub const fn rollback_index(&self) -> u64 {
        self.plan.rollback_index()
    }

    #[must_use]
    pub const fn component_count(&self) -> usize {
        self.plan.len()
    }

    /// Converts a boot candidate into a success proof only after every mandatory runtime health
    /// check has passed. In particular, a graphical desktop is insufficient: keyboard input,
    /// audio, the accessibility broker, speech and an independently usable accessible recovery
    /// path must all be healthy.
    pub fn successful_generation(
        &self,
        health: RuntimeHealthReport,
    ) -> Result<SuccessfulGeneration, RuntimeHealthError> {
        for required in REQUIRED_SUCCESS_HEALTH_CHECKS {
            if !health.passed(required) {
                return Err(RuntimeHealthError::MissingRequiredCheck(required));
            }
        }

        Ok(SuccessfulGeneration {
            generation: self.generation(),
            rollback_index: self.rollback_index(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeHealthCheck {
    Kernel,
    Storage,
    Input,
    Audio,
    AccessibilityBroker,
    Speech,
    AccessibleRecovery,
    Security,
    Update,
}

impl RuntimeHealthCheck {
    const fn bit(self) -> u16 {
        match self {
            Self::Kernel => 1 << 0,
            Self::Storage => 1 << 1,
            Self::Input => 1 << 2,
            Self::Audio => 1 << 3,
            Self::AccessibilityBroker => 1 << 4,
            Self::Speech => 1 << 5,
            Self::AccessibleRecovery => 1 << 6,
            Self::Security => 1 << 7,
            Self::Update => 1 << 8,
        }
    }
}

pub const REQUIRED_SUCCESS_HEALTH_CHECKS: [RuntimeHealthCheck; 9] = [
    RuntimeHealthCheck::Kernel,
    RuntimeHealthCheck::Storage,
    RuntimeHealthCheck::Input,
    RuntimeHealthCheck::Audio,
    RuntimeHealthCheck::AccessibilityBroker,
    RuntimeHealthCheck::Speech,
    RuntimeHealthCheck::AccessibleRecovery,
    RuntimeHealthCheck::Security,
    RuntimeHealthCheck::Update,
];

/// Semantic record of completed runtime probes.
///
/// Ordinary probes may record a PASS directly. Accessible recovery is deliberately different: it
/// can only be recorded by presenting `AccessibleRecoveryReady`, which itself proves keyboard,
/// structured diagnostics, rollback/reinstall/export actions and speech or braille.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeHealthReport {
    passed: u16,
}

impl RuntimeHealthReport {
    #[must_use]
    pub const fn new() -> Self {
        Self { passed: 0 }
    }

    pub fn mark_passed(
        &mut self,
        check: RuntimeHealthCheck,
    ) -> Result<(), RuntimeHealthRecordError> {
        if check == RuntimeHealthCheck::AccessibleRecovery {
            return Err(RuntimeHealthRecordError::RecoveryProofRequired);
        }
        self.passed |= check.bit();
        Ok(())
    }

    pub fn mark_accessible_recovery(&mut self, _proof: AccessibleRecoveryReady) {
        self.passed |= RuntimeHealthCheck::AccessibleRecovery.bit();
    }

    #[must_use]
    pub const fn passed(self, check: RuntimeHealthCheck) -> bool {
        self.passed & check.bit() != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeHealthRecordError {
    RecoveryProofRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeHealthError {
    MissingRequiredCheck(RuntimeHealthCheck),
}

/// Proof that boot composition, anti-rollback and all mandatory runtime health checks passed.
///
/// Fields are private so callers cannot construct this token without passing the checks above.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuccessfulGeneration {
    generation: u64,
    rollback_index: u64,
}

impl SuccessfulGeneration {
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn rollback_index(self) -> u64 {
        self.rollback_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_recovery_contract::{RecoveryCapability, RecoveryProbeReport};

    fn object(seed: u8) -> ObjectId {
        ObjectId::new([seed; OBJECT_DIGEST_BYTES]).unwrap()
    }

    fn complete_plan() -> GenerationPlan<8> {
        let mut plan = GenerationPlan::new(42, 7).unwrap();
        for (index, kind) in REQUIRED_BOOT_COMPONENTS.into_iter().enumerate() {
            plan.push(kind, object((index + 1) as u8)).unwrap();
        }
        plan
    }

    fn recovery_ready() -> AccessibleRecoveryReady {
        let mut report = RecoveryProbeReport::new();
        for capability in [
            RecoveryCapability::KeyboardInput,
            RecoveryCapability::StructuredDiagnostics,
            RecoveryCapability::SpeechOutput,
            RecoveryCapability::RollbackSelection,
            RecoveryCapability::SignedReinstall,
            RecoveryCapability::DiagnosticExport,
        ] {
            report.mark_passed(capability);
        }
        report.accessible_ready().unwrap()
    }

    fn complete_health() -> RuntimeHealthReport {
        let mut health = RuntimeHealthReport::new();
        for check in REQUIRED_SUCCESS_HEALTH_CHECKS {
            if check == RuntimeHealthCheck::AccessibleRecovery {
                health.mark_accessible_recovery(recovery_ready());
            } else {
                health.mark_passed(check).unwrap();
            }
        }
        health
    }

    #[test]
    fn rejects_zero_generation_and_zero_rollback_index() {
        assert!(matches!(
            GenerationPlan::<8>::new(0, 1),
            Err(GenerationPlanError::ZeroGeneration)
        ));
        assert!(matches!(
            GenerationPlan::<8>::new(1, 0),
            Err(GenerationPlanError::ZeroRollbackIndex)
        ));
        assert_eq!(ObjectId::new([0; OBJECT_DIGEST_BYTES]), None);
    }

    #[test]
    fn rejects_duplicate_component_roles() {
        let mut plan = GenerationPlan::<8>::new(1, 1).unwrap();
        plan.push(CoreComponentKind::Kernel, object(1)).unwrap();
        assert!(matches!(
            plan.push(CoreComponentKind::Kernel, object(2)),
            Err(GenerationPlanError::DuplicateComponent(
                CoreComponentKind::Kernel
            ))
        ));
    }

    #[test]
    fn accessibility_is_part_of_boot_readiness() {
        let mut plan = complete_plan();
        plan.components[4] = None;
        plan.components[4] = plan.components[7];
        plan.len = 7;

        assert!(matches!(
            plan.boot_candidate(7),
            Err(GenerationPlanError::MissingRequiredComponent(
                CoreComponentKind::AccessibilityBroker
            ))
        ));
    }

    #[test]
    fn rejects_generation_below_rollback_floor() {
        let plan = complete_plan();
        assert!(matches!(
            plan.boot_candidate(8),
            Err(GenerationPlanError::RollbackRejected {
                declared: 7,
                minimum_allowed: 8
            })
        ));
    }

    #[test]
    fn complete_generation_can_obtain_boot_candidate() {
        let plan = complete_plan();
        let candidate = plan.boot_candidate(7).unwrap();
        assert_eq!(candidate.generation(), 42);
        assert_eq!(candidate.rollback_index(), 7);
        assert_eq!(candidate.component_count(), 8);
    }

    #[test]
    fn graphical_boot_without_speech_cannot_be_successful() {
        let plan = complete_plan();
        let candidate = plan.boot_candidate(7).unwrap();
        let mut health = complete_health();
        health.passed &= !RuntimeHealthCheck::Speech.bit();

        assert_eq!(
            candidate.successful_generation(health),
            Err(RuntimeHealthError::MissingRequiredCheck(
                RuntimeHealthCheck::Speech
            ))
        );
    }

    #[test]
    fn accessible_recovery_requires_typed_recovery_proof() {
        let mut health = RuntimeHealthReport::new();
        assert_eq!(
            health.mark_passed(RuntimeHealthCheck::AccessibleRecovery),
            Err(RuntimeHealthRecordError::RecoveryProofRequired)
        );
        assert!(!health.passed(RuntimeHealthCheck::AccessibleRecovery));
        health.mark_accessible_recovery(recovery_ready());
        assert!(health.passed(RuntimeHealthCheck::AccessibleRecovery));
    }

    #[test]
    fn inaccessible_recovery_cannot_be_marked_successful() {
        let plan = complete_plan();
        let candidate = plan.boot_candidate(7).unwrap();
        let mut health = complete_health();
        health.passed &= !RuntimeHealthCheck::AccessibleRecovery.bit();

        assert_eq!(
            candidate.successful_generation(health),
            Err(RuntimeHealthError::MissingRequiredCheck(
                RuntimeHealthCheck::AccessibleRecovery
            ))
        );
    }

    #[test]
    fn all_runtime_health_checks_produce_success_proof() {
        let plan = complete_plan();
        let candidate = plan.boot_candidate(7).unwrap();
        let successful = candidate.successful_generation(complete_health()).unwrap();
        assert_eq!(successful.generation(), 42);
        assert_eq!(successful.rollback_index(), 7);
    }
}
