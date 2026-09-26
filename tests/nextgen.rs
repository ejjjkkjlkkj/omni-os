use st_synth::{audio,engine::{Engine,Options},synth::{self,Voice,VoiceQuality}};
#[test]
fn explicit_ssml_is_independent_across_threads() {
    let a=synth::Config{rate:100,pitch:115,voice:Voice::Male,quality:VoiceQuality::Modal};
    let b=synth::Config{rate:150,pitch:200,voice:Voice::Female,quality:VoiceQuality::Breathy};
    let text="<speak>Bonjour <voice name=\"child\">les amis</voice> !</speak>";
    let expected_a=synth::say_with_config(text,true,a);
    let expected_b=synth::say_with_config(text,true,b);
    let handles=(0..8).map(|i| std::thread::spawn(move||synth::say_with_config(text,true,if i%2==0{a}else{b}))).collect::<Vec<_>>();
    for (i,h) in handles.into_iter().enumerate(){assert_eq!(h.join().unwrap(),if i%2==0{expected_a.clone()}else{expected_b.clone()});}
    assert_ne!(expected_a,expected_b);
}
#[test]
fn master_rejects_invalid_samples_and_pads_odd_pcm24() {
    assert!(audio::master(&[f64::NAN],32000).is_err());
    assert!(audio::wav24(&[1.0]).is_err());
    let w=audio::wav24(&[0.0,0.5,-0.5]).unwrap();
    assert_eq!(w.len(),54);assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()),46);
    assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()),9);
    assert_eq!(&w[44..47],&[0,0,0]);
}
#[test]
fn stream_equals_batch_and_cancels() {
    let mut e=Engine::new(Options::default()).unwrap();
    let text="Bonjour. Les amis arrivent ?";
    let batch=e.synthesize(text).unwrap();let mut streamed=Vec::new();
    e.stream(text,|s|{streamed.extend_from_slice(s);true}).unwrap();
    assert_eq!(batch,streamed);assert!(batch.iter().all(|x|x.is_finite()&&x.abs()<1.0));
    let mut calls=0;assert_eq!(e.stream(text,|_|{calls+=1;false}).unwrap_err(),"Cancelled");assert_eq!(calls,1);
    assert!(e.synthesize("Bonjour.").is_ok());
}
#[test]
fn dates_decimals_times_and_abbreviations() {
    use st_synth::frontend::normalize;
    assert_eq!(normalize("M. Dupont, le 26/09/2026 à 14h05 : 12,05.",true),"monsieur Dupont, le 26 septembre 2026 à 14 heures 5 minutes : 12 virgule zéro cinq.");
    assert_eq!(normalize("Dr. Smith: 2024-02-29, 3.14!",false),"doctor Smith: 29 February 2024, 3 point one four!");
    assert_eq!(normalize("2023-02-29 99:99",false),"2023-02-29 99:99");
}
