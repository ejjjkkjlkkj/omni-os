//! `AWKN` native-kernel image header.
//!
//! The native kernel ships as a flat `objcopy -O binary` image whose first
//! bytes are a fixed 64-byte header emitted by `kernel/x86_64/linker.ld`. The
//! header is the single source of truth for where the image must be placed and
//! how much memory it needs, so the UEFI loader never hardcodes a load address
//! or recomputes a BSS size.
//!
//! The kernel is linked **non-relocatable** at [`AwknImageHeader::load_base`].
//! Loading it anywhere else silently corrupts every absolute reference in the
//! image, so the loader must treat a failed fixed-address allocation as a fatal
//! boot error rather than falling back to an arbitrary address.

use crate::UEFI_PAGE_SIZE;

/// `"AWKN"` read as a little-endian `u32`.
pub const AWKN_IMAGE_MAGIC: u32 = 0x4e4b_5741;

/// Header layout version understood by this loader.
pub const AWKN_IMAGE_VERSION: u32 = 1;

/// Encoded size of [`AwknImageHeader`], mirrored by the linker script.
pub const AWKN_IMAGE_HEADER_BYTES: u64 = 64;

/// Upper bound on a sane kernel image, used to reject corrupted headers before
/// they are turned into an allocation request.
const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageHeaderError {
    /// The file is shorter than the fixed header.
    TooSmall = 1,
    /// The first four bytes are not `AWKN`.
    InvalidMagic = 2,
    /// The header version is not understood by this loader.
    UnsupportedVersion = 3,
    /// `header_byte_len` disagrees with [`AWKN_IMAGE_HEADER_BYTES`].
    InvalidHeaderLength = 4,
    /// The load base is zero or not page aligned.
    InvalidLoadBase = 5,
    /// File/memory lengths are zero, misordered, unaligned or implausible.
    InvalidLength = 6,
    /// The BSS window is not inside the memory image.
    InvalidBssWindow = 7,
    /// The entry point is outside the loaded (non-BSS) part of the image.
    InvalidEntryPoint = 8,
    /// The actual file is shorter or longer than the header declares.
    FileLengthMismatch = 9,
    /// The end of the image does not fit in the physical address space.
    AddressOverflow = 10,
}

/// Parsed `AWKN` header describing how to place the native kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AwknImageHeader {
    /// Physical address the image must be loaded at. Not negotiable.
    pub load_base: u64,
    /// Page-aligned length of the on-disk part of the image.
    pub file_byte_len: u64,
    /// Page-aligned total footprint, BSS included.
    pub memory_byte_len: u64,
    /// Absolute address of `_start`.
    pub entry_point: u64,
    /// Offset of the BSS window from `load_base`.
    pub bss_offset: u64,
    /// Length of the BSS window.
    pub bss_byte_len: u64,
}

impl AwknImageHeader {
    /// Parses and fully validates the header at the start of `image`.
    ///
    /// `image` is the raw file as read from the EFI System Partition. The file
    /// may be **shorter** than [`Self::file_byte_len`], because `objcopy`
    /// truncates the trailing zero tail of the last loaded section; the loader
    /// compensates by zeroing the whole allocation before copying. It may never
    /// be longer.
    pub fn parse(image: &[u8]) -> Result<Self, ImageHeaderError> {
        if (image.len() as u64) < AWKN_IMAGE_HEADER_BYTES {
            return Err(ImageHeaderError::TooSmall);
        }

        if read_u32(image, 0) != AWKN_IMAGE_MAGIC {
            return Err(ImageHeaderError::InvalidMagic);
        }
        if read_u32(image, 4) != AWKN_IMAGE_VERSION {
            return Err(ImageHeaderError::UnsupportedVersion);
        }
        if read_u64(image, 56) != AWKN_IMAGE_HEADER_BYTES {
            return Err(ImageHeaderError::InvalidHeaderLength);
        }

        let header = Self {
            load_base: read_u64(image, 8),
            file_byte_len: read_u64(image, 16),
            memory_byte_len: read_u64(image, 24),
            entry_point: read_u64(image, 32),
            bss_offset: read_u64(image, 40),
            bss_byte_len: read_u64(image, 48),
        };
        header.validate()?;

        if image.len() as u64 > header.file_byte_len {
            return Err(ImageHeaderError::FileLengthMismatch);
        }

        Ok(header)
    }

