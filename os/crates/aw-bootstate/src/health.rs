//! Runtime-health record: how a trial generation's kernel tells the loader it is healthy.
//!
//! The kernel runs its checks during a trial attempt and stores this 32-byte record in the UEFI
//! variable `OmniHealth` (runtime services, UEFI 2.11 chapter 8). On the next boot the loader
//! reads and deletes the variable, and promotes the trial only when the record is exactly valid,
//! names the very attempt that was persisted before hand-off (generation and boot-state
//! sequence), reports every kernel-owned check as passed, and the loader's own checks pass too:
//! an accessible Recovery Core and an intact rollback target. Anything else leaves the attempt
//! unpromoted, so it falls back to the known-good generation: the fail-safe direction.

use aw_generation::{
    CoreComponentKind, GenerationPlan, ObjectId, REQUIRED_BOOT_COMPONENTS, RuntimeHealthCheck,
    RuntimeHealthReport,
};
use aw_recovery_contract::AccessibleRecoveryReady;

use crate::record::crc32;
use crate::{BootSelectionState, BootStateError, BootStateRecord};

/// UEFI variable holding the record: `OmniHealth` under the omni-os vendor GUID
/// `7c1e9a52-3b8d-4f0e-9a61-0d5c2e8b4f13` (bytes in EFI_GUID memory layout), non-volatile and
/// accessible at boot and run time.
pub const HEALTH_VARIABLE_NAME: &str = "OmniHealth";
pub const HEALTH_VENDOR_GUID: [u8; 16] = [
    0x52, 0x9a, 0x1e, 0x7c, 0x8d, 0x3b, 0x0e, 0x4f, 0x9a, 0x61, 0x0d, 0x5c, 0x2e, 0x8b, 0x4f, 0x13,
];
/// EFI_VARIABLE_NON_VOLATILE | BOOTSERVICE_ACCESS | RUNTIME_ACCESS.
pub const HEALTH_VARIABLE_ATTRIBUTES: u32 = 0x7;

/// Size of the encoded record.
pub const HEALTH_RECORD_BYTES: usize = 32;
const HEALTH_MAGIC: [u8; 8] = *b"OMNIHLT\x01";

/// Checks the kernel measures itself. Accessible recovery and update readiness are proven by the
/// loader, which owns the Recovery Core and the rollback target; a kernel claiming them is
/// rejected.
pub const KERNEL_HEALTH_CHECKS: [RuntimeHealthCheck; 7] = [
    RuntimeHealthCheck::Kernel,
    RuntimeHealthCheck::Storage,
    RuntimeHealthCheck::Input,
    RuntimeHealthCheck::Audio,
    RuntimeHealthCheck::AccessibilityBroker,
    RuntimeHealthCheck::Speech,
    RuntimeHealthCheck::Security,
];

