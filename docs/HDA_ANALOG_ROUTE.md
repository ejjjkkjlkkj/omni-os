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

The analog resolver now walks the selected pin's connection list instead of assuming that
the pin names a DAC directly. This covers the common ALC256 speaker topology
`0x14 (pin) -> 0x0c (mixer) -> 0x02 (DAC)`. It prefers the active selector input,
records the resolved depth/intermediate node, and unmutes the intermediate input amplifier
when the widget advertises one. Range-encoded connection entries are rejected rather than
guessed, so unresolved routes fail closed.

## Expected on the next physical boot

| Key | Expected |
|---|---|
| `OMNI_HDA_GPU_HDMI_SKIPPED` | 1 |
| `OMNI_HDA_SELECTION_PASS` | 0 |
| `OMNI_HDA_VENDOR_ID` / `OMNI_HDA_DEVICE_ID` | 4130 / 5603 (`1022:15E3`) |
| `OMNI_HDA_CODEC_VENDOR_ID` | 283902550 (`0x10EC0256`) |
| `OMNI_HDA_ANALOG_PIN_CANDIDATES` | ≥ 1 (ALC256 speaker pin is normally NID 0x14) |
| `OMNI_HDA_ROUTE_RESOLVED` | 1 |
| `OMNI_HDA_ROUTE_RESOLVED_DEPTH` | ≥ 1; a mixer hop is expected on the common ALC256 speaker path |
| `OMNI_HDA_ROUTE_INTERMEDIATE_NODE` | informative; commonly 12 (`0x0c`) for the ALC256 speaker route |
| `OMNI_HDA_DMA_PROGRESS` | 1 |
| `OMNI_HDA_ROUTE_PROGRAMMED` | 1, then a human confirms the tone on the speaker |

If the codec and route resolve but the speaker remains silent, the next suspect is
codec/platform-specific amplifier initialisation (for example Realtek vendor coefficients)
rather than controller DMA. The physical evidence keeps DMA progress, route resolution,
pin/EAPD state, converter selection and intermediate-amplifier programming separate so the
next failure can be localized without guessing.

## Firmware image

`tools/bios_hda_verbs.py` scans an image for pin-configuration verb tables. On
ASUS `M1603QAAS.308` (SHA-256 `12a93260…0de`), neither the raw image nor its 22
LZMA sections contain an Intel/AMI-style (0x71C..0x71F) or AMD Azalia
(`nid + config`, 0xFF-terminated) table: the codec's own defaults are used, so
the probe's live `F1C` reads are the source of truth. The image is only read,
never flashed.

## BIOS Setup coverage (static vs runtime HII)

`tools/bios_ifr_inventory.py` on `M1603QAAS.308` (report: `docs/M1603QAAS-308-hii-inventory.json`):

| | firmware image (static) | physical boot 2026-09-26 (runtime) |
|---|---|---|
| HII form sets | 11 | 5 |
| questions | 1263 | 462 |
| password questions | 2 | 0 |

The largest form set, `7b59104a-c00d-4158-87ff-f04d6396a915` (AMI Aptio Setup, 541 questions,
both password fields), is not published to a boot application: Aptio registers it only when
Setup is entered. The chain-loaded screen reader therefore reads about a third of the menus;
reading the real Setup needs the reader active while Setup runs (driver-resident, not a boot
app), and the secret-field rules must then cover these two password questions.

## Key content (screen reader included)

| Path | Role |
|---|---|
| `\EFI\BOOT\BOOTX64.EFI` | OmniProbe from the attested HIL artifact: evidence, HDA route, keyboard gate |
| `\EFI\OMNI\SCREENREADER.EFI` | accessible-windows UEFI screen reader (REALTIME.EFI), chain-loaded by OmniProbe after its evidence |
| `\OMNI-CHALLENGE.TXT`, `\OMNI-RUN-BINDING.JSON`, `\SHA256SUMS.TXT` | challenge, run/commit/hash binding, checksums of the key |
| partition 2 `OMNI-DATA` (exFAT) | ST release, NVDA add-on, BIOS reference (read-only), UEFI copies, past evidence, `LISEZMOI.TXT` — `tools/omni_key_data.ps1` under PsExec SYSTEM |

Verified locally under QEMU: `OMNI_DIAG_PASS` → `OMNI_SCREENREADER_LOAD/START` → reader collects
17 HII prompts and waits for interruptible navigation (`HII_GRAPH_NAV_READY=PASS`).

## Deploying (manual reboot)

`tools/omni_deploy_system.ps1 -RunId <successful HIL run> -ScreenReader <REALTIME.EFI>` downloads as the user, then runs
`tools/omni_deploy_physical.ps1 -RequireSystem` through `C:\Tools\PsExec\PsExec64.exe -s`; it
verifies the key's physical identity, backs up the whole key, removes the previous
boot's evidence, installs the attested EFI with a fresh challenge and sets a
one-shot BootNext. It never reboots.

After the physical boot, collect under SYSTEM with
`tools/omni_collect_physical.ps1`. The collector now fails closed unless the
attested evidence, `1022:15E3` / ALC256 selection, resolved/programmed route,
DMA progress, explicit DOWN+ENTER keyboard/navigation proof, and screen-reader
physical proof are all present. When the internal speaker tone was actually
heard, pass `-AudibleSpeakerConfirmed`; without that switch the release verdict
remains blocked instead of treating silence/non-observation as proof.
