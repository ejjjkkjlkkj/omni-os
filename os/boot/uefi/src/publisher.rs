//! Publisher signatures on kernel images (Ed25519, `aw-sign`).
//!
//! The publisher's public key is embedded when the loader is built (`OMNI_PUBLISHER_PUBKEY`, 64
//! hex digits); the loader itself is then covered by Secure Boot. Images enter the system at two
//! points only, and both check here: the pre-installation environment (the kernel on the
//! installation medium) and the Recovery Core's reinstall. A `KERNEL.SIG` next to an image holds
//! the 64-byte signature of `aw_sign::kernel_message(image)`.

use crate::aw_mark;

/// Outcome of checking one image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// Signed by the embedded publisher key.
    Valid,
    /// A signature is present but does not verify: always refused.
    Invalid,
    /// No signature file.
    Unsigned,
    /// This loader carries no publisher key (development build).
    NoKey,
}

impl Verdict {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Unsigned => "unsigned",
            Self::NoKey => "no_publisher_key",
        }
    }
}

/// The embedded publisher key, if this loader was built with one.
pub fn key() -> Option<[u8; 32]> {
    option_env!("OMNI_PUBLISHER_PUBKEY").and_then(aw_sign::parse_hex32)
}

/// Announce the embedded key (its first bytes, as a fingerprint).
pub fn report() {
    match key() {
        Some(key) => aw_mark!(
            "AW_PUBLISHER_KEY present=true fingerprint={:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            key[0],
            key[1],
            key[2],
            key[3],
            key[4],
            key[5],
            key[6],
            key[7]
        ),
        None => aw_mark!("AW_PUBLISHER_KEY present=false"),
    }
}

/// Check a kernel `image` against its signature file contents, if any.
pub fn check(image: &[u8], signature: Option<&[u8]>) -> Verdict {
    check_message(&aw_sign::kernel_message(image), signature)
}

/// Check a recovery image (network or removable) against its signature, if any.
pub fn check_recovery(image: &[u8], signature: Option<&[u8]>) -> Verdict {
    check_message(&aw_sign::recovery_message(image), signature)
}

fn check_message(message: &[u8], signature: Option<&[u8]>) -> Verdict {
    let Some(key) = key() else {
        return Verdict::NoKey;
    };
    let Some(signature) = signature else {
        return Verdict::Unsigned;
    };
    let Ok(signature) = <[u8; 64]>::try_from(signature) else {
        return Verdict::Invalid;
    };
    if aw_sign::verify(&key, message, &signature) {
        Verdict::Valid
    } else {
        Verdict::Invalid
    }
}
