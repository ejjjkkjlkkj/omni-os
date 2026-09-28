//! Symmetric multiprocessing bring-up.
//!
//! An application processor wakes in 16-bit real mode at a page-aligned address
//! below 1 MiB, which is all the SIPI message can express. Getting it from there
//! to 64-bit Rust is the whole job, and the order is forced:
//!
//! 1. a real-mode stub in low memory loads a GDT and enters 32-bit protected
//!    mode, then jumps *out of low memory* into the kernel image;
//! 2. a 32-bit stub inside `.text` enables PAE, `EFER.LME` **and `EFER.NXE`**,
//!    loads the bootstrap processor's CR3 and turns on paging;
//! 3. a 64-bit stub calls into Rust, which installs this CPU's own GDT and TSS.
//!
//! Step 2 is why step 1 has to leave low memory first. The kernel's page tables
//! map everything outside the kernel image as NX, so the instruction *after*
//! paging is enabled must already be fetched from a page that is executable -
//! and the trampoline's page is not one. `EFER.NXE` matters for the same
//! reason from the other side: with it clear, the NX bits those tables already
//! contain are reserved bits, and the first fetch faults.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use aw_acpi::{Madt, MadtEntry};
use aw_kernel_core::{KernelHandoff, UEFI_MEMORY_TYPE_CONVENTIONAL};

use crate::interrupts::{self, ApTables, MAX_CPUS};
use crate::local_apic::{X2APIC_ID_MSR, rdmsr, wrmsr};

/// Where the real-mode stub is copied, and therefore the SIPI vector: the
/// message carries a page number, so this must be page aligned and below 1 MiB.
/// It is baked into the stub's own absolute addressing, so it is a constant
/// rather than an allocation - the kernel verifies the page is free instead.
pub const TRAMPOLINE_BASE: u64 = 0x8000;
const SIPI_VECTOR: u8 = (TRAMPOLINE_BASE >> 12) as u8;

/// Per-AP bootstrap stack. Only used until the AP parks; anything that needs a
/// real stack budget comes with the scheduler.
const AP_STACK_SIZE: usize = 16 * 1024;

const X2APIC_ICR_MSR: u32 = 0x830;
/// Delivery mode 101 (INIT), no vector.
const ICR_INIT: u64 = 0x0000_0500;
/// Delivery mode 110 (Start-Up), vector = trampoline page number.
const ICR_STARTUP: u64 = 0x0000_0600;

#[repr(C, align(16))]
struct ApStack([u8; AP_STACK_SIZE]);

static mut AP_STACKS: [ApStack; MAX_CPUS] = [const { ApStack([0; AP_STACK_SIZE]) }; MAX_CPUS];

/// Handed to the AP by the 32-bit stub. Written before the SIPI, read once.
#[unsafe(no_mangle)]
static AW_AP_CR3: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
static AW_AP_STACK_TOP: AtomicU64 = AtomicU64::new(0);
/// Which per-CPU slot the AP currently being started owns. APs are started one
/// at a time, so a single slot is enough and needs no locking.
static AW_AP_INDEX: AtomicU32 = AtomicU32::new(0);

/// One AP's report, written by that AP and read by the bootstrap processor
/// after `online` is observed.
struct ApReport {
    online: AtomicBool,
    apic_id: AtomicU32,
    task_register: AtomicU32,
    gdt_base: AtomicU64,
    tss_base: AtomicU64,
    ist1_top: AtomicU64,
}

impl ApReport {
    const fn new() -> Self {
        Self {
            online: AtomicBool::new(false),
            apic_id: AtomicU32::new(0),
            task_register: AtomicU32::new(0),
            gdt_base: AtomicU64::new(0),
            tss_base: AtomicU64::new(0),
            ist1_top: AtomicU64::new(0),
        }
    }
}

static AP_REPORTS: [ApReport; MAX_CPUS] = [const { ApReport::new() }; MAX_CPUS];

