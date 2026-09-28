#![no_std]
#![forbid(unsafe_code)]

pub const RSDP_V1_LEN: usize = 20;
pub const RSDP_V2_MIN_LEN: usize = 36;
pub const RSDP_MAX_LEN: usize = 4096;
pub const SDT_HEADER_LEN: usize = 36;
pub const MCFG_HEADER_LEN: usize = 44;
pub const MCFG_ALLOCATION_LEN: usize = 16;
/// SDT header, then the 32-bit local APIC address and the 32-bit flags word.
pub const MADT_HEADER_LEN: usize = 44;
const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";
const MCFG_SIGNATURE: [u8; 4] = *b"MCFG";
const MADT_SIGNATURE: [u8; 4] = *b"APIC";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsdpError {
    TooShort,
    InvalidSignature,
    InvalidChecksum,
    InvalidLength,
    InvalidExtendedChecksum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsdpInfo {
    pub revision: u8,
    pub length: usize,
    pub rsdt_address: u32,
    pub xsdt_address: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdtError {
    TooShort,
    InvalidLength,
    InvalidChecksum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdtHeaderInfo {
    pub signature: [u8; 4],
    pub length: usize,
    pub revision: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McfgError {
    InvalidSdt(SdtError),
    InvalidSignature,
    InvalidAllocationLength,
    InvalidBusRange,
}

impl From<SdtError> for McfgError {
    fn from(value: SdtError) -> Self {
        Self::InvalidSdt(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McfgAllocation {
    pub base_address: u64,
    pub segment_group: u16,
    pub start_bus: u8,
    pub end_bus: u8,
}

impl McfgAllocation {
    pub const fn contains_bus(self, bus: u8) -> bool {
        bus >= self.start_bus && bus <= self.end_bus
    }

    pub const fn ecam_address(
        self,
        bus: u8,
        device: u8,
        function: u8,
        register_offset: u16,
    ) -> Option<u64> {
        if !self.contains_bus(bus) || device > 31 || function > 7 || register_offset > 0x0fff {
            return None;
        }

        let relative_bus = (bus - self.start_bus) as u64;
        Some(
            self.base_address
                + (relative_bus << 20)
                + ((device as u64) << 15)
                + ((function as u64) << 12)
                + register_offset as u64,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Mcfg<'a> {
    table: &'a [u8],
}

impl<'a> Mcfg<'a> {
    pub fn allocations(self) -> McfgAllocations<'a> {
        let (allocations, remainder) =
            self.table[MCFG_HEADER_LEN..].as_chunks::<MCFG_ALLOCATION_LEN>();
        debug_assert!(remainder.is_empty());
        McfgAllocations {
            allocations: allocations.iter(),
        }
    }
}

pub struct McfgAllocations<'a> {
    allocations: core::slice::Iter<'a, [u8; MCFG_ALLOCATION_LEN]>,
}

impl Iterator for McfgAllocations<'_> {
    type Item = McfgAllocation;

    fn next(&mut self) -> Option<Self::Item> {
        self.allocations
            .next()
            .map(|entry| parse_mcfg_allocation(entry))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.allocations.size_hint()
    }
}

impl ExactSizeIterator for McfgAllocations<'_> {}

#[must_use]
fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().copied().fold(0_u8, u8::wrapping_add)
}

/// Returns the total RSDP length after validating the signature and the
/// ACPI 1.0 checksum. A caller can use this to determine how many bytes must
/// be made available for full ACPI 2.0+ validation.
pub fn declared_length(prefix: &[u8]) -> Result<usize, RsdpError> {
    if prefix.len() < RSDP_V1_LEN {
        return Err(RsdpError::TooShort);
    }

    if &prefix[..RSDP_SIGNATURE.len()] != RSDP_SIGNATURE {
        return Err(RsdpError::InvalidSignature);
    }

    if checksum(&prefix[..RSDP_V1_LEN]) != 0 {
        return Err(RsdpError::InvalidChecksum);
    }

    if prefix[15] < 2 {
        return Ok(RSDP_V1_LEN);
    }

    if prefix.len() < 24 {
        return Err(RsdpError::TooShort);
    }

    let length = u32::from_le_bytes([prefix[20], prefix[21], prefix[22], prefix[23]]) as usize;
    if !(RSDP_V2_MIN_LEN..=RSDP_MAX_LEN).contains(&length) {
        return Err(RsdpError::InvalidLength);
    }

    Ok(length)
}

pub fn validate_rsdp(bytes: &[u8]) -> Result<RsdpInfo, RsdpError> {
    let length = declared_length(bytes)?;
    if bytes.len() < length {
        return Err(RsdpError::TooShort);
    }

    let revision = bytes[15];
    if revision >= 2 && checksum(&bytes[..length]) != 0 {
        return Err(RsdpError::InvalidExtendedChecksum);
    }

    let rsdt_address = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let xsdt_address = if revision >= 2 {
        Some(u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
        ]))
    } else {
        None
    };

    Ok(RsdpInfo {
        revision,
        length,
        rsdt_address,
        xsdt_address,
    })
}

pub fn validate_sdt(bytes: &[u8]) -> Result<SdtHeaderInfo, SdtError> {
    if bytes.len() < SDT_HEADER_LEN {
        return Err(SdtError::TooShort);
    }

    let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if length < SDT_HEADER_LEN || length > bytes.len() {
        return Err(SdtError::InvalidLength);
    }
    if checksum(&bytes[..length]) != 0 {
        return Err(SdtError::InvalidChecksum);
    }

    Ok(SdtHeaderInfo {
        signature: [bytes[0], bytes[1], bytes[2], bytes[3]],
        length,
        revision: bytes[8],
    })
}

pub fn validate_mcfg(bytes: &[u8]) -> Result<Mcfg<'_>, McfgError> {
    let header = validate_sdt(bytes)?;
    if header.signature != MCFG_SIGNATURE {
        return Err(McfgError::InvalidSignature);
    }
    if header.length < MCFG_HEADER_LEN
        || !(header.length - MCFG_HEADER_LEN).is_multiple_of(MCFG_ALLOCATION_LEN)
    {
        return Err(McfgError::InvalidAllocationLength);
    }

    let table = &bytes[..header.length];
    let (allocations, remainder) = table[MCFG_HEADER_LEN..].as_chunks::<MCFG_ALLOCATION_LEN>();
    if !remainder.is_empty() {
        return Err(McfgError::InvalidAllocationLength);
    }
    for chunk in allocations {
        let allocation = parse_mcfg_allocation(chunk);
        if allocation.start_bus > allocation.end_bus {
            return Err(McfgError::InvalidBusRange);
        }
    }

    Ok(Mcfg { table })
}

fn parse_mcfg_allocation(bytes: &[u8]) -> McfgAllocation {
    McfgAllocation {
        base_address: u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        segment_group: u16::from_le_bytes([bytes[8], bytes[9]]),
        start_bus: bytes[10],
        end_bus: bytes[11],
    }
}

/// Why a MADT (`APIC`) table was rejected.
///
/// Every variant is a refusal to guess. The MADT is the only description the
/// kernel gets of where the I/O APICs live and which global system interrupt a
/// legacy ISA IRQ actually arrives on, so a malformed table must fail closed
/// rather than fall back to the conventional addresses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MadtError {
    InvalidSdt(SdtError),
    InvalidSignature,
    /// The table is shorter than the fixed MADT header.
    TooShort,
    /// An entry declared a length of 0 or 1, which cannot advance the walk.
    ZeroLengthEntry,
    /// An entry claimed to extend past the end of the table.
    EntryOutOfBounds,
    /// A known entry type was shorter than its architectural layout.
    EntryTooShortForType,
}

impl From<SdtError> for MadtError {
    fn from(value: SdtError) -> Self {
        Self::InvalidSdt(value)
    }
}

/// Interrupt polarity as encoded in the MPS INTI flags (ACPI 6.5, 5.2.12.5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Polarity {
    /// `00`: whatever the bus specification says. ISA means active high.
    ConformsToBus,
    ActiveHigh,
    ActiveLow,
}

