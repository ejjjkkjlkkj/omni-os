//! SAPI client check (runs inside WinPE/WinRE): lists voices, then speaks with
//! each requested voice to a WAV file (engine only) and to the default audio
//! device (engine + audio stack). Prints one line per step with the HRESULT.
//!
//!   speak.exe <out-dir> [voice-name-substring...]
#![allow(non_snake_case)]

use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Media::Speech::*;
use windows::Win32::System::Com::*;

/// `speak.exe --ps`: process list (WinPE has no tasklist).
fn processes() {
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return println!("snapshot failed");
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(e.szExeFile.len());
            println!(
                "{:>6} {}",
                e.th32ProcessID,
                String::from_utf16_lossy(&e.szExeFile[..n])
            );
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
    }
}

/// `speak.exe --audio`: which layer of the audio stack exists (WinMM, MMDevice,
/// SAPI AudioOutput category), then a 1 s 440 Hz tone through WinMM.
fn audio_stack() {
    use windows::Win32::Media::Audio::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        println!("waveOutGetNumDevs = {}", waveOutGetNumDevs());
        match CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            Ok(e) => match e.EnumAudioEndpoints(eRender, DEVICE_STATE(DEVICE_STATEMASK_ALL)) {
                Ok(c) => {
                    println!(
                        "MMDevice render endpoints (all states) = {:?}",
                        c.GetCount()
                    );
                    println!(
                        "default render endpoint: {:?}",
                        e.GetDefaultAudioEndpoint(eRender, eConsole)
                            .map(|_| "present")
                    );
                }
                Err(x) => println!("EnumAudioEndpoints FAIL {x:?}"),
            },
            Err(x) => println!("MMDeviceEnumerator FAIL {x:?}"),
        }
        match CoCreateInstance::<_, ISpObjectTokenCategory>(
            &SpObjectTokenCategory,
            None,
            CLSCTX_ALL,
        ) {
            Ok(cat) => {
                println!(
                    "SPCAT_AUDIOOUT SetId: {:?}",
                    cat.SetId(SPCAT_AUDIOOUT, false)
                );
                match cat.EnumTokens(PCWSTR::null(), PCWSTR::null()) {
                    Ok(t) => {
                        let mut n = 0;
                        let _ = t.GetCount(&mut n);
                        println!("SAPI audio outputs = {n}");
                    }
                    Err(x) => println!("SAPI audio outputs EnumTokens FAIL {x:?}"),
                }
            }
            Err(x) => println!("SpObjectTokenCategory FAIL {x:?}"),
        }
        // 1 s tone through the legacy WinMM path.
        let fmt = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 1,
            nSamplesPerSec: 48000,
            nAvgBytesPerSec: 96000,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut h = HWAVEOUT::default();
        let r = waveOutOpen(Some(&mut h), WAVE_MAPPER, &fmt, None, None, CALLBACK_NULL);
        println!("waveOutOpen = {r}");
        if r == 0 {
            let mut pcm: Vec<i16> = (0..48000)
                .map(|i| {
                    ((i as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 8000.0) as i16
                })
                .collect();
            let mut hdr = WAVEHDR {
                lpData: windows::core::PSTR(pcm.as_mut_ptr() as *mut u8),
                dwBufferLength: 96000,
                ..Default::default()
            };
            println!(
                "waveOutPrepareHeader = {}",
                waveOutPrepareHeader(h, &mut hdr, std::mem::size_of::<WAVEHDR>() as u32)
            );
            println!(
                "waveOutWrite = {}",
                waveOutWrite(h, &mut hdr, std::mem::size_of::<WAVEHDR>() as u32)
            );
            std::thread::sleep(std::time::Duration::from_millis(1300));
            let _ = waveOutUnprepareHeader(h, &mut hdr, std::mem::size_of::<WAVEHDR>() as u32);
            let _ = waveOutClose(h);
        }
        println!("AUDIO_STACK_DONE");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "--ps").unwrap_or(false) {
        return processes();
    }
    if args.get(1).map(|a| a == "--audio").unwrap_or(false) {
        return audio_stack();
    }
    // `speak.exe --say <text>`: speak with the default SAPI voice on the default audio device.
    if args.get(1).map(|a| a == "--say").unwrap_or(false) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let text = HSTRING::from(args[2..].join(" "));
            match CoCreateInstance::<_, ISpVoice>(&SpVoice, None, CLSCTX_ALL) {
                Ok(v) => {
                    let t0 = std::time::Instant::now();
                    println!(
                        "say: {:?} in {} ms",
                        v.Speak(&text, 0, None),
                        t0.elapsed().as_millis()
                    );
                }
                Err(e) => println!("SpVoice FAIL {e:?}"),
            }
        }
        return;
    }
    let out = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let wanted: Vec<String> = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        vec!["ST Siwis".into(), "Hortense".into()]
    };
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        println!("CoInitializeEx {hr:?}");
        let cat: ISpObjectTokenCategory =
            match CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL) {
                Ok(c) => c,
                Err(e) => return println!("SpObjectTokenCategory FAIL {e:?}"),
            };
        if let Err(e) = cat.SetId(SPCAT_VOICES, false) {
            return println!("SetId FAIL {e:?}");
        }
        let tokens = match cat.EnumTokens(PCWSTR::null(), PCWSTR::null()) {
            Ok(t) => t,
            Err(e) => return println!("EnumTokens FAIL {e:?}"),
        };
        let mut n = 0u32;
        let _ = tokens.GetCount(&mut n);
        println!("voices: {n}");
        let mut list = Vec::new();
        for i in 0..n {
            if let Ok(t) = tokens.Item(i) {
                let name = t
                    .GetStringValue(PCWSTR::null())
                    .map(|p| p.to_string().unwrap_or_default())
                    .unwrap_or_default();
                println!("  voice {i}: {name}");
                list.push((name, t));
            }
        }
        for w in &wanted {
            let Some((name, token)) = list.iter().find(|(n, _)| n.contains(w.as_str())) else {
                println!("VOICE_MISSING {w}");
                continue;
            };
            let voice: ISpVoice = match CoCreateInstance(&SpVoice, None, CLSCTX_ALL) {
                Ok(v) => v,
                Err(e) => {
                    println!("SpVoice FAIL {e:?}");
                    continue;
                }
            };
            println!("SetVoice {name}: {:?}", voice.SetVoice(token));
            let text = HSTRING::from(
                "Environnement de récupération Windows. Choisir la disposition du clavier.",
            );
            // 1) to a WAV file: exercises the TTS engine alone.
            let file = format!("{out}\\{}.wav", w.replace(' ', "_"));
            match CoCreateInstance::<_, ISpStream>(&SpStream, None, CLSCTX_ALL) {
                Ok(stream) => {
                    let mut fmt = windows::Win32::Media::Audio::WAVEFORMATEX {
                        wFormatTag: 1,
                        nChannels: 1,
                        nSamplesPerSec: 48000,
                        nAvgBytesPerSec: 96000,
                        nBlockAlign: 2,
                        wBitsPerSample: 16,
                        cbSize: 0,
                    };
                    let fmt_id =
                        windows::core::GUID::from_u128(0xc31adbae_527f_4ff5_a230_f62bb61ff70c);
                    let r = stream.BindToFile(
                        &HSTRING::from(file.as_str()),
                        SPFM_CREATE_ALWAYS,
                        Some(&fmt_id),
                        Some(&mut fmt as *mut _ as *const _),
                        0,
                    );
                    println!("BindToFile {file}: {r:?}");
                    if r.is_ok() {
                        println!(
                            "SetOutput(file): {:?}",
                            voice.SetOutput(
                                &stream.cast::<windows::core::IUnknown>().unwrap(),
                                true
                            )
                        );
                        let t0 = std::time::Instant::now();
                        println!(
                            "Speak(file) {name}: {:?} in {} ms",
                            voice.Speak(&text, 0, None),
                            t0.elapsed().as_millis()
                        );
                        let _ = stream.Close();
                    }
                }
                Err(e) => println!("SpStream FAIL {e:?}"),
            }
            // 2) to the default audio device: engine + audio stack.
            println!(
                "SetOutput(default audio): {:?}",
                voice.SetOutput(None, true)
            );
            let t0 = std::time::Instant::now();
            println!(
                "Speak(audio) {name}: {:?} in {} ms",
                voice.Speak(&text, 0, None),
                t0.elapsed().as_millis()
            );
        }
        println!("SPEAK_TEST_DONE");
    }
}