const LOADER_OWNED: u16 =
    RuntimeHealthCheck::AccessibleRecovery.bit() | RuntimeHealthCheck::Update.bit();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthRecord {
    generation: u64,
    sequence: u64,
    passed: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthDecodeError {
    BadLength,
    BadMagic,
    BadChecksum,
    NonZeroReserved,
    ZeroGeneration,
    ClaimsLoaderCheck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromotionError {
    /// The persisted record is not a prepared trial attempt.
    NotAnAttempt,
    /// The health record belongs to another generation or another attempt.
    WrongAttempt {
        generation: u64,
        sequence: u64,
    },
    /// A kernel-owned check did not pass.
    MissingKernelCheck(RuntimeHealthCheck),
    /// The rollback target could not be verified by the loader.
    RollbackTargetUnverified,
    Plan,
    Health,
    State(BootStateError),
}

impl HealthRecord {
    #[must_use]
    pub const fn new(generation: u64, sequence: u64, passed: u16) -> Self {
        Self {
            generation,
            sequence,
            passed: passed & !LOADER_OWNED,
        }
    }

    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub const fn passed(self, check: RuntimeHealthCheck) -> bool {
        self.passed & check.bit() != 0
    }

    #[must_use]
    pub const fn passed_mask(self) -> u16 {
        self.passed
    }

    /// | bytes | field |
    /// |---|---|
    /// | 0..8 | magic `OMNIHLT\x01` |
    /// | 8..16 | generation |
    /// | 16..24 | boot-state sequence of the attempt |
    /// | 24..26 | passed checks (`RuntimeHealthCheck::bit`) |
    /// | 26..28 | reserved, zero |
    /// | 28..32 | CRC-32 of bytes 0..28 |
    #[must_use]
    pub fn encode(self) -> [u8; HEALTH_RECORD_BYTES] {
        let mut out = [0_u8; HEALTH_RECORD_BYTES];
        out[..8].copy_from_slice(&HEALTH_MAGIC);
        out[8..16].copy_from_slice(&self.generation.to_le_bytes());
        out[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        out[24..26].copy_from_slice(&self.passed.to_le_bytes());
        let crc = crc32(&out[..28]);
        out[28..].copy_from_slice(&crc.to_le_bytes());
        out
    }

    pub fn decode(data: &[u8]) -> Result<Self, HealthDecodeError> {
        if data.len() != HEALTH_RECORD_BYTES {
            return Err(HealthDecodeError::BadLength);
        }
        if data[..8] != HEALTH_MAGIC {
            return Err(HealthDecodeError::BadMagic);
        }
        let stored = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
        if crc32(&data[..28]) != stored {
            return Err(HealthDecodeError::BadChecksum);
        }
        if data[26] != 0 || data[27] != 0 {
            return Err(HealthDecodeError::NonZeroReserved);
        }
        let mut word = [0_u8; 8];
        word.copy_from_slice(&data[8..16]);
        let generation = u64::from_le_bytes(word);
        word.copy_from_slice(&data[16..24]);
        let sequence = u64::from_le_bytes(word);
        let passed = u16::from_le_bytes([data[24], data[25]]);
        if generation == 0 {
            return Err(HealthDecodeError::ZeroGeneration);
        }
        if passed & LOADER_OWNED != 0 {
            return Err(HealthDecodeError::ClaimsLoaderCheck);
        }
        Ok(Self {
            generation,
            sequence,
            passed,
        })
    }
}

/// Promotes the persisted trial attempt to known-good, or says exactly why not.
///
/// `recovery` proves the loader's Recovery Core is usable without sight; `rollback_verified`
/// states that the known-good generation's image still matches its recorded digest, so the update
/// path can always return to it. omni-os generations are monolithic: every core component is
/// served by the one kernel image, so each component of the plan is that image's digest.
pub fn promote_trial(
    record: BootStateRecord,
    health: HealthRecord,
    recovery: AccessibleRecoveryReady,
    rollback_verified: bool,
) -> Result<BootStateRecord, PromotionError> {
    let BootSelectionState::TrialAttempt { .. } = record.state() else {
        return Err(PromotionError::NotAnAttempt);
    };
    if health.generation != record.selected().generation() || health.sequence != record.sequence() {
        return Err(PromotionError::WrongAttempt {
            generation: health.generation,
            sequence: health.sequence,
        });
    }
    let mut report = RuntimeHealthReport::new();
    for check in KERNEL_HEALTH_CHECKS {
        if !health.passed(check) {
            return Err(PromotionError::MissingKernelCheck(check));
        }
        report
            .mark_passed(check)
            .map_err(|_| PromotionError::Health)?;
    }
    if !rollback_verified {
        return Err(PromotionError::RollbackTargetUnverified);
    }
    report
        .mark_passed(RuntimeHealthCheck::Update)
        .map_err(|_| PromotionError::Health)?;
    report.mark_accessible_recovery(recovery);

    let image: ObjectId = record.selected().manifest();
    let mut plan =
        GenerationPlan::<8>::new(record.selected().generation(), record.rollback_floor())
            .map_err(|_| PromotionError::Plan)?;
    for kind in REQUIRED_BOOT_COMPONENTS {
        plan.push(kind, image).map_err(|_| PromotionError::Plan)?;
    }
    debug_assert!(plan.contains(CoreComponentKind::Kernel));
    let candidate = plan
        .boot_candidate(record.rollback_floor())
        .map_err(|_| PromotionError::Plan)?;
    let healthy = candidate
        .successful_generation(report)
        .map_err(|_| PromotionError::Health)?;
    record
        .after_successful_trial(healthy)
        .map_err(PromotionError::State)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GenerationLocator;
    use aw_recovery_contract::{RecoveryCapability, RecoveryProbeReport};

    fn locator(generation: u64, seed: u8) -> GenerationLocator {
        GenerationLocator::new(generation, ObjectId::new([seed; 32]).unwrap()).unwrap()
    }

    fn attempt() -> BootStateRecord {
        BootStateRecord::new(
            6,
            locator(2, 2),
            locator(1, 1),
            1,
            BootSelectionState::TrialAttempt { tries_remaining: 1 },
        )
        .unwrap()
    }

    fn all_kernel_checks() -> u16 {
        KERNEL_HEALTH_CHECKS.iter().fold(0, |m, c| m | c.bit())
    }

    fn recovery() -> AccessibleRecoveryReady {
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

    #[test]
    fn round_trips_and_rejects_corruption() {
        let record = HealthRecord::new(2, 6, all_kernel_checks());
        let bytes = record.encode();
        assert_eq!(HealthRecord::decode(&bytes), Ok(record));
        for index in 0..HEALTH_RECORD_BYTES {
            let mut bad = bytes;
            bad[index] ^= 0x10;
            assert!(HealthRecord::decode(&bad).is_err(), "byte {index}");
        }
        assert_eq!(
            HealthRecord::decode(&bytes[..31]),
            Err(HealthDecodeError::BadLength)
        );
    }

    #[test]
    fn kernel_cannot_claim_loader_checks() {
        let record = HealthRecord::new(2, 6, 0xffff);
        assert!(!record.passed(RuntimeHealthCheck::AccessibleRecovery));
        assert!(!record.passed(RuntimeHealthCheck::Update));
        let mut forged = record.encode();
        forged[24..26].copy_from_slice(&0xffff_u16.to_le_bytes());
        let crc = crc32(&forged[..28]);
        forged[28..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(
            HealthRecord::decode(&forged),
            Err(HealthDecodeError::ClaimsLoaderCheck)
        );
    }

    #[test]
    fn promotes_the_exact_attempt_when_everything_passed() {
        let promoted = promote_trial(
            attempt(),
            HealthRecord::new(2, 6, all_kernel_checks()),
            recovery(),
            true,
        )
        .unwrap();
        assert_eq!(promoted.state(), BootSelectionState::Successful);
        assert_eq!(promoted.previous_successful().generation(), 2);
        assert_eq!(promoted.sequence(), 7);
    }

    #[test]
    fn refuses_stale_or_foreign_or_incomplete_health() {
        let checks = all_kernel_checks();
        assert!(matches!(
            promote_trial(attempt(), HealthRecord::new(2, 5, checks), recovery(), true),
            Err(PromotionError::WrongAttempt { .. })
        ));
        assert!(matches!(
            promote_trial(attempt(), HealthRecord::new(3, 6, checks), recovery(), true),
            Err(PromotionError::WrongAttempt { .. })
        ));
        let no_speech = checks & !RuntimeHealthCheck::Speech.bit();
        assert_eq!(
            promote_trial(
                attempt(),
                HealthRecord::new(2, 6, no_speech),
                recovery(),
                true
            ),
            Err(PromotionError::MissingKernelCheck(
                RuntimeHealthCheck::Speech
            ))
        );
        assert_eq!(
            promote_trial(
                attempt(),
                HealthRecord::new(2, 6, checks),
                recovery(),
                false
            ),
            Err(PromotionError::RollbackTargetUnverified)
        );
        let successful = BootStateRecord::new(
            6,
            locator(2, 2),
            locator(1, 1),
            1,
            BootSelectionState::Trial { tries_remaining: 2 },
        )
        .unwrap();
        assert_eq!(
            promote_trial(
                successful,
                HealthRecord::new(2, 6, checks),
                recovery(),
                true
            ),
            Err(PromotionError::NotAnAttempt)
        );
    }
}
