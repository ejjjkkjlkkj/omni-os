# Imported reference documentation

This directory holds documentation recovered from sibling accessibility projects
by the same author, kept here as cross-project reference. It is **reference
material only** — it does not describe this repository's own implementation and
is not part of its build, tests, or gates.

| Subdirectory | Source project | What it covers |
|---|---|---|
| `accessible-windows/` | `ejjjkkjlkkj/accessible-windows` | Clean-room x86-64 UEFI accessibility OS (Rust): firmware/kernel accessibility roadmap, kernel-boot proofs, hardware compatibility, recovery boot, storage/filesystem architecture. |
| `st/` | ST speech-synthesis engine (local) | The deterministic ST TTS engine used by `omni.voice_st` (the `OMNI_ST_HOME` runtime), plus its performance notes. |
| `android/` | `ejjjkkjlkkj/android` | Accessible virtualization suite (accessible QEMU/UTM), PC-port and reference-OS integration notes. |

These documents inform this project's UEFI/firmware accessibility, HDA audio,
HII/IFR, screen-reader and hardware-in-the-loop work, but the implementations
there are separate (Rust OS / TTS engine / virtualization) and are not vendored
into this repository's C/Python stack.