/// The APIC ID each slot was started for, so a report can be checked against
/// what was actually requested rather than against itself.
static AP_REQUESTED_APIC_IDS: [AtomicU32; MAX_CPUS] =
    [const { AtomicU32::new(u32::MAX) }; MAX_CPUS];

core::arch::global_asm!(
    ".set AW_AP_BASE, {base}",
    // Fixed offsets inside the copied page. The assembler refuses symbol
    // arithmetic inside a memory operand, and the real-mode code has to name
    // its GDT by absolute address, so the layout is pinned with `.org` instead
    // of computed from labels.
    ".set AW_AP_GDT_OFFSET, 0x40",
    ".set AW_AP_GDT_POINTER_OFFSET, 0x60",

    // ---- Real mode, copied to AW_AP_BASE ---------------------------------
    // 16-byte aligned in the image so the copy preserves every internal
    // alignment: the destination is page aligned, so anything the assembler
    // padded to 8 or 16 bytes here lands the same way there.
    ".section .text.ap_trampoline,\"ax\",@progbits",
    ".balign 16",
    ".global aw_ap_trampoline_start",
    ".code16",
    "aw_ap_trampoline_start:",
    "cli",
    "cld",
    "xor ax, ax",
    "mov ds, ax",
    "mov es, ax",
    "mov ss, ax",
    "lgdt [AW_AP_BASE + AW_AP_GDT_POINTER_OFFSET]",
    "mov eax, cr0",
    "or eax, 1",
    "mov cr0, eax",
    // Far jump into the kernel image's 32-bit stub. Encoded by hand: the
    // assembler has no Intel-syntax spelling for a 16-bit-mode far jump with a
    // 32-bit absolute offset.
    ".byte 0x66, 0xea",
    ".long aw_ap_protected_entry",
    ".word 0x08",

    ".org AW_AP_GDT_OFFSET",
    "aw_ap_gdt:",
    ".quad 0",
    ".quad 0x00cf9a000000ffff", // 0x08: 32-bit code, base 0, limit 4 GiB
    ".quad 0x00cf92000000ffff", // 0x10: data, base 0, limit 4 GiB
    ".quad 0x00209a0000000000", // 0x18: 64-bit code
    ".org AW_AP_GDT_POINTER_OFFSET",
    "aw_ap_gdt_pointer:",
    ".word 31",
    ".long AW_AP_BASE + AW_AP_GDT_OFFSET",
    ".global aw_ap_trampoline_end",
    "aw_ap_trampoline_end:",

    // ---- Protected mode, executing inside the kernel image ----------------
    ".section .text.ap_entry,\"ax\",@progbits",
    ".code32",
    "aw_ap_protected_entry:",
    "mov ax, 0x10",
    "mov ds, ax",
    "mov es, ax",
    "mov ss, ax",
    "mov fs, ax",
    "mov gs, ax",
    "mov esp, [{stack_top}]",
    "mov eax, cr4",
    "or eax, (1 << 5)", // PAE
    "mov cr4, eax",
    "mov eax, [{cr3}]",
    "mov cr3, eax",
    "mov ecx, 0xc0000080", // IA32_EFER
    "rdmsr",
    "or eax, (1 << 8) | (1 << 11)", // LME | NXE
    "wrmsr",
    "mov eax, cr0",
    "or eax, (1 << 31) | (1 << 16)", // PG | WP
    "mov cr0, eax",
    // Paging is on and this CPU is in compatibility mode; the far jump reloads
    // CS from the 64-bit descriptor and lands on a page the kernel maps
    // executable.
    ".byte 0xea",
    ".long aw_ap_long_entry",
    ".word 0x18",

    // ---- Long mode --------------------------------------------------------
    ".code64",
    "aw_ap_long_entry:",
    // ESP was loaded before the jump and zero-extends into RSP; the stack is
    // inside the kernel image, well below 4 GiB.
    "call {entry}",
    "2:",
    "hlt",
    "jmp 2b",

    base = const TRAMPOLINE_BASE,
    stack_top = sym AW_AP_STACK_TOP,
    cr3 = sym AW_AP_CR3,
    entry = sym ap_rust_entry,
);

