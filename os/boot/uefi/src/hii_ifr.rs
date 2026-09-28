//! Parse the firmware's own HII database into a list of named settings.
//!
//! The firmware describes its Setup menu as an HII database: string packages (numeric id ->
//! human text) and form (IFR) packages (the questions, each bound to a variable store at a
//! byte offset). Changing a setting from a loaded application is the well-known `setup_var`
//! method: find the question's variable store and offset from the IFR, then read/modify/write
//! that NVRAM variable. This module does the discovery half - it walks the database and
//! returns, for every one-of / checkbox / numeric question, its spoken name and the
//! `(variable store, offset, width)` needed to change it. Best-effort and bounds-checked: an
//! unfamiliar block ends that package rather than risking a bad read.
//!
//! It only reads the database, so it is safe on any machine; a firmware that exposes no forms
//! simply yields an empty list.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use uefi::boot;
use uefi::proto::hii::database::HiiDatabase;

/// One firmware setting discovered in the IFR: what it is called, and where its value lives.
pub struct Setting {
    /// The spoken prompt, resolved from the string package (e.g. "Intel VT-d").
    pub name: String,
    /// The variable store (EFI variable) name that holds the value (e.g. "Setup").
    pub store: String,
    /// That variable's vendor GUID.
    pub guid: [u8; 16],
    /// Byte offset of the value inside the variable.
    pub offset: u16,
    /// Value width in bytes (1, 2, 4 or 8).
    pub width: u8,
    /// For a one-of question, its choices as `(value, label)` (e.g. `(0, "Disabled")`), so a
    /// numeric value can be spoken by meaning. Empty for checkboxes and plain numerics.
    pub options: Vec<(u64, String)>,
}

impl Setting {
    /// The label of `value` if it matches one of this setting's choices, else `None`.
    pub fn label_for(&self, value: u64) -> Option<&str> {
        self.options
            .iter()
            .find(|(v, _)| *v == value)
            .map(|(_, text)| text.as_str())
    }
}