/// Interrupt trigger mode as encoded in the MPS INTI flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerMode {
    /// `00`: whatever the bus specification says. ISA means edge triggered.
    ConformsToBus,
    Edge,
    Level,
}

/// The MPS INTI flags word shared by override and NMI entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptFlags(pub u16);

impl InterruptFlags {
    #[must_use]
    pub const fn polarity(self) -> Polarity {
        match self.0 & 0b11 {
            0b01 => Polarity::ActiveHigh,
            0b11 => Polarity::ActiveLow,
            // `10` is reserved; treating it as "ask the bus" keeps the decoder
            // total without inventing a meaning for it.
            _ => Polarity::ConformsToBus,
        }
    }

    #[must_use]
    pub const fn trigger_mode(self) -> TriggerMode {
        match (self.0 >> 2) & 0b11 {
            0b01 => TriggerMode::Edge,
            0b11 => TriggerMode::Level,
            _ => TriggerMode::ConformsToBus,
        }
    }
}

/// One entry of the MADT's variable-length interrupt-controller list.
///
/// Types this kernel does not consume yet are preserved as [`MadtEntry::Other`]
/// with their raw type byte rather than dropped, so a later pass can tell the
/// difference between "no such entry" and "not decoded here".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MadtEntry {
    LocalApic {
        processor_uid: u8,
        apic_id: u8,
        flags: u32,
    },
    IoApic {
        id: u8,
        address: u32,
        gsi_base: u32,
    },
    InterruptSourceOverride {
        bus: u8,
        source_irq: u8,
        global_system_interrupt: u32,
        flags: InterruptFlags,
    },
    LocalX2Apic {
        x2apic_id: u32,
        flags: u32,
        processor_uid: u32,
    },
    Other {
        kind: u8,
    },
}

