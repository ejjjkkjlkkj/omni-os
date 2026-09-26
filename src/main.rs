use std::{env, fs, io::{self, Read}, path::PathBuf, process::ExitCode};
use st_synth::{synthesize_to_wav, set_voice, set_voice_quality, Voice, VoiceQuality};

#[cfg(windows)]
fn play_wav(path: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    #[link(name = "winmm")]
    extern "system" { fn PlaySoundW(sound: *const u16, module: usize, flags: u32) -> i32; }
    unsafe { PlaySoundW(wide.as_ptr(), 0, 0x00020002) != 0 }
}
#[cfg(not(windows))]
fn play_wav(_: &std::path::Path) -> bool { false }

fn help() {
    println!("ST {} — synthèse vocale FR/EN, PCM 32 kHz 16 bits mono
Usage: st [OPTIONS] [TEXTE...]
  -t, --text TEXTE           Texte à synthétiser
  -l, --lang fr|en           Langue (fr par défaut)
  -r, --rate 50..300         Vitesse en % (100)
  -p, --pitch 50..350        Hauteur en Hz (défaut de la voix)
  --voice male|female|child  Voix (male)
  --quality modal|breathy|pressed|creaky
  -o, --out FICHIER          WAV (speech_output.wav par défaut)
  --stdin                   Lire du texte UTF-8 sur stdin
  --play                    Jouer le WAV sur Windows
  --demo                    Générer demo_fr.wav et demo_en.wav
  --                        Fin des options
  -V, --version             Version
  -h, --help                Aide
Les démos sont enregistrées dans le dossier courant; --play active leur lecture.", env!("CARGO_PKG_VERSION"));
}
fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).peekable();
    if args.peek().is_none() { help(); return Ok(()); }
    let mut text = Vec::new();
    let (mut french, mut rate, mut pitch) = (true, 100, 0);
    let (mut play, mut stdin, mut demo) = (false, false, false);
    let mut output: Option<PathBuf> = None;
    let mut voice = Voice::Male;
    let mut quality = VoiceQuality::Modal;
    while let Some(arg) = args.next() {
        if arg == "--" { text.extend(args); break; }
        match arg.as_str() {
            "--help" | "-h" => { help(); return Ok(()); }
            "--version" | "-V" => { println!("st {}", env!("CARGO_PKG_VERSION")); return Ok(()); }
            "--play" => play = true,
            "--stdin" => stdin = true,
            "--demo" => demo = true,
            "--text" | "-t" | "--lang" | "-l" | "--rate" | "-r" | "--pitch" | "-p" | "--out" | "-o" | "--voice" | "--quality" => {
                let value = args.next().ok_or_else(|| format!("Valeur manquante pour {arg}"))?;
                match arg.as_str() {
                    "--text" | "-t" => text.push(value),
                    "--out" | "-o" => {
                        if value.is_empty() { return Err("Chemin de sortie vide".into()); }
                        output = Some(value.into());
                    }
                    "--lang" | "-l" => french = match value.as_str() { "fr" => true, "en" => false, _ => return Err("Langue attendue : fr ou en".into()) },
                    "--rate" | "-r" | "--pitch" | "-p" => {
                        let n: u32 = value.parse().map_err(|_| format!("Nombre invalide pour {arg}: {value}"))?;
                        let is_rate = matches!(arg.as_str(), "--rate" | "-r");
                        let max = if is_rate { 300 } else { 350 };
                        if !(50..=max).contains(&n) { return Err(format!("{arg}: valeur attendue entre 50 et {max}")); }
                        if is_rate { rate = n; } else { pitch = n; }
                    }
                    "--voice" => voice = match value.as_str() {
                        "male" | "m" => Voice::Male, "female" | "f" => Voice::Female, "child" | "c" => Voice::Child,
                        _ => return Err("Voix attendue : male, female ou child".into())
                    },
                    "--quality" => quality = match value.as_str() {
                        "modal" | "m" => VoiceQuality::Modal, "breathy" | "b" => VoiceQuality::Breathy,
                        "pressed" | "p" => VoiceQuality::Pressed, "creaky" | "c" => VoiceQuality::Creaky,
                        _ => return Err("Qualité attendue : modal, breathy, pressed ou creaky".into())
                    },
                    _ => unreachable!(),
                }
            }
            other if other.starts_with('-') => return Err(format!("Option inconnue : {other}")),
            _ => text.push(arg),
        }
    }
    set_voice(voice);
    set_voice_quality(quality);
    if pitch == 0 { pitch = voice.f0() as u32; }
    if demo {
        if stdin || !text.is_empty() || output.is_some() { return Err("--demo ne se combine pas avec du texte, --stdin ou --out".into()); }
        for (path, phrase, fr) in [
            ("demo_fr.wav", "Bonjour, le système est prêt. Les amis arrivent à quatre-vingt-dix heures ?", true),
            ("demo_en.wav", "Hello world. The firmware screen reader is ready. USB devices are available.", false),
        ] { save(phrase, fr, rate, pitch, &PathBuf::from(path), play)?; }
        return Ok(());
    }
    if stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).map_err(|e| format!("Lecture stdin : {e}"))?;
        text.push(buf);
    }
    let text = text.join(" ");
    if text.trim().is_empty() { return Err("Aucun texte fourni".into()); }
    save(&text, french, rate, pitch, &output.unwrap_or_else(|| "speech_output.wav".into()), play)
}
fn save(text: &str, french: bool, rate: u32, pitch: u32, path: &std::path::Path, play: bool) -> Result<(), String> {
    let wav = synthesize_to_wav(text, french, rate, pitch);
    if wav.len() <= 44 { return Err("Le texte ne produit aucun audio".into()); }
    fs::write(path, &wav).map_err(|e| format!("Écriture {} : {e}", path.display()))?;
    println!("{} octets -> {}", wav.len(), path.display());
    if play && !play_wav(path) { return Err(format!("Lecture audio impossible : {}", path.display())); }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("Erreur : {e}"); ExitCode::FAILURE }
    }
}
