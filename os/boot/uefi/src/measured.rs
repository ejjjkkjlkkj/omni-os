//! Boot integrity IDS (NIST SP 800-155, SP 800-193 "detect"): replays the firmware's TCG event
//! log, compares it with the TPM's own PCR 0-7, compares those with the previous boot's baseline,
//! and announces aloud any discrepancy - the measured-boot state a blind owner could otherwise
//! never check. Then it measures the kernel it is about to run into PCR 9 (extending the chain of
//! measurements past the loader, like other measured-boot loaders do). Read-only towards the
//! firmware; only `\OMNI\PCR.REF` and `\OMNI\IDS.LOG` are written on the ESP.

use alloc::format;
use alloc::string::String;

use aw_measured::{
    PCRS, Replay, changed, decode_baseline, encode_baseline, mask_list, mismatches, parse_pcr_read,
    pcr_read_command,
};
use uefi::boot;
use uefi::proto::media::file::Directory;
use uefi::proto::tcg::v2::{HashLogExtendEventFlags, PcrEventInputs, Tcg};
use uefi::proto::tcg::{AlgorithmId, EventType, PcrIndex};

use crate::aw_mark;
use crate::recovery::{read_file, write_file};
use crate::setup::{Lang, speak_dynamic};

const BASELINE: &str = "OMNI\\PCR.REF";
const LOG: &str = "OMNI\\IDS.LOG";
/// PCR the loader measures the kernel image into.
const KERNEL_PCR: u32 = 9;

fn open_tcg2() -> Option<boot::ScopedProtocol<Tcg>> {
    let handle = boot::get_handle_for_protocol::<Tcg>().ok()?;
    let mut tcg = boot::open_protocol_exclusive::<Tcg>(handle).ok()?;
    let present = tcg.get_capability().ok()?.tpm_present();
    present.then_some(tcg)
}

/// Check the measured boot, announce any discrepancy, keep the baseline up to date.
pub fn check(root: &mut Directory) {
    let Some(mut tcg) = open_tcg2() else {
        aw_mark!("AW_UEFI_MEASURED tpm=absent");
        return;
    };

    let mut replay = Replay::new();
    let mut events = 0_u32;
    let truncated;
    match tcg.get_event_log_v2() {
        Ok(log) => {
            truncated = log.is_truncated();
            for event in log.iter() {
                events += 1;
                let sha256 = event
                    .digests()
                    .into_iter()
                    .find(|(alg, _)| *alg == AlgorithmId::SHA256)
                    .map(|(_, digest)| digest);
                replay.event(
                    event.pcr_index().0,
                    event.event_type().0,
                    sha256,
                    event.event_data(),
                );
            }
        }
        Err(error) => {
            aw_mark!(
                "AW_UEFI_MEASURED tpm=present log=unavailable status={:?}",
                error.status()
            );
            return;
        }
    }

    let mut response = [0_u8; 512];
    let tpm = match tcg.submit_command(&pcr_read_command(), &mut response) {
        Ok(()) => parse_pcr_read(&response),
        Err(error) => {
            aw_mark!(
                "AW_UEFI_MEASURED tpm=present pcr_read=failed status={:?}",
                error.status()
            );
            return;
        }
    };
    let tpm = match tpm {
        Ok(tpm) => tpm,
        Err(error) => {
            aw_mark!("AW_UEFI_MEASURED tpm=present pcr_read=bad error={error:?}");
            return;
        }
    };

    let bad = mismatches(&replay.pcrs, &tpm);
    let complete = !truncated && replay.missing_sha256 == 0;
    let mut current = replay.pcrs;
    for (i, value) in tpm.iter().enumerate().take(PCRS) {
        if let Some(value) = value {
            current[i] = *value;
        }
    }
    let previous = read_file(root, BASELINE).and_then(|data| decode_baseline(&data));
    let (baseline, moved) = match previous {
        None => ("created", 0),
        Some(old) => {
            let moved = changed(&old, &current);
            (if moved == 0 { "same" } else { "changed" }, moved)
        }
    };
    let written = moved != 0 || previous.is_none();
    if written {
        write_file(root, BASELINE, &encode_baseline(&current));
    }

    let (mut b1, mut b2) = ([0_u8; 16], [0_u8; 16]);
    let bad_list = mask_list(bad, &mut b1);
    let moved_list = mask_list(moved, &mut b2);
    aw_mark!(
        "AW_UEFI_MEASURED tpm=present events={events} extended={} complete={complete} replay={} mismatched={bad_list} baseline={baseline} changed={moved_list}",
        replay.extended,
        if bad == 0 { "match" } else { "mismatch" }
    );
    for (i, value) in current.iter().enumerate() {
        aw_mark!("AW_UEFI_MEASURED_PCR index={i} sha256={}", hex(value));
    }

    // Speak only when there is something to act on: a log that does not replay to the TPM, or a
    // platform measurement that changed since the previous boot.
    let mut alerts = String::new();
    if bad != 0 {
        alerts.push_str(&format!(
            "Alerte d'intégrité. Le journal de démarrage mesuré ne correspond pas au module TPM, registres {}. ",
            spoken(bad_list)
        ));
    }
    if moved != 0 {
        alerts.push_str(&format!(
            "Attention. La configuration mesurée du démarrage a changé depuis le démarrage précédent, registres {}. {}",
            spoken(moved_list),
            meaning(moved)
        ));
    }
    if !alerts.is_empty() {
        let log = format!(
            "replay={} mismatched={bad_list} changed={moved_list}\n",
            if bad == 0 { "match" } else { "mismatch" }
        );
        write_file(root, LOG, log.as_bytes());
        aw_mark!("AW_UEFI_MEASURED_ALERT mismatched={bad_list} changed={moved_list}");
        let mut speaker = crate::audio::bring_up();
        let mut pending = None;
        speak_dynamic(&alerts, Lang::Fr, &mut speaker, &mut pending);
    }
}

/// What a changed PCR usually means, for the spoken alert.
fn meaning(mask: u8) -> &'static str {
    if mask & 0b0000_0011 != 0 {
        "Le micrologiciel ou ses réglages ont été modifiés."
    } else if mask & 0b1000_0000 != 0 {
        "La politique de démarrage sécurisé a été modifiée."
    } else if mask & 0b0001_0000 != 0 {
        "Le chargeur de démarrage a été modifié."
    } else {
        "Un élément mesuré du démarrage a été modifié."
    }
}

fn spoken(list: &str) -> String {
    list.replace(',', ", ")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Measure the kernel image into PCR 9 and the event log, before running it.
pub fn measure_kernel(image: &[u8]) {
    let Some(mut tcg) = open_tcg2() else {
        return;
    };
    let Ok(event) = PcrEventInputs::new_in_box(
        PcrIndex(KERNEL_PCR),
        EventType::IPL,
        b"omni-os kernel image",
    ) else {
        return;
    };
    let ok = tcg
        .hash_log_extend_event(HashLogExtendEventFlags::empty(), image, &event)
        .is_ok();
    aw_mark!(
        "AW_UEFI_MEASURED_KERNEL pcr={KERNEL_PCR} bytes={} ok={ok}",
        image.len()
    );
}
