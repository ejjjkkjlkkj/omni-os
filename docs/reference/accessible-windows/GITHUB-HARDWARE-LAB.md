# GitHub self-hosted physical hardware lab

The GitHub-hosted QEMU/OVMF job is an integration test. It is not a physical-hardware test.
Physical validation is handled by `.github/workflows/hardware-lab.yml` on a dedicated self-hosted x64 Linux controller with the custom label `accessible-windows-hardware-lab`.

## Required topology

Use two machines:

1. **Lab controller** — Linux x64, GitHub self-hosted runner, USB writer, and physical-control interface.
2. **DUT (device under test)** — the x86-64 UEFI PC that actually boots Accessible Windows.

Do not use the DUT itself as the GitHub runner. Rebooting the DUT would terminate the active runner job and makes reliable evidence collection impossible.

## GitHub runner labels

The controller must match all of:

- `self-hosted`
- `linux`
- `x64`
- `accessible-windows-hardware-lab`

The workflow is `workflow_dispatch` only and uses the `physical-hardware` GitHub Environment. Configure required reviewers for that environment before enabling destructive runs.

## Trusted local components

Repository code never receives unrestricted block-device access. The controller must have these root-owned, runner-non-writable local files:

- `/usr/local/sbin/aw-hardware-lab`
- `/usr/local/sbin/aw-dut-cycle-test`
- `/etc/accessible-windows/lab-usb-by-id`

Install `hardware-lab/aw-hardware-lab` manually after reviewing it:

```bash
sudo install -o root -g root -m 0755 hardware-lab/aw-hardware-lab /usr/local/sbin/aw-hardware-lab
```

`/etc/accessible-windows/lab-usb-by-id` must contain one persistent `/dev/disk/by-id/usb-*` path for a dedicated disposable USB device. Never use `/dev/sdX` because enumeration can change across boots.

The runner account needs narrowly scoped passwordless sudo for `/usr/local/sbin/aw-hardware-lab` only. Do not grant generic passwordless sudo.

## DUT backend contract

`/usr/local/sbin/aw-dut-cycle-test` is site-specific because power control and evidence capture depend on the lab hardware. It must be root-owned and non-writable by the runner. It receives:

```text
--boot-device <persistent USB by-id>
--image-sha256 <sha256>
--source-run <GitHub Actions run ID>
--commit <Git commit>
--result <output file>
```

It must independently control and observe the DUT and only return success when the same physical boot produced at least these exact evidence lines:

```text
AW_PHYSICAL_BOOT_RESULT=PASS
AW_INTERNAL_DISK_UNCHANGED=PASS
AW_NATIVE_KERNEL_ENTRY=PASS
```

For accessibility validation, add these when implemented:

```text
AW_NONVISUAL_BOOT_FEEDBACK=PASS
AW_NATIVE_AUDIO=PASS
AW_BOOT_SPEECH=PASS
```

The backend must fail closed when evidence is missing or ambiguous.

## Recommended physical-control backends

Any implementation is acceptable if it is isolated from repository code and produces auditable evidence. Examples include:

- USB serial/debug capture from the DUT;
- network-controlled relay or smart PDU for power cycling;
- hardware KVM/serial console;
- firmware configured to prefer the dedicated removable USB for the test cycle;
- a second USB microcontroller used only for reset/power and evidence signals.

Do not make visual framebuffer inspection the only PASS criterion for an accessibility-first system.

## Workflow inputs

`Physical hardware lab` requires:

- `source_run_id`: successful CI run that produced `accessible-windows-uefi-x86_64`;
- `expected_image_sha256`: exact raw image digest from that CI run's `SHA256SUMS.txt`;
- `destructive_confirmation`: exactly `ERASE_DEDICATED_USB`.

The workflow downloads the exact CI artifact, validates all CI checksums, validates the requested image digest, invokes the trusted local helper, and uploads the physical evidence file.

## USB safety gates

The local helper refuses the write unless all of these are true:

- target is configured locally, not supplied by repository code;
- path is `/dev/disk/by-id/usb-*`;
- target resolves to a whole block disk;
- Linux reports it as removable;
- target is not the root filesystem parent disk;
- target and its partitions are unmounted;
- device is large enough for the image;
- downloaded image SHA-256 exactly matches the requested digest.

After writing, it performs byte-for-byte read-back comparison over the image length before allowing the DUT test to start.

## Physical PASS remains separate

A cloud CI PASS must never update `USB physical boot`, `Physical PC boot`, or `Accessible physical proof` to PASS. Those gates change only from evidence produced by this hardware-lab path or another documented real-hardware procedure.
