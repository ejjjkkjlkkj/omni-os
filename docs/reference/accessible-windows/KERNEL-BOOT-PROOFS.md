# Kernel boot proofs

This document defines what the project accepts as evidence that a kernel
subsystem works, and records the proofs that currently exist.

## The rule

A component is never PASS because it compiled, and never PASS because a flag is
set in a data structure. For anything that runs on bare metal, PASS requires an
execution under QEMU/OVMF (or on hardware) that produced an observable marker
which the CPU could only have produced by actually doing the thing.

States are kept distinct: **PASS**, **PARTIAL**, **TO PROVE**, **TO BUILD**,
**BLOCKED**. Downgrading is normal: a subsystem that was PASS returns to TO
PROVE the moment its proof stops being replayed.

## Running the proofs

```bash
pwsh -NoProfile -File scripts/verify-windows.ps1
```

Host quality gates first (fmt, check, test, clippy on the workspace, plus clippy
on the kernel crate for every feature combination it ships - the kernel is
outside the workspace, so `clippy --workspace` never sees it), then:

```bash
pwsh -NoProfile -File scripts/Invoke-BootProofs.ps1
```

`Invoke-BootProofs.ps1` owns every marker assertion. It builds each kernel
configuration, boots it on a fresh QEMU/OVMF machine (q35, TCG, `-cpu max`), and
checks two lists per configuration:

- **required** markers that must appear, and
- **forbidden** markers that must not.

The forbidden list is what makes the suite honest. A boot that reaches
`AW_NATIVE_KERNEL_IDLE` while also printing `AW_MEMORY_PROTECTION_FAIL` is a
failure. So is one that prints `AW_NATIVE_EXCEPTION` on the normal path.

`scripts/Invoke-KernelBoot.ps1` is the single-run harness underneath: one
invocation builds one configuration, boots it, and returns the captured debug
log. Evidence lands in `target/boot-evidence/<name>/`, alongside the unstripped
kernel ELF for symbol-level triage.

## Current proofs