    fn validate(&self) -> Result<(), ImageHeaderError> {
        if self.load_base == 0 || !self.load_base.is_multiple_of(UEFI_PAGE_SIZE) {
            return Err(ImageHeaderError::InvalidLoadBase);
        }
        if self.file_byte_len == 0
            || !self.file_byte_len.is_multiple_of(UEFI_PAGE_SIZE)
            || !self.memory_byte_len.is_multiple_of(UEFI_PAGE_SIZE)
            || self.memory_byte_len < self.file_byte_len
            || self.memory_byte_len > MAX_IMAGE_BYTES
        {
            return Err(ImageHeaderError::InvalidLength);
        }

        let bss_end = self
            .bss_offset
            .checked_add(self.bss_byte_len)
            .ok_or(ImageHeaderError::InvalidBssWindow)?;
        if self.bss_offset < self.file_byte_len || bss_end > self.memory_byte_len {
            return Err(ImageHeaderError::InvalidBssWindow);
        }

        // `_start` must live in bytes that actually come from the file.
        let entry_offset = self
            .entry_point
            .checked_sub(self.load_base)
            .ok_or(ImageHeaderError::InvalidEntryPoint)?;
        if entry_offset < AWKN_IMAGE_HEADER_BYTES || entry_offset >= self.file_byte_len {
            return Err(ImageHeaderError::InvalidEntryPoint);
        }

        self.load_base
            .checked_add(self.memory_byte_len)
            .ok_or(ImageHeaderError::AddressOverflow)?;

        Ok(())
    }

    /// Number of 4 KiB pages the loader must allocate at [`Self::load_base`].
    #[must_use]
    pub const fn page_count(&self) -> u64 {
        self.memory_byte_len.div_ceil(UEFI_PAGE_SIZE)
    }

