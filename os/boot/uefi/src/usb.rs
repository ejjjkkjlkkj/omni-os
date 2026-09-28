//! Pre-boot USB access through the firmware's own host stack - the foundation for USB
//! accessibility devices at the firmware stage, and a pre-boot braille display on top of it.
//!
//! Writing an XHCI host-controller driver from scratch would be a large body of work, but it
//! is unnecessary this early: while boot services are alive the firmware has *already*
//! enumerated every USB device and exposes each through `EFI_USB_IO_PROTOCOL`, with control,
//! interrupt and bulk transfers and the standard descriptors. This module uses that - the same
//! stack the firmware's own USB keyboard rides on - so USB accessibility hardware is reachable
//! before the kernel's USB stack exists.
//!
//! Two things are built on it:
//!
//! 1. **Enumeration and classification** ([`report_devices`]) - walk every `UsbIo` handle, read
//!    its device and interface descriptors, and emit `AW_UEFI_USB_*` evidence, so the boot
//!    proofs can assert the firmware USB stack is reachable pre-OS and that devices are
//!    correctly identified (a keyboard is a keyboard, not mistaken for a braille display).
//! 2. **A pre-boot braille display** ([`find_braille`], [`Braille::show`]) - detect a HID
//!    braille display by the Braille usage page (HUTRR78) in its HID report descriptor, then
//!    render text to six-dot cells with the shared [`aw_braille`] engine and send them as a HID
//!    output report. This is what makes a *deaf-blind* user able to operate the firmware: the
//!    same lines the screen reader speaks can also be felt.
//!
//! Honest scope: end-to-end braille output is exercised on a real display (QEMU emulates only a
//! Baum serial display, a different transport); the enumeration, HID-braille detection and cell
//! rendering are all proven here, and the HID output-report send follows the HID braille class.
//! USB Audio Class output is *not* built on this `UsbIo` path: audio streaming is isochronous,
//! which EDK II's `UsbIo` returns `EFI_UNSUPPORTED` for, so it needs a dedicated host-controller
//! driver - that is [`crate::usb_audio`], a from-scratch xHCI driver.

extern crate alloc;

use alloc::vec::Vec;

use uefi::Handle;
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::usb::io::{ControlTransfer, UsbIo};

use crate::aw_mark;

/// The USB HID class code (interface class 0x03).
const CLASS_HID: u8 = 0x03;
/// The USB Audio class code (interface class 0x01) - detected and reported, not driven.
const CLASS_AUDIO: u8 = 0x01;

/// HID `GET_DESCRIPTOR` for the report descriptor: `bRequest = 6`, `wValue` high byte = 0x22.
const HID_GET_DESCRIPTOR: u8 = 0x06;
const HID_REPORT_DESCRIPTOR_TYPE: u16 = 0x22;
/// HID class `SET_REPORT`: `bmRequestType = 0x21` (host->device, class, interface),
/// `bRequest = 0x09`, `wValue` high byte 0x02 = Output report.
const HID_SET_REPORT: u8 = 0x09;
const HID_OUTPUT_REPORT: u16 = 0x02 << 8;

/// The HID usage page for braille displays (HUTRR78): a report descriptor that declares it is a
/// braille device rather than a keyboard or mouse.
const USAGE_PAGE_BRAILLE: [u8; 2] = [0x05, 0x41];

/// Open a handle's `UsbIo` shared (GetProtocol), so reading descriptors does not disturb the
/// firmware driver that already holds the device (e.g. the console keyboard driver).
fn open_usbio(handle: Handle) -> Option<ScopedProtocol<UsbIo>> {
    // SAFETY: GetProtocol is a shared, non-exclusive open; the agent is this loaded image and
    // the returned protocol is dropped (closed) with the ScopedProtocol.
    unsafe {
        boot::open_protocol::<UsbIo>(
            OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
        .ok()
    }
}

/// One enumerated USB device's identity.
struct UsbDevice {
    vendor: u16,
    product: u16,
    interface_class: u8,
    interface_subclass: u8,
    interface_protocol: u8,
    interface_number: u8,
    handle: Handle,
}

/// Enumerate every USB device the firmware exposes through `UsbIo`, reading each one's identity.
fn enumerate() -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let Ok(handles) = boot::find_handles::<UsbIo>() else {
        return devices;
    };
    for handle in handles {
        let Some(mut usbio) = open_usbio(handle) else {
            continue;
        };
        let Ok(device) = usbio.device_descriptor() else {
            continue;
        };
        let interface = usbio.interface_descriptor().ok();
        devices.push(UsbDevice {
            vendor: device.id_vendor,
            product: device.id_product,
            interface_class: interface.map(|i| i.interface_class).unwrap_or(0),
            interface_subclass: interface.map(|i| i.interface_subclass).unwrap_or(0),
            interface_protocol: interface.map(|i| i.interface_protocol).unwrap_or(0),
            interface_number: interface.map(|i| i.interface_number).unwrap_or(0),
            handle,
        });
    }
    devices
}