| Subsystem | State | Evidence marker |
|---|---|---|
| Fixed-base image load | PASS | `AW_KERNEL_IMAGE_HEADER_OK base=0x200000`, `AW_NATIVE_KERNEL_LOAD_OK … mode=fixed_base` |
| GDT + segment reload | PASS | `AW_GDT_SEGMENTS_RELOADED cs=0x…08 ss=0x…10` |
| IDT, 256 vectors | PASS | `AW_IDT_VECTOR_COUNT 32/256` |
| TSS 64-bit + IST1 | PASS | `AW_TSS_LOADED tr=0x18`, `AW_TSS_IST_READY vector=8 ist=1` |
| Real #UD delivery | PASS | `AW_NATIVE_EXCEPTION vector=6 name=invalid-opcode` |
| Real #DF on IST1 | PASS | `AW_DOUBLE_FAULT_IST_OK` (fault frame lands inside the IST range) |
| Kernel-owned page tables | PASS | `AW_VMM_CR3 prev=… new=…`, `AW_VMM_ACTIVE` |
| CPU protection bits | PASS | `AW_SECURITY_ENFORCED wp=1 nx=1 smep=1 smap=1 umip=1`, `AW_SECURITY_BASELINE_OK` |
| NX (no execute from data) | PASS | `AW_MEMORY_PROTECTION_OK name=nx-execute-data error_code=0x11` |
| NX (no execute from rodata) | PASS | `AW_MEMORY_PROTECTION_OK name=nx-execute-rodata error_code=0x11` |
| W^X (no write to `.text`) | PASS | `AW_MEMORY_PROTECTION_OK name=wx-write-text error_code=0x03` |
| Guard page below #DF stack | PASS | `AW_MEMORY_PROTECTION_OK name=guard-page error_code=0x00` |
| Persistent frame allocator | PASS | `AW_FRAME_ALLOCATOR_OK`; one owner backs the page tables and every later mapping |
| Runtime map/unmap (live tables) | PASS | `AW_VMM_MAP_OK` → `AW_VMM_MAP_READBACK_OK` → `AW_VMM_MAP_TRANSLATE_OK` → `AW_VMM_UNMAP_OK` |
| Runtime unmap negative test | PASS | `AW_VMM_UNMAP_FAULT_OK`; the unmapped address faults not-present after the TLB shootdown |
| Kernel heap / global allocator | PASS | `AW_HEAP_PROOF_OK`; `Box`/`Vec` allocate, a vector grows and sums, a freed block is reused, over-aligned allocations align |
| virtio-blk device read (legacy) | PASS | `AW_VIRTIO_BLK_PROOF_OK` (`virtio-blk`); one virtqueue reads sector 0 and it is a FAT boot sector |
| AHCI/SATA sector read (DMA) | PASS | `AW_AHCI_PROOF_OK` (`ahci`); brings up an AHCI HBA, finds the port with a disk, and reads LBA 0 by DMA (READ DMA EXT), checking the 0x55AA signature - the real controller model VMware and physical PCs use |
| AHCI/SATA sector write (DMA) | PASS | `AW_AHCI_WRITE_PROOF_OK` (`ahci-write`); writes a known pattern to LBA 0 of a dedicated scratch disk by DMA (WRITE DMA EXT) and reads it back byte-for-byte - never run against a data disk |
| NVMe controller identify | PASS | `AW_NVME_PROOF_OK` (`nvme`); the first memory-mapped controller - its BAR0, placed above the 4 GiB identity window by firmware, is mapped uncached into the page tables, the admin queue pair is set up and the controller enabled, then IDENTIFY CONTROLLER runs and the model number it wrote back by DMA (`AW_NVME_IDENTIFY_OK model=QEMU NVMe Ctrl`) is read from the result - real content, not a status bit |
| GPT partition table read | PASS | `AW_GPT_PROOF_OK` (`gpt`); reads the GPT header off an AHCI disk, validates the `EFI PART` signature and header CRC32, walks the entry array and identifies the first partition (the ESP by type GUID) - read-only, so it also runs on VMware's real GPT boot disk |
| GPT partition table write | PASS | `AW_GPTWRITE_PROOF_OK` (`gpt-write`); on a blank 8 MiB scratch disk, write a protective MBR, a primary and backup GPT header (each with its own CRC32) and an entry array with one ESP, then read the table back and validate both headers' CRCs and the ESP entry - and the independent reader then validates the same table (`AW_GPT_PROOF_OK`, ESP at LBA 34). The installer's partitioning half, gated so it never touches a data disk |
| File read through the full storage stack | PASS | `AW_FSPART_PROOF_OK` (`gpt`); reads a file from the ESP's own FAT16 filesystem at the partition offset the GPT reported - AHCI -> GPT -> partition -> FAT -> file, the way an installed system's own files are reached |
| FAT16 file read | PASS | `AW_FS_PROOF_OK` (`virtio-blk`); parse the BPB, find `HELLO.TXT`, follow its cluster chain, match the bytes |
| FAT16 file write | PASS | `AW_FATWRITE_PROOF_OK` (`fat-write`); on a dedicated scratch FAT16 disk over AHCI, create `NEWFILE.TXT` - allocate a free cluster, write the data, chain the FAT in every copy, add a root-directory entry - then read it back through the ordinary reader and match the bytes (`AW_FATWRITE_READBACK_OK`); the installer foundation, gated so it never touches a data disk |
| FAT16 format (mkfs) | PASS | `AW_MKFS_PROOF_OK` (`mkfs`); write a fresh empty FAT16 volume onto a blank scratch disk - boot sector/BPB, two FATs with their reserved entries, a zeroed root directory (cluster count computed into the FAT16 range) - then create a file in it and read it back (`AW_MKFS_READBACK_OK`); with the GPT writer, the kernel can build a whole disk from blank. Gated, scratch disk only |
| Disk build (installer capstone) | PASS | `AW_DISKBUILD_PROOF_OK` (`disk-build`); on one blank 8 MiB scratch disk the kernel writes a GPT, re-reads it to find the ESP (LBA 34), formats that partition as FAT16, writes `INSTALL.TXT`, then reads it back through the whole stack it just built - GPT -> partition -> FAT -> file (`AW_DISKBUILD_READBACK_OK`). This is what an installer does to provision a target disk, proven end to end from blank. Gated, scratch disk only |
| Userland ELF loader | PASS | `AW_USER_LOADER_PROOF_OK` (`virtio-blk`); read `USERPROG.ELF` off the FAT16 disk, map its PT_LOAD segment as user pages, run it at CPL3, and see it report 0xC0DE through a syscall and exit |
| Userland preemptive multitasking | PASS | `AW_USER_INIT_PROOF_OK` (`virtio-blk`); two spinner programs (`USERA.ELF`/`USERB.ELF`) loaded from disk, each on its own kernel stack, are preempted back and forth by the timer at CPL3 and their counters advance comparably - fair alternation, not one starved |
| virtio-net ARP exchange (legacy) | PASS | `AW_VIRTIO_NET_PROOF_OK` (`net`); reads its MAC from config, `AW_VIRTIO_NET_QUIET_OK` shows the receive ring idle while nothing is sent, then an ARP request draws the SLIRP gateway's reply (`spa=10.0.2.2`) |
| virtio-net ICMP echo (IPv4) | PASS | `AW_VIRTIO_NET_ICMP_PROOF_OK` (`net`); one layer up from ARP - a routed IPv4 datagram with its own header checksum carries an ICMP echo request to the gateway, addressed on the wire to the hardware address ARP just resolved, and the gateway's echo reply comes back with our exact identifier, sequence and 16-byte payload (`AW_VIRTIO_NET_ICMP_REPLY_OK`) |
| virtio-net DHCP exchange (UDP) | PASS | `AW_VIRTIO_NET_DHCP_PROOF_OK` (`net`); one layer up again - with no address yet, a broadcast DHCPDISCOVER (UDP with its pseudo-header checksum) draws SLIRP's built-in DHCP server's DHCPOFFER, matched by the 32-bit transaction id echoed back and carrying the leased address (`AW_VIRTIO_NET_DHCP_OFFER_OK yiaddr=10.0.2.15`) |
| 16550 serial console (COM1) | PASS | `AW_SERIAL_PROOF_OK` (`serial`); loopback self-test, then a banner appears in the host COM1 log |
| APIC timer IRQ delivery | PASS | `AW_APIC_TIMER_FIRED`, `AW_APIC_TIMER_MONOTONIC_OK ticks>=8` |
| APIC timer negative test | PASS | `AW_APIC_TIMER_MASKED_STOPPED` then `AW_APIC_TIMER_UNMASKED_RESUMED` |
| MADT parse + ISA IRQ override | PASS | `AW_IOAPIC_ROUTED isa_irq=0 gsi=2` (the override, not the IRQ number) |
| I/O APIC device IRQ delivery | PASS | `AW_IOAPIC_IRQ_FIRED`, `AW_IOAPIC_IRQ_MONOTONIC_OK ticks>=8` |
| I/O APIC negative test | PASS | `AW_IOAPIC_MASKED_STOPPED` then `AW_IOAPIC_UNMASKED_RESUMED` |
| MSI delivery | PASS | `AW_MSI_FIRED`, `AW_MSI_MONOTONIC_OK ticks>=8` (`msi-smoke`) |
| MSI negative test | PASS | `AW_MSI_MASKED_STOPPED` then `AW_MSI_UNMASKED_RESUMED` |
| MSI-X | TO BUILD | - |
| INTx routing through ACPI `_PRT` | TO BUILD | - |
| SMP bring-up (INIT-SIPI-SIPI) | PASS | `AW_SMP_ONLINE online=3 started=3` (`smp`, `-smp 4`) |
| Per-CPU GDT, TSS and IST | PASS | `AW_SMP_PER_CPU_TABLES_OK cpus=3` (distinct GDT/TSS/IST1 per CPU) |
| AP identity | PASS | `AW_SMP_AP_ONLINE apic_id=N requested=N`, read by the AP from its own APIC |
| Per-CPU state via `GS` | PASS | `AW_PERCPU_PROOF_OK cpus=4` (`smp`); each CPU's block reached through `gs:[0]`, index/APIC id distinct per CPU |
| Per-CPU interrupt counters | PASS | `AW_PERCPU_BSP ... timer_ticks>=8 device_ticks>=8`, counted into the block of the CPU that ran the ISR |
| Per-CPU timer armed on an AP | PASS | `AW_PERCPU_AP_TIMER_OK aps=3` (`smp`); each AP arms its own Local APIC timer, idles under `sti`/`hlt`, and its own block's `timer_ticks` advances |
| Per-CPU #DF on an AP's IST | PASS | `AW_DOUBLE_FAULT_IST_OK` (`ap-double-fault-smoke`); an application processor forces a real #DF and the fault frame lands inside *that CPU's own* IST1 range, range-checked against the per-CPU block's bounds (not the bootstrap processor's) |
| Scheduler / anything running on an AP | PASS | `AW_AP_SCHED_PROOF_OK cpu=1 threads=2` (`ap-scheduler-smoke`); an application processor runs two kernel threads that context switch and take turns, before going on to report online and idle under its own timer - it no longer only parks in `hlt` |
| Ring 3 entry (`iretq` to CPL3) | PASS | `AW_RING3_PROOF_OK`; a mapped user page runs at CPL3, its syscall's saved RIP lands inside the user page |
| Versioned syscall ABI (`sysret`) | PASS | `AW_SYSCALL_ABI_PROOF_OK version=1`; dispatch table, `SYS_ADD`=5, `SYS_EXIT`, each returning via `sysret` |
| Validated user-pointer copy | PASS | `AW_SYSCALL_WRITE copied=13`; `SYS_WRITE` bounds-checks the user pointer and copies it in with SMAP `stac`/`clac` |
| Cooperative scheduler / threads | PASS | `AW_SCHED_PROOF_OK threads=3 switches=29`; three kernel threads context switch and take ten turns each |
| Monotonic TSC clock | PASS | `AW_CLOCK_PROOF_OK`; TSC monotonic, frequency calibrated against a PIT channel-2 one-shot |
| CMOS RTC wall clock | PASS | `AW_RTC_PROOF_OK` (`normal`); reads the date/time from the CMOS RTC over ports 0x70/0x71, waiting out any update-in-progress and accepting only a stable reading, decodes BCD/12-hour per Status Register B, and sanity-checks a plausible calendar date - read-only, so it runs on the normal boot path (and on VMware) |
| HDA audio playback | PASS | `AW_HDA_PLAYBACK_PROOF_OK` (`hda`); the machine's real audio path, and what will carry spoken screen-reader output. Find the Intel HD Audio controller on PCI, bring it out of reset, stand up the CORB/RIRB command/response rings, and read the codec's vendor/device id back over the ring (`AW_HDA_CODEC_ID vendor_device=0x…1af40012`, real data the codec produced). Then walk the codec's widget graph to a DAC and an output pin (`AW_HDA_OUTPUT dac= pin=`), configure them, program output stream 0 with a BDL over a PCM tone buffer, start it, and prove the link position advances (`AW_HDA_DMA_ADVANCED position=`) - the controller is streaming samples from memory by DMA, not holding a status bit. QEMU `intel-hda` + `hda-output`; audibility on physical hardware is a separate validation. The same driver runs at the firmware stage (see "Firmware-stage HDA audio"); this kernel instance is what will carry the installer's spoken output |
| Firmware-stage spoken screen reader | PASS | `AW_UEFI_AUDIO_SPEAK bytes=…` (`hda`); the UEFI screen reader speaks its lines aloud, in words, through the machine's real audio codec, before the kernel exists - Machado & Vieira (arXiv:1712.03186) realized and taken past where they stopped (they left DMA open and validated only the codec beep). It finds the HDA controller, brings it up, reads the codec id over CORB/RIRB (`AW_UEFI_HDA_CODEC_ID vendor_device=`), walks to a DAC + output pin (`AW_UEFI_HDA_READY dac= pin=`), and for each boot line streams a pre-recorded PCM speech clip by DMA (`AW_UEFI_AUDIO_SPEAK bytes=`, one per line: "Accessible Windows", "Screen reader active…", "Starting…", "Loading the operating system"). Clips are synthesized offline by `scripts/gen-speech.ps1` and embedded. On a machine with no HDA controller it falls back to the PC-speaker chime, so the boot is audibly confirmed on thin laptops (real codec, spoken words) and older desktops (buzzer) alike. Sound is not captured headless - the proof is the codec answering and the clips streaming (link position advancing); audible speech on physical hardware is a separate validation |
| Firmware-stage audible cue | PASS | `AW_UEFI_SND_PROOF_OK` (`normal`); the first sound of Accessible Windows, before the kernel exists: a startup chime through the PC speaker (PIT channel 2 + port 0x61, the same timer the clock calibration uses), so a blind user hears the accessible boot come up whether or not they can see the console, plus keyboard cues during review. Headless QEMU captures no audio, so the proof is that the speaker interface was actually driven - port 0x61 read back gated (`AW_UEFI_SND_GATED`) after two ascending tones (`AW_UEFI_SND_TONE freq=660/990`) and silenced afterward (`AW_UEFI_SND_SILENCED`). A machine with no beeper stays silent yet still passes (the port bits latch); spoken words, on the machine's real speakers, await the HDA-audio + speech-synthesis work. Audibility on physical hardware is a separate validation |
| Firmware-stage screen reader | PASS | `AW_UEFI_SR_PROOF_OK` (`normal`); the native screen reader speaks - and is operable - before the kernel is even loaded, inside the UEFI boot application (after GOP detection, before ExitBootServices). It builds the boot screen's semantic tree, validates every node against the accessibility invariants, and voices it through the *same* `aw-screen-reader`/`aw-accessibility` engine the kernel and installer use - so the wording cannot drift between boot and desktop. The utterances go to the **visible** UEFI text console (`ConOut`, i.e. the physical screen on real hardware), not only the debug console the markers are asserted on: `AW_UEFI_SR_SPEAK "Accessible Windows, window"`, `AW_UEFI_SR_SPEAK "Screen reader active at firmware stage"`, `AW_UEFI_SR_SPEAK "Display <w> by <h>"` (the *real* GOP mode, so unpinned), `AW_UEFI_SR_SPEAK "Loading the operating system"`. It then presents a **complete, operable accessible Setup Utility with real menus and submenus** (`AW_UEFI_SR_READY`), modeled on real firmware setups (AMI Aptio, the ASUS UEFI BIOS Utility) but spoken and usable without sight: five tabs - **Main, Advanced, Boot, Security, Save and Exit** (`AW_UEFI_SETUP_TAB "Boot, tab, 3 of 5"`) - and, under them, submenus you descend into and back out of (CPU Configuration, Boot Option Priorities, Secure Boot; `AW_UEFI_SETUP_MENU "…"`), each with a settings list, a per-item help line and a key legend. Left/Right change tab, Up/Down move the highlight, Home/End jump to the ends, Enter opens a submenu or selects, Escape steps back out (and boots normally at the top). Every fixed screen, submenu title and action is **spoken through the real HDA codec** from a pre-recorded clip in one consistent voice, with an instructions clip on entry; the **screen-reader keys** a blind user expects work on every screen - **Space** re-reads the item, **A** *reads the whole screen* top to bottom, **H**/**F1** reads its help (`AW_UEFI_SETUP_HELP`), **W** says where they are, and **S** *spells the focused line character by character* (`AW_UEFI_SETUP_SPELL`) from a synthesized A-Z/0-9 alphabet, the answer to reading a runtime-composed device name aloud by its exact letters - and speech is **interruptible (barge-in)**: any key cuts the current clip short instead of talking over the next action. Main/Advanced/Security **read and speak** real machine state - the **SMBIOS system identity** a real BIOS shows (`System <manufacturer> <product>`, `Serial number`, `BIOS <vendor> <version> (<date>)`, Type 0/1), plus firmware vendor/version, RTC time, memory, display mode, CPU brand, **virtualization support** via CPUID + `IA32_FEATURE_CONTROL`, and Secure Boot/Setup Mode - the settings a blind user could never reach in a silent firmware. The **Boot** tab enumerates the machine's *real* boot options from the firmware's own `BootOrder`/`Boot####` variables (`AW_UEFI_BOOT_ENUM count=`, one `AW_UEFI_BOOT_ENTRY index= id= "…"` per active option - under QEMU/OVMF: `UiApp`, `UEFI QEMU HARDDISK`, `EFI Internal Shell`; on VMware EFI: `EFI VMware Virtual SATA Hard Drive`, `EFI Internal Shell`), shows `BootCurrent`/`Timeout`, and opens on the safe default `AW_UEFI_MENU_ITEM "Boot normally, menu item, 1 of N"`; for any device a blind user can **boot it now** (sets `BootNext` and restarts), **make it the persistent default** or **move it up/down** in the order (rewrites `BootOrder`), and **Save and Exit** offers Boot normally, **Enter firmware setup** (through `OsIndications`), **Reset** and **Shut down** (UEFI runtime `ResetSystem`). Every irreversible action (booting a device, entering firmware setup, reset, shut down) requires a **spoken confirmation** first (`AW_UEFI_SETUP_CONFIRM`; Enter confirms, Escape cancels) so a single accidental key never reboots the machine - WCAG 2.2 SC 3.3.4 error prevention, realized in firmware - and every persistent change says "Done" rather than only a tone. This is the one interaction that works on *every* machine: the keyboard here is the firmware's own, so a **USB keyboard works before any kernel USB stack exists**. With no key pressed (the proof harness presses none) the countdown boots normally on its own (`AW_UEFI_SR_CONTINUE reason=timeout`), so an unattended boot always proceeds. Operable, nonvisual delivery evidence, universally - the whole firmware setup, spoken, from the first screen through the choice of what happens next |
| Native screen reader speech | PASS | `AW_SR_PROOF_OK` (`normal`); builds the semantic tree of the installer's first screen, validates every node against the accessibility invariants, and emits the exact utterance the announcement engine hands a speech/braille device for each control in focus order (`AW_SR_SPEAK "Enable screen reader at boot, check box, checked"`, `AW_SR_SPEAK "Speech rate, slider, 40%"`, ...), plus a state-change event (`AW_SR_EVENT "not checked"`) - nonvisual delivery evidence that a blind user is told what each control is, in words. Wording unit-tested in `aw-screen-reader`; proven to run unchanged in the kernel |
| Screen reader keyboard navigation | PASS | `AW_SR_NAV_PROOF_OK` (`normal`); Tabs across the installer's four focus stops in order - skipping a heading, static text and a disabled control - speaking each landing with its position (`AW_SR_TAB "Speech rate, slider, 40%, 2 of 4"`), wraps from the last stop back to the first (`AW_SR_TAB_WRAP`), then Shift+Tabs backward (`AW_SR_SHIFT_TAB`): deterministic keyboard semantics with spoken feedback, the whole operable loop with no pointer or visual cue. `FocusRing` unit-tested in `aw-screen-reader` |
| Braille rendering | PASS | `AW_BRAILLE_PROOF_OK` (`normal`); runs the pipeline end to end - semantic node, then the utterance the screen reader speaks, then Grade 1 six-dot braille via `aw-braille` - and checks the cells for "Install, button" against the known pattern before emitting them as hex (`AW_BRAILLE_CELLS 20 0a 1d ...`, the display transport's bytes) and Unicode glyphs (`⠠⠊⠝⠎⠞⠁⠇⠇⠂⠀⠃⠥⠞⠞⠕⠝`). Braille delivery evidence for a deaf-blind user; translation unit-tested |
| Preemptive scheduling | PASS | `AW_PREEMPT_PROOF_OK threads=3 switches=12`; three kernel threads that never yield are switched by the timer interrupt alone (each advances a counter), in exactly twelve timer-driven context switches, and the timer proof either side still passes |
| Ring 3 preemption / user scheduler | PASS | `AW_RING3_PREEMPT_PROOF_OK`; a CPL3 user thread that never makes a syscall spins on a counter with interrupts enabled, the timer preempts it (conditional `swapgs` keeps the per-CPU GS correct), and after six timer-driven switches the kernel takes control back on its own |
| User-page `swapgs` on entry | PASS | `AW_SWAPGS_PROOF_OK`; CPL3 runs on a distinct user `GS` base, and the syscall entry's `swapgs` makes `gs:[0]` reach this CPU's real per-CPU block (null if the swap were missing) |
| PS/2 keyboard input (IRQ1) | PASS | `AW_KBD_PROOF_OK` (`normal`); the input half of an accessible boot - a machine a blind user cannot drive is not accessible. Brings up the 8042 controller (self-test `0xAA`->`0x55`, keyboard interface test `0xAB`->`0x00`, config byte read back with the keyboard IRQ and set-1 translation bits set: `AW_KBD_CONTROLLER_OK`), routes ISA IRQ 1 through the same I/O APIC the device-IRQ proof used (`AW_KBD_ROUTED gsi=1 vector=`), then proves the *real* delivery-and-decode path: the controller's own `0xD2` command ("present this byte as keyboard input") injects a scancode that raises a genuine IRQ1 edge - the exact path a keypress takes (device -> output buffer -> IRQ1 -> ISR -> port 0x60 read -> set-1 decode) - and the handler must receive it and decode it to the expected key (`AW_KBD_KEY name=space`/`enter`/`up`/`down`, the last two through the `0xE0` extended prefix). A mask test proves the counter is driven by delivery not polling (`AW_KBD_MASKED_STOPPED`), and delivery resumes once unmasked (`AW_KBD_UNMASKED_RESUMED`). On hardware with "USB legacy support" (the firmware default on most desktops and many laptops) a USB keyboard is presented through this same 8042 path, so this drives it with no USB stack; real keys take the identical path |
| Kernel spoken menu output | PASS | `AW_HDA_SPEECH_PROOF_OK` (`hda`); the accessible menu's voice on real hardware. After the tone proof, the same codec is set to the 24 kHz mono speech format (`AW_HDA_SPEECH_READY`) and a real pre-recorded menu clip is streamed by DMA, the link position advancing (`AW_HDA_SPEECH_DMA_ADVANCED position=`) exactly as the tone did - so a blind user hears each menu item spoken through the machine's actual audio codec, not only the (often absent) PC speaker. Clips are synthesized offline by `scripts/gen-menu-speech.ps1` (Windows SAPI, English voice) to match the wording `aw-screen-reader` produces, and embedded in the kernel; the live menu plays the focused item's clip as the selection moves and the title clip when it opens. On a machine with no HDA controller this reports `AW_HDA_SPEECH_UNAVAILABLE` and the menu falls back to its on-screen focus bar. Sound is not captured headless, so the proof is the codec answering and the clip streaming (link position advancing); audibility on a physical speaker is a separate validation |
| Accessible boot menu | PASS | `AW_MENU_PROOF_OK` (`normal`); the first screen the user drives. A keyboard-navigable menu rendered on the framebuffer with the focused item shown as a solid selection bar (visible without colour vision), each landing voiced through the same `aw-screen-reader` engine as the installer and desktop so the wording never drifts (`AW_MENU_SPEAK "System information, menu item, 2 of 3"`). The proof drives the *same* handler the live menu uses with a fixed key script and checks every landing and the activated action (`AW_MENU_FOCUS index=`, `AW_MENU_SELECT name=`), including wrap-around (`AW_MENU_WRAP_OK`); the keyboard's real IRQ-to-key path is proved in the row above, so together they cover the whole key-to-action loop. After the idle marker the kernel enters this menu live (`AW_MENU_INTERACTIVE_BEGIN`) on the real keyboard - Up/Down or Tab move, Enter selects, Reboot pulses the 8042 reset line - flushing any stale input first so nothing auto-selects. Under headless boot no key arrives, so it parks under `hlt` after the proof has already passed |
| Framebuffer text console | PASS | `AW_FBCON_PROOF_OK` (`normal`); the first post-firmware output that survives on real hardware. Every kernel marker goes to the `0xE9` debug port and COM1 - both QEMU/dev-board fixtures absent on a laptop - and the UEFI stage's visible text (`ConOut`) is gone after ExitBootServices, so without this a physical boot shows nothing after the firmware hands off. The console takes the linear framebuffer the loader described (`AW_FBCON_READY width= height=`), clears it, and renders readable text with the public-domain 8x8 font: a banner the instant the kernel takes over (before anything that could hang), the screen-reader's spoken lines mirrored as they are voiced, and a closing "System ready" at idle. The proof is not "we wrote some pixels": it draws a known glyph (`A`) at a known origin and reads *every* pixel of it back from framebuffer memory, decoded into the framebuffer's own channel order, requiring each to match the scaled font bitmap - `AW_FBCON_GLYPH_READBACK_OK char=A lit=28` (the exact lit-pixel count of the `A` bitmap), which a uniform/blank buffer cannot produce. Inert with an explicit `AW_FBCON_UNAVAILABLE` when the firmware exposes no directly writable RGB/BGR framebuffer, so it never guesses at memory it was not given. Headless QEMU still renders to the GOP framebuffer, so the read-back runs in CI; legibility on a physical panel is a separate validation |
| Physical hardware boot | TO PROVE | never run on real hardware from this tree |

Error codes above are `#PF` error codes (Intel SDM 4.7): bit 0 present, bit 1
write, bit 2 user, bit 4 instruction fetch. `0x11` is *present + instruction
fetch*, which is what NX produces - and is deliberately distinguished from
`0x10`, a fetch from a page that simply is not mapped, which would prove nothing
about NX.

## Why the proofs are shaped this way

**The APIC timer counts, and the counter is checked against masking.** A
monotonically increasing counter on its own does not distinguish a real ISR from
a polling artefact. The proof requires several deliveries, then masks
`LVT_TIMER` and requires the counter to freeze, then unmasks it and requires it
to move again.

**A device interrupt is proved on a device, not on the local APIC.** The timer
proof shows the CPU taking an interrupt the CPU itself generated, which says
nothing about the path a peripheral uses. The I/O APIC proof drives the 8254,
so the interrupt has to leave a device, cross a redirection entry, and arrive on
the vector that entry names. The pin is not assumed either: the MADT's interrupt
source overrides are applied, and the log records `isa_irq=0 gsi=2` - the case
where taking the IRQ number for the global system interrupt number would have
silently programmed the wrong pin.

**The MSI proof pokes the device throughout the masked window.** A periodic
timer keeps firing on its own, so masking it and watching the counter freeze is
enough. A device only fires when asked, so a frozen counter would prove nothing
if nobody were asking. The proof therefore keeps requesting interrupts for the
whole masked window: the counter staying still means the device's own MSI enable
bit suppressed interrupts that were actively being requested.

The `msi-smoke` configuration is the only one that builds a driver for QEMU's
`edu` device, behind the `msi-proof-device` feature. It exists because `edu` can
be asked to raise an interrupt without first implementing a real controller's
command protocol; everything it exercises - the capability walk, the message
encoding, bus mastering, the vector plumbing - is what an NVMe or xHCI driver
will use unchanged.

**The virtio-net proof makes the reply attributable to the request.** A card both
consumes and produces buffers, so a frame appearing in the receive ring proves
nothing unless it can be tied to something the guest did. The `net` proof posts
its receive buffers, then holds a window in which it sends nothing and requires
the receive ring to stay empty (`AW_VIRTIO_NET_QUIET_OK`) - the same shape as the
MSI masked window. Only then does it broadcast an ARP request, and it accepts the
result only if a reply comes back with opcode 2 and sender address 10.0.2.2, the
SLIRP gateway that could only answer because the request really left the guest.

**An application processor is online only if it says so itself.** A counter the
bootstrap processor increments after sending a SIPI proves that a SIPI was sent.
Each AP instead reports the APIC ID it read from *its own* local APIC, plus the
GDT, TSS and IST1 addresses it actually loaded, and the suite requires those to
match the CPU that was asked for and to differ from every other CPU's. Sharing a
TSS between two CPUs is not a subtle bug - the busy bit `ltr` sets makes the
second `ltr` a `#GP`, and two CPUs faulting onto one IST stack corrupt each
other - so "the tables are private" is checked rather than assumed.

**The memory protections fault on purpose.** Page-table flags describe an
intention; only a `#PF` with the right error code shows the CPU enforcing it.
Each probe arms a narrow expectation in the exception handler (one vector, one
page), performs the offending access, and resumes at a recovery label. A probe
that does *not* fault reports `no-fault` and fails the suite - the dangerous
outcome here is silence, not a crash.

This is also why `common_exception_entry` restores the full context and
`iretq`s rather than halting: faults have to be recoverable for the probes to
work, and user-mode fault handling will need exactly the same machinery.

## Known limitations

- The proofs run under TCG emulation. Timing-sensitive behaviour and
  vendor-specific errata are not covered; hardware validation on one AMD and one
  Intel platform remains a separate, unmet requirement.
- The identity map covers the low 4 GiB. A machine whose firmware leaves the
  kernel stack or the framebuffer above that boundary is not yet supported.
- Only the #DF emergency stack has a guard page. The bootstrap kernel still runs
  on the stack the UEFI loader handed over; guarding it requires the kernel to
  allocate and switch to its own stack first.
- Application processors park in `hlt` with interrupts masked, on a fixed
  bound of 8 CPUs. Their #DF stacks have a guard *page* but not a guard
  *hole*: the kernel's page tables were built before those stacks existed, so
  an AP stack overflow is currently silent where the bootstrap processor's
  faults. An AP is also never sent an interrupt, so its IDT is loaded but
  unexercised.
- MSI is proved on one emulated device with one vector. MSI-X, multiple
  vectors per device, and per-vector masking are untouched, as is INTx routing
  through the ACPI `_PRT` - which needs an AML interpreter, so a device without
  MSI cannot currently be routed at all.
- The kernel image is linked non-relocatable at 2 MiB. If firmware ever owns
  that range the loader fails the boot loudly (`reason=fixed_base_unavailable`)
  rather than misloading; a relocatable or higher-half image is the long-term
  answer.

## Second hypervisor: VMware Workstation

Every proof above runs under QEMU/OVMF. The dossier (sections 3.1 and 20) also
asks for validation on other hypervisors on the way to real hardware.
`scripts/Invoke-VMwareBoot.ps1` boots the same `dist` image under **VMware
Workstation** - its own EFI firmware and a virtual SATA controller, a completely
different firmware and device model than QEMU/OVMF - with COM1 routed to a file:

```bash
pwsh -NoProfile -File scripts/Invoke-VMwareBoot.ps1
```

| Subsystem | State | Evidence marker |
|---|---|---|
| Full clean boot on VMware EFI | PASS | `AW_VMWARE_BOOT_OK`; the native kernel reaches `AW_NATIVE_KERNEL_IDLE` on VMware with no `AW_NATIVE_EXCEPTION`/`AW_NATIVE_KERNEL_PANIC`, and the serial banner appears exactly once (no crash-reboot loop) - VMware's firmware booted `\EFI\BOOT\BOOTX64.EFI`, the loader handed off, and every native proof (W^X, Ring 3, swapgs, scheduler, APIC timer, clock, PCIe scan) ran on it |
| Framebuffer console on VMware GOP | PASS | `AW_FBCON_PROOF_OK` in the COM1 capture; `AW_FBCON_READY width=1024 height=768` on VMware's own SVGA framebuffer, and the rendered `A` glyph reads back with all 28 lit pixels - the on-screen console works on a different GOP than OVMF's |
| PS/2 keyboard on VMware i8042 | PASS | `AW_KBD_PROOF_OK` in the capture; VMware's own 8042 self-tests, routes IRQ1, and delivers every injected scancode (space/enter/up/down) through a real IRQ with the mask/resume test intact - keyboard input works on a second, independent controller model |
| Accessible menu on VMware | PASS | `AW_MENU_PROOF_OK` then `AW_MENU_INTERACTIVE_BEGIN`; the menu navigates and the live loop arms the keyboard for the user. HDA is absent from the VMX, so spoken output reports `AW_HDA_SPEECH_UNAVAILABLE` and the menu falls back to its on-screen focus bar - the documented graceful degradation, not a failure |

0xE9 debugcon is a QEMU/Bochs convenience VMware does not have, so `debug_write`
mirrors every marker onto the real 16550 once `serial::prove` confirms it, and the
whole boot is captured on the guest COM1.

**What it took.** The first attempt reboot-looped right after the serial banner.
With the markers mirrored to COM1 the crash was pinned to the CR3 switch: the
kernel's identity map marks every non-code page NX, but the NX bit is only valid
with `EFER.NXE` set. QEMU/OVMF leaves it on; VMware's EFI leaves it off, so NX was
a reserved bit and the first stack access on the new map raised a reserved-bit
`#PF` and triple-faulted. The kernel now enables `EFER.NXE` itself before the map
goes live rather than trusting the firmware - a portability fix that matters for
real hardware too, not just VMware. The `physical=45 linear=48` address widths and
`vendor=amd` in the capture confirm this is a genuinely different CPU model than
the QEMU runs.