unsafe extern "C" {
    static aw_ap_trampoline_start: u8;
    static aw_ap_trampoline_end: u8;
}

/// First 64-bit Rust instruction an application processor executes.
extern "C" fn ap_rust_entry() -> ! {
    let cpu = AW_AP_INDEX.load(Ordering::Acquire) as usize;

    // SAFETY: CPL0 with interrupts masked, once, on this AP's own slot.
    let tables = unsafe { interrupts::install_for_ap(cpu) };

    // SAFETY: CPL0. Each AP enables its own local APIC; nothing here touches
    // another CPU's state.
    let apic_id = unsafe {
        let _ = crate::apic_timer::prepare_x2apic();
        rdmsr(X2APIC_ID_MSR) as u32
    };

    // Give this AP its own GS-reachable per-CPU block, so any interrupt it later
    // takes is attributed to it rather than to a shared counter (dossier
    // section 8). The bootstrap processor reads it back by index once this AP is
    // online.
    // SAFETY: CPL0 on this AP, run once, on its own unique slot below MAX_CPUS.
    let _ = unsafe { crate::percpu::install(cpu, apic_id) };

    // Record this AP's own IST1 bounds so its #DF handler can confirm a fault
    // lands on its own emergency stack rather than another CPU's.
    if let Some(t) = tables.as_ref()
        && let Some(block) = crate::percpu::by_index(cpu)
    {
        block.set_ist1_bounds(t.ist1_start, t.ist1_top);
    }

    // Dedicated build only: one application processor runs the cooperative
    // scheduler, proving threads context switch on a CPU other than the bootstrap
    // processor before it goes on to report online and idle normally.
    #[cfg(feature = "ap-scheduler-smoke-test")]
    if cpu == 1 {
        // SAFETY: CPL0 on this AP, interrupts masked, run once on this AP.
        unsafe { crate::scheduler::prove_ap_scheduler(cpu) };
    }

    // Dedicated build only: one application processor deliberately double-faults,
    // before it reports online, to prove the #DF resolves on *its own* per-CPU
    // IST1 (dossier section 5.3). It never returns, so bring-up sees it stay
    // offline - which the ap-double-fault-smoke configuration expects.
    #[cfg(feature = "ap-double-fault-smoke-test")]
    if cpu == 1 {
        crate::debug_write("AW_AP_DOUBLE_FAULT_SMOKE cpu=1\n");
        // SAFETY: this AP has installed its own GDT/TSS/IST1 and per-CPU block, so
        // the forced #DF resolves on its own IST1.
        unsafe { crate::interrupts::trigger_double_fault_smoke() }
    }

    if let Some(ApTables {
        task_register,
        gdt_base,
        tss_base,
        ist1_top,
        ..
    }) = tables
        && cpu < MAX_CPUS
    {
        let report = &AP_REPORTS[cpu];
        report.apic_id.store(apic_id, Ordering::Relaxed);
        report
            .task_register
            .store(u32::from(task_register), Ordering::Relaxed);
        report.gdt_base.store(gdt_base, Ordering::Relaxed);
        report.tss_base.store(tss_base, Ordering::Relaxed);
        report.ist1_top.store(ist1_top, Ordering::Relaxed);
        // Published last: the bootstrap processor reads the rest only after it
        // observes this.
        report.online.store(true, Ordering::Release);

        // With this AP online and its per-CPU block installed, bring its own
        // Local APIC timer up and idle under interrupts. The timer gate lives in
        // the shared IDT the bootstrap processor already installed, and every
        // other interrupt source is routed to the bootstrap processor, so the
        // only vector this AP can take is its own timer - counted into this AP's
        // per-CPU block, which is what proves a per-CPU timer on an application
        // processor (dossier section 8, roadmap P0 step 5).
        //
        // SAFETY: CPL0 on this AP; x2APIC and the per-CPU block are set up above
        // and the timer gate is present in the shared IDT.
        unsafe {
            if crate::apic_timer::start_periodic_running().is_ok() {
                core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
            }
        }
    }

    loop {
        // SAFETY: this AP services only its own Local APIC timer if it armed one;
        // otherwise interrupts stay masked. `hlt` parks until the next one.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// Every enabled processor the MADT describes, bootstrap processor included.
fn madt_apic_ids(madt: Madt<'static>, into: &mut [u32; MAX_CPUS]) -> usize {
    let mut count = 0;
    for entry in madt.entries() {
        let (id, flags) = match entry {
            // Bit 0 enabled, or bit 1 "online capable": anything else is a
            // processor firmware says must not be started.
            MadtEntry::LocalApic { apic_id, flags, .. } => (u32::from(apic_id), flags),
            MadtEntry::LocalX2Apic {
                x2apic_id, flags, ..
            } => (x2apic_id, flags),
            _ => continue,
        };
        if flags & 0b11 == 0 || count >= into.len() {
            continue;
        }
        if into[..count].contains(&id) {
            continue;
        }
        into[count] = id;
        count += 1;
    }
    count
}

/// Whether the trampoline page is conventional memory nobody else owns.
fn trampoline_page_is_free(handoff: &KernelHandoff) -> bool {
    let Some(descriptors) = crate::memory_map_descriptors(handoff) else {
        return false;
    };
    descriptors.iter().any(|descriptor| {
        descriptor.is_valid()
            && descriptor.memory_type == UEFI_MEMORY_TYPE_CONVENTIONAL
            && descriptor.physical_start <= TRAMPOLINE_BASE
            && TRAMPOLINE_BASE + 4096
                <= descriptor.physical_start + descriptor.page_count.saturating_mul(4096)
    })
}

/// Bounded busy-wait. The APs are started before any calibrated time source
/// exists, so the INIT/SIPI delays and the online timeout are spin counts.
fn spin(iterations: u32) {
    for _ in 0..iterations {
        core::hint::spin_loop();
    }
}

const INIT_SETTLE_SPINS: u32 = 2_000_000;
const SIPI_SETTLE_SPINS: u32 = 200_000;
const ONLINE_TIMEOUT_SPINS: u32 = 100_000_000;

/// # Safety
/// CPL0 on the bootstrap processor, with x2APIC enabled.
unsafe fn send_ipi(destination: u32, command: u64) {
    // SAFETY: the x2APIC ICR is a single 64-bit MSR write; no delivery-status
    // polling is needed or possible in x2APIC mode.
    unsafe { wrmsr(X2APIC_ICR_MSR, (u64::from(destination) << 32) | command) };
}

#[derive(Clone, Copy, Debug)]
pub struct SmpBringUp {
    pub described: usize,
    pub started: usize,
    pub online: usize,
    pub bootstrap_apic_id: u32,
}

/// One application processor, as the bootstrap processor observed it.
#[derive(Clone, Copy, Debug)]
pub struct ApSummary {
    pub cpu: usize,
    pub requested_apic_id: u32,
    pub reported_apic_id: u32,
    pub task_register: u16,
    pub gdt_base: u64,
    pub tss_base: u64,
    pub ist1_top: u64,
}

#[must_use]
pub fn ap_summary(cpu: usize) -> Option<ApSummary> {
    let report = AP_REPORTS.get(cpu)?;
    if !report.online.load(Ordering::Acquire) {
        return None;
    }
    Some(ApSummary {
        cpu,
        requested_apic_id: AP_REQUESTED_APIC_IDS[cpu].load(Ordering::Relaxed),
        reported_apic_id: report.apic_id.load(Ordering::Relaxed),
        task_register: report.task_register.load(Ordering::Relaxed) as u16,
        gdt_base: report.gdt_base.load(Ordering::Relaxed),
        tss_base: report.tss_base.load(Ordering::Relaxed),
        ist1_top: report.ist1_top.load(Ordering::Relaxed),
    })
}

/// Start every application processor the MADT describes.
///
/// APs are started strictly one at a time and waited for, which is what lets a
/// single index and a single stack pointer be handed across without locking.
///
/// # Safety
///
/// CPL0 on the bootstrap processor, after the kernel owns its page tables, the
/// IDT is installed and x2APIC is enabled.
pub unsafe fn bring_up(
    handoff: &KernelHandoff,
    madt: Madt<'static>,
) -> Result<SmpBringUp, &'static str> {
    // SAFETY: CPL0; reading the APIC ID MSR has no side effects.
    let bootstrap_apic_id = unsafe { rdmsr(X2APIC_ID_MSR) } as u32;

    let mut apic_ids = [0_u32; MAX_CPUS];
    let described = madt_apic_ids(madt, &mut apic_ids);
    if described == 0 {
        return Err("no_processors_described");
    }
    if !trampoline_page_is_free(handoff) {
        return Err("trampoline_page_not_conventional");
    }

    // SAFETY: CR3 is read-only here and the tables it names are the kernel's.
    let cr3: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    if cr3 > u64::from(u32::MAX) {
        // The 32-bit stub loads CR3 with a 32-bit move.
        return Err("cr3_above_4gib");
    }
    AW_AP_CR3.store(cr3, Ordering::Release);

    // SAFETY: the trampoline symbols bound a byte range inside `.text`, and the
    // destination page was just confirmed to be free conventional memory.
    unsafe {
        let start = core::ptr::from_ref(&aw_ap_trampoline_start);
        let end = core::ptr::from_ref(&aw_ap_trampoline_end);
        let len = end as usize - start as usize;
        core::ptr::copy_nonoverlapping(start, TRAMPOLINE_BASE as usize as *mut u8, len);
    }

    let mut started = 0;
    let mut online = 0;
    // Slot 0 belongs to the bootstrap processor whatever order the MADT lists
    // processors in, so slots are handed out here rather than taken from the
    // table's own indexing.
    let mut cpu = 0;
    for &apic_id in apic_ids[..described].iter() {
        if apic_id == bootstrap_apic_id {
            continue;
        }
        cpu += 1;
        if cpu >= MAX_CPUS {
            break;
        }
        if apic_id > u32::from(u8::MAX) {
            // INIT/SIPI would reach it, but the AP's 8-bit-destination era
            // tables would not; refuse rather than start a CPU that cannot be
            // addressed consistently later.
            continue;
        }

        // SAFETY: this AP's own stack, untouched until it runs.
        let stack_top = unsafe {
            let stack = core::ptr::addr_of!(AP_STACKS).cast::<ApStack>().add(cpu);
            (stack as u64 + AP_STACK_SIZE as u64) & !0xf_u64
        };
        AW_AP_STACK_TOP.store(stack_top, Ordering::Release);
        AW_AP_INDEX.store(cpu as u32, Ordering::Release);
        AP_REQUESTED_APIC_IDS[cpu].store(apic_id, Ordering::Relaxed);
        started += 1;

        // SAFETY: CPL0 with x2APIC enabled; the universal INIT-SIPI-SIPI
        // sequence, with the second SIPI retained because the first can be
        // lost on real hardware.
        unsafe {
            send_ipi(apic_id, ICR_INIT);
            spin(INIT_SETTLE_SPINS);
            send_ipi(apic_id, ICR_STARTUP | u64::from(SIPI_VECTOR));
            spin(SIPI_SETTLE_SPINS);
            if !AP_REPORTS[cpu].online.load(Ordering::Acquire) {
                send_ipi(apic_id, ICR_STARTUP | u64::from(SIPI_VECTOR));
            }
        }

        let mut waited = 0;
        while waited < ONLINE_TIMEOUT_SPINS && !AP_REPORTS[cpu].online.load(Ordering::Acquire) {
            waited += 1;
            core::hint::spin_loop();
        }
        if AP_REPORTS[cpu].online.load(Ordering::Acquire) {
            online += 1;
        }
    }

    Ok(SmpBringUp {
        described,
        started,
        online,
        bootstrap_apic_id,
    })
}
