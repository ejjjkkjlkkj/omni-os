#![no_main]

use aw_storefs::wire::{
    MANIFEST_HEADER_BYTES, OBJECT_RECORD_BYTES, decode_manifest, encode_manifest,
    encoded_manifest_len,
};
use libfuzzer_sys::fuzz_target;

const MAX_OBJECTS: usize = 32;
const MAX_CANONICAL_BYTES: usize = MANIFEST_HEADER_BYTES + MAX_OBJECTS * OBJECT_RECORD_BYTES;

fuzz_target!(|data: &[u8]| {
    if let Ok(manifest) = decode_manifest::<MAX_OBJECTS>(data) {
        let expected_len = encoded_manifest_len(manifest.len()).expect("accepted manifest length");
        let mut encoded = [0_u8; MAX_CANONICAL_BYTES];
        let used = encode_manifest(&manifest, &mut encoded).expect("accepted manifest re-encodes");
        assert_eq!(used, expected_len);
        assert_eq!(&encoded[..used], data);

        let reparsed = decode_manifest::<MAX_OBJECTS>(&encoded[..used])
            .expect("canonical manifest must reparse");
        assert_eq!(reparsed.generation(), manifest.generation());
        assert_eq!(reparsed.volume_blocks(), manifest.volume_blocks());
        assert_eq!(reparsed.len(), manifest.len());
        for index in 0..manifest.len() {
            assert_eq!(reparsed.object(index), manifest.object(index));
        }
    }
});
