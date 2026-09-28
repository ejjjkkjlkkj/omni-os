#![no_std]
#![forbid(unsafe_code)]

//! Pure encoding and validation for the x86-64 Global Descriptor Table (GDT)
//! and Interrupt Descriptor Table (IDT).
//!
//! This crate deliberately contains no `unsafe` code and does not itself
//! load any table into the CPU. It only builds the byte-exact descriptor
//! values and answers the classification questions (does this exception
//! push an error code, what gate type does it need) that the kernel needs
//! to get right before it ever executes `lgdt`/`lidt`. The kernel crate is
//! responsible for placing the produced bytes in memory and executing the
//! privileged instructions, with narrowly scoped and documented `unsafe`.
//!
//! The same split applies to the two routers a device interrupt can arrive
//! through: [`ioapic`] encodes redirection-table entries and [`msi`] encodes
//! message address/data pairs, and neither performs any access.

pub mod ioapic;
pub mod msi;

/// CPU privilege level used by both segment and gate descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivilegeLevel {
    Ring0,
    Ring3,
}

impl PrivilegeLevel {
    #[must_use]
    const fn as_u64(self) -> u64 {
        match self {
            Self::Ring0 => 0,
            Self::Ring3 => 3,
        }
    }

    #[must_use]
    const fn as_u16(self) -> u16 {
        match self {
            Self::Ring0 => 0,
            Self::Ring3 => 3,
        }
    }
}

/// A GDT segment selector: table index plus requested privilege level.
///
/// Only flat GDT layouts are represented; the table indicator bit is always
/// zero (this project does not use an LDT).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentSelector(u16);

impl SegmentSelector {
    /// Maximum GDT index representable in a 16-bit selector, given 3 bits
    /// are reserved for the table indicator and requested privilege level.
    pub const MAX_INDEX: u16 = u16::MAX >> 3;

    #[must_use]
    pub const fn new(index: u16, privilege_level: PrivilegeLevel) -> Option<Self> {
        if index > Self::MAX_INDEX {
            return None;
        }
        Some(Self((index << 3) | privilege_level.as_u16()))
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn index(self) -> u16 {
        self.0 >> 3
    }
}

/// One 8-byte GDT descriptor.
///
/// Base and limit are fixed at zero/maximum: in long mode the CPU ignores
/// the base and limit of code and data segments entirely, so encoding a
/// flat descriptor once and reusing it for every ring is sufficient and
/// avoids representing fields that would otherwise silently do nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GdtEntry(u64);

impl GdtEntry {
    pub const NULL: Self = Self(0);

    const PRESENT: u64 = 1 << 47;
    const DESCRIPTOR_TYPE_CODE_OR_DATA: u64 = 1 << 44;
    const READABLE_OR_WRITABLE: u64 = 1 << 41;
    const EXECUTABLE: u64 = 1 << 43;
    const LONG_MODE_CODE: u64 = 1 << 53;

    #[must_use]
    const fn dpl(privilege_level: PrivilegeLevel) -> u64 {
        privilege_level.as_u64() << 45
    }

    /// A flat 64-bit code segment: present, non-conforming, readable, long
    /// mode.
    #[must_use]
    pub const fn code64(privilege_level: PrivilegeLevel) -> Self {
        Self(
            Self::PRESENT
                | Self::DESCRIPTOR_TYPE_CODE_OR_DATA
                | Self::READABLE_OR_WRITABLE
                | Self::EXECUTABLE
                | Self::LONG_MODE_CODE
                | Self::dpl(privilege_level),
        )
    }