const MADT_LOCAL_APIC: u8 = 0;
const MADT_IO_APIC: u8 = 1;
const MADT_INTERRUPT_SOURCE_OVERRIDE: u8 = 2;
const MADT_LOCAL_X2APIC: u8 = 9;

const MADT_LOCAL_APIC_LEN: usize = 8;
const MADT_IO_APIC_LEN: usize = 12;
const MADT_INTERRUPT_SOURCE_OVERRIDE_LEN: usize = 10;
const MADT_LOCAL_X2APIC_LEN: usize = 16;

/// The ISA bus number used by interrupt source overrides.
pub const MADT_ISA_BUS: u8 = 0;

/// A MADT whose header and complete entry list have already been validated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Madt<'a> {
    table: &'a [u8],
}

impl<'a> Madt<'a> {
    /// Physical address of the local APIC registers, as reported by firmware.
    #[must_use]
    pub fn local_apic_address(self) -> u32 {
        u32::from_le_bytes([
            self.table[36],
            self.table[37],
            self.table[38],
            self.table[39],
        ])
    }

    #[must_use]
    pub fn flags(self) -> u32 {
        u32::from_le_bytes([
            self.table[40],
            self.table[41],
            self.table[42],
            self.table[43],
        ])
    }

    /// Bit 0 of the flags word: the platform also has a pair of 8259 PICs,
    /// which must be masked before APIC-mode interrupts are enabled.
    #[must_use]
    pub fn dual_8259_present(self) -> bool {
        self.flags() & 1 != 0
    }

    #[must_use]
    pub fn entries(self) -> MadtEntries<'a> {
        MadtEntries {
            rest: &self.table[MADT_HEADER_LEN..],
        }
    }

    /// First I/O APIC whose window covers `gsi`, and the offset of `gsi` within
    /// that I/O APIC's redirection table.
    #[must_use]
    pub fn io_apic_for_gsi(self, gsi: u32) -> Option<(u8, u32, u32)> {
        self.entries().find_map(|entry| match entry {
            MadtEntry::IoApic {
                id,
                address,
                gsi_base,
            } if gsi >= gsi_base => Some((id, address, gsi - gsi_base)),
            _ => None,
        })
    }

    /// Resolve a legacy ISA IRQ to the global system interrupt it is really
    /// delivered on, together with the polarity and trigger mode to program.
    ///
    /// Identity mapping is the ACPI default, but it is routinely wrong: the
    /// timer's IRQ 0 is commonly overridden to GSI 2, and the ACPI SCI is
    /// commonly overridden to level/low. Programming an I/O APIC from the IRQ
    /// number alone is exactly the bug this lookup exists to prevent.
    #[must_use]
    pub fn resolve_isa_irq(self, irq: u8) -> (u32, Polarity, TriggerMode) {
        for entry in self.entries() {
            if let MadtEntry::InterruptSourceOverride {
                bus,
                source_irq,
                global_system_interrupt,
                flags,
            } = entry
                && bus == MADT_ISA_BUS
                && source_irq == irq
            {
                return (
                    global_system_interrupt,
                    flags.polarity(),
                    flags.trigger_mode(),
                );
            }
        }

        (
            u32::from(irq),
            Polarity::ConformsToBus,
            TriggerMode::ConformsToBus,
        )
    }
}

/// Iterator over a validated MADT's entry list.
///
/// [`validate_madt`] has already walked the whole list, so every `length` byte
/// seen here is non-zero and in bounds and the iteration always terminates.
pub struct MadtEntries<'a> {
    rest: &'a [u8],
}

impl Iterator for MadtEntries<'_> {
    type Item = MadtEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.len() < 2 {
            return None;
        }

        let kind = self.rest[0];
        let length = usize::from(self.rest[1]);
        if length < 2 || length > self.rest.len() {
            return None;
        }

        let (entry, rest) = self.rest.split_at(length);
        self.rest = rest;
        Some(parse_madt_entry(kind, entry))
    }
}

