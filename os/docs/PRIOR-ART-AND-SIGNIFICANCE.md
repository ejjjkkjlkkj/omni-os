# Prior art and significance: an accessible firmware, grounded

This note places the project's firmware-stage accessibility work in the context of the
published state of the art, the relevant law and standards, and the honest limits of what has
been done. Its purpose is to let a technical reader judge the work on the record, not on
enthusiasm: every claim of a "first" is stated narrowly and tied to a specific prior source and
to a machine-checkable marker or file in this repository.

## The problem, and why it is not a niche one

A PC's firmware setup — the pre-boot screens that set boot order, Secure Boot, virtualization,
SATA mode, and dozens of other options — is, for a blind user, a black box. It runs before any
operating system, so a screen reader (NVDA, JAWS, VoiceOver, Orca) is not loaded and cannot be
loaded: there is nothing to attach to. In accessibility terms a firmware setup is **closed
functionality** — a system that "precludes the user from adding peripherals or software" and so
cannot rely on the user's own assistive technology
([U.S. Access Board, 402 Closed Functionality](https://www.corada.com/documents/36-cfr-parts-1194-NPRM-preamble/402)).

For closed functionality, accessibility is not optional and it is not the user's job to bolt on;
it must be **built in**:

- **United States — Section 508, §402.2:** "ICT with a display screen shall be speech-output
  enabled for full and independent use by individuals with vision impairments," which "in actual
  practice for all but the simplest ICT … means ensuring that the ICT has built-in speech
  output"
  ([Access Board §402 analysis](https://www.corada.com/documents/36-cfr-parts-1194-NPRM-preamble/402)).
- **European Union — EN 301 549 (harmonised standard under the European Accessibility Act),
  Chapter 5, closed functionality:** where a product does not permit assistive technology to be
  attached, it must provide its own output, including its own audio output
  ([ETSI EN 301 549 V3.2.1](https://www.etsi.org/deliver/etsi_en/301500_301599/301549/03.02.01_60/en_301549v030201p.pdf)).

A firmware setup that speaks itself is exactly what both regimes call for. Today essentially no
mainstream firmware provides it — which is the gap this project addresses.

## What already existed, stated precisely

Prior work on this exact problem is thin, and where it exists it stopped short of working speech.

- **Machado & Vieira, "UEFI BIOS Accessibility for the Visually Impaired" (IEEE, 2017;
  [arXiv:1712.03186](https://arxiv.org/abs/1712.03186)).** The seminal academic treatment. It
  built a prototype that reached an HDA-compatible codec from UEFI and validated the codec's
  **beep generator**; DMA-streamed PCM speech was left as an open problem. It is a proof of
  reachability, not a talking setup.

- **GSoC EFI audio output protocol (2021), Ethin Probst, for EDK II.** The most serious attempt
  to give UEFI real audio. Per the author's own report, it implemented HDA device detection,
  controller reset, codec-tree construction, stream and buffer-descriptor-list setup — but
  **"Sound does not play, although DMA does work,"** the CORB/RIRB ring communication returned
  corrupted data, the BDL layout did not match the emulator, and it was **"never converted to
  driver form or exposed a protocol"** and **not merged into EDK II**, the author calling it
  "majorly incomplete"
  ([GSoC report](https://gist.github.com/ethindp/82420c25f3c63b6652e4a22766bec95d)). A later
  write-up records that the effort pivoted from VirtIO to USB audio and **"failed to complete the
  implementation by the end of GSoC"** (April 2022)
  ([tait.tech, UEFI Audio & Accessibility](https://tait.tech/blog/uefi-audio/)).

- **The UEFI specification itself.** There is still **no audio output protocol in the UEFI
  specification** (through 2.11, December 2024); audio at the firmware stage remains
  unstandardised, which is why every implementation must build its own codec driver
  ([tait.tech](https://tait.tech/blog/uefi-audio/)).

- **Apple VoiceOver at startup** is often cited as a counterexample, but it is **post-firmware**:
  it runs inside **macOS Recovery**, an operating-system environment, and is enabled with
  Command-F5 *after* the boot chime, not in the Mac's firmware
  ([AppleVis, Recovery mode and VoiceOver](https://www.applevis.com/forum/accessibility-advocacy/recovery-mode-voiceover)).
  It is excellent, and it is not the firmware setup.

- **The Linux/console tradition** — GRUB's serial terminal for remote blind administration,
  Speakup (`s` at the Debian installer prompt), and BRLTTY — is likewise **post-firmware**: it
  begins at the boot loader or kernel/console, never inside the firmware's own setup
  ([GRUB manual](https://www.gnu.org/software/grub/manual/grub/grub.html);
  BRLTTY is post-kernel only).

The short version: a **research prototype that beeped (2017)**, a **GSoC effort that never made
sound and was never merged (2021)**, **no standard protocol (2024)**, and, for real products, a
**post-OS recovery reader (Apple)** and **post-OS console tools (Linux)**. No mainstream firmware
vendor ships a talking setup.

## What this project does that the prior art did not

Each item below is narrow, and each is backed by code in this repository and by an `AW_UEFI_*`
marker asserted on the debug console and the COM1 mirror during a real boot (see
[KERNEL-BOOT-PROOFS.md](KERNEL-BOOT-PROOFS.md) and
[FIRMWARE-ACCESSIBILITY-ROADMAP.md](FIRMWARE-ACCESSIBILITY-ROADMAP.md)).

| Advance over prior art | Where prior work stopped | Evidence here |
| --- | --- | --- |
| **DMA-streamed PCM speech through HDA** from UEFI | 2017 validated only the codec beep; GSoC 2021 got DMA working but **"sound does not play"** and never exposed a protocol | `boot/uefi/src/hda.rs` streams real 24 kHz PCM via a working CORB/RIRB ring and a routed output path; `AW_UEFI_HDA_READY`, `AW_UEFI_AUDIO_SPEAK` |
| **A second and third self-built backend** — AC'97 and paravirtual **VirtIO-sound** | GSoC's VirtIO plan was abandoned (not upstream in QEMU at the time) | `ac97.rs`, `virtio_snd.rs`; `AW_UEFI_AUDIO_BACKEND channel=ac97\|virtio`, `AW_UEFI_VIRTIO_SND_PLAY` (captured to WAV) |
| **A complete, spoken, keyboard-operable Setup Utility** with submenus, read on the firmware's own keyboard | no prior firmware setup speaks itself | `setup.rs`; `AW_UEFI_SETUP_TAB`, `AW_UEFI_MENU_ITEM` |
| **Reading and *changing* the firmware's own HII settings** by name (SetVariable and RouteConfig) | GSoC listed HII voicing as future work never reached | `hii_ifr.rs`, `setup.rs`; `AW_UEFI_HII_SETTINGS`, `AW_UEFI_ROUTECONFIG_*` |
| **A runtime formant speech synthesizer** for arbitrary dynamic text, bilingual (EN/FR) | no prior firmware-stage synthesizer exists | `synth.rs`; `AW_UEFI_SYNTH_SELFTEST`, `AW_UEFI_SYNTH_SPEAK` |
| **Real machine state and boot actions** spoken and performed (SMBIOS, TPM/TCG2, Secure Boot keys, `BootOrder`, RTC set) | — | `setup.rs`; `AW_UEFI_SECURITY`, `AW_UEFI_BOOT_ENTRY`, `AW_UEFI_LOADOPTS` |
| **Pre-boot braille** over the firmware's own `EFI_USB_IO_PROTOCOL`, detecting a HID braille display by the Braille usage page | BRLTTY is post-kernel; HID-braille ecosystem support is itself young | `usb.rs`; `AW_UEFI_USB_SUMMARY`, `AW_UEFI_BRAILLE_SHOW` |

The through-line is that this project reached **working speech and a full operable interface at
the firmware stage**, which the 2017 prototype and the 2021 GSoC effort did not, and which no
shipping firmware does.

## Braille, precisely

USB HID braille displays are defined by the Braille Display Usage Page,
[HUTRR78 (May 2018)](https://usb.org/sites/default/files/hutrr78_-_creation_of_a_braille_display_usage_page_0.pdf).
Ecosystem support is recent: as of December 2020 BRLTTY did not yet support HUTRR78
([BRLTTY mailing list](http://brltty.app/pipermail/brltty/2020-December/017939.html)). This
project detects a braille display by that usage page in its HID report descriptor and renders
cells with the shared `aw-braille` engine, using the firmware's `UsbIo` rather than a bespoke
XHCI driver. Enumeration and discriminating detection are proven on QEMU; the cell send is
exercised on a real display (QEMU emulates only a Baum *serial* display, a different transport).

## The voice, honestly

The runtime synthesizer is a Klatt-style cascade formant synthesizer with a Rosenberg glottal
source, a bilingual grapheme-to-phoneme frontend (the French frontend and phoneme inventory
ported from the companion **Sintaise UEFI TTS**), and pitch declination. It is **intelligible,
not natural** — it sounds like early DECtalk. That is a deliberate and defensible trade at the
firmware stage: with no OS, no filesystem of voice data, and soft-float only, the goal is that a
blind user can *understand* a device name or value they otherwise could not hear at all, not that
the voice pass for human. Formant synthesis is the right tool for this constraint, as eSpeak NG
and the historical Klatt synthesizers show.

## What is not done — stated so no one has to guess

- **USB Audio Class output** is *nearly* there, and the attempt itself is a first. Because EDK
  II's `UsbIo` returns `EFI_UNSUPPORTED` for isochronous transfers (the wall the 2021 GSoC hit),
  `boot/uefi/src/usb_audio.rs` is a from-scratch xHCI host-controller driver: it brings up the
  controller, enumerates the device (Enable Slot, Address Device, EP0 control transfers reading
  the device descriptor), and configures the isochronous endpoint and `SET_INTERFACE` — all
  proven on QEMU, already past where the GSoC effort stopped. The one remaining step, the
  isochronous data burst, trips QEMU's host-controller-error bit and is not yet resolved. USB
  audio does not yet make sound, but the hardest parts — a working xHCI stack and USB
  enumeration from a bootable application — are done and on the record.
- **End-to-end braille on physical hardware** is not yet demonstrated in this repository (no
  emulated HID braille display exists to prove it headlessly).
- **Physical-hardware audio** beyond QEMU/VMware is asserted only where a machine has been booted;
  the codec-routing code is written to read real topology, but "it made sound on this exact
  laptop" is a per-machine claim, not a universal one. See
  [REAL-HARDWARE-TEST.md](REAL-HARDWARE-TEST.md).

## Why it belongs on the record

Measured against the literature, this is, to the authors' knowledge, the first firmware-stage
implementation to move from *reaching* the audio hardware (2017) and *failing to make it speak*
(2021) to a **working, DMA-streamed, self-voicing firmware setup** — with a synthesizer for
dynamic text, multiple audio backends, real settings read and changed, and a pre-boot braille
path — all built without a standard UEFI audio protocol because none exists. It does for the
firmware stage what Section 508 §402.2 and EN 301 549 already require of every other piece of
closed-functionality ICT, and it does it in the open, with the evidence attached.

## Sources

- Machado & Vieira, *UEFI BIOS Accessibility for the Visually Impaired*, IEEE 2017 — <https://arxiv.org/abs/1712.03186>
- GSoC 2021 EFI audio output protocol report (Ethin Probst) — <https://gist.github.com/ethindp/82420c25f3c63b6652e4a22766bec95d>
- Tait Hoyem, *UEFI Audio Protocol & UEFI BIOS Accessibility* — <https://tait.tech/blog/uefi-audio/>
- U.S. Access Board, *402 Closed Functionality* (Section 508 §402.2) — <https://www.corada.com/documents/36-cfr-parts-1194-NPRM-preamble/402>
- ETSI EN 301 549 V3.2.1 (2021-03) — <https://www.etsi.org/deliver/etsi_en/301500_301599/301549/03.02.01_60/en_301549v030201p.pdf>
- AppleVis, *Recovery mode and VoiceOver* — <https://www.applevis.com/forum/accessibility-advocacy/recovery-mode-voiceover>
- USB-IF, *HUTRR78 — Creation of a Braille Display Usage Page* (May 2018) — <https://usb.org/sites/default/files/hutrr78_-_creation_of_a_braille_display_usage_page_0.pdf>
- BRLTTY on HUTRR78 (Dec 2020) — <http://brltty.app/pipermail/brltty/2020-December/017939.html>
- GNU GRUB Manual (serial terminal) — <https://www.gnu.org/software/grub/manual/grub/grub.html>
