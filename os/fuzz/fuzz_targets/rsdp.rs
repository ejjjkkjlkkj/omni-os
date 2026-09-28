#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = aw_acpi::declared_length(data);
    let _ = aw_acpi::validate_rsdp(data);
});
