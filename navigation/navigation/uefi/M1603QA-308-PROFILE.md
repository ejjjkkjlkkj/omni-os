# ASUS VivoBook M1603QA BIOS 308 — HII navigation profile

This profile is derived from the supplied `M1603QAAS.308` firmware image.

- Firmware image SHA-256: `12a932605d3a1ca35dd5023b14a0c7fdc2f8713465655265795f0f9799aaa0de`
- Setup FFS/package GUID: `899407d7-99fe-43d8-9a21-79ec328cac21`
- Main FormSet GUID: `7b59104a-c00d-4158-87ff-f04d6396a915`
- Extracted forms: 63
- Extracted VarStores: 48
- Extracted English strings: 1336

## Primary ASUS notebook forms

| FormId | Title | Confirmed controls |
|---|---|---|
| `0x2716` | Main | BIOS information, vendor/version, VBIOS/EC, processor, memory, system information, serial, MAC/asset when exposed, date, time, access level |
| `0x2717` | Advanced | Internal Pointing Device, Fn Lock, Wake On Lid Open, SVM Mode, MyASUS auto-install mechanism, ASUS EZ Flash 3, SMART, Network Stack, USB, Trusted Computing, SATA, NVMe |
| `0x2718` | Boot | Fast Boot, Boot Option Priorities, Boot Option #N, Add New Boot Option, Delete Boot Option |
| `0x2719` | Security | Administrator/User password state and entry, I/O Interface Security, Secure Boot |
| `0x271A` | Save & Exit | Save/discard, defaults, Boot Override, EFI Shell |
| `0x271B` | Save & Exit | Save/discard/reset variants, user defaults, Boot Override |

## Confirmed referenced forms

- `0x2743` SMART Settings
- `0x2727` Network Stack Configuration
- `0x2728` USB Configuration
- Trusted Computing form
- SATA Configuration form
- `0x2760` NVMe Configuration
- I/O Interface Security form
- Secure Boot form

## Important runtime rule

The firmware uses `SuppressIf`, `GrayOutIf`, and `DisableIf` extensively. The screen reader must therefore retain IFR scope metadata and must not treat the static package order as equivalent to the currently visible firmware UI.

The native navigation engine tracks:

- FormId
- QuestionId
- conditional scope flags
- semantic IFR role
- prompt/help strings
- structural navigation across controls and forms

The next correctness gate is evaluating conditional expressions against live VarStores so hidden/disabled controls can be filtered according to the current firmware state.


## Native screen-reader command map

The BIOS 308 reader now exposes a read-only navigation layer plus a **non-persistent edit preview**:

| Key | Function |
|---|---|
| Arrow keys / Tab / Home / End / Page Up / Page Down | Move focus |
| Enter | Open a referenced HII form when safe |
| Esc | Return to the parent form |
| H | Speak HII contextual help |
| V | Speak the live firmware value |
| W | Where Am I: form + control + value |
| P | Speak position in the current form |
| L | Speak control details: limits, size bounds, list capacity, clock storage |
| R | Reload the current form from live VarStores and preserve focus |
| F/B/X/C/E (+ uppercase reverse variants) | Structural navigation by form/button/checkbox/choice/editable control |
| O / Shift+O | Preview next / previous OneOf value in RAM |
| + / - | Preview the next / previous numeric value using the IFR step and limits |
| Space | Preview checkbox toggle in RAM |
| M | Speak the number of pending preview edits |
| Z | Discard the preview edit for the current control |
| D | Discard every staged preview edit |
| S | Save is deliberately blocked until a verified HII ConfigAccess/RouteConfig commit path exists |

### Safety invariant

Preview edits never call `SetVariable`, `RouteConfig`, or a vendor callback. They are stored only in the reader's RAM, announced as **preview**, and disappear when discarded or when the EFI application exits. Controls that are `GrayOutIf`-disabled or have unresolved conditional state cannot be activated or staged.

This gives the navigation layer a complete edit interaction model without risking Secure Boot, TPM, boot-order, password, or platform settings before the firmware-native commit path is validated.


### Specialized HII semantics

The reader now preserves metadata for `OneOf`, `Numeric`, `String`, `Password`, `OrderedList`, `Date`, and `Time` opcodes. Numeric limits and step values, string size limits, ordered-list capacity, and date/time storage class can be spoken with `L`.

Password questions have a hard privacy rule: the reader announces **protected** and does not attempt to read the underlying VarStore value for speech. Preview changes are still RAM-only. When a staged choice, checkbox, or numeric control affects a `SuppressIf`, `GrayOutIf`, or `DisableIf` expression, `R` and the automatic post-edit refresh evaluate the dependency using the staged value so the previewed form structure matches the proposed setting without writing firmware.

Question-header flags are also semantic: `READ_ONLY`, `CALLBACK`, `RESET_REQUIRED`, `RECONNECT_REQUIRED`, and `OPTIONS_ONLY` are tracked. Read-only questions cannot be staged; callback/reset/reconnect requirements are exposed through control details so a future commit transaction cannot silently bypass firmware behavior.


### Long HII speech

Normalized HII labels, option names, form titles, and contextual help can now retain up to 64 characters. The HDA renderer still keeps each low-level DMA synthesis chunk at 32 characters, but a phrase queue splits longer utterances at word boundaries, automatically starts the next chunk, and remains interruptible on every new navigation event. The same queue is used by blocking boot-proof speech and realtime focus speech, while reusing the existing DMA allocation.


### Hybrid firmware-word speech

The native HDA speech path no longer has to spell every common BIOS word letter by letter. A compact generated lexicon maps recurring firmware words such as `boot`, `security`, `configuration`, `password`, `network`, `save`, and `exit` to sequences from the existing first-party allophone bank. This adds word-level pronunciation without adding whole-word PCM assets.

Unknown words, acronyms, serials, and vendor-specific tokens still use the deterministic French letter-name fallback, so coverage remains complete. Word units use a shorter 3 ms inter-allophone gap; fallback spelling keeps the 12 ms grapheme gap. The 64-character interruptible phrase queue remains unchanged.
