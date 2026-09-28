#![no_main]

use aw_fs_core::wire::{decode_checkpoint_payload, encode_checkpoint_payload};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(record) = decode_checkpoint_payload(data) {
        let canonical = encode_checkpoint_payload(record);
        assert_eq!(decode_checkpoint_payload(&canonical), Ok(record));
        assert_eq!(encode_checkpoint_payload(record), canonical);
    }
});
