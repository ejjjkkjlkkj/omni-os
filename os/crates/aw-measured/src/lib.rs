//! Measured-boot integrity check (boot integrity IDS), `no_std`, no `unsafe`.
//!
//! The firmware records every measurement it extends into the TPM in the TCG event log. This
//! crate replays the SHA-256 bank of that log for the platform PCRs 0-7 (firmware code, firmware
//! configuration, option ROMs, boot manager and loader, GPT, Secure Boot policy), builds and
//! parses the `TPM2_PCR_Read` command that fetches the TPM's own values, and compares the two:
//! a log that does not replay to the TPM's registers has been tampered with or is incomplete.
//! It also keeps a baseline of the PCRs from a previous boot, so a change of firmware, loader or
//! Secure Boot policy between two boots is detected and can be announced.

#![no_std]
#![forbid(unsafe_code)]

use aw_sha256::sha256;

/// Platform PCRs covered: 0 to 7.
pub const PCRS: usize = 8;
/// A SHA-256 PCR value.
pub type Digest = [u8; 32];

/// `EV_NO_ACTION`: informational event, never extended into a PCR.
pub const EV_NO_ACTION: u32 = 0x0000_0003;
/// `TPM_ALG_SHA256`.
pub const TPM_ALG_SHA256: u16 = 0x000b;
const TPM_ST_NO_SESSIONS: u16 = 0x8001;
const TPM_CC_PCR_READ: u32 = 0x0000_017e;
const STARTUP_LOCALITY: &[u8] = b"StartupLocality\0";

/// `new = SHA-256(old || measurement)`, the TPM extend operation.
pub fn extend(pcr: &mut Digest, measurement: &Digest) {
    let mut buffer = [0_u8; 64];
    buffer[..32].copy_from_slice(pcr);
    buffer[32..].copy_from_slice(measurement);
    *pcr = sha256(&buffer);
}

/// Replay of the SHA-256 bank of a TCG event log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// Expected PCR 0-7 values.
    pub pcrs: [Digest; PCRS],
    /// Events extended into PCR 0-7.
    pub extended: u32,
    /// Events for PCR 0-7 that carried no SHA-256 digest (the replay cannot be complete).
    pub missing_sha256: u32,
}

impl Default for Replay {
    fn default() -> Self {
        Self::new()
    }
}

impl Replay {
    /// All PCRs start at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pcrs: [[0; 32]; PCRS],
            extended: 0,
            missing_sha256: 0,
        }
    }

    /// Feed one event of the log, in order. PCRs above 7 are ignored.
    pub fn event(&mut self, pcr: u32, event_type: u32, sha256_digest: Option<&[u8]>, data: &[u8]) {
        let Ok(index) = usize::try_from(pcr) else {
            return;
        };
        if index >= PCRS {
            return;
        }
        if event_type == EV_NO_ACTION {
            // The StartupLocality event sets PCR 0's initial value to the locality the TPM was
            // started from (TCG PC Client PFP, 10.4.5.3), before anything is extended.
            if index == 0
                && self.extended == 0
                && data.len() > STARTUP_LOCALITY.len()
                && data.starts_with(STARTUP_LOCALITY)
            {
                self.pcrs[0] = [0; 32];
                self.pcrs[0][31] = data[STARTUP_LOCALITY.len()];
            }
            return;
        }
        match sha256_digest.and_then(|d| <&Digest>::try_from(d).ok()) {
            Some(digest) => {
                extend(&mut self.pcrs[index], digest);
                self.extended += 1;
            }
            None => self.missing_sha256 += 1,
        }
    }
}

/// `TPM2_PCR_Read` of the SHA-256 bank, PCR 0-7.
#[must_use]
pub fn pcr_read_command() -> [u8; 20] {
    let mut c = [0_u8; 20];
    c[0..2].copy_from_slice(&TPM_ST_NO_SESSIONS.to_be_bytes());
    c[2..6].copy_from_slice(&20_u32.to_be_bytes());
    c[6..10].copy_from_slice(&TPM_CC_PCR_READ.to_be_bytes());
    c[10..14].copy_from_slice(&1_u32.to_be_bytes()); // one selection
    c[14..16].copy_from_slice(&TPM_ALG_SHA256.to_be_bytes());
    c[16] = 3; // sizeofSelect
    c[17] = 0xff; // PCR 0-7
    c
}

