//! Stress / fault injection for the screen-reader contract.
//!
//! cargo run --release --example stress -- [compact|neural] [iterations]
//! Prints process handles/threads/working set at start and end, and fails
//! (exit 1) on any unexpected error, hang (> 30 s per call) or leak signal.
use st_synth::engine::{Backend, Engine, Options};
use std::time::{Duration, Instant};

#[cfg(windows)]
fn process_counters() -> (u32, u64) {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn GetProcessHandleCount(p: *mut core::ffi::c_void, n: *mut u32) -> i32;
        fn K32GetProcessMemoryInfo(p: *mut core::ffi::c_void, c: *mut [usize; 10], cb: u32) -> i32;
    }
    unsafe {
        let mut handles = 0u32;
        GetProcessHandleCount(GetCurrentProcess(), &mut handles);
        let mut m = [0usize; 10];
        m[0] = std::mem::size_of::<[usize; 10]>();
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut m, m[0] as u32);
        (handles, m[2] as u64 / (1 << 20)) // [0] = cb+PageFaultCount, [1] = peak, [2] = WorkingSetSize
    }
}

fn texts(i: usize) -> String {
    match i % 9 {
        0 => "Bouton.".into(),
        1 => "Menu Fichier, élément de menu.".into(),
        2 => format!("Élément numéro {i}, sélectionné."),
        3 => "Ça va ? Œuvre, naïve, façade — « guillemets » et emoji 🙂.".into(),
        4 => "C:\\Users\\adm\\Documents\\rapport.docx".into(),
        5 => "Un. Deux. Trois. Quatre. Cinq. Six. Sept. Huit.".into(),
        6 => "OK.".into(),
        7 => "a".repeat(40) + ".",
        _ => format!("Le {}/09/2026 à 14h{:02}, 12,5 %.", 1 + i % 28, i % 60),
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let neural = args.first().map(|s| s == "neural").unwrap_or(false);
    let n: usize = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(if neural { 400 } else { 2000 });
    let options = Options {
        backend: if neural {
            Backend::Neural
        } else {
            Backend::Compact
        },
        voice: if neural { "ff_siwis" } else { "female" }.into(),
        ..Options::default()
    };
    let mut engine = Engine::new(options.clone())?;
    engine.synthesize("Prêt.")?;
    let (h0, m0) = process_counters();
    println!("start handles={h0} working_set_mb={m0}");
    let (mut ok, mut cancelled, mut rejected, mut worst) = (0usize, 0usize, 0usize, Duration::ZERO);

    // Invalid inputs are rejected, never crash.
    for bad in ["", "   ", "...", &"x".repeat(70_000)] {
        match engine.stream(bad, |_| true) {
            Err(_) => rejected += 1,
            Ok(()) => return Err(format!("accepted invalid input of {} bytes", bad.len())),
        }
    }
    for i in 0..n {
        let text = texts(i);
        let t = Instant::now();
        let mut chunks = 0usize;
        // 2 of 3 calls are interrupted after the first chunk (focus changes).
        let r = engine.stream(&text, |_| {
            chunks += 1;
            i % 3 == 0 || chunks < 1
        });
        worst = worst.max(t.elapsed());
        if t.elapsed() > Duration::from_secs(30) {
            return Err(format!("call {i} took {:?}", t.elapsed()));
        }
        match r {
            Ok(()) => ok += 1,
            Err(e) if e == "Cancelled" => cancelled += 1,
            Err(e) => return Err(format!("call {i} ({text:?}): {e}")),
        }
        if neural && i == n / 2 {
            // Fault injection: kill the worker; the next call may fail once, then recover.
            let pid = engine.neural_pid().ok_or("no worker")?;
            std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .output()
                .map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_millis(300));
            let first = engine.synthesize("Après arrêt du worker.");
            let second = engine.synthesize("Reprise.");
            println!(
                "worker killed: next call {}, following call {}",
                if first.is_ok() {
                    "ok"
                } else {
                    "error (expected)"
                },
                if second.is_ok() { "ok" } else { "ERROR" }
            );
            second?;
            if engine.neural_pid() == Some(pid) {
                return Err("worker was not restarted".into());
            }
        }
    }
    // Repeated engine creation/destruction.
    for _ in 0..if neural { 3 } else { 200 } {
        let mut e = Engine::new(options.clone())?;
        e.synthesize("OK.")?;
    }
    let (h1, m1) = process_counters();
    println!("end handles={h1} working_set_mb={m1}");
    println!(
        "calls={n} ok={ok} cancelled={cancelled} rejected_invalid={rejected} worst_call_ms={}",
        worst.as_millis()
    );
    if h1 > h0 + 50 {
        return Err(format!("handle growth {h0} -> {h1}"));
    }
    println!("STRESS PASS");
    Ok(())
}
