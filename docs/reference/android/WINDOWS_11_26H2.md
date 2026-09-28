# Windows 11 26H2 target for AccessibleQEMU

Windows 11 26H2 x64 is the primary desktop target for AccessibleQEMU.

## Release requirement

A stable AccessibleQEMU release is not considered validated on Windows merely because it compiles on Windows Server. The critical GUI and virtualization workflows must also pass on a real Windows 11 26H2 x64 system.

Required host accessibility validation:

- Microsoft UI Automation exposure for every interactive control;
- NVDA keyboard-only operation;
- JAWS keyboard-only operation;
- Narrator keyboard-only smoke test;
- predictable focus order and focus restoration;
- no mouse-only, hover-only or icon-only critical command;
- readable status and error text;
- high-contrast and display-scaling compatibility.

Required virtualization validation:

- QEMU x86_64 starts from the AccessibleQEMU GUI;
- WHPX is preferred when available;
- TCG remains a functional fallback when WHPX is unavailable;
- Accessible Android ISO can boot from the GUI;
- persistent QCOW2 storage can be attached;
- QMP/serial recovery remains available independently of the guest framebuffer.

## GitHub Actions strategy

GitHub-hosted x64 Windows runners currently use Windows Server, not Windows 11 26H2. They are used for compile and packaging compatibility only.

The real Windows 11 26H2 validation job targets a dedicated self-hosted runner with labels:

```text
self-hosted
Windows
X64
windows-11-26h2
```

Because this repository is public, such a runner must be isolated and must not execute untrusted pull-request code. The 26H2 runtime job is therefore manual-only unless a hardened disposable runner pool is introduced later.

## Windows executable

The release artifact is expected to contain at least:

```text
AccessibleQEMU.exe
```

QEMU itself may initially be installed separately. A later packaging stage may bundle a known-good QEMU/OVMF distribution when redistribution, update and security requirements are fully defined.

## QEMU discovery order

AccessibleQEMU should resolve QEMU in this order on Windows:

1. an explicit path selected by the user;
2. `qemu-system-x86_64.exe` found in `PATH`;
3. `C:\Program Files\qemu\qemu-system-x86_64.exe`;
4. `C:\Program Files (x86)\qemu\qemu-system-x86_64.exe`;
5. a future application-local bundled QEMU directory.

Failure to locate QEMU must be reported as accessible text with a corrective action; the GUI must not silently fail.
