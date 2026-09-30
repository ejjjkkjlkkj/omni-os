//! Publisher tool for omni-os kernel signatures (Ed25519, `aw-sign`).
//!
//! The secret is a 32-byte seed given as 64 hex digits in `OMNI_SIGNING_SEED` (never on the
//! command line, so it stays out of process listings and shell history).
//!
//!   omni-sign public                         print the public key (hex), to embed in the loader
//!                                            at build time as OMNI_PUBLISHER_PUBKEY
//!   omni-sign kernel <KERNEL.BIN> <KERNEL.SIG>   write the 64-byte signature of the image
//!   omni-sign recovery <IMAGE.EFI> <IMAGE.EFI.sig>   same, for a recovery image
//!   omni-sign verify <pubkey-hex> <KERNEL.BIN> <KERNEL.SIG>   exit 0 when the signature holds

use std::process::ExitCode;

fn seed() -> Result<[u8; 32], String> {
    let text = std::env::var("OMNI_SIGNING_SEED").map_err(|_| "OMNI_SIGNING_SEED is not set")?;
    aw_sign::parse_hex32(&text).ok_or_else(|| "OMNI_SIGNING_SEED must be 64 hex digits".into())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [command] if command == "public" => {
            println!("{}", hex(&aw_sign::public_key(&seed()?)));
            Ok(())
        }
        [command, image, signature] if command == "kernel" => {
            let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
            let sig = aw_sign::sign(&seed()?, &aw_sign::kernel_message(&data));
            std::fs::write(signature, sig).map_err(|e| format!("{signature}: {e}"))
        }
        [command, image, signature] if command == "recovery" => {
            let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
            let sig = aw_sign::sign(&seed()?, &aw_sign::recovery_message(&data));
            std::fs::write(signature, sig).map_err(|e| format!("{signature}: {e}"))
        }
        [command, public, image, signature] if command == "verify" => {
            let key = aw_sign::parse_hex32(public).ok_or("public key must be 64 hex digits")?;
            let data = std::fs::read(image).map_err(|e| format!("{image}: {e}"))?;
            let sig: [u8; 64] = std::fs::read(signature)
                .map_err(|e| format!("{signature}: {e}"))?
                .try_into()
                .map_err(|_| "a signature is exactly 64 bytes")?;
            if aw_sign::verify(&key, &aw_sign::kernel_message(&data), &sig) {
                println!("OMNI_SIGNATURE=VALID");
                Ok(())
            } else {
                Err("OMNI_SIGNATURE=INVALID".into())
            }
        }
        _ => Err(
            "usage: omni-sign public | kernel <image> <sig> | verify <pubkey> <image> <sig>".into(),
        ),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
