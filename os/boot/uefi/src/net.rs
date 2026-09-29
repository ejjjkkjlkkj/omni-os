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
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::media::file::Directory;
use uefi::proto::network::http::{HttpBinding, HttpHelper};
use uefi::proto::network::ip4config2::Ip4Config2;
use uefi::proto::network::snp::{NetworkState, SimpleNetwork};
use uefi::{Handle, Status};
use uefi_raw::protocol::network::http::HttpStatusCode;
use uefi_raw::protocol::network::ip4_config2::{Ip4Config2DataType, Ip4Config2InterfaceInfo};

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

/// What DHCP gave this machine.
pub struct Lease {
    pub address: [u8; 4],
    pub mask: [u8; 4],
    pub gateway: Option<[u8; 4]>,
    pub dns: Option<[u8; 4]>,
}

fn first_address(data: &[u8]) -> Option<[u8; 4]> {
    let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
    (bytes != [0; 4]).then_some(bytes)
}

pub(crate) fn dotted(address: [u8; 4]) -> alloc::string::String {
    alloc::format!(
        "{}.{}.{}.{}",
        address[0],
        address[1],
        address[2],
        address[3]
    )
}

/// The default route learned by DHCP. `GATEWAY` data only holds a manually configured
/// gateway, so read the interface's route table instead: the firmware returns it right after
/// the `Ip4Config2InterfaceInfo` structure, as 12-byte (subnet, mask, gateway) entries; the
/// default route is the one whose mask is 0.0.0.0.
fn default_gateway(config: &mut Ip4Config2) -> Option<[u8; 4]> {
    let data = config.get_data(Ip4Config2DataType::INTERFACE_INFO).ok()?;
    let count_at = core::mem::offset_of!(Ip4Config2InterfaceInfo, route_table_size);
    let count = u32::from_le_bytes(data.get(count_at..count_at + 4)?.try_into().ok()?);
    let base = core::mem::size_of::<Ip4Config2InterfaceInfo>();
    (0..usize::try_from(count).ok()?).find_map(|index| {
        let entry = data.get(base + 12 * index..base + 12 * index + 12)?;
        (entry[4..8] == [0; 4])
            .then(|| first_address(&entry[8..12]))
            .flatten()
    })
}

/// Obtain an IPv4 address by DHCP through the firmware's own stack (`Ip4Config2`).
///
/// Only ever called on an explicit request (`reason` says which); the loader never opens the
/// network by itself. The firmware tears the stack down at ExitBootServices, so nothing stays
/// reachable once the kernel runs.
pub fn dhcp(reason: &str) -> Result<Lease, Status> {
    aw_mark!("AW_UEFI_NET_DHCP_BEGIN reason={reason}");
    let handles = boot::find_handles::<Ip4Config2>().map_err(|error| {
        aw_mark!(
            "AW_UEFI_NET_DHCP_FAIL reason=no_ip4_stack status={:?}",
            error.status()
        );
        error.status()
    })?;
    let mut last = Status::NOT_FOUND;
    for handle in handles.iter().copied() {
        let Ok(mut config) = Ip4Config2::new(handle) else {
            continue;
        };
        if let Err(error) = config.ifup() {
            last = error.status();
            aw_mark!("AW_UEFI_NET_DHCP_ATTEMPT_FAIL status={last:?}");
            continue;
        }
        let Ok(info) = config.get_interface_info() else {
            continue;
        };
        let lease = Lease {
            address: info.station_addr.octets(),
            mask: info.subnet_mask.octets(),
            gateway: default_gateway(&mut config).or_else(|| {
                config
                    .get_data(Ip4Config2DataType::GATEWAY)
                    .ok()
                    .and_then(|data| first_address(&data))
            }),
            dns: config
                .get_data(Ip4Config2DataType::DNS_SERVER)
                .ok()
                .and_then(|data| first_address(&data)),
        };
        aw_mark!(
            "AW_UEFI_NET_DHCP_OK address={} mask={} gateway={} dns={}",
            dotted(lease.address),
            dotted(lease.mask),
            lease.gateway.map_or_else(|| "none".into(), dotted),
            lease.dns.map_or_else(|| "none".into(), dotted)
        );
        return Ok(lease);
    }
    aw_mark!("AW_UEFI_NET_DHCP_FAIL reason=no_lease status={last:?}");
    Err(last)
}

