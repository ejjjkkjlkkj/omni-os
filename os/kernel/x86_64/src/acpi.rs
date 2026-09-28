//! Locate ACPI description tables in the firmware's physical memory.
//!
//! The UEFI loader validates the RSDP and hands its physical address across in
//! the boot handoff. Everything below that - the root table and the individual
//! description tables - is walked here, because the kernel is the first stage
//! that owns page tables covering the whole low 4 GiB and can therefore read
//! them safely.
//!
//! Parsing itself lives in [`aw_acpi`], which is `forbid(unsafe_code)` and
//! host-tested. This module only turns physical addresses into bounded slices.

use aw_acpi::{Madt, MadtError, RSDP_V2_MIN_LEN, RsdpError, SDT_HEADER_LEN, SdtError};

use crate::virtual_memory::IDENTITY_GIB;

/// Every ACPI table has to live inside the window the kernel identity-maps.
/// A table outside it is not read, because reading it would fault.
const IDENTITY_LIMIT: u64 = IDENTITY_GIB << 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpiError {
    /// The handoff address is null, or the table would cross the identity map.
    Unreachable,
    Rsdp(RsdpError),
    /// The RSDP declares neither an XSDT nor an RSDT.
    NoRootTable,
    RootTable(SdtError),
    /// The root table lists no table with the requested signature.
    NotFound,
    Madt(MadtError),
}

impl AcpiError {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Rsdp(_) => "invalid_rsdp",
            Self::NoRootTable => "no_root_table",
            Self::RootTable(_) => "invalid_root_table",
            Self::NotFound => "not_found",
            Self::Madt(_) => "invalid_madt",
        }
    }
}

/// Borrow `len` bytes of identity-mapped physical memory.
///
/// # Safety
///
/// The caller must only use the returned bytes as immutable firmware data.
/// Firmware tables are never freed after `ExitBootServices` for the memory
/// types ACPI uses, so the `'static` lifetime is sound for their contents.
unsafe fn physical_slice(address: u64, len: usize) -> Option<&'static [u8]> {
    if address == 0 {
        return None;
    }
    let end = address.checked_add(len as u64)?;
    if end > IDENTITY_LIMIT {
        return None;
    }

    // SAFETY: the range is non-null and entirely inside the identity map the
    // kernel installed, so every byte is readable at its physical address.
    Some(unsafe { core::slice::from_raw_parts(address as usize as *const u8, len) })
}

/// Read one SDT completely, given the address of its header.
unsafe fn read_table(address: u64) -> Option<&'static [u8]> {
    // SAFETY: delegated to `physical_slice`; the header is read first so the
    // declared length is known before the full table is borrowed.
    let header = unsafe { physical_slice(address, SDT_HEADER_LEN) }?;
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if length < SDT_HEADER_LEN {
        return None;
    }
    // SAFETY: same reasoning, now for the length the header declares.
    unsafe { physical_slice(address, length) }
}

/// The root table and the width of the physical pointers it holds.
struct RootTable {
    table: &'static [u8],
    pointer_width: usize,
}

impl RootTable {
    fn pointers(&self) -> impl Iterator<Item = u64> + '_ {
        let width = self.pointer_width;
        self.table[SDT_HEADER_LEN..]
            .chunks_exact(width)
            .map(move |entry| match width {
                8 => u64::from_le_bytes([
                    entry[0], entry[1], entry[2], entry[3], entry[4], entry[5], entry[6], entry[7],
                ]),
                _ => u64::from(u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]])),
            })
    }
}

