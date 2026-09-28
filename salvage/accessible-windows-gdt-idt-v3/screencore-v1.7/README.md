# Salvaged from `accessible-windows-uefi-screencore-v1.7.img`

Recovered 2026-09-21 from the pre-built image in `~/Downloads`. That image's ESP held:

| File | Size | What it is |
| --- | --- | --- |
| `EFI/BOOT/BOOTX64.EFI` | 40 448 B | **The ScreenCore engine** (recovered here as `screencore-BOOTX64.EFI`). |
| `EFI/BOOT/AWORIG.EFI` | 27 344 384 B | An older build of the repo's UEFI app (`aw-uefi-boot.efi`, ~27 MB). Superseded by a fresh `cargo build` of `boot/uefi` — **not** salvaged. |
| `KERNEL.BIN` | 933 888 B | The native kernel, same as the current build produces. |

## What ScreenCore is (from its embedded strings — no source in this repo)

A standalone UEFI **audio/accessibility diagnostic**, internally `AudioCore V10` /
`VoiceCore` / `SCREENCORE_NEXT_V17`. It is more advanced than the repo's
`boot/uefi/src/hda.rs` in discovery, but it is a **scanner, not a player**.

Capabilities it advertises:
- Enumerates **every** HDA controller (`AW_HDA_CONTROLLER_PASS count=`) and identifies
  the codec vendor as REALTEK / AMD / INTEL / GENERIC — the multi-controller,
  prefer-the-analog-codec logic this repo's player was just given.
- ACPI/MCFG/ECAM validation, reads **NHLT** (the I2S/SoundWire audio topology table)
  and is **AMD ACP** aware (`AW_PLATFORM_EXPECTATIONS ... HDA AMD_ACP`).
- Full screen-reader UI: rotor, history, French speech, rate/pitch, mute, `P` = scan HDA.
- Recovery chain: `voice_hq->voice_boot`, `audio_hda->usb_audio`,
  `diagnostics_voice->text->beep`.

## The key finding

ScreenCore **gates real playback** behind a safety check and does not stream audio:
- `AW_HDA_DMA_PENDING safety_gate=physical_validation`
- `AW_HDA_PREFLIGHT_STATE running=0 safe_to_program_after_DMA_map`
- `AW_PHYSICAL_SPEECH_PASS=0 reason=hardware_not_observed`

So on any machine it maps out the HDA path but **plays nothing by design** — which is
why booting this image is silent. The component that actually streams speech is the
repo's `boot/uefi/src/hda.rs` (the ~27 MB app), which this session improved to select
the analog Realtek codec instead of the GPU's HDMI-audio codec.

`screencore-strings.txt` is the full printable-string dump for reference.