fn u16le(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn u32le(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// Decode a NUL-terminated UCS-2 string starting at `at`, returning it and the index just
/// past its terminator.
fn read_ucs2(data: &[u8], at: usize) -> (String, usize) {
    let mut out = String::new();
    let mut i = at;
    while i + 1 < data.len() {
        let unit = u16le(data, i);
        i += 2;
        if unit == 0 {
            break;
        }
        out.push(char::from_u32(u32::from(unit)).unwrap_or('?'));
    }
    (out, i)
}

/// Decode a NUL-terminated ASCII string starting at `at`, returning it and the index past NUL.
fn read_ascii(data: &[u8], at: usize) -> (String, usize) {
    let mut out = String::new();
    let mut i = at;
    while i < data.len() && data[i] != 0 {
        out.push(data[i] as char);
        i += 1;
    }
    (out, i + 1)
}

/// Build the string-id -> text map from one STRINGS package. String ids are assigned in
/// order from 1; skip blocks advance the counter without producing a string.
fn parse_strings(pkg: &[u8], out: &mut BTreeMap<u16, String>) {
    // EFI_HII_STRING_PACKAGE_HDR: after the 4-byte package header come header_size (u32) and
    // string_info_offset (u32); the string blocks begin at string_info_offset.
    if pkg.len() < 12 {
        return;
    }
    let string_info_offset = u32le(pkg, 8) as usize;
    let mut r = string_info_offset;
    let mut sid: u16 = 1;
    while r < pkg.len() {
        let block = pkg[r];
        r += 1;
        match block {
            0x00 => break, // END
            0x10 | 0x11 => {
                // STRING_SCSU / _FONT: (optional font id) then ASCII-ish text.
                if block == 0x11 && r < pkg.len() {
                    r += 1;
                }
                let (text, next) = read_ascii(pkg, r);
                out.insert(sid, text);
                sid += 1;
                r = next;
            }
            0x14 | 0x15 => {
                // STRING_UCS2 / _FONT: (optional font id) then UCS-2 text.
                if block == 0x15 && r < pkg.len() {
                    r += 1;
                }
                let (text, next) = read_ucs2(pkg, r);
                out.insert(sid, text);
                sid += 1;
                r = next;
            }
            0x21 => {
                // SKIP2: advance the id counter by a u16 count.
                if r + 2 > pkg.len() {
                    break;
                }
                sid = sid.wrapping_add(u16le(pkg, r));
                r += 2;
            }
            0x22 => {
                // SKIP1: advance by a u8 count.
                if r >= pkg.len() {
                    break;
                }
                sid = sid.wrapping_add(u16::from(pkg[r]));
                r += 1;
            }
            _ => break, // Unfamiliar block: stop rather than misread the rest.
        }
    }
}

/// Walk one FORMS (IFR) package, recording variable stores and questions.
fn parse_ifr(pkg: &[u8], strings: &BTreeMap<u16, String>, settings: &mut Vec<Setting>) {
    let mut stores: BTreeMap<u16, (String, [u8; 16])> = BTreeMap::new();
    // The index of the one-of question whose options are currently being collected, so the
    // EFI_IFR_ONE_OF_OPTION opcodes that follow attach to it.
    let mut current_oneof: Option<usize> = None;
    let mut r = 4; // skip the package header
    while r + 2 <= pkg.len() {
        let opcode = pkg[r];
        let oplen = (pkg[r + 1] & 0x7f) as usize;
        if oplen < 2 || r + oplen > pkg.len() {
            break;
        }
        let body = &pkg[r..r + oplen];
        match opcode {
            0x24 => {
                // EFI_IFR_VARSTORE: header(2) guid(16) varstore_id(2) size(2) name(ascii).
                if body.len() >= 24 {
                    let mut guid = [0u8; 16];
                    guid.copy_from_slice(&body[2..18]);
                    let vsid = u16le(body, 18);
                    let (name, _) = read_ascii(body, 22);
                    stores.insert(vsid, (name, guid));
                }
            }
            0x26 => {
                // EFI_IFR_VARSTORE_EFI: header(2) varstore_id(2) guid(16) attributes(4)
                // size(2) name(ascii). The modern form carries Name (older ones stop at
                // attributes); only the named form can be reached as an EFI variable.
                if body.len() >= 26 {
                    let vsid = u16le(body, 2);
                    let mut guid = [0u8; 16];
                    guid.copy_from_slice(&body[4..20]);
                    let (name, _) = read_ascii(body, 26);
                    stores.insert(vsid, (name, guid));
                }
            }
            0x05..=0x07 => {
                // ONE_OF / CHECKBOX / NUMERIC: question header at +2 -> prompt(2) help(2)
                // qid(2) varstoreid(2) varstoreinfo/offset(2) qflags(1); one-of/numeric carry
                // a size in the flags byte that follows.
                current_oneof = None;
                if body.len() >= 13 {
                    let prompt = u16le(body, 2);
                    let vsid = u16le(body, 8);
                    let offset = u16le(body, 10);
                    let width = if opcode == 0x06 {
                        1
                    } else if body.len() >= 14 {
                        1u8 << (body[13] & 0x03)
                    } else {
                        1
                    };
                    if let Some((store, guid)) = stores.get(&vsid) {
                        let name = strings
                            .get(&prompt)
                            .filter(|text| !text.is_empty())
                            .cloned()
                            .unwrap_or_else(|| alloc::format!("setting at offset {offset}"));
                        settings.push(Setting {
                            name,
                            store: store.clone(),
                            guid: *guid,
                            offset,
                            width,
                            options: Vec::new(),
                        });
                        if opcode == 0x05 {
                            current_oneof = Some(settings.len() - 1);
                        }
                    }
                }
            }
            0x09 => {
                // EFI_IFR_ONE_OF_OPTION: header(2) option(StringId u16) flags(u8) type(u8)
                // value(EFI_IFR_TYPE_VALUE). Attach the choice to the open one-of.
                if let Some(idx) = current_oneof
                    && body.len() >= 6
                {
                    let option = u16le(body, 2);
                    let value_type = body[5];
                    let vsize = match value_type {
                        0x01 => 2, // UINT16
                        0x02 => 4, // UINT32
                        0x03 => 8, // UINT64
                        _ => 1,    // UINT8 / BOOLEAN / other
                    };
                    let mut value = [0u8; 8];
                    if 6 + vsize <= body.len() {
                        value[..vsize].copy_from_slice(&body[6..6 + vsize]);
                    }
                    let text = strings.get(&option).cloned().unwrap_or_default();
                    if !text.is_empty() {
                        settings[idx]
                            .options
                            .push((u64::from_le_bytes(value), text));
                    }
                }
            }
            _ => {}
        }
        r += oplen;
    }
}

/// Size in bytes of the firmware's exported HII database (0 if the protocol is absent). A
/// non-zero value with zero parsed settings means the firmware exposes an HII database but no
/// varstore-bound questions this parser recognizes - useful boot evidence to tell the two
/// apart.
pub fn database_len() -> usize {
    let Ok(handle) = boot::get_handle_for_protocol::<HiiDatabase>() else {
        return 0;
    };
    let Ok(db) = boot::open_protocol_exclusive::<HiiDatabase>(handle) else {
        return 0;
    };
    db.export_all_raw().map(|raw| raw.len()).unwrap_or(0)
}

/// Enumerate every named setting the firmware publishes through its HII database, or an empty
/// list when it publishes none (or exposes no HII Database protocol).
pub fn enumerate_settings() -> Vec<Setting> {
    let mut settings = Vec::new();
    let Ok(handle) = boot::get_handle_for_protocol::<HiiDatabase>() else {
        return settings;
    };
    let Ok(db) = boot::open_protocol_exclusive::<HiiDatabase>(handle) else {
        return settings;
    };
    let Ok(raw) = db.export_all_raw() else {
        return settings;
    };
    let data: &[u8] = &raw;

    // The export is a run of package lists: [guid(16) length(4)] then packages up to an END.
    let mut pos = 0usize;
    while pos + 20 <= data.len() {
        let list_len = u32le(data, pos + 16) as usize;
        if list_len < 20 || pos + list_len > data.len() {
            break;
        }
        let list = &data[pos..pos + list_len];

        // First pass over this list: gather its strings. Then a second pass reads the IFR with
        // those names in hand (strings and forms live in the same package list).
        let mut strings: BTreeMap<u16, String> = BTreeMap::new();
        let mut walk = |want_strings: bool, settings: &mut Vec<Setting>| {
            let mut q = 20;
            while q + 4 <= list.len() {
                let header = u32le(list, q);
                let plen = (header & 0x00ff_ffff) as usize;
                let ptype = (header >> 24) as u8;
                if plen < 4 || q + plen > list.len() {
                    break;
                }
                let pkg = &list[q..q + plen];
                // HII package types: FORMS (IFR) = 0x02, STRINGS = 0x04.
                match (want_strings, ptype) {
                    (true, 0x04) => parse_strings(pkg, &mut strings),
                    (false, 0x02) => parse_ifr(pkg, &strings, settings),
                    (_, 0xdf) => break, // END package
                    _ => {}
                }
                q += plen;
            }
        };
        let mut scratch = Vec::new();
        walk(true, &mut scratch);
        walk(false, &mut settings);

        pos += list_len;
    }
    settings
}
