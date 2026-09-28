//! Build a complete, installable disk from blank (dossier section 12, roadmap
//! Phase 3 "installer / disk provisioning").
//!
//! This is the capstone of the storage stack: on a single blank scratch disk the
//! kernel writes a GPT partition table, formats the resulting EFI System Partition
//! as FAT16, writes a file into it, and then reads that file back through the whole
//! stack it just built - GPT -> partition -> FAT -> file. It is exactly what an
//! installer does to provision a target disk, proven end to end.
//!
//! Gated and scratch-disk-only: it partitions and formats the whole disk, so it
//! must never touch a data disk and is never in the normal boot path.

use crate::fat16::{self, SectorSink, SectorSource};
use crate::gpt;
use crate::{debug_write, debug_write_u64};

const INSTALL_NAME: &[u8; 11] = b"INSTALL TXT";
const INSTALL_DATA: &[u8] = b"AW-DISK-BUILD-OK\n";

/// Provision a blank disk of `total_sectors` and prove a file survives the round
/// trip through the layout the kernel just created. Emits `AW_DISKBUILD_*`.
pub fn prove<S: SectorSource + SectorSink>(device: &S, total_sectors: u64) {
    debug_write("AW_DISKBUILD_BEGIN sectors=");
    debug_write_u64(total_sectors);
    debug_write("\n");

    // 1. Partition: write a GPT with one ESP.
    if let Err(reason) = gpt::write_table(device, total_sectors) {
        debug_write("AW_DISKBUILD_FAIL reason=partition_");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_DISKBUILD_PARTITIONED\n");

    // 2. Read the table back to find the ESP - the same discovery an installer does.
    let Some((esp_lba, esp_last)) = gpt::find_first_partition(device) else {
        debug_write("AW_DISKBUILD_FAIL reason=esp_not_found\n");
        return;
    };
    let esp_sectors = esp_last - esp_lba + 1;
    debug_write("AW_DISKBUILD_ESP lba=");
    debug_write_u64(esp_lba);
    debug_write(" sectors=");
    debug_write_u64(esp_sectors);
    debug_write("\n");

    // 3. Format the ESP as FAT16.
    if let Err(reason) = fat16::format(device, esp_lba, esp_sectors) {
        debug_write("AW_DISKBUILD_FAIL reason=format_");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_DISKBUILD_FORMATTED\n");

    // 4. Write a file into the freshly formatted ESP.
    if let Err(reason) = fat16::write_file(device, esp_lba, INSTALL_NAME, INSTALL_DATA) {
        debug_write("AW_DISKBUILD_FAIL reason=write_");
        debug_write(reason);
        debug_write("\n");
        return;
    }
    debug_write("AW_DISKBUILD_WROTE\n");

    // 5. Read it back through the whole stack the kernel just built: re-discover the
    //    ESP via the GPT, then read the file by name through FAT.
    let Some((esp_lba2, _)) = gpt::find_first_partition(device) else {
        debug_write("AW_DISKBUILD_FAIL reason=esp_reread\n");
        return;
    };
    match fat16::read_named(device, esp_lba2, INSTALL_NAME) {
        Some(ref data) if data.as_slice() == INSTALL_DATA => {
            debug_write("AW_DISKBUILD_READBACK_OK\n");
            debug_write("AW_DISKBUILD_PROOF_OK\n");
        }
        Some(_) => debug_write("AW_DISKBUILD_FAIL reason=content_mismatch\n"),
        None => debug_write("AW_DISKBUILD_FAIL reason=read_failed\n"),
    }
}