fn parse_madt_entry(kind: u8, entry: &[u8]) -> MadtEntry {
    match kind {
        MADT_LOCAL_APIC if entry.len() >= MADT_LOCAL_APIC_LEN => MadtEntry::LocalApic {
            processor_uid: entry[2],
            apic_id: entry[3],
            flags: u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
        },
        MADT_IO_APIC if entry.len() >= MADT_IO_APIC_LEN => MadtEntry::IoApic {
            id: entry[2],
            address: u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
            gsi_base: u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]),
        },
        MADT_INTERRUPT_SOURCE_OVERRIDE if entry.len() >= MADT_INTERRUPT_SOURCE_OVERRIDE_LEN => {
            MadtEntry::InterruptSourceOverride {
                bus: entry[2],
                source_irq: entry[3],
                global_system_interrupt: u32::from_le_bytes([
                    entry[4], entry[5], entry[6], entry[7],
                ]),
                flags: InterruptFlags(u16::from_le_bytes([entry[8], entry[9]])),
            }
        }
        MADT_LOCAL_X2APIC if entry.len() >= MADT_LOCAL_X2APIC_LEN => MadtEntry::LocalX2Apic {
            x2apic_id: u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
            flags: u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]),
            processor_uid: u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]),
        },
        _ => MadtEntry::Other { kind },
    }
}

/// Minimum entry length architecturally required for a type this crate decodes.
const fn required_entry_length(kind: u8) -> usize {
    match kind {
        MADT_LOCAL_APIC => MADT_LOCAL_APIC_LEN,
        MADT_IO_APIC => MADT_IO_APIC_LEN,
        MADT_INTERRUPT_SOURCE_OVERRIDE => MADT_INTERRUPT_SOURCE_OVERRIDE_LEN,
        MADT_LOCAL_X2APIC => MADT_LOCAL_X2APIC_LEN,
        _ => 2,
    }
}

/// Validate a MADT header and its entire entry list before anything reads it.
///
/// The walk is performed once, here, so [`Madt::entries`] cannot loop forever
/// on a zero-length entry or read past the declared table length.
pub fn validate_madt(bytes: &[u8]) -> Result<Madt<'_>, MadtError> {
    let header = validate_sdt(bytes)?;
    if header.signature != MADT_SIGNATURE {
        return Err(MadtError::InvalidSignature);
    }
    if header.length < MADT_HEADER_LEN {
        return Err(MadtError::TooShort);
    }

    let table = &bytes[..header.length];
    let mut rest = &table[MADT_HEADER_LEN..];
    while !rest.is_empty() {
        if rest.len() < 2 {
            return Err(MadtError::EntryOutOfBounds);
        }
        let kind = rest[0];
        let length = usize::from(rest[1]);
        if length < 2 {
            return Err(MadtError::ZeroLengthEntry);
        }
        if length > rest.len() {
            return Err(MadtError::EntryOutOfBounds);
        }
        if length < required_entry_length(kind) {
            return Err(MadtError::EntryTooShortForType);
        }
        rest = &rest[length..];
    }

    Ok(Madt { table })
}

// ---- FADT and the \_S5 sleep package (power off / reset) -------------------

const FADT_SIGNATURE: [u8; 4] = *b"FACP";
/// FADT flag: the RESET_REG / RESET_VALUE pair is supported.
const FADT_RESET_REG_SUP: u32 = 1 << 10;

/// ACPI Generic Address Structure: where a register lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenericAddress {
    /// 0 = system memory, 1 = system I/O, 2 = PCI configuration space.
    pub space: u8,
    pub bit_width: u8,
    pub address: u64,
}

impl GenericAddress {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let raw = bytes.get(..12)?;
        let address = u64::from_le_bytes(raw[4..12].try_into().ok()?);
        (address != 0).then_some(Self {
            space: raw[0],
            bit_width: raw[1],
            address,
        })
    }
}

