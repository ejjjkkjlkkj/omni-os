use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const FIXED: &[&str] = &[
    "setup_setup",
    "setup_main",
    "setup_advanced",
    "setup_boot",
    "setup_security",
    "setup_save_exit",
    "setup_system_information",
    "setup_cpu_model",
    "setup_firmware_time",
    "setup_firmware_vendor",
    "setup_firmware_revision",
    "setup_uefi_revision",
    "setup_display_information",
    "setup_display_resolution",
    "setup_boot_current",
    "setup_cpu_configuration",
    "setup_architecture",
    "setup_virtualization_capability",
    "setup_boot_option_priorities",
    "setup_boot_now",
    "setup_set_as_default",
    "setup_secure_boot",
    "setup_secure_boot_status",
    "setup_setup_mode",
    "setup_boot_normally",
    "setup_reset_system",
    "setup_shut_down",
    "setup_back",
    "setup_enabled",
    "setup_disabled",
    "setup_unavailable",
    "setup_supported",
    "setup_not_supported",
    "setup_selected",
    "setup_boot_option",
    "setup_action_succeeded",
    "setup_action_failed",
    "setup_value",
    "setup_instructions_navigation",
    "setup_instructions_select",
    "setup_instructions_timeout",
    "setup_confirm_restart",
    "setup_confirm_shutdown",
    "setup_cancel",
];

const SYMBOLS: &[&str] = &["dash", "dot", "colon", "slash", "underscore"];

fn install_asset(source_dir: &Path, out_dir: &Path, stem: &str) -> bool {
    let source = source_dir.join(format!("{stem}.pcm"));
    let destination = out_dir.join(format!("{stem}.pcm"));
    println!("cargo:rerun-if-changed={}", source.display());
    if source.is_file() {
        fs::copy(source, destination).expect("copy firmware speech asset");
        true
    } else {
        fs::write(destination, [0_u8, 0_u8]).expect("write firmware speech fallback");
        false
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let source_dir = manifest.join("src/speech");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("out dir")).join("aw-speech");
    fs::create_dir_all(&out_dir).expect("create firmware speech output directory");

    let mut complete = true;
    for stem in FIXED {
        complete &= install_asset(&source_dir, &out_dir, stem);
    }
    for character in 'a'..='z' {
        complete &= install_asset(&source_dir, &out_dir, &format!("spell_{character}"));
    }
    for digit in '0'..='9' {
        complete &= install_asset(&source_dir, &out_dir, &format!("spell_{digit}"));
    }
    for symbol in SYMBOLS {
        complete &= install_asset(&source_dir, &out_dir, &format!("spell_{symbol}"));
    }

    println!(
        "cargo:rustc-env=AW_SETUP_SPEECH_REAL={}",
        if complete { "1" } else { "0" }
    );
}