/// # Safety
///
/// `rsdp_address` must be the address firmware reported, which the loader has
/// already validated as an RSDP.
unsafe fn root_table(rsdp_address: u64) -> Result<RootTable, AcpiError> {
    // SAFETY: bounded read of firmware data.
    let prefix =
        unsafe { physical_slice(rsdp_address, RSDP_V2_MIN_LEN) }.ok_or(AcpiError::Unreachable)?;
    let declared = aw_acpi::declared_length(prefix).map_err(AcpiError::Rsdp)?;
    // SAFETY: same, now sized by the RSDP's own declared length.
    let rsdp = unsafe { physical_slice(rsdp_address, declared) }.ok_or(AcpiError::Unreachable)?;
    let info = aw_acpi::validate_rsdp(rsdp).map_err(AcpiError::Rsdp)?;

    // ACPI 2.0+ requires the XSDT to be used when present; the RSDT stays as
    // the fallback for firmware that only publishes the 32-bit form.
    let (address, pointer_width) = match info.xsdt_address {
        Some(xsdt) if xsdt != 0 => (xsdt, 8),
        _ if info.rsdt_address != 0 => (u64::from(info.rsdt_address), 4),
        _ => return Err(AcpiError::NoRootTable),
    };

    // SAFETY: bounded read of firmware data.
    let table = unsafe { read_table(address) }.ok_or(AcpiError::Unreachable)?;
    aw_acpi::validate_sdt(table).map_err(AcpiError::RootTable)?;

    Ok(RootTable {
        table,
        pointer_width,
    })
}

/// What the kernel needs to power the machine off or reset it: the decoded
/// FADT, and SLP_TYPa/SLP_TYPb of the `\_S5` sleep state if the firmware's AML
/// declares it.
#[derive(Clone, Copy, Debug)]
pub struct PowerControl {
    pub fadt: aw_acpi::Fadt,
    pub s5: Option<(u8, u8)>,
}

/// Find the FADT, then `\_S5` in the DSDT (or, failing that, an SSDT).
///
/// # Safety
///
/// Same contract as [`find_madt`].
pub unsafe fn find_power_control(rsdp_address: u64) -> Result<PowerControl, AcpiError> {
    // SAFETY: delegated; the caller guarantees a firmware-provided RSDP.
    let root = unsafe { root_table(rsdp_address) }?;
    let mut fadt = None;
    for address in root.pointers() {
        // SAFETY: bounded read of firmware data; unreadable entries are skipped.
        let Some(table) = (unsafe { read_table(address) }) else {
            continue;
        };
        if table[..4] == *b"FACP" {
            fadt = aw_acpi::validate_fadt(table).ok();
            break;
        }
    }
    let fadt = fadt.ok_or(AcpiError::NotFound)?;
    // SAFETY: bounded read of the DSDT the FADT points at.
    let mut s5 = unsafe { read_table(fadt.dsdt) }
        .filter(|dsdt| aw_acpi::validate_sdt(dsdt).is_ok())
        .and_then(|dsdt| aw_acpi::find_s5(&dsdt[SDT_HEADER_LEN..]));
    if s5.is_none() {
        for address in root.pointers() {
            // SAFETY: as above.
            let Some(table) = (unsafe { read_table(address) }) else {
                continue;
            };
            if table[..4] == *b"SSDT" && aw_acpi::validate_sdt(table).is_ok() {
                s5 = aw_acpi::find_s5(&table[SDT_HEADER_LEN..]);
                if s5.is_some() {
                    break;
                }
            }
        }
    }
    Ok(PowerControl { fadt, s5 })
}

/// Find and validate the MADT (`APIC`) table.
///
/// # Safety
///
/// Must run after the kernel's identity map is active, with `rsdp_address`
/// taken from the validated boot handoff.
pub unsafe fn find_madt(rsdp_address: u64) -> Result<Madt<'static>, AcpiError> {
    // SAFETY: delegated; the caller guarantees a firmware-provided RSDP.
    let root = unsafe { root_table(rsdp_address) }?;

    for address in root.pointers() {
        // SAFETY: bounded read of firmware data; unreadable entries are skipped
        // rather than trusted.
        let Some(table) = (unsafe { read_table(address) }) else {
            continue;
        };
        if table[..4] != *b"APIC" {
            continue;
        }
        return aw_acpi::validate_madt(table).map_err(AcpiError::Madt);
    }

    Err(AcpiError::NotFound)
}