    /// First physical address after the image.
    #[must_use]
    pub const fn memory_end_exclusive(&self) -> u64 {
        self.load_base + self.memory_byte_len
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut value = [0_u8; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(value)
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    let mut value = [0_u8; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors what `kernel/x86_64/linker.ld` emits for a 2 MiB-based image.
    fn encode(header: &AwknImageHeader, magic: u32, version: u32, header_len: u64) -> [u8; 64] {
        let mut bytes = [0_u8; 64];
        bytes[0..4].copy_from_slice(&magic.to_le_bytes());
        bytes[4..8].copy_from_slice(&version.to_le_bytes());
        bytes[8..16].copy_from_slice(&header.load_base.to_le_bytes());
        bytes[16..24].copy_from_slice(&header.file_byte_len.to_le_bytes());
        bytes[24..32].copy_from_slice(&header.memory_byte_len.to_le_bytes());
        bytes[32..40].copy_from_slice(&header.entry_point.to_le_bytes());
        bytes[40..48].copy_from_slice(&header.bss_offset.to_le_bytes());
        bytes[48..56].copy_from_slice(&header.bss_byte_len.to_le_bytes());
        bytes[56..64].copy_from_slice(&header_len.to_le_bytes());
        bytes
    }

    const VALID: AwknImageHeader = AwknImageHeader {
        load_base: 0x0020_0000,
        file_byte_len: 0xd000,
        memory_byte_len: 0xe000,
        entry_point: 0x0020_1000,
        bss_offset: 0xd000,
        bss_byte_len: 0x1000,
    };

    fn valid_bytes() -> [u8; 64] {
        encode(&VALID, AWKN_IMAGE_MAGIC, AWKN_IMAGE_VERSION, 64)
    }

    #[test]
    fn parses_a_well_formed_header() {
        assert_eq!(AwknImageHeader::parse(&valid_bytes()), Ok(VALID));
    }

    #[test]
    fn reports_page_count_and_end() {
        assert_eq!(VALID.page_count(), 14);
        assert_eq!(VALID.memory_end_exclusive(), 0x0020_e000);
    }

    #[test]
    fn accepts_a_file_truncated_by_objcopy() {
        // objcopy drops the zero tail of the last loaded section, so the file
        // is legitimately shorter than the declared page-aligned length.
        let mut bytes = valid_bytes().to_vec();
        bytes.resize(0xc098, 0);
        assert_eq!(AwknImageHeader::parse(&bytes), Ok(VALID));
    }

    #[test]
    fn rejects_a_file_longer_than_declared() {
        let mut bytes = valid_bytes().to_vec();
        bytes.resize(0xd001, 0);
        assert_eq!(
            AwknImageHeader::parse(&bytes),
            Err(ImageHeaderError::FileLengthMismatch)
        );
    }

    #[test]
    fn rejects_a_short_file() {
        assert_eq!(
            AwknImageHeader::parse(&[0_u8; 63]),
            Err(ImageHeaderError::TooSmall)
        );
    }

    #[test]
    fn rejects_foreign_magic_and_version() {
        assert_eq!(
            AwknImageHeader::parse(&encode(&VALID, 0x1234_5678, AWKN_IMAGE_VERSION, 64)),
            Err(ImageHeaderError::InvalidMagic)
        );
        assert_eq!(
            AwknImageHeader::parse(&encode(&VALID, AWKN_IMAGE_MAGIC, 2, 64)),
            Err(ImageHeaderError::UnsupportedVersion)
        );
        assert_eq!(
            AwknImageHeader::parse(&encode(&VALID, AWKN_IMAGE_MAGIC, AWKN_IMAGE_VERSION, 48)),
            Err(ImageHeaderError::InvalidHeaderLength)
        );
    }

    #[test]
    fn rejects_an_unaligned_or_zero_load_base() {
        for load_base in [0, 0x0020_0001] {
            let header = AwknImageHeader { load_base, ..VALID };
            assert_eq!(
                AwknImageHeader::parse(&encode(&header, AWKN_IMAGE_MAGIC, AWKN_IMAGE_VERSION, 64)),
                Err(ImageHeaderError::InvalidLoadBase)
            );
        }
    }

    #[test]
    fn rejects_inconsistent_lengths() {
        let cases = [
            AwknImageHeader {
                file_byte_len: 0,
                ..VALID
            },
            AwknImageHeader {
                file_byte_len: 0xd001,
                ..VALID
            },
            AwknImageHeader {
                memory_byte_len: 0xc000,
                ..VALID
            },
            AwknImageHeader {
                memory_byte_len: MAX_IMAGE_BYTES + UEFI_PAGE_SIZE,
                ..VALID
            },
        ];
        for header in cases {
            assert_eq!(
                AwknImageHeader::parse(&encode(&header, AWKN_IMAGE_MAGIC, AWKN_IMAGE_VERSION, 64)),
                Err(ImageHeaderError::InvalidLength)
            );
        }
    }

    #[test]
    fn rejects_a_bss_window_outside_the_image() {
        let overlapping = AwknImageHeader {
            bss_offset: 0xc000,
            ..VALID
        };
        assert_eq!(
            AwknImageHeader::parse(&encode(
                &overlapping,
                AWKN_IMAGE_MAGIC,
                AWKN_IMAGE_VERSION,
                64
            )),
            Err(ImageHeaderError::InvalidBssWindow)
        );

        let past_end = AwknImageHeader {
            bss_byte_len: 0x2000,
            ..VALID
        };
        assert_eq!(
            AwknImageHeader::parse(&encode(&past_end, AWKN_IMAGE_MAGIC, AWKN_IMAGE_VERSION, 64)),
            Err(ImageHeaderError::InvalidBssWindow)
        );
    }

    #[test]
    fn rejects_an_entry_point_outside_the_loaded_image() {
        let cases = [
            AwknImageHeader {
                entry_point: 0x0010_0000,
                ..VALID
            },
            AwknImageHeader {
                entry_point: 0x0020_0010,
                ..VALID
            },
            AwknImageHeader {
                entry_point: 0x0020_d000,
                ..VALID
            },
        ];
        for header in cases {
            assert_eq!(
                AwknImageHeader::parse(&encode(&header, AWKN_IMAGE_MAGIC, AWKN_IMAGE_VERSION, 64)),
                Err(ImageHeaderError::InvalidEntryPoint)
            );
        }
    }
}