/// The parts of the FADT a kernel needs to power off and reset the machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fadt {
    pub dsdt: u64,
    pub smi_cmd: u32,
    pub acpi_enable: u8,
    /// PM1a control block I/O port (from X_PM1a_CNT_BLK when present).
    pub pm1a_cnt: u32,
    pub pm1b_cnt: u32,
    /// The reset register and value, only when the FADT says they are supported.
    pub reset: Option<(GenericAddress, u8)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FadtError {
    InvalidSdt(SdtError),
    InvalidSignature,
    TooShort,
    NoPm1aControl,
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
fn le_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// Validate and decode a FADT. The 64-bit `X_` fields win over the legacy 32-bit
/// ones whenever the table is long enough to carry them and they are non-zero.
pub fn validate_fadt(bytes: &[u8]) -> Result<Fadt, FadtError> {
    let header = validate_sdt(bytes).map_err(FadtError::InvalidSdt)?;
    let table = &bytes[..header.length];
    if header.signature != FADT_SIGNATURE {
        return Err(FadtError::InvalidSignature);
    }
    if table.len() < 116 {
        return Err(FadtError::TooShort);
    }
    let dsdt32 = u64::from(le_u32(table, 40).ok_or(FadtError::TooShort)?);
    let dsdt = le_u64(table, 140).filter(|&x| x != 0).unwrap_or(dsdt32);
    let smi_cmd = le_u32(table, 48).ok_or(FadtError::TooShort)?;
    let acpi_enable = table[52];
    let io_port = |legacy_at: usize, extended_at: usize| -> u32 {
        table
            .get(extended_at..)
            .and_then(GenericAddress::parse)
            .filter(|gas| gas.space == 1 && gas.address <= u64::from(u16::MAX))
            .map_or_else(
                || le_u32(table, legacy_at).unwrap_or(0),
                |gas| gas.address as u32,
            )
    };
    let pm1a_cnt = io_port(64, 172);
    let pm1b_cnt = io_port(68, 184);
    if pm1a_cnt == 0 {
        return Err(FadtError::NoPm1aControl);
    }
    let flags = le_u32(table, 112).ok_or(FadtError::TooShort)?;
    let reset = if flags & FADT_RESET_REG_SUP != 0 && table.len() >= 129 {
        GenericAddress::parse(&table[116..128]).map(|gas| (gas, table[128]))
    } else {
        None
    };
    Ok(Fadt {
        dsdt,
        smi_cmd,
        acpi_enable,
        pm1a_cnt,
        pm1b_cnt,
        reset,
    })
}

/// Decode an AML PkgLength; returns (length, bytes used).
fn aml_pkg_length(bytes: &[u8]) -> Option<(usize, usize)> {
    let lead = *bytes.first()?;
    let follow = usize::from(lead >> 6);
    if follow == 0 {
        return Some((usize::from(lead & 0x3f), 1));
    }
    let mut length = usize::from(lead & 0x0f);
    for i in 0..follow {
        length |= usize::from(*bytes.get(1 + i)?) << (4 + 8 * i);
    }
    Some((length, 1 + follow))
}

/// One AML integer constant: ZeroOp, OneOp, BytePrefix, WordPrefix.
fn aml_small_integer(bytes: &[u8]) -> Option<(u8, usize)> {
    match *bytes.first()? {
        0x00 => Some((0, 1)),
        0x01 => Some((1, 1)),
        0x0a => Some((*bytes.get(1)?, 2)),
        0x0b => Some((*bytes.get(1)?, 3)), // SLP_TYP is 3 bits: low byte suffices
        _ => None,
    }
}

/// Find `Name(\_S5_, Package(){ SLP_TYPa, SLP_TYPb, ... })` in an AML table
/// (DSDT or SSDT) and return (SLP_TYPa, SLP_TYPb). The object is looked up
/// byte-wise, the usual approach before a full AML interpreter exists.
pub fn find_s5(aml: &[u8]) -> Option<(u8, u8)> {
    let mut at = 0;
    while let Some(found) = aml.get(at..)?.windows(4).position(|w| w == b"_S5_") {
        let name = at + found;
        at = name + 4;
        // NameOp directly before, or NameOp then a root prefix '\'.
        let named = (name >= 1 && aml[name - 1] == 0x08)
            || (name >= 2 && aml[name - 2] == 0x08 && aml[name - 1] == b'\\');
        if !named || aml.get(name + 4) != Some(&0x12) {
            continue;
        }
        let Some((_, used)) = aml_pkg_length(&aml[name + 5..]) else {
            continue;
        };
        let mut p = name + 5 + used + 1; // skip NumElements
        let Some((typa, n)) = aml.get(p..).and_then(aml_small_integer) else {
            continue;
        };
        p += n;
        let typb = aml
            .get(p..)
            .and_then(aml_small_integer)
            .map_or(0, |(v, _)| v);
        return Some((typa & 7, typb & 7));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fadt_bytes(length: usize, flags: u32) -> [u8; 276] {
        let mut t = [0u8; 276];
        t[0..4].copy_from_slice(b"FACP");
        t[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        t[8] = 6;
        t[40..44].copy_from_slice(&0x1111_0000u32.to_le_bytes()); // DSDT
        t[48..52].copy_from_slice(&0xb2u32.to_le_bytes()); // SMI_CMD
        t[52] = 0xf1; // ACPI_ENABLE
        t[64..68].copy_from_slice(&0x604u32.to_le_bytes()); // PM1a_CNT_BLK
        t[112..116].copy_from_slice(&flags.to_le_bytes());
        t[116] = 1; // reset reg: system I/O
        t[117] = 8;
        t[120..128].copy_from_slice(&0xcf9u64.to_le_bytes());
        t[128] = 0x06; // RESET_VALUE
        t[9] = 0;
        let sum = checksum(&t[..length]);
        t[9] = 0u8.wrapping_sub(sum);
        t
    }

    #[test]
    fn fadt_legacy_and_reset() {
        let t = fadt_bytes(244, 1 << 10);
        let f = validate_fadt(&t[..244]).unwrap();
        assert_eq!(f.dsdt, 0x1111_0000);
        assert_eq!(f.pm1a_cnt, 0x604);
        assert_eq!(f.smi_cmd, 0xb2);
        assert_eq!(f.acpi_enable, 0xf1);
        let (gas, value) = f.reset.unwrap();
        assert_eq!((gas.space, gas.address, value), (1, 0xcf9, 6));
    }

    #[test]
    fn fadt_reset_only_when_flagged_and_x_fields_win() {
        let mut t = fadt_bytes(244, 0);
        t[140..148].copy_from_slice(&0x2222_0000u64.to_le_bytes()); // X_DSDT
        t[172] = 1; // X_PM1a_CNT_BLK: system I/O
        t[176..184].copy_from_slice(&0x4004u64.to_le_bytes());
        t[9] = 0;
        let sum = checksum(&t[..244]);
        t[9] = 0u8.wrapping_sub(sum);
        let f = validate_fadt(&t[..244]).unwrap();
        assert_eq!(f.reset, None);
        assert_eq!(f.dsdt, 0x2222_0000);
        assert_eq!(f.pm1a_cnt, 0x4004);
    }

    #[test]
    fn fadt_rejects_bad_input() {
        let mut t = fadt_bytes(244, 0);
        t[0] = b'X';
        t[9] = 0;
        let sum = checksum(&t[..244]);
        t[9] = 0u8.wrapping_sub(sum);
        assert_eq!(validate_fadt(&t[..244]), Err(FadtError::InvalidSignature));
        let t = fadt_bytes(100, 0);
        assert_eq!(validate_fadt(&t[..100]), Err(FadtError::TooShort));
    }

    #[test]
    fn s5_byte_prefix_package() {
        // Name(_S5, Package(0x04){ 0x05, 0x05, Zero, Zero })  (QEMU q35 shape)
        let aml = [
            0x10, 0x08, b'_', b'S', b'5', b'_', 0x12, 0x0a, 0x04, 0x0a, 0x05, 0x0a, 0x05, 0x00,
            0x00,
        ];
        assert_eq!(find_s5(&aml), Some((5, 5)));
    }

    #[test]
    fn s5_root_prefix_zero_one_and_long_pkglength() {
        let aml = [
            0x08, b'\\', b'_', b'S', b'5', b'_', 0x12, 0x40, 0x01, 0x02, 0x00, 0x01,
        ];
        assert_eq!(find_s5(&aml), Some((0, 1)));
    }

    #[test]
    fn s5_ignores_non_name_occurrences() {
        let aml = [
            b'_', b'S', b'5', b'_', 0x12, 0x06, 0x02, 0x0a, 0x07, 0x0a, 0x07,
        ];
        assert_eq!(find_s5(&aml), None);
        assert_eq!(find_s5(&[]), None);
        assert_eq!(find_s5(&[0x08, b'_', b'S', b'5', b'_', 0x12]), None);
    }

    fn set_checksum(bytes: &mut [u8], checksum_offset: usize, length: usize) {
        bytes[checksum_offset] = 0;
        let sum = checksum(&bytes[..length]);
        bytes[checksum_offset] = 0_u8.wrapping_sub(sum);
    }

    fn valid_v2_rsdp() -> [u8; RSDP_V2_MIN_LEN] {
        let mut rsdp = [0_u8; RSDP_V2_MIN_LEN];
        rsdp[..8].copy_from_slice(RSDP_SIGNATURE);
        rsdp[9..15].copy_from_slice(b"AWOS  ");
        rsdp[15] = 2;
        rsdp[16..20].copy_from_slice(&0x1234_5000_u32.to_le_bytes());
        rsdp[20..24].copy_from_slice(&(RSDP_V2_MIN_LEN as u32).to_le_bytes());
        rsdp[24..32].copy_from_slice(&0x1234_5678_9abc_d000_u64.to_le_bytes());
        set_checksum(&mut rsdp, 8, RSDP_V1_LEN);
        set_checksum(&mut rsdp, 32, RSDP_V2_MIN_LEN);
        rsdp
    }

    fn valid_mcfg() -> [u8; MCFG_HEADER_LEN + MCFG_ALLOCATION_LEN] {
        let mut table = [0_u8; MCFG_HEADER_LEN + MCFG_ALLOCATION_LEN];
        table[..4].copy_from_slice(b"MCFG");
        let table_len = table.len();
        table[4..8].copy_from_slice(&(table_len as u32).to_le_bytes());
        table[8] = 1;
        table[10..16].copy_from_slice(b"AWOS  ");
        table[16..24].copy_from_slice(b"GENERIC ");

        let entry = &mut table[MCFG_HEADER_LEN..];
        entry[0..8].copy_from_slice(&0xe000_0000_u64.to_le_bytes());
        entry[8..10].copy_from_slice(&0_u16.to_le_bytes());
        entry[10] = 0;
        entry[11] = 0xff;

        set_checksum(&mut table, 9, table_len);
        table
    }

    #[test]
    fn accepts_valid_v2_rsdp() {
        let rsdp = valid_v2_rsdp();
        let info = validate_rsdp(&rsdp).expect("valid RSDP must parse");

        assert_eq!(info.revision, 2);
        assert_eq!(info.length, RSDP_V2_MIN_LEN);
        assert_eq!(info.rsdt_address, 0x1234_5000);
        assert_eq!(info.xsdt_address, Some(0x1234_5678_9abc_d000));
    }

    #[test]
    fn rejects_bad_signature() {
        let mut rsdp = valid_v2_rsdp();
        rsdp[0] = b'X';
        assert_eq!(validate_rsdp(&rsdp), Err(RsdpError::InvalidSignature));
    }

    #[test]
    fn rejects_bad_v1_checksum() {
        let mut rsdp = valid_v2_rsdp();
        rsdp[10] ^= 1;
        assert_eq!(validate_rsdp(&rsdp), Err(RsdpError::InvalidChecksum));
    }

    #[test]
    fn rejects_bad_extended_checksum() {
        let mut rsdp = valid_v2_rsdp();
        rsdp[35] ^= 1;
        assert_eq!(
            validate_rsdp(&rsdp),
            Err(RsdpError::InvalidExtendedChecksum)
        );
    }

    #[test]
    fn rejects_unreasonable_declared_length() {
        let mut rsdp = valid_v2_rsdp();
        rsdp[20..24].copy_from_slice(&8_u32.to_le_bytes());
        set_checksum(&mut rsdp, 8, RSDP_V1_LEN);
        assert_eq!(validate_rsdp(&rsdp), Err(RsdpError::InvalidLength));
    }

    #[test]
    fn validates_sdt_checksum_and_length() {
        let table = valid_mcfg();
        let header = validate_sdt(&table).expect("valid SDT must parse");
        assert_eq!(header.signature, *b"MCFG");
        assert_eq!(header.length, table.len());
    }

    #[test]
    fn parses_mcfg_ecam_allocation() {
        let table = valid_mcfg();
        let mcfg = validate_mcfg(&table).expect("valid MCFG must parse");
        let allocation = mcfg.allocations().next().expect("one allocation");
        assert_eq!(allocation.base_address, 0xe000_0000);
        assert_eq!(allocation.segment_group, 0);
        assert_eq!(allocation.start_bus, 0);
        assert_eq!(allocation.end_bus, 0xff);
        assert_eq!(
            allocation.ecam_address(2, 5, 3, 0x120),
            Some(0xe000_0000 + (2_u64 << 20) + (5_u64 << 15) + (3_u64 << 12) + 0x120)
        );
    }

    /// A MADT shaped like the one QEMU's q35 machine publishes: one local APIC,
    /// one I/O APIC at the conventional address, and the IRQ 0 -> GSI 2
    /// override that makes the timer arrive on a different pin than its IRQ
    /// number suggests.
    fn valid_madt() -> [u8; MADT_HEADER_LEN + 8 + 12 + 10 + 10] {
        let mut table = [0_u8; MADT_HEADER_LEN + 8 + 12 + 10 + 10];
        table[..4].copy_from_slice(b"APIC");
        let table_len = table.len();
        table[4..8].copy_from_slice(&(table_len as u32).to_le_bytes());
        table[8] = 5;
        table[10..16].copy_from_slice(b"AWOS  ");
        table[36..40].copy_from_slice(&0xfee0_0000_u32.to_le_bytes());
        // PCAT_COMPAT: the legacy 8259 pair is present and must be masked.
        table[40..44].copy_from_slice(&1_u32.to_le_bytes());

        let mut at = MADT_HEADER_LEN;
        // Processor local APIC, enabled.
        table[at] = 0;
        table[at + 1] = 8;
        table[at + 2] = 1;
        table[at + 3] = 0;
        table[at + 4..at + 8].copy_from_slice(&1_u32.to_le_bytes());
        at += 8;

        // I/O APIC 0 at 0xfec00000, covering GSI 0 upwards.
        table[at] = 1;
        table[at + 1] = 12;
        table[at + 2] = 0;
        table[at + 4..at + 8].copy_from_slice(&0xfec0_0000_u32.to_le_bytes());
        table[at + 8..at + 12].copy_from_slice(&0_u32.to_le_bytes());
        at += 12;

        // ISA IRQ 0 is really delivered on GSI 2, bus-default polarity/trigger.
        table[at] = 2;
        table[at + 1] = 10;
        table[at + 2] = MADT_ISA_BUS;
        table[at + 3] = 0;
        table[at + 4..at + 8].copy_from_slice(&2_u32.to_le_bytes());
        table[at + 8..at + 10].copy_from_slice(&0_u16.to_le_bytes());
        at += 10;

        // ISA IRQ 9 (the ACPI SCI) is level triggered and active low.
        table[at] = 2;
        table[at + 1] = 10;
        table[at + 2] = MADT_ISA_BUS;
        table[at + 3] = 9;
        table[at + 4..at + 8].copy_from_slice(&9_u32.to_le_bytes());
        table[at + 8..at + 10].copy_from_slice(&0b1111_u16.to_le_bytes());

        set_checksum(&mut table, 9, table_len);
        table
    }

    #[test]
    fn parses_madt_header_and_entry_list() {
        let table = valid_madt();
        let madt = validate_madt(&table).expect("valid MADT must parse");

        assert_eq!(madt.local_apic_address(), 0xfee0_0000);
        assert!(madt.dual_8259_present());

        let mut entries = madt.entries();
        assert_eq!(
            entries.next(),
            Some(MadtEntry::LocalApic {
                processor_uid: 1,
                apic_id: 0,
                flags: 1,
            })
        );
        assert_eq!(
            entries.next(),
            Some(MadtEntry::IoApic {
                id: 0,
                address: 0xfec0_0000,
                gsi_base: 0,
            })
        );
        assert_eq!(madt.entries().count(), 4);
    }

    #[test]
    fn resolves_overridden_and_identity_mapped_isa_irqs() {
        let table = valid_madt();
        let madt = validate_madt(&table).expect("valid MADT must parse");

        // The timer: overridden onto another pin, bus-default electrical spec.
        assert_eq!(
            madt.resolve_isa_irq(0),
            (2, Polarity::ConformsToBus, TriggerMode::ConformsToBus)
        );
        // The SCI: overridden and explicitly level/low.
        assert_eq!(
            madt.resolve_isa_irq(9),
            (9, Polarity::ActiveLow, TriggerMode::Level)
        );
        // Everything without an override stays identity mapped.
        assert_eq!(
            madt.resolve_isa_irq(4),
            (4, Polarity::ConformsToBus, TriggerMode::ConformsToBus)
        );
    }

    #[test]
    fn locates_the_io_apic_owning_a_global_system_interrupt() {
        let table = valid_madt();
        let madt = validate_madt(&table).expect("valid MADT must parse");

        assert_eq!(madt.io_apic_for_gsi(2), Some((0, 0xfec0_0000, 2)));
    }

    #[test]
    fn rejects_madt_with_a_zero_length_entry() {
        let mut table = valid_madt();
        table[MADT_HEADER_LEN + 1] = 0;
        let table_len = table.len();
        set_checksum(&mut table, 9, table_len);

        assert_eq!(validate_madt(&table), Err(MadtError::ZeroLengthEntry));
    }

    #[test]
    fn rejects_madt_entry_that_runs_past_the_table() {
        let mut table = valid_madt();
        table[MADT_HEADER_LEN + 1] = 0xff;
        let table_len = table.len();
        set_checksum(&mut table, 9, table_len);

        assert_eq!(validate_madt(&table), Err(MadtError::EntryOutOfBounds));
    }

    #[test]
    fn rejects_io_apic_entry_too_short_for_its_type() {
        let mut table = valid_madt();
        // Shrink the I/O APIC entry to 4 bytes and absorb the difference into
        // the following entry, so only the per-type length rule can catch it.
        let io_apic = MADT_HEADER_LEN + 8;
        table[io_apic + 1] = 4;
        table[io_apic + 4] = 0x7f;
        table[io_apic + 5] = 8;
        let table_len = table.len();
        set_checksum(&mut table, 9, table_len);

        assert_eq!(validate_madt(&table), Err(MadtError::EntryTooShortForType));
    }

    #[test]
    fn rejects_madt_with_a_foreign_signature() {
        let mut table = valid_madt();
        table[..4].copy_from_slice(b"MCFG");
        let table_len = table.len();
        set_checksum(&mut table, 9, table_len);

        assert_eq!(validate_madt(&table), Err(MadtError::InvalidSignature));
    }

    #[test]
    fn rejects_mcfg_with_invalid_bus_range() {
        let mut table = valid_mcfg();
        table[MCFG_HEADER_LEN + 10] = 10;
        table[MCFG_HEADER_LEN + 11] = 2;
        let table_len = table.len();
        set_checksum(&mut table, 9, table_len);
        assert!(matches!(
            validate_mcfg(&table),
            Err(McfgError::InvalidBusRange)
        ));
    }
}
