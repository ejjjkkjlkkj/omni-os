//! UEFI runtime services for the kernel: the EFI Memory Attributes Table (UEFI 2.11, 4.6.4).
//!
//! The kernel runs with its own W^X page tables and calls runtime services (the health record of
//! a trial attempt is a UEFI variable) in one of two ways, chosen here:
//!
//! - `mapped`: the Memory Attributes Table splits every runtime image into read-only code
//!   (`EFI_MEMORY_RO`) and non-executable data (`EFI_MEMORY_XP`). The loader hands over the code
//!   ranges, merged, and the kernel maps them read-only and executable in its own tables.
//! - `firmware_tables` (`code_range_count == 0`): the table is missing, or lists runtime code
//!   that is also writable (images not split into sections). Such code can never enter the
//!   kernel's W^X tables, so the kernel runs each firmware call on the firmware's own page tables
//!   (still intact: the kernel only ever reuses conventional memory) and switches back after it.

use aw_kernel_core::{
    BOOT_ATTEMPT_TRIAL, FirmwareCodeRange, FirmwareRuntimeHandoff, MAX_FIRMWARE_CODE_RANGES,
    UEFI_PAGE_SIZE,
};
use uefi::{guid, system};

use crate::aw_mark;

const MEMORY_ATTRIBUTES_TABLE: uefi::Guid = guid!("dcfa911d-26eb-469f-a220-38b7dc461220");
const RUNTIME_SERVICES_CODE: u32 = 5;
const EFI_MEMORY_XP: u64 = 0x4000;
const EFI_MEMORY_RO: u64 = 0x20000;
/// Upper bound on table entries walked (firmware data).
const MAX_ENTRIES: u32 = 1024;

fn read<const N: usize>(address: usize) -> [u8; N] {
    let mut out = [0_u8; N];
    // SAFETY: callers bound `address` inside the Memory Attributes Table the firmware published.
    unsafe { core::ptr::copy_nonoverlapping(address as *const u8, out.as_mut_ptr(), N) };
    out
}

/// Read-only runtime code ranges from the Memory Attributes Table, or why they cannot be used.
fn code_ranges(
    ranges: &mut [FirmwareCodeRange; MAX_FIRMWARE_CODE_RANGES],
) -> Result<usize, &'static str> {
    let table = system::with_config_table(|tables| {
        tables
            .iter()
            .find(|entry| entry.guid == MEMORY_ATTRIBUTES_TABLE)
            .map(|entry| entry.address as usize)
    });
    let table = table
        .filter(|a| *a != 0)
        .ok_or("no_memory_attributes_table")?;
    let count = u32::from_le_bytes(read::<4>(table + 4)).min(MAX_ENTRIES);
    let stride = u32::from_le_bytes(read::<4>(table + 8)) as usize;
    if stride < 40 {
        return Err("bad_descriptor_size");
    }
    let mut used = 0_usize;
    for index in 0..count as usize {
        let entry = table + 16 + index * stride;
        let kind = u32::from_le_bytes(read::<4>(entry));
        let start = u64::from_le_bytes(read::<8>(entry + 8));
        let pages = u64::from_le_bytes(read::<8>(entry + 24));
        let attributes = u64::from_le_bytes(read::<8>(entry + 32));
        // Data (and anything not runtime code) stays RW and NX in the kernel's map.
        if kind != RUNTIME_SERVICES_CODE || attributes & EFI_MEMORY_XP != 0 {
            continue;
        }
        if attributes & EFI_MEMORY_RO == 0 {
            return Err("runtime_code_not_read_only");
        }
        let end = pages
            .checked_mul(UEFI_PAGE_SIZE)
            .and_then(|bytes| start.checked_add(bytes))
            .ok_or("range_overflow")?;
        // Merge with the previous range when contiguous (sections of one image are).
        if used > 0 && ranges[used - 1].end == start {
            ranges[used - 1].end = end;
            continue;
        }
        if used == MAX_FIRMWARE_CODE_RANGES {
            return Err("too_many_code_ranges");
        }
        ranges[used] = FirmwareCodeRange { start, end };
        used += 1;
    }
    if used == 0 {
        return Err("no_runtime_code");
    }
    Ok(used)
}

/// Builds the runtime handoff (with the trial attempt this boot runs, if any).
pub fn firmware_runtime(trial: Option<(u64, u64)>) -> Option<FirmwareRuntimeHandoff> {
    let mut handoff = FirmwareRuntimeHandoff::NONE;
    let mode = match code_ranges(&mut handoff.code_ranges) {
        Ok(count) => {
            handoff.code_range_count = count as u32;
            "mapped"
        }
        Err(reason) => {
            handoff.code_ranges = [FirmwareCodeRange::NONE; MAX_FIRMWARE_CODE_RANGES];
            aw_mark!("AW_UEFI_RUNTIME_MAT usable=false reason={reason}");
            "firmware_tables"
        }
    };
    handoff.runtime_services = uefi::table::system_table_raw()
        // SAFETY: the system table is valid until ExitBootServices; only a pointer is read.
        .map_or(0, |st| unsafe { st.as_ref().runtime_services } as u64);
    if let Some((generation, sequence)) = trial {
        handoff.boot_attempt = BOOT_ATTEMPT_TRIAL;
        handoff.trial_generation = generation;
        handoff.trial_sequence = sequence;
    }
    if !handoff.is_valid() {
        aw_mark!("AW_UEFI_RUNTIME_HANDOFF present=false reason=invalid");
        return None;
    }
    aw_mark!(
        "AW_UEFI_RUNTIME_HANDOFF present=true mode={mode} code_ranges={} trial={}",
        handoff.code_range_count,
        trial.is_some()
    );
    Some(handoff)
}
