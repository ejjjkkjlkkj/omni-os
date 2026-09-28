#![no_std]
#![forbid(unsafe_code)]

use aw_generation::{ObjectId, SuccessfulGeneration};

pub const MAX_TRIAL_BOOT_ATTEMPTS: u8 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenerationLocator {
    generation: u64,
    manifest: ObjectId,
}

impl GenerationLocator {
    #[must_use]
    pub const fn new(generation: u64, manifest: ObjectId) -> Option<Self> {
        if generation == 0 {
            return None;
        }
        Some(Self {
            generation,
            manifest,
        })
    }

    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn manifest(self) -> ObjectId {
        self.manifest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootSelectionState {
    /// A trial generation is eligible for another boot attempt.
    Trial {
        tries_remaining: u8,
    },
    /// One attempt has already been consumed and this exact state must be persisted before control
    /// is transferred to the trial generation. `tries_remaining` is the number of future attempts
    /// that remain if the current attempt does not become healthy.
    TrialAttempt {
        tries_remaining: u8,
    },
    Successful,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootStateRecord {
    sequence: u64,
    selected: GenerationLocator,
    previous_successful: GenerationLocator,
    rollback_floor: u64,
    state: BootSelectionState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootStateError {
    ZeroSequence,
    ZeroRollbackFloor,
    InvalidTrialAttempts,
    NoUsableRecord,
    ConflictingSequence,
    SequenceOverflow,
    NotTrial,
    NotPreparedTrial,
    HealthGenerationMismatch { selected: u64, healthy: u64 },
    HealthRollbackRejected { declared: u64, minimum: u64 },
}

impl BootStateRecord {
    pub fn new(
        sequence: u64,
        selected: GenerationLocator,
        previous_successful: GenerationLocator,
        rollback_floor: u64,
        state: BootSelectionState,
    ) -> Result<Self, BootStateError> {
        if sequence == 0 {
            return Err(BootStateError::ZeroSequence);
        }
        if rollback_floor == 0 {
            return Err(BootStateError::ZeroRollbackFloor);
        }
        match state {
            BootSelectionState::Trial { tries_remaining }
                if tries_remaining == 0 || tries_remaining > MAX_TRIAL_BOOT_ATTEMPTS =>
            {
                return Err(BootStateError::InvalidTrialAttempts);
            }
            BootSelectionState::TrialAttempt { tries_remaining }
                if tries_remaining >= MAX_TRIAL_BOOT_ATTEMPTS =>
            {
                return Err(BootStateError::InvalidTrialAttempts);
            }
            _ => {}
        }

        Ok(Self {
            sequence,
            selected,
            previous_successful,
            rollback_floor,
            state,
        })
    }

    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub const fn selected(self) -> GenerationLocator {
        self.selected
    }

    #[must_use]
    pub const fn previous_successful(self) -> GenerationLocator {
        self.previous_successful
    }

    #[must_use]
    pub const fn rollback_floor(self) -> u64 {
        self.rollback_floor
    }

    #[must_use]
    pub const fn state(self) -> BootSelectionState {
        self.state
    }

    /// Consumes one trial attempt before transferring control to the selected generation.
    ///
    /// Callers must durably persist the returned record before entering the trial generation. This
    /// makes sudden power loss count as a failed attempt instead of allowing an unbounded reboot
    /// loop on a broken generation.
    pub fn prepare_trial_boot(self) -> Result<Self, BootStateError> {
        let BootSelectionState::Trial { tries_remaining } = self.state else {
            return Err(BootStateError::NotTrial);
        };
        let next_sequence = self
            .sequence
            .checked_add(1)
            .ok_or(BootStateError::SequenceOverflow)?;

        Ok(Self {
            sequence: next_sequence,
            selected: self.selected,
            previous_successful: self.previous_successful,
            rollback_floor: self.rollback_floor,
            state: BootSelectionState::TrialAttempt {
                tries_remaining: tries_remaining - 1,
            },
        })
    }

    /// Records that the already-consumed trial attempt failed.
    ///
    /// If future attempts remain, the generation returns to `Trial`. If the consumed attempt was
    /// the final one, selection atomically falls back to the previous known-good generation.
    pub fn after_failed_trial(self) -> Result<Self, BootStateError> {
        let BootSelectionState::TrialAttempt { tries_remaining } = self.state else {
            return Err(BootStateError::NotPreparedTrial);
        };
        let next_sequence = self
            .sequence
            .checked_add(1)
            .ok_or(BootStateError::SequenceOverflow)?;

        if tries_remaining > 0 {
            return Ok(Self {
                sequence: next_sequence,
                selected: self.selected,
                previous_successful: self.previous_successful,
                rollback_floor: self.rollback_floor,
                state: BootSelectionState::Trial { tries_remaining },
            });
        }

        Ok(Self {
            sequence: next_sequence,
            selected: self.previous_successful,
            previous_successful: self.previous_successful,
            rollback_floor: self.rollback_floor,
            state: BootSelectionState::Successful,
        })
    }

    /// Treats an interrupted prepared attempt exactly like a reported failure.
    ///
    /// This is the boot-time path used after reset or power loss when the persisted record still
    /// says `TrialAttempt`: the prior attempt was consumed before transfer but never promoted.
    pub fn after_interrupted_trial(self) -> Result<Self, BootStateError> {
        self.after_failed_trial()
    }

    /// Marks a prepared trial generation successful only with a runtime-health proof produced by
    /// `aw-generation` after all mandatory checks passed, including speech and accessible recovery.
    ///
    /// Requiring `TrialAttempt` proves that an attempt was consumed and persisted before the
    /// generation ran. This prevents power loss from bypassing attempt accounting and prevents a
    /// graphical-only boot from becoming the new known-good generation.
    pub fn after_successful_trial(
        self,
        healthy: SuccessfulGeneration,
    ) -> Result<Self, BootStateError> {
        let BootSelectionState::TrialAttempt { .. } = self.state else {
            return Err(BootStateError::NotPreparedTrial);
        };
        if healthy.generation() != self.selected.generation() {
            return Err(BootStateError::HealthGenerationMismatch {
                selected: self.selected.generation(),
                healthy: healthy.generation(),
            });
        }
        if healthy.rollback_index() < self.rollback_floor {
            return Err(BootStateError::HealthRollbackRejected {
                declared: healthy.rollback_index(),
                minimum: self.rollback_floor,
            });
        }

        let next_sequence = self
            .sequence
            .checked_add(1)
            .ok_or(BootStateError::SequenceOverflow)?;
        Ok(Self {
            sequence: next_sequence,
            selected: self.selected,
            previous_successful: self.selected,
            rollback_floor: self.rollback_floor.max(healthy.rollback_index()),
            state: BootSelectionState::Successful,
        })
    }
}

/// Selects the newest usable copy from two independently validated boot-state records.
///
/// Checksum/authentication validation is intentionally outside this function. Callers must pass
/// only records that already passed their on-disk integrity/authentication checks. Equal sequence
/// numbers with different contents are treated as an ambiguous split-brain condition and fail
/// closed.
pub fn select_newest_record(
    first: Option<BootStateRecord>,
    second: Option<BootStateRecord>,
) -> Result<BootStateRecord, BootStateError> {
    match (first, second) {
        (None, None) => Err(BootStateError::NoUsableRecord),
        (Some(record), None) | (None, Some(record)) => Ok(record),
        (Some(first), Some(second)) => {
            if first.sequence() > second.sequence() {
                Ok(first)
            } else if second.sequence() > first.sequence() {
                Ok(second)
            } else if first == second {
                Ok(first)
            } else {
                Err(BootStateError::ConflictingSequence)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_generation::{
        GenerationPlan, REQUIRED_BOOT_COMPONENTS, REQUIRED_SUCCESS_HEALTH_CHECKS,
        RuntimeHealthCheck, RuntimeHealthReport,
    };
    use aw_recovery_contract::{AccessibleRecoveryReady, RecoveryCapability, RecoveryProbeReport};

    fn object(seed: u8) -> ObjectId {
        ObjectId::new([seed; 32]).unwrap()
    }

    fn locator(generation: u64, seed: u8) -> GenerationLocator {
        GenerationLocator::new(generation, object(seed)).unwrap()
    }

    fn trial(sequence: u64, tries_remaining: u8) -> BootStateRecord {
        BootStateRecord::new(
            sequence,
            locator(42, 1),
            locator(41, 2),
            7,
            BootSelectionState::Trial { tries_remaining },
        )
        .unwrap()
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

    fn healthy_generation(generation: u64, rollback_index: u64) -> SuccessfulGeneration {
        let mut plan = GenerationPlan::<8>::new(generation, rollback_index).unwrap();
        for (index, kind) in REQUIRED_BOOT_COMPONENTS.into_iter().enumerate() {
            plan.push(kind, object((index + 10) as u8)).unwrap();
        }
        let candidate = plan.boot_candidate(rollback_index).unwrap();
        let mut health = RuntimeHealthReport::new();
        for check in REQUIRED_SUCCESS_HEALTH_CHECKS {
            if check == RuntimeHealthCheck::AccessibleRecovery {
                health.mark_accessible_recovery(recovery_ready());
            } else {
                health.mark_passed(check).unwrap();
            }
        }
        candidate.successful_generation(health).unwrap()
    }

    #[test]
    fn rejects_invalid_record_shape() {
        assert!(matches!(
            BootStateRecord::new(
                0,
                locator(42, 1),
                locator(41, 2),
                7,
                BootSelectionState::Successful
            ),
            Err(BootStateError::ZeroSequence)
        ));
        assert!(matches!(
            BootStateRecord::new(
                1,
                locator(42, 1),
                locator(41, 2),
                0,
                BootSelectionState::Successful
            ),
            Err(BootStateError::ZeroRollbackFloor)
        ));
        assert!(matches!(
            BootStateRecord::new(
                1,
                locator(42, 1),
                locator(41, 2),
                7,
                BootSelectionState::Trial { tries_remaining: 0 }
            ),
            Err(BootStateError::InvalidTrialAttempts)
        ));
        assert!(matches!(
            BootStateRecord::new(
                1,
                locator(42, 1),
                locator(41, 2),
                7,
                BootSelectionState::TrialAttempt {
                    tries_remaining: MAX_TRIAL_BOOT_ATTEMPTS
                }
            ),
            Err(BootStateError::InvalidTrialAttempts)
        ));
        assert!(GenerationLocator::new(0, object(1)).is_none());
    }

    #[test]
    fn redundant_selection_uses_highest_sequence() {
        let selected = select_newest_record(Some(trial(10, 3)), Some(trial(11, 2))).unwrap();
        assert_eq!(selected.sequence(), 11);
        assert_eq!(
            selected.state(),
            BootSelectionState::Trial { tries_remaining: 2 }
        );
    }

    #[test]
    fn equal_sequence_conflict_fails_closed() {
        assert!(matches!(
            select_newest_record(Some(trial(10, 3)), Some(trial(10, 2))),
            Err(BootStateError::ConflictingSequence)
        ));
    }

    #[test]
    fn preparing_trial_consumes_attempt_before_transfer() {
        let prepared = trial(10, 3).prepare_trial_boot().unwrap();
        assert_eq!(prepared.sequence(), 11);
        assert_eq!(prepared.selected().generation(), 42);
        assert_eq!(
            prepared.state(),
            BootSelectionState::TrialAttempt { tries_remaining: 2 }
        );
    }

    #[test]
    fn failed_prepared_trial_returns_to_retryable_trial() {
        let next = trial(10, 3)
            .prepare_trial_boot()
            .unwrap()
            .after_failed_trial()
            .unwrap();
        assert_eq!(next.sequence(), 12);
        assert_eq!(next.selected().generation(), 42);
        assert_eq!(
            next.state(),
            BootSelectionState::Trial { tries_remaining: 2 }
        );
    }

    #[test]
    fn interrupted_prepared_trial_consumes_attempt_after_power_loss() {
        let persisted_before_transfer = trial(10, 2).prepare_trial_boot().unwrap();
        assert_eq!(
            persisted_before_transfer.state(),
            BootSelectionState::TrialAttempt { tries_remaining: 1 }
        );

        let recovered_on_next_boot = persisted_before_transfer.after_interrupted_trial().unwrap();
        assert_eq!(recovered_on_next_boot.sequence(), 12);
        assert_eq!(
            recovered_on_next_boot.state(),
            BootSelectionState::Trial { tries_remaining: 1 }
        );
    }

    #[test]
    fn exhausted_prepared_trial_rolls_back_to_last_successful_generation() {
        let prepared = trial(10, 1).prepare_trial_boot().unwrap();
        assert_eq!(
            prepared.state(),
            BootSelectionState::TrialAttempt { tries_remaining: 0 }
        );

        let next = prepared.after_interrupted_trial().unwrap();
        assert_eq!(next.sequence(), 12);
        assert_eq!(next.selected().generation(), 41);
        assert_eq!(next.previous_successful().generation(), 41);
        assert_eq!(next.state(), BootSelectionState::Successful);
    }

    #[test]
    fn successful_record_cannot_prepare_trial_attempt() {
        let record = BootStateRecord::new(
            10,
            locator(41, 2),
            locator(41, 2),
            7,
            BootSelectionState::Successful,
        )
        .unwrap();
        assert!(matches!(
            record.prepare_trial_boot(),
            Err(BootStateError::NotTrial)
        ));
    }

    #[test]
    fn unprepared_trial_cannot_be_promoted() {
        assert!(matches!(
            trial(10, 3).after_successful_trial(healthy_generation(42, 7)),
            Err(BootStateError::NotPreparedTrial)
        ));
    }

    #[test]
    fn trial_success_requires_matching_accessibility_health_proof() {
        let next = trial(10, 3)
            .prepare_trial_boot()
            .unwrap()
            .after_successful_trial(healthy_generation(42, 7))
            .unwrap();
        assert_eq!(next.sequence(), 12);
        assert_eq!(next.selected().generation(), 42);
        assert_eq!(next.previous_successful().generation(), 42);
        assert_eq!(next.state(), BootSelectionState::Successful);
        assert_eq!(next.rollback_floor(), 7);
    }

    #[test]
    fn health_proof_for_another_generation_is_rejected() {
        assert_eq!(
            trial(10, 3)
                .prepare_trial_boot()
                .unwrap()
                .after_successful_trial(healthy_generation(43, 7)),
            Err(BootStateError::HealthGenerationMismatch {
                selected: 42,
                healthy: 43,
            })
        );
    }

    #[test]
    fn health_proof_below_persisted_rollback_floor_is_rejected() {
        assert_eq!(
            trial(10, 3)
                .prepare_trial_boot()
                .unwrap()
                .after_successful_trial(healthy_generation(42, 6)),
            Err(BootStateError::HealthRollbackRejected {
                declared: 6,
                minimum: 7,
            })
        );
    }
}
