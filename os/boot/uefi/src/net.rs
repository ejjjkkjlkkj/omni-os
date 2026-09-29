//! Pre-boot network, deny by default (`docs/UEFI-CAPABILITIES.md`, section 5).
//!
//! Step 1 of the network roadmap: discover the firmware's network interfaces through the
//! Simple Network Protocol and report them - strictly read-only. Each SNP is opened shared
//! (`GetProtocol`), so the firmware's own network drivers stay bound, and it is never
//! started, initialized, reset or used to transmit or receive: this loader sends no packet
//! and accepts none. Later steps (DHCP, HTTPS remediation) open the network only on an
//! explicit request, and only toward an allowlist.

use crate::aw_mark;
use alloc::vec::Vec;
use uefi::Handle;
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::network::snp::{NetworkState, SimpleNetwork};

/// Whether the cable (or the radio association) is up, as far as the interface can tell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Up,
    Down,
    /// The interface does not report media presence (`MediaPresentSupported` is false).
    Unknown,
}

impl Link {
    const fn marker(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Unknown => "unknown",
        }
    }
}

/// One network interface, as the firmware describes it.
pub struct Interface {
    pub mac: [u8; 6],
    pub link: Link,
    /// The SNP state the firmware left it in; this loader never changes it.
    pub initialized: bool,
    /// IANA hardware type (`1` = Ethernet).
    pub if_type: u8,
}

/// Open a handle's SNP shared, so the firmware's MNP/IP drivers stay bound to it.
fn open_snp(handle: Handle) -> Option<ScopedProtocol<SimpleNetwork>> {
    // SAFETY: GetProtocol is a shared, non-exclusive open; the agent is this loaded image and
    // the protocol is closed when the ScopedProtocol drops. Only `mode()` is read through it.
    unsafe {
        boot::open_protocol::<SimpleNetwork>(
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

/// Every physical network interface the firmware exposes, read without touching it.
///
/// A firmware network stack publishes SNP on more than one handle for the same card (the
/// controller itself plus child handles its MNP/VLAN layers create), so handles are
/// de-duplicated by permanent MAC address: two real interfaces never share one.
pub fn interfaces() -> Vec<Interface> {
    let Ok(handles) = boot::find_handles::<SimpleNetwork>() else {
        return Vec::new();
    };
    let mut seen: Vec<[u8; 6]> = Vec::new();
    let mut found = Vec::new();
    for snp in handles.iter().filter_map(|&handle| open_snp(handle)) {
        let mode = snp.mode();
        let permanent = mode.permanent_address.into_ethernet_addr();
        if seen.contains(&permanent) {
            continue;
        }
        seen.push(permanent);
        let link = if !bool::from(mode.media_present_supported) {
            Link::Unknown
        } else if bool::from(mode.media_present) {
            Link::Up
        } else {
            Link::Down
        };
        found.push(Interface {
            mac: mode.current_address.into_ethernet_addr(),
            link,
            initialized: mode.state == NetworkState::INITIALIZED,
            if_type: mode.if_type,
        });
    }
    found
}

/// Boot evidence: how many interfaces exist and their state, with the policy stated.
/// Returns the interfaces so the caller can speak them.
pub fn report() -> Vec<Interface> {
    let nics = interfaces();
    aw_mark!(
        "AW_UEFI_NET nics={} policy=deny-by-default transmitted=0",
        nics.len()
    );
    for (index, nic) in nics.iter().enumerate() {
        let m = nic.mac;
        aw_mark!(
            "AW_UEFI_NET_NIC index={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} link={} initialized={} if_type={}",
            index,
            m[0],
            m[1],
            m[2],
            m[3],
            m[4],
            m[5],
            nic.link.marker(),
            nic.initialized,
            nic.if_type
        );
    }
    nics
}
