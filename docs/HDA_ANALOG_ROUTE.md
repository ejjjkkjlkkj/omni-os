# HDA analog route on the ASUS M1603QA — status and next physical gate

## What the last physical boot proved (OMNI-DIAG, 2026-09-26 18:37, commit 3d0e9ba)

| Evidence | Value | Meaning |
|---|---|---|
| `OMNI_HDA_CONTROLLERS` | 2 | two HDA controllers enumerated |
| `OMNI_HDA_BUS/DEVICE/FUNCTION` | 3 / 0 / 1 | the probe claimed the first one |
| `OMNI_HDA_VENDOR_ID/DEVICE_ID` | 4098 / 5687 = `1002:1637` | **GPU HDMI/DP audio controller** |
| `OMNI_HDA_CODEC_VENDOR_ID` | 268610049 = `0x1002AA01` | AMD HDMI codec, digital pins only |
| `OMNI_HDA_ANALOG_PIN_CANDIDATES` | 0 | no speaker/headphone pin exists on that codec |
| `OMNI_HDA_DMA_LPIB_BEFORE/AFTER` | 0 → 712 | controller DMA transport works |
| `OMNI_HDA_ROUTE_PROGRAMMED` | 0 | consequence: no analog route, no sound |
| `OMNI_KEYBOARD_STATUS` | `0x8000000000000012` (EFI_TIMEOUT) | no key pressed in the window |

Windows on the same machine lists the analog path as controller `1022:15E3`
(platform HDA) with codec **Realtek ALC256** (`HDAUDIO\FUNC_01&VEN_10EC&DEV_0256&SUBSYS_104317EF`),
and the HDMI path as `1002:1637` / `1002:AA01`.

## Fix (branch `uefi-hda-analog-controller-select-20260927`)

Pass 0 of controller selection skips GPU HDMI controllers (PCI vendor 0x1002 / 0x10DE);
pass 1 accepts them only when no other controller gave a valid MMIO window.
New evidence keys: `OMNI_HDA_GPU_HDMI_SKIPPED`, `OMNI_HDA_SELECTION_PASS`.

## Expected on the next physical boot

| Key | Expected |
|---|---|
| `OMNI_HDA_GPU_HDMI_SKIPPED` | 1 |
| `OMNI_HDA_SELECTION_PASS` | 0 |
| `OMNI_HDA_VENDOR_ID` / `OMNI_HDA_DEVICE_ID` | 4130 / 5603 (`1022:15E3`) |
| `OMNI_HDA_CODEC_VENDOR_ID` | 283902550 (`0x10EC0256`) |
| `OMNI_HDA_ANALOG_PIN_CANDIDATES` | ≥ 1 (ALC256 speaker pin is normally NID 0x14) |
| `OMNI_HDA_DMA_PROGRESS` | 1 |
| `OMNI_HDA_ROUTE_PROGRAMMED` | 1, then a human confirms the tone on the speaker |

If the codec is right but `ROUTE_PROGRAMMED` stays 0, the next suspects are the
ALC256 vendor coefficient initialisation that Linux applies before the speaker
amplifier works, and a mixer node between the converter and pin 0x14.

## Firmware image

`tools/bios_hda_verbs.py` scans an image for pin-configuration verb tables. On
ASUS `M1603QAAS.308` (SHA-256 `12a93260…0de`), neither the raw image nor its 22
LZMA sections contain an Intel/AMI-style (0x71C..0x71F) or AMD Azalia
(`nid + config`, 0xFF-terminated) table: the codec's own defaults are used, so
the probe's live `F1C` reads are the source of truth. The image is only read,
never flashed.

## Key content (screen reader included)

| Path | Role |
|---|---|
| `\EFI\BOOT\BOOTX64.EFI` | OmniProbe from the attested HIL artifact: evidence, HDA route, keyboard gate |
| `\EFI\OMNI\SCREENREADER.EFI` | accessible-windows UEFI screen reader (REALTIME.EFI), chain-loaded by OmniProbe after its evidence |
| `\OMNI-CHALLENGE.TXT`, `\OMNI-RUN-BINDING.JSON`, `\SHA256SUMS.TXT` | challenge, run/commit/hash binding, checksums of the key |

Verified locally under QEMU: `OMNI_DIAG_PASS` → `OMNI_SCREENREADER_LOAD/START` → reader collects
17 HII prompts and waits for interruptible navigation (`HII_GRAPH_NAV_READY=PASS`).

## Deploying (manual reboot)

`tools/omni_deploy_system.ps1 -RunId <successful HIL run> -ScreenReader <REALTIME.EFI>` downloads as the user, then runs
`tools/omni_deploy_physical.ps1 -RequireSystem` through `C:\Tools\PsExec\PsExec64.exe -s`; it
verifies the key's physical identity, backs up the whole key, removes the previous
boot's evidence, installs the attested EFI with a fresh challenge and sets a
one-shot BootNext. It never reboots.
