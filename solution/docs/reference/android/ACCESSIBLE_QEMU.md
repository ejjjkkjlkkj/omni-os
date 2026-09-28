# AccessibleQEMU accessibility contract

AccessibleQEMU is the reference virtual-machine manager for Accessible Android. Accessibility is a release gate, not an optional enhancement.

## Non-negotiable requirements

Every user-facing operation MUST be operable without a mouse and MUST expose usable semantics to the host operating system accessibility API.

A release fails if any critical operation requires sight, pointer hover, drag-and-drop, unlabeled icon-only controls, inaccessible custom drawing, or an undocumented key sequence.

Critical operations include:

- create/open/delete a VM;
- choose an ISO or disk image;
- start, pause, reset, stop and force-stop a VM;
- configure CPU, memory, firmware, storage, network, audio and display;
- attach/eject removable media;
- inspect VM state and errors;
- open serial console and logs;
- create/restore/delete snapshots;
- run the Accessible Android installer;
- run accessibility/runtime validation;
- export QCOW2, RAW, VDI and VMDK images.

## Host accessibility

The graphical frontend MUST expose a semantic accessibility tree through the native host APIs:

- Windows: UI Automation, for NVDA, JAWS and Narrator;
- Linux: AT-SPI, for Orca and compatible assistive technology;
- macOS: NSAccessibility, for VoiceOver.

The initial frontend implementation uses Rust with egui/eframe and AccessKit. Custom widgets are allowed only when they expose correct role, name, value, state, focus and actions.

## Keyboard contract

The entire application MUST be navigable with standard keyboard interaction:

- Tab / Shift+Tab moves between interactive controls in logical order;
- arrow keys navigate menus, lists, radio groups, tabs and trees where appropriate;
- Enter or Space activates the focused control;
- Escape closes transient UI without destroying data;
- Alt/menu shortcuts expose all primary menus on platforms that support them;
- every critical command also has a discoverable command-palette or menu entry;
- focus is always visible and programmatically exposed;
- focus is restored predictably after dialogs close.

No function may exist only behind hover, right-click, drag-and-drop, touch gesture or a graphical hotspot.

## Dialog and status contract

- Every input has a persistent accessible label.
- Every validation error identifies the field and explains the corrective action.
- Progress updates must be available as text and not only animation.
- Destructive actions require an explicit confirmation containing the VM/disk name.
- Background VM state changes must be announced through an accessible status region and written to logs.
- Error dialogs must expose copyable diagnostic text.

## Visual accessibility

The GUI must remain usable for low-vision users:

- scalable interface and text;
- high-contrast compatible rendering;
- no information encoded only by color;
- clear focus indication;
- no mandatory animation;
- layouts must remain usable with large text and increased scaling.

## Guest accessibility

AccessibleQEMU must not depend on the guest framebuffer for accessibility.

For Accessible Android the reference stack is:

1. host screen reader operates AccessibleQEMU itself;
2. TalkBack + offline TTS operate Android from first boot;
3. serial console and QMP remain an independent recovery/control path;
4. a future Android accessibility bridge may expose selected guest semantic information to the host, but it MUST NOT replace TalkBack.

## Accessibility recovery

At least one fully keyboard-accessible recovery path must remain available if the graphical guest is unusable:

- serial console;
- VM power/reset controls;
- QMP commands;
- logs;
- accessibility health check;
- boot previous Android A/B slot when supported.

## Release gates

Before a stable release, the following must pass on supported platforms:

- keyboard-only task suite;
- accessibility-tree inspection;
- screen-reader smoke tests;
- focus-order tests;
- dialog/name/role/state tests;
- no unlabeled interactive controls;
- no critical pointer-only operation;
- Accessible Android boot/runtime accessibility validation.

Target screen readers:

- Windows: NVDA, JAWS, Narrator;
- Linux: Orca;
- macOS: VoiceOver.

Automated tests are necessary but not sufficient. A stable release also requires real screen-reader validation for critical workflows.
