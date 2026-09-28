#![no_std]
#![forbid(unsafe_code)]

use aw_recovery_contract::RecoveryEvent;

pub trait StructuredDiagnosticSink {
    fn emit(&mut self, event: RecoveryEvent) -> bool;
}

pub trait SpeechSink {
    fn speak(&mut self, event: RecoveryEvent) -> bool;
}

pub trait BrailleSink {
    fn present(&mut self, event: RecoveryEvent) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDeliveryError {
    StructuredDiagnosticsUnavailable,
    NoDirectNonVisualOutput,
}

/// Evidence that one exact recovery event reached structured diagnostics and at least one direct
/// nonvisual output channel. A graphical renderer is deliberately absent from this trust boundary.
///
/// The event is stored inside the proof so callers cannot reuse output evidence for a different
/// diagnostic identity, action or generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryDeliveryEvidence {
    event: RecoveryEvent,
    speech: bool,
    braille: bool,
}

impl RecoveryDeliveryEvidence {
    #[must_use]
    pub const fn event(self) -> RecoveryEvent {
        self.event
    }

    #[must_use]
    pub const fn speech_delivered(self) -> bool {
        self.speech
    }

    #[must_use]
    pub const fn braille_delivered(self) -> bool {
        self.braille
    }
}

/// Delivers the exact same structured recovery event to diagnostics, speech and braille.
///
/// Structured diagnostics are mandatory and are attempted first. Speech and braille are then both
/// attempted so a failure in one channel cannot hide a working fallback. Success requires at least
/// one direct nonvisual channel. Visual output is intentionally outside this API and can never make
/// this function succeed.
pub fn deliver_recovery_event<D, S, B>(
    event: RecoveryEvent,
    diagnostics: &mut D,
    speech: &mut S,
    braille: &mut B,
) -> Result<RecoveryDeliveryEvidence, RecoveryDeliveryError>
where
    D: StructuredDiagnosticSink,
    S: SpeechSink,
    B: BrailleSink,
{
    if !diagnostics.emit(event) {
        return Err(RecoveryDeliveryError::StructuredDiagnosticsUnavailable);
    }

    let speech_delivered = speech.speak(event);
    let braille_delivered = braille.present(event);
    if !speech_delivered && !braille_delivered {
        return Err(RecoveryDeliveryError::NoDirectNonVisualOutput);
    }

    Ok(RecoveryDeliveryEvidence {
        event,
        speech: speech_delivered,
        braille: braille_delivered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_recovery_contract::{RecoveryAction, RecoveryDiagnosticCode, RecoverySeverity};

    #[derive(Default)]
    struct Recorder {
        accepted: bool,
        seen: Option<RecoveryEvent>,
    }

    impl Recorder {
        fn accepting() -> Self {
            Self {
                accepted: true,
                seen: None,
            }
        }
    }

    impl StructuredDiagnosticSink for Recorder {
        fn emit(&mut self, event: RecoveryEvent) -> bool {
            self.seen = Some(event);
            self.accepted
        }
    }

    impl SpeechSink for Recorder {
        fn speak(&mut self, event: RecoveryEvent) -> bool {
            self.seen = Some(event);
            self.accepted
        }
    }

    impl BrailleSink for Recorder {
        fn present(&mut self, event: RecoveryEvent) -> bool {
            self.seen = Some(event);
            self.accepted
        }
    }

    fn event() -> RecoveryEvent {
        RecoveryEvent::new(
            RecoveryDiagnosticCode::SpeechUnavailable,
            RecoverySeverity::Critical,
            RecoveryAction::BootPreviousGeneration,
            Some(42),
        )
    }

    #[test]
    fn speech_success_produces_nonvisual_evidence() {
        let expected = event();
        let mut diagnostics = Recorder::accepting();
        let mut speech = Recorder::accepting();
        let mut braille = Recorder::default();

        let evidence =
            deliver_recovery_event(expected, &mut diagnostics, &mut speech, &mut braille).unwrap();

        assert_eq!(evidence.event(), expected);
        assert!(evidence.speech_delivered());
        assert!(!evidence.braille_delivered());
        assert_eq!(diagnostics.seen, Some(expected));
        assert_eq!(speech.seen, Some(expected));
        assert_eq!(braille.seen, Some(expected));
    }

    #[test]
    fn braille_is_a_real_fallback_when_speech_fails() {
        let expected = event();
        let mut diagnostics = Recorder::accepting();
        let mut speech = Recorder::default();
        let mut braille = Recorder::accepting();

        let evidence =
            deliver_recovery_event(expected, &mut diagnostics, &mut speech, &mut braille).unwrap();

        assert_eq!(evidence.event(), expected);
        assert!(!evidence.speech_delivered());
        assert!(evidence.braille_delivered());
        assert_eq!(diagnostics.seen, Some(expected));
        assert_eq!(speech.seen, Some(expected));
        assert_eq!(braille.seen, Some(expected));
    }

    #[test]
    fn both_nonvisual_channels_missing_fails_closed() {
        let mut diagnostics = Recorder::accepting();
        let mut speech = Recorder::default();
        let mut braille = Recorder::default();

        assert_eq!(
            deliver_recovery_event(event(), &mut diagnostics, &mut speech, &mut braille),
            Err(RecoveryDeliveryError::NoDirectNonVisualOutput)
        );
    }

    #[test]
    fn structured_diagnostics_failure_stops_success_before_output_claim() {
        let mut diagnostics = Recorder::default();
        let mut speech = Recorder::accepting();
        let mut braille = Recorder::accepting();

        assert_eq!(
            deliver_recovery_event(event(), &mut diagnostics, &mut speech, &mut braille),
            Err(RecoveryDeliveryError::StructuredDiagnosticsUnavailable)
        );
        assert_eq!(speech.seen, None);
        assert_eq!(braille.seen, None);
    }

    #[test]
    fn every_sink_receives_identical_event_identity_and_action() {
        let expected = event();
        let mut diagnostics = Recorder::accepting();
        let mut speech = Recorder::accepting();
        let mut braille = Recorder::accepting();

        let evidence =
            deliver_recovery_event(expected, &mut diagnostics, &mut speech, &mut braille).unwrap();

        assert_eq!(evidence.event(), expected);
        assert!(evidence.speech_delivered());
        assert!(evidence.braille_delivered());
        for seen in [diagnostics.seen, speech.seen, braille.seen] {
            assert_eq!(seen, Some(expected));
        }
    }
}