/// One-shot network request left by the machine's owner on the ESP (`\OMNI\NET.REQ`).
/// The request is consumed (deleted) before acting, so it can never repeat on its own.
pub fn on_request(root: &mut Directory) {
    const REQUEST: &str = "OMNI\\NET.REQ";
    let Some(request) = crate::recovery::read_file(root, REQUEST) else {
        return;
    };
    let consumed = crate::recovery::remove_file(root, REQUEST);
    aw_mark!("AW_UEFI_NET_REQUEST consumed={consumed}");
    if !consumed || dhcp("request_file").is_err() {
        return;
    }
    // Optional network recovery line: `recover <http-url> sha256=<64 hex>`. The image is started
    // only if its digest equals the one the owner pinned in the request.
    let text = core::str::from_utf8(&request).unwrap_or("");
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("recover") {
            continue;
        }
        let (Some(url), Some(pin)) = (words.next(), words.next()) else {
            aw_mark!("AW_UEFI_NET_RECOVERY_FAIL reason=malformed_request");
            continue;
        };
        match pin.strip_prefix("sha256=").and_then(parse_digest) {
            Some(digest) => network_recovery(url, &digest),
            None => aw_mark!("AW_UEFI_NET_RECOVERY_FAIL reason=no_pinned_digest"),
        }
    }
}

fn parse_digest(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0_u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Upper bound for a recovery image fetched over the network.
const MAX_RECOVERY_BYTES: usize = 64 * 1024 * 1024;

/// Fetch a recovery image with the firmware's HTTP stack, verify it against the pinned SHA-256,
/// then start it through `LoadImage` (Secure Boot policy applies). Integrity does not depend on
/// the transport: a modified or truncated download is refused before any byte runs.
fn network_recovery(url: &str, pinned: &[u8; 32]) {
    aw_mark!("AW_UEFI_NET_RECOVERY_BEGIN url={url}");
    let image = match fetch(url) {
        Ok(image) => image,
        Err(reason) => {
            aw_mark!("AW_UEFI_NET_RECOVERY_FAIL reason={reason}");
            return;
        }
    };
    let digest = aw_sha256::sha256(&image);
    if &digest != pinned {
        aw_mark!(
            "AW_UEFI_NET_RECOVERY_REFUSED reason=digest_mismatch bytes={}",
            image.len()
        );
        return;
    }
    aw_mark!("AW_UEFI_NET_RECOVERY_VERIFIED bytes={}", image.len());
    match boot::load_image(
        boot::image_handle(),
        boot::LoadImageSource::FromBuffer {
            buffer: &image,
            file_path: None,
        },
    ) {
        Ok(child) => {
            aw_mark!("AW_UEFI_NET_RECOVERY_START");
            let status = boot::start_image(child).err().map(|e| e.status());
            aw_mark!("AW_UEFI_NET_RECOVERY_RETURNED status={status:?}");
        }
        Err(error) => aw_mark!(
            "AW_UEFI_NET_RECOVERY_REFUSED reason=load_image status={:?}",
            error.status()
        ),
    }
}

fn fetch(url: &str) -> Result<Vec<u8>, &'static str> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("unsupported_scheme");
    }
    let nic = boot::find_handles::<HttpBinding>()
        .ok()
        .and_then(|handles| handles.first().copied())
        .ok_or("no_http_stack")?;
    let mut http = HttpHelper::new(nic).map_err(|_| "http_open")?;
    http.configure().map_err(|_| "http_configure")?;
    http.request_get(url).map_err(|_| "http_request")?;
    let first = http.response_first(true).map_err(|_| "http_response")?;
    if first.status != HttpStatusCode::STATUS_200_OK {
        aw_mark!("AW_UEFI_NET_RECOVERY_HTTP status={:?}", first.status);
        return Err("http_status");
    }
    let length = first
        .headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .ok_or("no_content_length")?;
    if length > MAX_RECOVERY_BYTES {
        return Err("too_large");
    }
    let mut body = first.body;
    while body.len() < length {
        let before = body.len();
        http.response_more(&mut body).map_err(|_| "http_body")?;
        if body.len() == before {
            return Err("http_stalled");
        }
    }
    body.truncate(length);
    aw_mark!("AW_UEFI_NET_RECOVERY_FETCHED bytes={}", body.len());
    Ok(body)
}
