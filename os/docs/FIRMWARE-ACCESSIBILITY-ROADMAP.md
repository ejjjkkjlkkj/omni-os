# Firmware-stage accessibility: what ships, and the roadmap

Accessible Windows speaks and is operable from the UEFI boot application, before the
kernel loads — see the "Firmware-stage screen reader" row in
[KERNEL-BOOT-PROOFS.md](KERNEL-BOOT-PROOFS.md). This note records what is implemented and
the gaps that remain, from a survey of the state of the art (no mainstream firmware
vendor ships a talking BIOS; only research prototypes and Apple's post-firmware VoiceOver
exist).

## Implemented

- **Universal, self-built audio** — UEFI has no standard audio protocol, so the project
  builds its own drivers rather than depending on any vendor, behind a pluggable backend
  (`boot/uefi/src/audio.rs`): **Intel HDA** and **AC'97** both stream pre-recorded PCM by
  DMA (Machado & Vieira, [arXiv:1712.03186](https://arxiv.org/abs/1712.03186), realized and
  taken past where they stopped at the codec beep), and the **PC speaker** is the universal
  last resort. The same clips are spoken on whatever a machine has; the active backend is
  reported (`AW_UEFI_AUDIO_BACKEND channel=hda|ac97|pc_speaker`).
- **A complete, tabbed Setup Utility with submenus**, modeled on AMI Aptio / the ASUS UEFI
  BIOS Utility (Main, Advanced → CPU Configuration, Boot → Boot Option Priorities →
  device, Security → Secure Boot, Save and Exit), spoken and keyboard-operable.
- **Real machine state, read and spoken**: SMBIOS system identity (manufacturer, product,
  serial, BIOS vendor/version/date — Type 0/1), firmware vendor/version, UEFI revision,
  RTC time, installed memory, display mode, CPU brand, virtualization support (CPUID +
  `IA32_FEATURE_CONTROL`), Secure Boot / Setup Mode, `BootCurrent`, `Timeout`.
- **Real boot actions**: enumerate `BootOrder`/`Boot####`; boot a device now (`BootNext`),
  make it the persistent default or move it up/down (`BootOrder`); enter the firmware's own
  setup (`OsIndications`); reset / shut down (`ResetSystem`).
- **Screen-reader affordances**: one consistent voice for all fixed scaffolding, an
  instructions clip on entry, repeat (Space), read-all (A), help (H/F1), where-am-I (W),
  first/last (Home/End), spell-by-character (S) from a synthesized A–Z/0–9 alphabet, and
  interruptible speech (barge-in).
- **COM1 serial mirror** so VMware and physical hardware capture the same `AW_UEFI_*`
  markers as QEMU's 0xE9 debug port.
- **Runtime speech synthesis** for arbitrary dynamic text — a from-scratch Klatt-style
  cascade formant synthesizer (`boot/uefi/src/synth.rs`), so the enumerated boot-device
  names, CPU brand, memory sizes, resolutions and firmware setting values are spoken as
  *words*, not just spelled. **Bilingual**: English letter-to-sound rules drive the word path
  in English; in French, a full French grapheme-to-phoneme frontend and phoneme inventory
  (nasal vowels and all), ported from the companion Sintaise UEFI TTS, pronounce the setup's
  French labels and values (`AW_UEFI_SYNTH_SPEAK "Système, QEMU Standard PC…"` proven on OVMF).
  Numbers are read in words in both languages (including the irregular soixante-dix /
  quatre-vingts). It emits the same 24 kHz mono PCM the codecs already stream, so nothing new
  sits below it; the speech rate and pitch are adjustable live (`[`/`]`, `,`/`.`). Proven at
  boot on OVMF (`AW_UEFI_SYNTH_SELFTEST`). Honest scope: intelligible and robotic, like early
  DECtalk — the right trade for understanding a value you otherwise could not hear at all.
- **TPM and Secure Boot key state** — the TCG2 TPM presence/PCR-bank state and the PK/KEK/db/dbx
  certificate counts are read and spoken (Security submenu and agent), proven headless as
  `AW_UEFI_SECURITY`.

## Roadmap (status of the surveyed gaps)

Most of what this section once listed as future work is now built and proven; each item below
says what shipped and what, if anything, remains. The one genuinely large piece still open is a
USB host-controller (XHCI) driver with isochronous support, needed only for USB Audio Class.

- **Audio hardware coverage beyond HDA and AC'97**: **VirtIO-sound is done** — a modern
  VirtIO 1.x PCI driver (`boot/uefi/src/virtio_snd.rs`) that negotiates the device, sets up the
  control and TX split virtqueues, and streams PCM through the virtio-snd handshake; proven on
  QEMU (`-device virtio-sound-pci`), where `AW_UEFI_AUDIO_BACKEND channel=virtio` and the boot
  clips play as `AW_UEFI_VIRTIO_SND_PLAY`, captured to WAV. **USB Audio Class**: a from-scratch
  xHCI host-controller driver (`boot/uefi/src/usb_audio.rs`) - EDK II's `UsbIo` returns
  `EFI_UNSUPPORTED` for the isochronous transfers audio needs, the exact wall the 2021 GSoC
  effort hit, so this brings up its own controller (rings, DCBAA, scratchpads, run), enumerates
  the device (Enable Slot, Address Device, EP0 control transfers), and configures the
  isochronous endpoint and `SET_INTERFACE`. Proven that far on QEMU (`-device usb-audio`):
  `AW_UEFI_XHCI_RUNNING`/`_SLOT`/`_ADDRESSED`, `AW_UEFI_USB_AUDIO_DESC vid=0x46f4`,
  `_EP_CONFIGURED`, `_STREAMING` - already past where the GSoC work stopped. Remaining: the final
  isochronous data burst trips QEMU's host-controller-error bit (`_PLAY completed=0`), not yet
  resolved. Tracks the still-unstandardized UEFI audio
  work (no audio output protocol in the UEFI spec as of 2.11, Dec 2024; see the GSoC effort and
  [tait.tech/blog/uefi-audio](https://tait.tech/blog/uefi-audio/)).
- **HII integration** — *done* for settings: the firmware's own HII database is parsed
  ([UEFI 2.11 ch. 33](https://uefi.org/specs/UEFI/2.11/33_Human_Interface_Infrastructure.html)),
  so the settings only the firmware owns (SATA mode, XMP, CSM, …) are enumerated, their values
  read (by NVRAM variable or the Config Routing export), spoken by meaning, and changed by name
  via SetVariable or RouteConfig (`boot/uefi/src/hii_ifr.rs`, `setup.rs`). What remains is
  presentation parity with the firmware's *own* form layout (grouping, dependency expressions);
  the settings themselves are already reachable and voiced.
- **Pre-boot braille** via a USB HID Braille display
  ([HUTRR78](https://usb.org/sites/default/files/hutrr78_-_creation_of_a_braille_display_usage_page_0.pdf))
  — *built* (`boot/uefi/src/usb.rs`). Rather than write an XHCI driver, it uses the firmware's
  own USB stack through `EFI_USB_IO_PROTOCOL` (alive during boot services), enumerates every USB
  device, detects a braille display by the Braille usage page in its HID report descriptor, and
  sends `aw-braille` cells as a HID output report. The enumeration and discriminating detection
  are proven on QEMU (`AW_UEFI_USB_DEVICE`/`AW_UEFI_USB_SUMMARY`: a USB keyboard and mouse are
  enumerated and correctly classified `braille=false`); the cell send is exercised on a real
  display (QEMU emulates only a Baum *serial* display, a different transport). BRLTTY is
  post-kernel only.
- **More screen-reader depth**: *done* — adjustable rate/volume/pitch, phonetic Alpha/Bravo
  spelling, cycled verbosity levels (`V`), punctuation levels (`X`), read-by-word of the focused
  line (`O`), and an independent review cursor (`N`/`B` to survey lines without moving the
  selection, `G` to route focus there); read-by-line falls out of the review cursor. All proven
  driven from the keyboard under QEMU (`AW_UEFI_REVIEW`, `AW_UEFI_VERBOSITY`, …). What a future
  pass could still add: key/character echo toggles and independent character-review within a line.
- **More real UEFI settings**: *done* — setting the RTC clock (raw `SetTime` from the agent,
  "set time 14:30" / "set date 2026-09-21"), the `Driver####`/`SysPrep####` load lists (read
  and spoken, `AW_UEFI_LOADOPTS`), and richer Secure Boot key/certificate state (PK/KEK/db/dbx)
  and TPM presence.

## Standards framing

EN 301 549 (Chapter 5) and Section 508 (§402.2) require **built-in speech output** for *closed
functionality* — systems that do not permit assistive technology to attach, which a BIOS/UEFI
setup is (you cannot load NVDA/JAWS/VoiceOver before there is an OS). Section 508 §402.2 states
that such ICT "shall be speech-output enabled … for full and independent use by individuals with
vision impairments," which in practice "means ensuring that the ICT has built-in speech output."
A firmware setup that speaks itself is exactly what those standards call for.

For the full prior-art comparison and citations — the 2017 beep-only prototype, the 2021 GSoC
EFI-audio effort that never made sound and was never merged, the absence of any UEFI audio
protocol through 2.11 (2024), and why Apple's Recovery VoiceOver and the GRUB/BRLTTY tradition
are post-firmware rather than firmware-stage — see
[PRIOR-ART-AND-SIGNIFICANCE.md](PRIOR-ART-AND-SIGNIFICANCE.md).