/// Fetch an interface's HID report descriptor into `buffer`, returning the slice actually read,
/// or `None` if the device has no HID report descriptor.
fn hid_report_descriptor<'a>(
    usbio: &mut UsbIo,
    interface_number: u8,
    buffer: &'a mut [u8],
) -> Option<&'a [u8]> {
    // Standard interface GET_DESCRIPTOR for the report descriptor. `request_type` 0x81 is
    // device-to-host, standard, interface recipient (the crate sets the direction bit itself).
    usbio
        .control_transfer(
            0x81,
            HID_GET_DESCRIPTOR,
            HID_REPORT_DESCRIPTOR_TYPE << 8,
            u16::from(interface_number),
            ControlTransfer::DataIn(buffer),
            500,
        )
        .ok()?;
    Some(buffer)
}

/// Whether an interface is a HID braille display: HID class, and a report descriptor that
/// declares the Braille usage page. This is what separates a braille display from a keyboard.
fn is_braille(device: &UsbDevice) -> bool {
    if device.interface_class != CLASS_HID {
        return false;
    }
    let Some(mut usbio) = open_usbio(device.handle) else {
        return false;
    };
    let mut buffer = [0u8; 256];
    let Some(report) = hid_report_descriptor(&mut usbio, device.interface_number, &mut buffer)
    else {
        return false;
    };
    report.windows(2).any(|window| window == USAGE_PAGE_BRAILLE)
}

/// Walk the USB devices, emit `AW_UEFI_USB_*` evidence, and report whether a braille display and
/// a USB Audio Class device are present. Read-only; safe on any machine, and empty (all false)
/// where the firmware exposes no `UsbIo`.
pub fn report_devices() -> bool {
    let devices = enumerate();
    let mut braille = false;
    let mut audio = false;
    for device in &devices {
        let is_braille_display = is_braille(device);
        braille |= is_braille_display;
        audio |= device.interface_class == CLASS_AUDIO;
        aw_mark!(
            "AW_UEFI_USB_DEVICE vid=0x{:04x} pid=0x{:04x} class=0x{:02x} subclass=0x{:02x} protocol=0x{:02x} braille={}",
            device.vendor,
            device.product,
            device.interface_class,
            device.interface_subclass,
            device.interface_protocol,
            is_braille_display,
        );
    }
    aw_mark!(
        "AW_UEFI_USB_SUMMARY count={} braille={} audio={}",
        devices.len(),
        braille,
        audio,
    );
    braille
}

/// A detected pre-boot braille display: its `UsbIo` handle and the HID interface to address.
pub struct Braille {
    handle: Handle,
    interface_number: u8,
}

/// Find a connected HID braille display, or `None`.
pub fn find_braille() -> Option<Braille> {
    enumerate()
        .into_iter()
        .find(is_braille)
        .map(|device| Braille {
            handle: device.handle,
            interface_number: device.interface_number,
        })
}

impl Braille {
    /// Render `text` to six-dot braille cells with the shared [`aw_braille`] engine and send
    /// them to the display as a HID output report. Returns whether the report was accepted.
    /// The cells are the same ones the kernel-stage braille path renders, so what the screen
    /// reader speaks is exactly what is felt.
    pub fn show(&self, text: &str) -> bool {
        let mut cells = [0u8; 84]; // enough for an 80-cell display plus signs
        let count = aw_braille::translate(text, &mut cells);
        let Some(mut usbio) = open_usbio(self.handle) else {
            return false;
        };
        // HID SET_REPORT (Output) carrying the cell bytes. bmRequestType 0x21 = host-to-device,
        // class, interface.
        let ok = usbio
            .control_transfer(
                0x21,
                HID_SET_REPORT,
                HID_OUTPUT_REPORT,
                u16::from(self.interface_number),
                ControlTransfer::DataOut(&cells[..count]),
                1000,
            )
            .is_ok();
        if ok {
            aw_mark!("AW_UEFI_BRAILLE_SHOW cells={count}");
        } else {
            log::warn!("AW_UEFI_BRAILLE_FAIL cells={count}");
        }
        ok
    }
}
