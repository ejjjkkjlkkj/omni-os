//! Platform services of UEFI 2.11 that a loader can use without changing the firmware, read-only:
//! - `EFI_RNG_PROTOCOL`: hardware entropy is available and returns data;
//! - the EFI System Resource Table (ESRT): the updatable firmware components, their
//!   versions and the result of the last update attempt;
//! - the recovery load options of the boot manager: `PlatformRecovery####` set by
//!   the platform, `OsRecoveryOrder`/`OsRecovery####` set by an operating system.
//!
//! Each finding is a marker; nothing is written.

use alloc::string::ToString;

use uefi::proto::rng::Rng;
use uefi::runtime::{self, VariableVendor};
use uefi::table::cfg::ConfigTableEntry;
use uefi::{boot, system};

use crate::aw_mark;

/// Upper bound on ESRT entries read (the table is firmware data; bound what is walked).
const MAX_ESRT_ENTRIES: u32 = 64;
const ESRT_HEADER_BYTES: usize = 16;
const ESRT_ENTRY_BYTES: usize = 40;

pub fn report() {
    inventory();
    rng();
    esrt();
    recovery_options();
}

fn rng() {
    let Ok(handle) = boot::get_handle_for_protocol::<Rng>() else {
        aw_mark!("AW_UEFI_PLATFORM_RNG present=false");
        return;
    };
    let Ok(mut rng) = boot::open_protocol_exclusive::<Rng>(handle) else {
        aw_mark!("AW_UEFI_PLATFORM_RNG present=true usable=false");
        return;
    };
    let mut sample = [0_u8; 32];
    let ok = rng.get_rng(None, &mut sample).is_ok();
    // The bytes are never logged; only that the source answered with non-constant data.
    let varied = sample.iter().any(|b| *b != sample[0]);
    aw_mark!("AW_UEFI_PLATFORM_RNG present=true usable={ok} varied={varied}");
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn esrt() {
    let address = system::with_config_table(|tables| {
        tables
            .iter()
            .find(|entry| entry.guid == ConfigTableEntry::ESRT_GUID)
            .map(|entry| entry.address as usize)
    });
    let Some(address) = address.filter(|a| *a != 0) else {
        aw_mark!("AW_UEFI_PLATFORM_ESRT present=false");
        return;
    };
    // SAFETY: the firmware publishes the ESRT in boot-services-accessible memory; the header is
    // read first and the entry count is bounded before the entries are read.
    let header = unsafe { core::slice::from_raw_parts(address as *const u8, ESRT_HEADER_BYTES) };
    let count = read_u32(header, 0);
    let version = read_u32(header, 8);
    aw_mark!("AW_UEFI_PLATFORM_ESRT present=true resources={count} version={version}");
    let walked = count.min(MAX_ESRT_ENTRIES) as usize;
    // SAFETY: as above, `walked` entries of the documented 40-byte layout follow the header.
    let entries = unsafe {
        core::slice::from_raw_parts(
            (address + ESRT_HEADER_BYTES) as *const u8,
            walked * ESRT_ENTRY_BYTES,
        )
    };
    for entry in entries.as_chunks::<ESRT_ENTRY_BYTES>().0 {
        let class = uefi::Guid::from_bytes(entry[..16].try_into().unwrap_or([0; 16]));
        aw_mark!(
            "AW_UEFI_PLATFORM_ESRT_ENTRY class={class} type={} version={:#x} lowest={:#x} last_attempt_version={:#x} last_attempt_status={}",
            read_u32(entry, 16),
            read_u32(entry, 20),
            read_u32(entry, 24),
            read_u32(entry, 32),
            read_u32(entry, 36)
        );
    }
}

fn recovery_options() {
    let (mut platform, mut os, mut order) = (0_u32, 0_u32, false);
    for key in runtime::variable_keys().flatten() {
        let name = key.name.to_string();
        if key.vendor == VariableVendor::GLOBAL_VARIABLE && name.starts_with("PlatformRecovery") {
            platform += 1;
        } else if name == "OsRecoveryOrder" {
            order = true;
        } else if name.starts_with("OsRecovery") {
            os += 1;
        }
    }
    aw_mark!(
        "AW_UEFI_PLATFORM_RECOVERY platform_options={platform} os_recovery_order={order} os_options={os}"
    );
}

/// Probe every protocol the EDK II reference declares (UEFI and PI, `protocols_gen.rs`) and report
/// which ones this firmware installs, with their handle counts: the complete answer to "what does
/// this machine's UEFI provide", for the owner and for the conformance matrix.
fn inventory() {
    let mut present = 0_usize;
    for (name, bytes) in crate::protocols_gen::PROTOCOLS {
        let guid = uefi::Guid::from_bytes(bytes);
        let handles = boot::locate_handle_buffer(boot::SearchType::ByProtocol(&guid))
            .map(|buffer| buffer.len())
            .unwrap_or(0);
        if handles > 0 {
            present += 1;
            aw_mark!("AW_UEFI_PROTOCOL name={name} handles={handles}");
        }
    }
    aw_mark!(
        "AW_UEFI_INVENTORY known={} present={present}",
        crate::protocols_gen::PROTOCOLS.len()
    );
}
