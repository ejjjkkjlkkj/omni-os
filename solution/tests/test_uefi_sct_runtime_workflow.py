from __future__ import annotations

import pathlib
import unittest


WORKFLOW = pathlib.Path(".github/workflows/uefi-sct-runtime.yml")
PROFILE = pathlib.Path("ci/ovmf-sct-platform.ini")


class UefiSctRuntimeWorkflowTests(unittest.TestCase):
    def test_fresh_run_is_started_once_then_resumed(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        marker = "OMNI-FIR.STA"
        remove_marker = r"rm FS%i:\Sct\OMNI-FIR.STA"
        fresh = "Sct -r -a -f -v"
        resume = "Sct -c -f -v"

        self.assertIn(marker, text)
        self.assertIn(remove_marker, text)
        self.assertIn(fresh, text)
        self.assertIn(resume, text)
        self.assertLess(text.index(remove_marker), text.index(fresh))

    def test_runtime_requires_completion_and_real_sct_results(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("OMNI_SCT_RETURNED", text)
        self.assertIn("::/Sct/Overall", text)
        self.assertIn('python tools/sct_summary.py "$SUMMARY_LOG"', text)
        self.assertIn("--baseline ci/ovmf-sct-known-failures.json", text)
        self.assertIn("--json-out sct-summary.json --strict", text)

    def test_runtime_preserves_progress_diagnostics(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("SCT runtime heartbeat:", text)
        self.assertIn("dump_sct_diagnostics", text)
        self.assertIn("trap on_termination TERM INT HUP", text)
        self.assertIn("SCT runtime received a termination signal", text)

    def test_runtime_binds_an_explicit_qemu_platform_profile(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")
        profile = PROFILE.read_text(encoding="utf-8")

        self.assertIn('cp ci/ovmf-sct-platform.ini "$SCT_PROFILE"', text)
        self.assertIn("cmp -s ci/ovmf-sct-platform.ini sct-profile-from-image.ini", text)
        self.assertIn("-vga none", text)
        self.assertNotIn("-net none", text)
        self.assertIn("libslirp-dev", text)
        self.assertIn("-netdev user,id=net0,ipv6=off", text)
        self.assertIn("-device e1000e,netdev=net0", text)
        self.assertIn("-device qemu-xhci,id=xhci", text)
        self.assertIn("-device usb-kbd,bus=xhci.0", text)

        self.assertIn("GraphicalConsoleDevices   = no", profile)
        self.assertIn("BootFromNetworkDevices    = yes", profile)
        self.assertIn("UsbBusSupport             = yes", profile)
        self.assertIn("NVMExpressPassThru        = yes", profile)
        self.assertIn("AtaPassThru               = no", profile)
        self.assertIn("ExtScsiPassThru           = no", profile)
        self.assertIn("BootFromScsi              = no", profile)
        self.assertIn("UEFIIPv6Support           = no", profile)
        self.assertIn("BlueToothClassicSupport   = no", profile)
        self.assertIn("IPSecSupport              = no", profile)

    def test_runtime_uses_host_authenticated_vars_and_nvme_only_storage(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("-D QEMU_PV_VARS=TRUE", text)
        self.assertIn("-D SECURE_BOOT_ENABLE=TRUE", text)
        self.assertIn("-bios OVMF.qemuvars.fd", text)
        self.assertIn("-device uefi-vars-x64,jsonfile=uefi-vars.json", text)
        self.assertIn("-machine q35,accel=tcg,sata=off", text)
        self.assertIn("-drive if=none,id=sctnvme,file=sct-runtime.img,format=raw", text)
        self.assertIn("-device nvme,drive=sctnvme,serial=OMNISCT0001", text)
        self.assertIn("uefi-vars.json", text)
        self.assertNotIn("OVMF_SCT_VARS.fd", text)
        self.assertNotIn("nvme-test.img", text)
        self.assertNotIn("-drive file=sct-runtime.img,format=raw", text)


if __name__ == "__main__":
    unittest.main()
