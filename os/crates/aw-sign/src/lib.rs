//! Publisher signatures for omni-os images: SHA-512 (FIPS 180-4) and Ed25519 (RFC 8032),
//! self-contained and `no_std`, so the UEFI loader verifies what it installs or reinstalls
//! without any third-party code.
//!
//! A kernel image is signed over a domain-separated message, [`kernel_message`]: the ASCII tag
//! `omni-os-kernel-v1` followed by the image's SHA-256. Verification is the cofactorless RFC 8032
//! check `[S]B = R + [k]A` with `S < L` and canonical point encodings; it runs in variable time,
//! which is fine for public data (only signing touches the secret, and signing runs on the
//! publisher's machine).

#![no_std]
#![forbid(unsafe_code)]

// ---- SHA-512 --------------------------------------------------------------------------------

const K512: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

/// Streaming SHA-512.
#[derive(Clone)]
pub struct Sha512 {
    state: [u64; 8],
    block: [u8; 128],
    used: usize,
    total: u128,
}

impl Default for Sha512 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha512 {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09e667f3bcc908,
                0xbb67ae8584caa73b,
                0x3c6ef372fe94f82b,
                0xa54ff53a5f1d36f1,
                0x510e527fade682d1,
                0x9b05688c2b3e6c1f,
                0x1f83d9abfb41bd6b,
                0x5be0cd19137e2179,
            ],
            block: [0; 128],
            used: 0,
            total: 0,
        }
    }

    fn compress(&mut self) {
        let mut w = [0_u64; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let mut bytes = [0_u8; 8];
            bytes.copy_from_slice(&self.block[8 * i..8 * i + 8]);
            *word = u64::from_be_bytes(bytes);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K512[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total += data.len() as u128;
        while !data.is_empty() {
            let take = (128 - self.used).min(data.len());
            self.block[self.used..self.used + take].copy_from_slice(&data[..take]);
            self.used += take;
            data = &data[take..];
            if self.used == 128 {
                self.compress();
                self.used = 0;
            }
        }
    }

    #[must_use]
    pub fn finalize(mut self) -> [u8; 64] {
        let bits = self.total * 8;
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 112 {
            self.block[self.used..].fill(0);
            self.compress();
            self.used = 0;
        }
        self.block[self.used..112].fill(0);
        self.block[112..].copy_from_slice(&bits.to_be_bytes());
        self.compress();
        let mut out = [0_u8; 64];
        for (chunk, word) in out.as_chunks_mut::<8>().0.iter_mut().zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

#[must_use]
pub fn sha512(data: &[u8]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize()
}

// ---- GF(2^255 - 19), radix 2^51 -------------------------------------------------------------

const MASK: u64 = (1 << 51) - 1;

#[derive(Clone, Copy)]
struct Fe([u64; 5]);

const P_MINUS_2: [u8; 32] = exponent(0xeb, 0x7f);
const P_MINUS_5_OVER_8: [u8; 32] = exponent(0xfd, 0x0f);
const P_MINUS_1_OVER_4: [u8; 32] = exponent(0xfb, 0x1f);

/// Little-endian exponent `low, 0xff * 30, high`.
const fn exponent(low: u8, high: u8) -> [u8; 32] {
    let mut out = [0xff_u8; 32];
    out[0] = low;
    out[31] = high;
    out
}

fn load8(bytes: &[u8], at: usize) -> u64 {
    let mut word = [0_u8; 8];
    word.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(word)
}

impl Fe {
    const ZERO: Self = Self([0; 5]);
    const ONE: Self = Self([1, 0, 0, 0, 0]);

    fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self([
            load8(bytes, 0) & MASK,
            (load8(bytes, 6) >> 3) & MASK,
            (load8(bytes, 12) >> 6) & MASK,
            (load8(bytes, 19) >> 1) & MASK,
            (load8(bytes, 24) >> 12) & MASK,
        ])
    }

    fn carry(mut t: [u64; 5]) -> Self {
        for _ in 0..2 {
            t[1] += t[0] >> 51;
            t[0] &= MASK;
            t[2] += t[1] >> 51;
            t[1] &= MASK;
            t[3] += t[2] >> 51;
            t[2] &= MASK;
            t[4] += t[3] >> 51;
            t[3] &= MASK;
            t[0] += 19 * (t[4] >> 51);
            t[4] &= MASK;
        }
        Self(t)
    }

    fn to_bytes(self) -> [u8; 32] {
        let mut t = Self::carry(self.0).0;
        // Subtract p once if t >= p.
        let mut q = (t[0] + 19) >> 51;
        q = (t[1] + q) >> 51;
        q = (t[2] + q) >> 51;
        q = (t[3] + q) >> 51;
        q = (t[4] + q) >> 51;
        t[0] += 19 * q;
        t[1] += t[0] >> 51;
        t[0] &= MASK;
        t[2] += t[1] >> 51;
        t[1] &= MASK;
        t[3] += t[2] >> 51;
        t[2] &= MASK;
        t[4] += t[3] >> 51;
        t[3] &= MASK;
        t[4] &= MASK;
        let words = [
            t[0] | (t[1] << 51),
            (t[1] >> 13) | (t[2] << 38),
            (t[2] >> 26) | (t[3] << 25),
            (t[3] >> 39) | (t[4] << 12),
        ];
        let mut out = [0_u8; 32];
        for (chunk, word) in out.as_chunks_mut::<8>().0.iter_mut().zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    fn add(self, other: Self) -> Self {
        let mut t = [0_u64; 5];
        for (i, slot) in t.iter_mut().enumerate() {
            *slot = self.0[i] + other.0[i];
        }
        Self::carry(t)
    }

    fn sub(self, other: Self) -> Self {
        // self + 2p - other, limb by limb (every limb stays positive).
        const TWO_P: [u64; 5] = [
            0x000f_ffff_ffff_ffda,
            0x000f_ffff_ffff_fffe,
            0x000f_ffff_ffff_fffe,
            0x000f_ffff_ffff_fffe,
            0x000f_ffff_ffff_fffe,
        ];
        let mut t = [0_u64; 5];
        for (i, slot) in t.iter_mut().enumerate() {
            *slot = self.0[i] + TWO_P[i] - other.0[i];
        }
        Self::carry(t)
    }

    fn neg(self) -> Self {
        Self::ZERO.sub(self)
    }

    fn mul(self, other: Self) -> Self {
        let [a0, a1, a2, a3, a4] = self.0.map(u128::from);
        let [b0, b1, b2, b3, b4] = other.0.map(u128::from);
        let (b1_19, b2_19, b3_19, b4_19) = (b1 * 19, b2 * 19, b3 * 19, b4 * 19);
        let mut r = [
            a0 * b0 + a1 * b4_19 + a2 * b3_19 + a3 * b2_19 + a4 * b1_19,
            a0 * b1 + a1 * b0 + a2 * b4_19 + a3 * b3_19 + a4 * b2_19,
            a0 * b2 + a1 * b1 + a2 * b0 + a3 * b4_19 + a4 * b3_19,
            a0 * b3 + a1 * b2 + a2 * b1 + a3 * b0 + a4 * b4_19,
            a0 * b4 + a1 * b3 + a2 * b2 + a3 * b1 + a4 * b0,
        ];
        let mask = u128::from(MASK);
        for i in 0..4 {
            r[i + 1] += r[i] >> 51;
            r[i] &= mask;
        }
        r[0] += 19 * (r[4] >> 51);
        r[4] &= mask;
        Self::carry(r.map(|limb| limb as u64))
    }

    fn square(self) -> Self {
        self.mul(self)
    }

    fn pow(self, exponent: &[u8; 32]) -> Self {
        let mut result = Self::ONE;
        for byte in exponent.iter().rev() {
            for bit in (0..8).rev() {
                result = result.square();
                if (byte >> bit) & 1 == 1 {
                    result = result.mul(self);
                }
            }
        }
        result
    }

    fn invert(self) -> Self {
        self.pow(&P_MINUS_2)
    }

    fn equals(self, other: Self) -> bool {
        self.to_bytes() == other.to_bytes()
    }

    fn is_negative(self) -> bool {
        self.to_bytes()[0] & 1 == 1
    }

    fn small(value: u64) -> Self {
        Self([value, 0, 0, 0, 0])
    }
}

/// d = -121665 / 121666.
fn curve_d() -> Fe {
    Fe::small(121_665).neg().mul(Fe::small(121_666).invert())
}

// ---- Edwards points, extended coordinates -------------------------------------------------------

#[derive(Clone, Copy)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

impl Point {
    const IDENTITY: Self = Self {
        x: Fe::ZERO,
        y: Fe::ONE,
        z: Fe::ONE,
        t: Fe::ZERO,
    };

    fn add(self, other: Self, d2: Fe) -> Self {
        let a = self.y.sub(self.x).mul(other.y.sub(other.x));
        let b = self.y.add(self.x).mul(other.y.add(other.x));
        let c = self.t.mul(d2).mul(other.t);
        let d = self.z.add(self.z).mul(other.z);
        let (e, f, g, h) = (b.sub(a), d.sub(c), d.add(c), b.add(a));
        Self {
            x: e.mul(f),
            y: g.mul(h),
            t: e.mul(h),
            z: f.mul(g),
        }
    }

    fn neg(self) -> Self {
        Self {
            x: self.x.neg(),
            y: self.y,
            z: self.z,
            t: self.t.neg(),
        }
    }

    fn scalar_mul(self, scalar: &[u8; 32], d2: Fe) -> Self {
        let mut result = Self::IDENTITY;
        for byte in scalar.iter().rev() {
            for bit in (0..8).rev() {
                result = result.add(result, d2);
                if (byte >> bit) & 1 == 1 {
                    result = result.add(self, d2);
                }
            }
        }
        result
    }

    fn compress(self) -> [u8; 32] {
        let zi = self.z.invert();
        let x = self.x.mul(zi);
        let y = self.y.mul(zi);
        let mut out = y.to_bytes();
        out[31] |= u8::from(x.is_negative()) << 7;
        out
    }

    fn decompress(bytes: &[u8; 32], d: Fe) -> Option<Self> {
        let sign = bytes[31] >> 7;
        let mut y_bytes = *bytes;
        y_bytes[31] &= 0x7f;
        let y = Fe::from_bytes(&y_bytes);
        if y.to_bytes() != y_bytes {
            return None; // non-canonical y (>= p)
        }
        let yy = y.square();
        let u = yy.sub(Fe::ONE);
        let v = d.mul(yy).add(Fe::ONE);
        let v3 = v.square().mul(v);
        let v7 = v3.square().mul(v);
        let mut x = u.mul(v3).mul(u.mul(v7).pow(&P_MINUS_5_OVER_8));
        let vxx = v.mul(x.square());
        if !vxx.equals(u) {
            if !vxx.equals(u.neg()) {
                return None;
            }
            x = x.mul(Fe::small(2).pow(&P_MINUS_1_OVER_4));
        }
        if x.equals(Fe::ZERO) && sign == 1 {
            return None;
        }
        if u8::from(x.is_negative()) != sign {
            x = x.neg();
        }
        Some(Self {
            x,
            y,
            z: Fe::ONE,
            t: x.mul(y),
        })
    }
}

/// Encoding of the base point B (y = 4/5, x positive).
const BASE: [u8; 32] = {
    let mut out = [0x66_u8; 32];
    out[0] = 0x58;
    out
};

// ---- Scalars modulo L = 2^252 + 27742317777372353535851937790883648493 ------------------------

const L: [u64; 4] = [
    0x5812631a5cf5d3ed,
    0x14def9dea2f79cd6,
    0x0000000000000000,
    0x1000000000000000,
];

fn geq_l(a: &[u64; 4]) -> bool {
    for i in (0..4).rev() {
        if a[i] != L[i] {
            return a[i] > L[i];
        }
    }
    true
}

fn sub_l(a: &mut [u64; 4]) {
    let mut borrow = 0_u64;
    for i in 0..4 {
        let (x, b1) = a[i].overflowing_sub(L[i]);
        let (y, b2) = x.overflowing_sub(borrow);
        a[i] = y;
        borrow = u64::from(b1 || b2);
    }
}

/// `2a + bit` modulo L, for a < L.
fn double_add_bit(a: &mut [u64; 4], bit: u64) {
    let mut carry = bit;
    for limb in a.iter_mut() {
        let next = *limb >> 63;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
    if geq_l(a) {
        sub_l(a);
    }
}

/// `a + b` modulo L, for a, b < L.
fn add_mod_l(a: &[u64; 4], b: &[u64; 4]) -> [u64; 4] {
    let mut out = [0_u64; 4];
    let mut carry = 0_u64;
    for i in 0..4 {
        let (x, c1) = a[i].overflowing_add(b[i]);
        let (y, c2) = x.overflowing_add(carry);
        out[i] = y;
        carry = u64::from(c1 || c2);
    }
    if geq_l(&out) {
        sub_l(&mut out);
    }
    out
}

/// Little-endian bytes reduced modulo L.
fn reduce(bytes: &[u8]) -> [u64; 4] {
    let mut r = [0_u64; 4];
    for byte in bytes.iter().rev() {
        for bit in (0..8).rev() {
            double_add_bit(&mut r, u64::from((byte >> bit) & 1));
        }
    }
    r
}

fn mul_mod_l(a: &[u64; 4], b: &[u64; 4]) -> [u64; 4] {
    let mut r = [0_u64; 4];
    for i in (0..4).rev() {
        for bit in (0..64).rev() {
            double_add_bit(&mut r, 0);
            if (b[i] >> bit) & 1 == 1 {
                r = add_mod_l(&r, a);
            }
        }
    }
    r
}

fn scalar_bytes(s: &[u64; 4]) -> [u8; 32] {
    let mut out = [0_u8; 32];
    for (chunk, word) in out.as_chunks_mut::<8>().0.iter_mut().zip(s) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

// ---- Ed25519 ----------------------------------------------------------------------------------

/// Verifies an Ed25519 signature (RFC 8032, 5.1.7).
#[must_use]
pub fn verify(public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let d = curve_d();
    let d2 = d.add(d);
    let Some(a) = Point::decompress(public_key, d) else {
        return false;
    };
    let Some(base) = Point::decompress(&BASE, d) else {
        return false;
    };
    let mut r_bytes = [0_u8; 32];
    r_bytes.copy_from_slice(&signature[..32]);
    let mut s_bytes = [0_u8; 32];
    s_bytes.copy_from_slice(&signature[32..]);
    let mut s = [0_u64; 4];
    for (limb, chunk) in s.iter_mut().zip(s_bytes.as_chunks::<8>().0.iter()) {
        *limb = load8(chunk, 0);
    }
    if geq_l(&s) {
        return false;
    }
    // R must be a valid point encoding.
    if Point::decompress(&r_bytes, d).is_none() {
        return false;
    }
    let mut hasher = Sha512::new();
    hasher.update(&r_bytes);
    hasher.update(public_key);
    hasher.update(message);
    let k = scalar_bytes(&reduce(&hasher.finalize()));
    let check = base
        .scalar_mul(&s_bytes, d2)
        .add(a.neg().scalar_mul(&k, d2), d2);
    check.compress() == r_bytes
}

/// Public key of a 32-byte secret seed (RFC 8032, 5.1.5).
#[must_use]
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    let (scalar, _) = expand(seed);
    let d = curve_d();
    base_point(d).scalar_mul(&scalar, d.add(d)).compress()
}

fn base_point(d: Fe) -> Point {
    Point::decompress(&BASE, d).unwrap_or(Point::IDENTITY)
}

fn expand(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = sha512(seed);
    let mut scalar = [0_u8; 32];
    scalar.copy_from_slice(&h[..32]);
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;
    let mut prefix = [0_u8; 32];
    prefix.copy_from_slice(&h[32..]);
    (scalar, prefix)
}

/// Signs `message` with a 32-byte secret seed (RFC 8032, 5.1.6). For the publisher's tools.
#[must_use]
pub fn sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let (scalar, prefix) = expand(seed);
    let d = curve_d();
    let d2 = d.add(d);
    let base = base_point(d);
    let public = base.scalar_mul(&scalar, d2).compress();
    let mut hasher = Sha512::new();
    hasher.update(&prefix);
    hasher.update(message);
    let r = reduce(&hasher.finalize());
    let r_point = base.scalar_mul(&scalar_bytes(&r), d2).compress();
    let mut hasher = Sha512::new();
    hasher.update(&r_point);
    hasher.update(&public);
    hasher.update(message);
    let k = reduce(&hasher.finalize());
    let s = add_mod_l(&r, &mul_mod_l(&k, &reduce(&scalar)));
    let mut out = [0_u8; 64];
    out[..32].copy_from_slice(&r_point);
    out[32..].copy_from_slice(&scalar_bytes(&s));
    out
}

/// Tag that starts every signed kernel message.
pub const KERNEL_TAG: &[u8; 17] = b"omni-os-kernel-v1";

/// The message a publisher signs for a kernel image: [`KERNEL_TAG`] then its SHA-256.
#[must_use]
pub fn kernel_message(image: &[u8]) -> [u8; 49] {
    let mut out = [0_u8; 49];
    out[..17].copy_from_slice(KERNEL_TAG);
    out[17..].copy_from_slice(&aw_sha256::sha256(image));
    out
}

/// Tag that starts every signed recovery-image message (network or removable recovery).
pub const RECOVERY_TAG: &[u8; 19] = b"omni-os-recovery-v1";

/// The message a publisher signs for a recovery image: [`RECOVERY_TAG`] then its SHA-256.
#[must_use]
pub fn recovery_message(image: &[u8]) -> [u8; 51] {
    let mut out = [0_u8; 51];
    out[..19].copy_from_slice(RECOVERY_TAG);
    out[19..].copy_from_slice(&aw_sha256::sha256(image));
    out
}

/// Parses 64 hex digits (a public key as embedded at build time).
#[must_use]
pub fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 {
        return None;
    }
    let mut out = [0_u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex<const N: usize>(text: &str) -> [u8; N] {
        let mut out = [0_u8; N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn sha512_known_answers() {
        assert_eq!(
            sha512(b"abc"),
            hex::<64>(
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
            )
        );
        assert_eq!(
            sha512(b""),
            hex::<64>(
                "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
            )
        );
        // Two-block message (FIPS 180-2 example).
        assert_eq!(
            sha512(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"),
            hex::<64>("8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909")
        );
    }

    struct Vector {
        seed: &'static str,
        public: &'static str,
        message: &'static str,
        signature: &'static str,
    }

    // RFC 8032, 7.1, tests 1 to 3.
    const VECTORS: [Vector; 3] = [
        Vector {
            seed: "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
            public: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            message: "",
            signature: "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        },
        Vector {
            seed: "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
            public: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            message: "72",
            signature: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        },
        Vector {
            seed: "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
            public: "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
            message: "af82",
            signature: "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
        },
    ];

    fn message(text: &str) -> std::vec::Vec<u8> {
        (0..text.len() / 2)
            .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    extern crate std;

    #[test]
    fn rfc8032_vectors_sign_and_verify() {
        for vector in VECTORS {
            let seed = hex::<32>(vector.seed);
            let public = hex::<32>(vector.public);
            let signature = hex::<64>(vector.signature);
            let msg = message(vector.message);
            assert_eq!(public_key(&seed), public);
            assert_eq!(sign(&seed, &msg), signature);
            assert!(verify(&public, &msg, &signature));
        }
    }

    #[test]
    fn rejects_forgeries() {
        let vector = &VECTORS[1];
        let public = hex::<32>(vector.public);
        let signature = hex::<64>(vector.signature);
        let msg = message(vector.message);
        for byte in 0..64 {
            let mut bad = signature;
            bad[byte] ^= 0x01;
            assert!(
                !verify(&public, &msg, &bad),
                "flipped signature byte {byte}"
            );
        }
        assert!(!verify(&public, b"s", &signature));
        let mut other = public;
        other[0] ^= 1;
        assert!(!verify(&other, &msg, &signature));
        // S >= L is refused (malleability).
        let mut high = signature;
        high[63] |= 0xf0;
        assert!(!verify(&public, &msg, &high));
    }

    #[test]
    fn kernel_messages_are_domain_separated() {
        let seed = [7_u8; 32];
        let image = b"kernel image";
        let sig = sign(&seed, &kernel_message(image));
        assert!(verify(&public_key(&seed), &kernel_message(image), &sig));
        assert!(!verify(
            &public_key(&seed),
            &kernel_message(b"kernel imagf"),
            &sig
        ));
        assert!(!verify(&public_key(&seed), image, &sig));
        // A kernel signature never passes as a recovery-image signature, and back.
        assert!(!verify(&public_key(&seed), &recovery_message(image), &sig));
        assert_eq!(parse_hex32(&"ab".repeat(32)), Some([0xab; 32]));
        assert_eq!(parse_hex32("zz"), None);
    }
}