    /// A flat 64-bit writable data segment.
    #[must_use]
    pub const fn data64(privilege_level: PrivilegeLevel) -> Self {
        Self(
            Self::PRESENT
                | Self::DESCRIPTOR_TYPE_CODE_OR_DATA
                | Self::READABLE_OR_WRITABLE
                | Self::dpl(privilege_level),
        )
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn is_present(self) -> bool {
        self.0 & Self::PRESENT != 0
    }
}

/// Gate type for an IDT entry. Both are 64-bit gates; the only architectural
/// difference is whether `IF` is cleared on entry (interrupt gate) or left
/// alone (trap gate).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateType {
    Interrupt,
    Trap,
}

impl GateType {
    #[must_use]
    const fn type_bits(self) -> u8 {
        match self {
            Self::Interrupt => 0b1110,
            Self::Trap => 0b1111,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdtEntryError {
    /// The interrupt-stack-table index must fit in 3 bits (0 selects "no
    /// IST switch", 1-7 select `TSS.IST[index]`).
    InvalidInterruptStackTableIndex,
}

/// One 16-byte IDT gate descriptor.
///
/// `#[repr(C)]` fixes field order and forbids reordering: the kernel places
/// this struct directly in the table the CPU reads with `lidt`, so its
/// layout must match the architectural gate descriptor byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    interrupt_stack_table_index: u8,
    type_attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    /// A not-present entry. Delivering an interrupt through a missing gate
    /// raises `#GP`, which is the safe failure mode for an unhandled or not
    /// yet wired-up vector.
    pub const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        interrupt_stack_table_index: 0,
        type_attributes: 0,
        offset_mid: 0,
        offset_high: 0,
        reserved: 0,
    };

    const PRESENT: u8 = 1 << 7;

    pub const fn new(
        handler_address: u64,
        code_selector: SegmentSelector,
        gate_type: GateType,
        privilege_level: PrivilegeLevel,
        interrupt_stack_table_index: u8,
    ) -> Result<Self, IdtEntryError> {
        if interrupt_stack_table_index > 7 {
            return Err(IdtEntryError::InvalidInterruptStackTableIndex);
        }

        let type_attributes =
            Self::PRESENT | ((privilege_level.as_u64() as u8) << 5) | gate_type.type_bits();

        Ok(Self {
            offset_low: (handler_address & 0xffff) as u16,
            selector: code_selector.raw(),
            interrupt_stack_table_index,
            type_attributes,
            offset_mid: ((handler_address >> 16) & 0xffff) as u16,
            offset_high: (handler_address >> 32) as u32,
            reserved: 0,
        })
    }

    #[must_use]
    pub const fn is_present(self) -> bool {
        self.type_attributes & Self::PRESENT != 0
    }

    #[must_use]
    pub const fn handler_address(self) -> u64 {
        (self.offset_low as u64)
            | ((self.offset_mid as u64) << 16)
            | ((self.offset_high as u64) << 32)
    }

    #[must_use]
    pub const fn selector(self) -> SegmentSelector {
        SegmentSelector(self.selector)
    }

    #[must_use]
    pub const fn interrupt_stack_table_index(self) -> u8 {
        self.interrupt_stack_table_index
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorTablePointerError {
    EmptyTable,
    TableTooLarge,
}

/// The operand loaded by `lgdt`/`lidt`: a byte limit (table size minus one)
/// and the linear base address of the table.
///
/// `#[repr(C, packed)]` matches the layout the CPU instruction requires: a
/// 16-bit limit immediately followed by a 64-bit base, with no padding.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

impl DescriptorTablePointer {
    pub const fn new(
        base: u64,
        entry_count: usize,
        entry_size: usize,
    ) -> Result<Self, DescriptorTablePointerError> {
        let Some(byte_len) = entry_count.checked_mul(entry_size) else {
            return Err(DescriptorTablePointerError::TableTooLarge);
        };
        if byte_len == 0 {
            return Err(DescriptorTablePointerError::EmptyTable);
        }
        let limit = byte_len - 1;
        if limit > u16::MAX as usize {
            return Err(DescriptorTablePointerError::TableTooLarge);
        }
        Ok(Self {
            limit: limit as u16,
            base,
        })
    }

    #[must_use]
    pub const fn limit(&self) -> u16 {
        self.limit
    }

    #[must_use]
    pub const fn base(&self) -> u64 {
        self.base
    }
}

/// Total number of vectors in a full x86-64 IDT (32 CPU-reserved exception
/// vectors plus 224 user-definable interrupt vectors).
pub const IDT_ENTRY_COUNT: usize = 256;

/// The lowest vector number available for external/software interrupts.
/// Vectors below this are reserved by the architecture for CPU exceptions.
pub const FIRST_USABLE_INTERRUPT_VECTOR: u8 = 32;

/// Whether the CPU pushes an error code on the stack before entering the
/// handler for this exception vector. The kernel's assembly stub for each
/// vector must match this exactly: pushing a placeholder for the vectors
/// that don't push one, and not pushing one for the vectors that do,
/// otherwise the handler reads the stack at the wrong offset.
///
/// Vectors 32 and above (external/software interrupts) never push an error
/// code.
#[must_use]
pub const fn exception_pushes_error_code(vector: u8) -> bool {
    matches!(vector, 8 | 10 | 11 | 12 | 13 | 14 | 17 | 21 | 29 | 30)
}

/// Short, stable names for the CPU-reserved exception vectors (0-31), for
/// debug-console diagnostics. Vectors that are architecturally reserved and
/// currently unused by any x86-64 CPU return `"reserved"`.
#[must_use]
pub const fn exception_name(vector: u8) -> &'static str {
    match vector {
        0 => "divide-error",
        1 => "debug",
        2 => "nmi",
        3 => "breakpoint",
        4 => "overflow",
        5 => "bound-range-exceeded",
        6 => "invalid-opcode",
        7 => "device-not-available",
        8 => "double-fault",
        9 => "reserved",
        10 => "invalid-tss",
        11 => "segment-not-present",
        12 => "stack-segment-fault",
        13 => "general-protection-fault",
        14 => "page-fault",
        15 => "reserved",
        16 => "x87-floating-point",
        17 => "alignment-check",
        18 => "machine-check",
        19 => "simd-floating-point",
        20 => "virtualization",
        21 => "control-protection",
        22..=27 => "reserved",
        28 => "hypervisor-injection",
        29 => "vmm-communication",
        30 => "security",
        31 => "reserved",
        _ => "external-interrupt",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_selector_encodes_index_and_privilege_level() {
        let selector = SegmentSelector::new(1, PrivilegeLevel::Ring0).unwrap();
        assert_eq!(selector.raw(), 0x08);
        assert_eq!(selector.index(), 1);

        let selector = SegmentSelector::new(2, PrivilegeLevel::Ring3).unwrap();
        assert_eq!(selector.raw(), 0x13);
        assert_eq!(selector.index(), 2);
    }

    #[test]
    fn segment_selector_rejects_out_of_range_index() {
        assert!(
            SegmentSelector::new(SegmentSelector::MAX_INDEX + 1, PrivilegeLevel::Ring0).is_none()
        );
        assert!(SegmentSelector::new(SegmentSelector::MAX_INDEX, PrivilegeLevel::Ring0).is_some());
    }

    #[test]
    fn null_gdt_entry_is_not_present() {
        assert_eq!(GdtEntry::NULL.raw(), 0);
        assert!(!GdtEntry::NULL.is_present());
    }

    #[test]
    fn code64_descriptor_matches_known_long_mode_bit_pattern() {
        // Reference bit pattern for a flat ring-0 64-bit code segment:
        // present | type=code/data | readable | executable | long-mode.
        let expected = (1u64 << 47) | (1u64 << 44) | (1u64 << 41) | (1u64 << 43) | (1u64 << 53);
        assert_eq!(GdtEntry::code64(PrivilegeLevel::Ring0).raw(), expected);
        assert!(GdtEntry::code64(PrivilegeLevel::Ring0).is_present());
    }

    #[test]
    fn data64_descriptor_matches_known_long_mode_bit_pattern() {
        let expected = (1u64 << 47) | (1u64 << 44) | (1u64 << 41);
        assert_eq!(GdtEntry::data64(PrivilegeLevel::Ring0).raw(), expected);
    }

    #[test]
    fn ring3_descriptors_set_the_dpl_bits() {
        let code = GdtEntry::code64(PrivilegeLevel::Ring3).raw();
        let dpl = (code >> 45) & 0b11;
        assert_eq!(dpl, 3);
    }

    #[test]
    fn idt_entry_round_trips_a_64_bit_handler_address() {
        let selector = SegmentSelector::new(1, PrivilegeLevel::Ring0).unwrap();
        let handler_address = 0xffff_8000_1234_5678;
        let entry = IdtEntry::new(
            handler_address,
            selector,
            GateType::Interrupt,
            PrivilegeLevel::Ring0,
            0,
        )
        .unwrap();

        assert_eq!(entry.handler_address(), handler_address);
        assert_eq!(entry.selector(), selector);
        assert!(entry.is_present());
        assert_eq!(entry.interrupt_stack_table_index(), 0);
    }

    #[test]
    fn idt_entry_rejects_out_of_range_ist_index() {
        let selector = SegmentSelector::new(1, PrivilegeLevel::Ring0).unwrap();
        assert_eq!(
            IdtEntry::new(0, selector, GateType::Interrupt, PrivilegeLevel::Ring0, 8),
            Err(IdtEntryError::InvalidInterruptStackTableIndex)
        );
        assert!(IdtEntry::new(0, selector, GateType::Interrupt, PrivilegeLevel::Ring0, 7).is_ok());
    }

    #[test]
    fn missing_idt_entry_is_not_present() {
        assert!(!IdtEntry::MISSING.is_present());
        assert_eq!(IdtEntry::MISSING.handler_address(), 0);
    }

    #[test]
    fn trap_and_interrupt_gates_use_distinct_type_bits() {
        let selector = SegmentSelector::new(1, PrivilegeLevel::Ring0).unwrap();
        let interrupt = IdtEntry::new(0, selector, GateType::Interrupt, PrivilegeLevel::Ring0, 0)
            .unwrap()
            .type_attributes;
        let trap = IdtEntry::new(0, selector, GateType::Trap, PrivilegeLevel::Ring0, 0)
            .unwrap()
            .type_attributes;
        assert_ne!(interrupt & 0b1111, trap & 0b1111);
    }

    #[test]
    fn descriptor_table_pointer_computes_limit_as_size_minus_one() {
        let pointer = DescriptorTablePointer::new(0x1000, 3, 8).unwrap();
        assert_eq!(pointer.limit(), 3 * 8 - 1);
        assert_eq!(pointer.base(), 0x1000);
    }

    #[test]
    fn descriptor_table_pointer_rejects_empty_table() {
        assert_eq!(
            DescriptorTablePointer::new(0x1000, 0, 8),
            Err(DescriptorTablePointerError::EmptyTable)
        );
    }

    #[test]
    fn descriptor_table_pointer_rejects_overflowing_limit() {
        assert_eq!(
            DescriptorTablePointer::new(0x1000, IDT_ENTRY_COUNT + 1, 4096),
            Err(DescriptorTablePointerError::TableTooLarge)
        );
    }

    #[test]
    fn full_idt_size_fits_in_a_16_bit_limit() {
        assert!(DescriptorTablePointer::new(0x1000, IDT_ENTRY_COUNT, 16).is_ok());
    }

    #[test]
    fn error_code_classification_matches_the_architectural_table() {
        for vector in 0..=31u8 {
            let expected = matches!(vector, 8 | 10 | 11 | 12 | 13 | 14 | 17 | 21 | 29 | 30);
            assert_eq!(exception_pushes_error_code(vector), expected);
        }
        assert!(!exception_pushes_error_code(32));
        assert!(!exception_pushes_error_code(255));
    }

    #[test]
    fn every_reserved_exception_vector_has_a_name() {
        for vector in 0..=31u8 {
            assert!(!exception_name(vector).is_empty());
        }
        assert_eq!(exception_name(32), "external-interrupt");
    }
}
