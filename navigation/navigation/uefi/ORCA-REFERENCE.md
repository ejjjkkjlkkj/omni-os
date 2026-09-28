# Orca as a design reference for UEFI navigation

This project does **not** embed Orca, AT-SPI, GNOME, Speech Dispatcher, or Python in the firmware runtime.

Reference reviewed: the GNOME Orca source tree (51.x), especially its focus manager, event manager, speech generator, input-event manager, and “Where Am I” presenter. Orca is LGPL-2.1; no Orca source code is copied into this repository.

## Patterns adopted

- **Focus-centered presentation:** every navigation move resolves one current HII control and announces semantic role plus label.
- **Immediate interruption:** a new focus command cancels the active DMA utterance before starting the new one.
- **Where Am I:** `W` re-announces the semantic context of the current HII control.
- **Structural navigation:** lowercase moves forward and uppercase moves backward:
  - `b` / `B`: button
  - `x` / `X`: checkbox
  - `c` / `C`: choice / ordered-list control
  - `e` / `E`: editable/value controls
- **Wrapping:** structural navigation wraps through the HII prompt set.

## Deliberately not adopted

- AT-SPI object transport and D-Bus
- GNOME/GTK integration
- Python runtime
- Orca application/toolkit scripts
- Speech Dispatcher / Spiel dependency

The UEFI implementation remains freestanding C over native HII, Simple Text Input, PCI/HDA MMIO, and first-party VoiceCore units.
