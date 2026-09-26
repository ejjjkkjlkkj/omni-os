use std::process::Command;
fn st(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_st")).args(args).output().unwrap()
}
#[test]
fn invalid_arguments_fail() {
    for args in [vec!["--rate"], vec!["--rate", "abc"], vec!["--rate", "301"], vec!["--pitch", "0"], vec!["--lang", "de"], vec!["--voice", "unknown"], vec!["--quality", "unknown"], vec!["--unknown"], vec!["--text", " "]] {
        assert!(!st(&args).status.success(), "{args:?}");
    }
}
#[test]
fn version_matches_package() {
    let result = st(&["--version"]);
    assert!(result.status.success());
    assert!(String::from_utf8(result.stdout).unwrap().contains(env!("CARGO_PKG_VERSION")));
}
#[test]
fn wav_header_and_audio_are_valid() {
    let path = std::env::temp_dir().join(format!("st-test-{}.wav", std::process::id()));
    let result = st(&["--text", "Bonjour les amis", "--out", path.to_str().unwrap()]);
    assert!(result.status.success());
    let data = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(&data[..4], b"RIFF");
    assert_eq!(&data[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(data[24..28].try_into().unwrap()), 32000);
    assert_eq!(u32::from_le_bytes(data[40..44].try_into().unwrap()) as usize, data.len()-44);
    assert!(data[44..].iter().any(|&b| b != 0));
}
#[test]
fn write_failure_is_reported() {
    assert!(!st(&["--text", "Bonjour", "--out", ""]).status.success());
    let directory = std::env::temp_dir();
    assert!(!st(&["--text", "Bonjour", "--out", directory.to_str().unwrap()]).status.success());
}
#[test]
fn ffi_rejects_invalid_utf8_and_null() {
    unsafe {
        let mut len = 123;
        assert!(st_synth::st_synthesize_wav(std::ptr::null(), 1, true, 100, 115, &mut len).is_null());
        assert_eq!(len, 0);
        assert!(st_synth::st_synthesize_wav([255u8].as_ptr(), 1, true, 100, 115, &mut len).is_null());
        assert_eq!(len, 0);
        st_synth::st_free_wav(std::ptr::null_mut(), 0);
    }
}