/// Why a `TPM2_PCR_Read` response was not usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcrReadError {
    /// Shorter than its fields, or its size field disagrees with the buffer.
    Truncated,
    /// The TPM returned an error code.
    ResponseCode(u32),
    /// The response is for another bank or has an inconsistent digest list.
    Malformed,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], PcrReadError> {
        let end = self.at.checked_add(n).ok_or(PcrReadError::Truncated)?;
        let slice = self.data.get(self.at..end).ok_or(PcrReadError::Truncated)?;
        self.at = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, PcrReadError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, PcrReadError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, PcrReadError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Parse a `TPM2_PCR_Read` response: the value of each PCR 0-7 the TPM returned.
pub fn parse_pcr_read(response: &[u8]) -> Result<[Option<Digest>; PCRS], PcrReadError> {
    let mut r = Reader {
        data: response,
        at: 0,
    };
    let _tag = r.u16()?;
    let size = usize::try_from(r.u32()?).map_err(|_| PcrReadError::Truncated)?;
    if size < 10 || size > response.len() {
        return Err(PcrReadError::Truncated);
    }
    r.data = &response[..size];
    let code = r.u32()?;
    if code != 0 {
        return Err(PcrReadError::ResponseCode(code));
    }
    let _update_counter = r.u32()?;
    let selections = r.u32()?;
    let mut mask = 0_u32;
    for _ in 0..selections {
        let hash = r.u16()?;
        let size = usize::from(r.u8()?);
        let bits = r.take(size)?;
        if hash != TPM_ALG_SHA256 {
            return Err(PcrReadError::Malformed);
        }
        for (byte, value) in bits.iter().enumerate().take(4) {
            mask |= u32::from(*value) << (8 * byte);
        }
    }
    let count = r.u32()?;
    if count != mask.count_ones() {
        return Err(PcrReadError::Malformed);
    }
    let mut out = [None; PCRS];
    let mut pcr = 0_usize;
    for _ in 0..count {
        while pcr < 32 && mask & (1 << pcr) == 0 {
            pcr += 1;
        }
        let len = usize::from(r.u16()?);
        let digest = r.take(len)?;
        let digest = Digest::try_from(digest).map_err(|_| PcrReadError::Malformed)?;
        if pcr < PCRS {
            out[pcr] = Some(digest);
        }
        pcr += 1;
    }
    Ok(out)
}

/// Bit `i` set when the replayed PCR `i` differs from the value the TPM returned (a PCR the TPM
/// did not return counts as different).
#[must_use]
pub fn mismatches(replayed: &[Digest; PCRS], tpm: &[Option<Digest>; PCRS]) -> u8 {
    let mut mask = 0;
    for i in 0..PCRS {
        if tpm[i].as_ref() != Some(&replayed[i]) {
            mask |= 1 << i;
        }
    }
    mask
}

/// Bit `i` set when PCR `i` differs between two boots.
#[must_use]
pub fn changed(old: &[Digest; PCRS], new: &[Digest; PCRS]) -> u8 {
    let mut mask = 0;
    for i in 0..PCRS {
        if old[i] != new[i] {
            mask |= 1 << i;
        }
    }
    mask
}

/// Size of an encoded baseline: magic, eight PCRs, SHA-256 of both.
pub const BASELINE_BYTES: usize = 8 + 32 * PCRS + 32;
const BASELINE_MAGIC: &[u8; 8] = b"OMNIPCR\x01";

/// Encode the PCR values of a boot, self-checked by a trailing SHA-256.
#[must_use]
pub fn encode_baseline(pcrs: &[Digest; PCRS]) -> [u8; BASELINE_BYTES] {
    let mut out = [0_u8; BASELINE_BYTES];
    out[..8].copy_from_slice(BASELINE_MAGIC);
    for (i, pcr) in pcrs.iter().enumerate() {
        out[8 + 32 * i..8 + 32 * (i + 1)].copy_from_slice(pcr);
    }
    let check = sha256(&out[..BASELINE_BYTES - 32]);
    out[BASELINE_BYTES - 32..].copy_from_slice(&check);
    out
}

/// Decode a baseline; `None` when it is absent, truncated or damaged.
#[must_use]
pub fn decode_baseline(data: &[u8]) -> Option<[Digest; PCRS]> {
    if data.len() != BASELINE_BYTES || &data[..8] != BASELINE_MAGIC {
        return None;
    }
    if sha256(&data[..BASELINE_BYTES - 32])[..] != data[BASELINE_BYTES - 32..] {
        return None;
    }
    let mut pcrs = [[0_u8; 32]; PCRS];
    for (i, pcr) in pcrs.iter_mut().enumerate() {
        pcr.copy_from_slice(&data[8 + 32 * i..8 + 32 * (i + 1)]);
    }
    Some(pcrs)
}

/// Comma-separated PCR numbers of a mask (`"none"` when empty), for markers and speech.
pub fn mask_list(mask: u8, out: &mut [u8; 16]) -> &str {
    if mask == 0 {
        return "none";
    }
    let mut n = 0;
    for i in 0..PCRS {
        if mask & (1 << i) != 0 {
            if n > 0 {
                out[n] = b',';
                n += 1;
            }
            out[n] = b'0' + i as u8;
            n += 1;
        }
    }
    core::str::from_utf8(&out[..n]).unwrap_or("none")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Digest {
        let mut d = [0_u8; 32];
        for (i, b) in d.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        d
    }

    #[test]
    fn extend_matches_the_tpm_definition() {
        // Extending a zero PCR with a zero digest is SHA-256 of 64 zero bytes.
        let mut pcr = [0_u8; 32];
        extend(&mut pcr, &[0; 32]);
        assert_eq!(
            pcr,
            hex("f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b")
        );
    }

    #[test]
    fn replay_follows_the_log_and_skips_no_action() {
        let m1 = sha256(b"firmware");
        let m2 = sha256(b"loader");
        let mut replay = Replay::new();
        replay.event(0, 0x8000_0008, Some(&m1), b"");
        replay.event(0, EV_NO_ACTION, Some(&m2), b"ignored");
        replay.event(4, 0x8000_0003, Some(&m2), b"");
        replay.event(9, 0x0d, Some(&m2), b""); // outside 0-7
        let mut pcr0 = [0; 32];
        extend(&mut pcr0, &m1);
        let mut pcr4 = [0; 32];
        extend(&mut pcr4, &m2);
        assert_eq!(replay.pcrs[0], pcr0);
        assert_eq!(replay.pcrs[4], pcr4);
        assert_eq!(replay.pcrs[1], [0; 32]);
        assert_eq!(replay.extended, 2);
        replay.event(1, 1, None, b"");
        replay.event(1, 1, Some(&[0; 20]), b"");
        assert_eq!(replay.missing_sha256, 2);
    }

    #[test]
    fn startup_locality_sets_pcr0_initial_value() {
        let mut replay = Replay::new();
        replay.event(0, EV_NO_ACTION, None, b"StartupLocality\0\x03");
        assert_eq!(replay.pcrs[0][31], 3);
        let m = sha256(b"x");
        replay.event(0, 1, Some(&m), b"");
        let mut expected = [0; 32];
        expected[31] = 3;
        extend(&mut expected, &m);
        assert_eq!(replay.pcrs[0], expected);
        // Once something is extended, a late StartupLocality event changes nothing.
        let before = replay.pcrs[0];
        replay.event(0, EV_NO_ACTION, None, b"StartupLocality\0\x04");
        assert_eq!(replay.pcrs[0], before);
    }

    fn response(mask: [u8; 3], digests: &[Digest], code: u32) -> ([u8; 512], usize) {
        let mut b = [0_u8; 512];
        let mut n = 10;
        b[0..2].copy_from_slice(&0x8001_u16.to_be_bytes());
        b[6..10].copy_from_slice(&code.to_be_bytes());
        let mut put = |b: &mut [u8; 512], bytes: &[u8]| {
            b[n..n + bytes.len()].copy_from_slice(bytes);
            n += bytes.len();
        };
        put(&mut b, &7_u32.to_be_bytes());
        put(&mut b, &1_u32.to_be_bytes());
        put(&mut b, &TPM_ALG_SHA256.to_be_bytes());
        put(&mut b, &[3]);
        put(&mut b, &mask);
        put(&mut b, &(digests.len() as u32).to_be_bytes());
        for d in digests {
            put(&mut b, &32_u16.to_be_bytes());
            put(&mut b, d);
        }
        b[2..6].copy_from_slice(&(n as u32).to_be_bytes());
        (b, n)
    }

    #[test]
    fn command_is_a_well_formed_pcr_read() {
        let c = pcr_read_command();
        assert_eq!(&c[..10], &[0x80, 0x01, 0, 0, 0, 20, 0, 0, 0x01, 0x7e]);
        assert_eq!(&c[10..], &[0, 0, 0, 1, 0, 0x0b, 3, 0xff, 0, 0]);
    }

    #[test]
    fn parses_all_eight_pcrs() {
        let digests: [Digest; 8] = core::array::from_fn(|i| [i as u8 + 1; 32]);
        let (b, n) = response([0xff, 0, 0], &digests, 0);
        let pcrs = parse_pcr_read(&b[..n]).unwrap();
        for i in 0..8 {
            assert_eq!(pcrs[i], Some(digests[i]));
        }
        assert_eq!(mismatches(&digests, &pcrs), 0);
        let mut replayed = digests;
        replayed[7][0] ^= 1;
        assert_eq!(mismatches(&replayed, &pcrs), 0x80);
    }

    #[test]
    fn partial_response_maps_digests_to_the_returned_selection() {
        let (b, n) = response([0b0001_0010, 0, 0], &[[4; 32], [9; 32]], 0);
        let pcrs = parse_pcr_read(&b[..n]).unwrap();
        assert_eq!(pcrs[1], Some([4; 32]));
        assert_eq!(pcrs[4], Some([9; 32]));
        assert_eq!(pcrs[0], None);
        assert_eq!(mismatches(&[[4; 32]; 8], &pcrs) & 1, 1);
    }

    #[test]
    fn rejects_bad_responses() {
        let (b, n) = response([0xff, 0, 0], &[[0; 32]; 8], 0x101);
        assert_eq!(
            parse_pcr_read(&b[..n]),
            Err(PcrReadError::ResponseCode(0x101))
        );
        let (b, n) = response([0xff, 0, 0], &[[0; 32]; 7], 0);
        assert_eq!(parse_pcr_read(&b[..n]), Err(PcrReadError::Malformed));
        let (b, n) = response([0xff, 0, 0], &[[0; 32]; 8], 0);
        assert_eq!(parse_pcr_read(&b[..n - 1]), Err(PcrReadError::Truncated));
        // Every prefix and every single-byte corruption parses or fails without panicking.
        for len in 0..n {
            let _ = parse_pcr_read(&b[..len]);
        }
        for i in 0..n {
            let mut c = b;
            c[i] ^= 0xa5;
            let _ = parse_pcr_read(&c[..n]);
        }
    }

    #[test]
    fn baseline_round_trips_and_detects_damage() {
        let pcrs: [Digest; 8] = core::array::from_fn(|i| sha256(&[i as u8]));
        let encoded = encode_baseline(&pcrs);
        assert_eq!(decode_baseline(&encoded), Some(pcrs));
        for i in 0..BASELINE_BYTES {
            let mut c = encoded;
            c[i] ^= 1;
            assert_eq!(decode_baseline(&c), None, "byte {i}");
        }
        assert_eq!(decode_baseline(&encoded[..BASELINE_BYTES - 1]), None);
        let mut other = pcrs;
        other[4] = [0; 32];
        other[7] = [1; 32];
        assert_eq!(changed(&pcrs, &other), 0x90);
    }

    #[test]
    fn mask_lists_pcr_numbers() {
        let mut buf = [0; 16];
        assert_eq!(mask_list(0, &mut buf), "none");
        assert_eq!(mask_list(0x91, &mut buf), "0,4,7");
        assert_eq!(mask_list(0xff, &mut buf), "0,1,2,3,4,5,6,7");
    }
}
