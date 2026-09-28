//! Screen-reader latency: one persistent engine, a navigation-like sequence of
//! utterances (with repeats), time to first audio (TTFA) per utterance.
//!
//! cargo run --release --example latency -- [neural|compact] [fr|en] [rounds]
use st_synth::engine::{Backend, Engine, Options};
use std::time::Instant;

const FR: &[&str] = &[
    "Menu Fichier.",
    "Nouveau.",
    "Ouvrir.",
    "Enregistrer, bouton.",
    "Case à cocher, non cochée.",
    "Lien, Accueil.",
    "Titre de niveau 2, Paramètres.",
    "Bonjour, bienvenue dans ST.",
    "Le fichier pèse 12,5 mégaoctets.",
    "Voulez-vous enregistrer les modifications ?",
];
const EN: &[&str] = &[
    "File menu.",
    "New.",
    "Open.",
    "Save, button.",
    "Checkbox, not checked.",
    "Link, Home.",
    "Heading level 2, Settings.",
    "Hello, welcome to ST.",
    "The file is 12.5 megabytes.",
    "Do you want to save your changes?",
];

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let neural = args.first().map(|s| s != "compact").unwrap_or(true);
    let french = args.get(1).map(|s| s != "en").unwrap_or(true);
    let rounds: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let voice = match (neural, french) {
        (true, true) => "ff_siwis",
        (true, false) => "af_heart",
        _ => "female",
    };
    let t = Instant::now();
    let mut engine = Engine::new(Options {
        backend: if neural {
            Backend::Neural
        } else {
            Backend::Compact
        },
        french,
        voice: voice.into(),
        ..Options::default()
    })?;
    println!("load_ms={:.0}", t.elapsed().as_secs_f64() * 1000.0);
    let mut all = Vec::new();
    for round in 0..rounds {
        for text in if french { FR } else { EN } {
            let start = Instant::now();
            let mut ttfa = None;
            let mut samples = 0usize;
            engine.stream(text, |chunk| {
                ttfa.get_or_insert(start.elapsed());
                samples += chunk.len();
                true
            })?;
            let ms = ttfa.unwrap().as_secs_f64() * 1000.0;
            all.push((round, ms));
            println!(
                "round={round} ttfa_ms={ms:7.1} audio_s={:.2} {text}",
                samples as f64 / 48000.0
            );
        }
    }
    // Interruption: stop after the first chunk (key press), then speak the next item.
    let long = if french {
        "Voici un long paragraphe. Il contient plusieurs phrases. La lecture sera interrompue. Personne ne l'entendra en entier."
    } else {
        "Here is a long paragraph. It has several sentences. Reading will be interrupted. Nobody will hear all of it."
    };
    for next in if french {
        ["Menu Édition.", "Fermer, bouton."]
    } else {
        ["Edit menu.", "Close, button."]
    } {
        let _ = engine.stream(long, |_| false);
        let start = Instant::now();
        let mut ttfa = None;
        engine.stream(next, |_| {
            ttfa.get_or_insert(start.elapsed());
            true
        })?;
        println!(
            "after_cancel ttfa_ms={:7.1} {next}",
            ttfa.unwrap().as_secs_f64() * 1000.0
        );
    }
    if let Some(stats) = engine.neural_stats() {
        println!("cache {stats}");
    }
    for round in 0..rounds {
        let mut v: Vec<f64> = all.iter().filter(|x| x.0 == round).map(|x| x.1).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "round {round}: median_ttfa_ms={:.1} max_ttfa_ms={:.1}",
            v[v.len() / 2],
            v[v.len() - 1]
        );
    }
    Ok(())
}
