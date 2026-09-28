//! Screen-reader navigation benchmark: announcements in the VoiceCore/announce.py
//! format ("name, role, state."), each interrupted by the next focus change after
//! the first audio chunk, like Tab / arrow navigation. Reports TTFA percentiles.
//!
//! cargo run --release --example navigation -- [fr|en] [idle_seconds_before] [compact]
//! Point ST_CACHE_DIR at an empty folder for a cold run; rerun for a warm disk cache.
use st_synth::engine::{Backend, Engine, Options};
use std::time::{Duration, Instant};

const FR: &[&str] = &[
    "Fichier, élément de menu.",
    "Édition, élément de menu.",
    "Affichage, élément de menu.",
    "Enregistrer, bouton.",
    "Annuler, bouton.",
    "OK, bouton.",
    "Démarrage sécurisé, case à cocher, cochée.",
    "Mode avancé, case à cocher, non cochée.",
    "Volume, curseur, 40.",
    "Nom d'utilisateur, zone d'édition.",
    "Mot de passe, zone d'édition, protégé.",
    "Rapport trimestriel, élément de liste.",
    "Général, onglet, sélectionné.",
    "Windows Boot Manager, entrée de démarrage.",
    "Réseau, onglet.",
    "Imprimer, bouton, indisponible.",
    "Documents, lien.",
    "Mises à jour, titre.",
    "Explorateur de fichiers, bouton.",
    "Paramètres, bouton.",
];
const EN: &[&str] = &[
    "File, menu item.",
    "Edit, menu item.",
    "View, menu item.",
    "Save, button.",
    "Cancel, button.",
    "OK, button.",
    "Secure Boot, checkbox, checked.",
    "Advanced mode, checkbox, not checked.",
    "Volume, slider, 40.",
    "User name, edit.",
    "Password, edit, protected.",
    "Quarterly report, list item.",
    "General, tab, selected.",
    "Windows Boot Manager, boot entry.",
    "Network, tab.",
    "Print, button, unavailable.",
    "Documents, link.",
    "Updates, heading.",
    "File Explorer, button.",
    "Settings, button.",
];

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((p / 100.0) * (v.len() - 1) as f64).round() as usize]
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let french = args.first().map(|s| s != "en").unwrap_or(true);
    let idle: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let compact = args.get(2).map(|s| s == "compact").unwrap_or(false);
    let voice = match (compact, french) {
        (true, _) => "female",
        (false, true) => "ff_siwis",
        (false, false) => "af_heart",
    };
    let t = Instant::now();
    let mut engine = Engine::new(Options {
        backend: if compact {
            Backend::Compact
        } else {
            Backend::Neural
        },
        french,
        voice: voice.into(),
        ..Options::default()
    })?;
    println!("load_ms={:.0}", t.elapsed().as_secs_f64() * 1000.0);
    if idle > 0 {
        engine.stream(if french { "Prêt." } else { "Ready." }, |_| true)?; // selects voice for prewarm
        std::thread::sleep(Duration::from_secs(idle));
    }
    let items = if french { FR } else { EN };
    let mut ttfa = Vec::new();
    for text in items {
        let start = Instant::now();
        let mut first = None;
        // Focus moves on as soon as the user hears the start of the announcement.
        let r = engine.stream(text, |_| {
            first.get_or_insert(start.elapsed());
            false
        });
        if !matches!(&r, Err(e) if e == "Cancelled") {
            r?;
        }
        let ms = first.map(|d| d.as_secs_f64() * 1000.0).unwrap_or(f64::NAN);
        println!("ttfa_ms={ms:7.1} {text}");
        ttfa.push(ms);
    }
    let resume_start = Instant::now();
    let mut resume = None;
    engine.stream(items[0], |_| {
        resume.get_or_insert(resume_start.elapsed());
        true
    })?;
    println!(
        "P50={:.1} P90={:.1} P95={:.1} P99={:.1} ms (n={})",
        pct(&mut ttfa, 50.0),
        pct(&mut ttfa, 90.0),
        pct(&mut ttfa, 95.0),
        pct(&mut ttfa, 99.0),
        ttfa.len()
    );
    println!(
        "final full announcement ttfa_ms={:.1}",
        resume.unwrap().as_secs_f64() * 1000.0
    );
    if let Some(stats) = engine.neural_stats() {
        println!("cache {stats}");
    }
    Ok(())
}
