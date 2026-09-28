//! Master format and band-limited conversion. Upsampling adds no source bandwidth.
pub const MASTER_RATE: u32 = 48_000;

pub fn master(input: &[f64], source_rate: u32) -> Result<Vec<f32>, String> {
    if input.is_empty() || input.iter().any(|x| !x.is_finite()) {
        return Err("Empty or non-finite audio".into());
    }
    if !(8000..=MASTER_RATE).contains(&source_rate) { return Err("Unsupported sample rate".into()); }
    // DC blocker (1st-order high-pass, -3 dB at 20 Hz): the glottal source leaves a
    // few % of offset, which wastes headroom and fails VoiceCore's PCM gate.
    let r = 1.0 - 2.0*std::f64::consts::PI*20.0/source_rate as f64;
    let (mut px, mut py) = (input[0], 0.0);
    let input: Vec<f64> = input.iter().map(|&x| { py = x - px + r*py; px = x; py }).collect();
    let input = &input[..];
    let n = input.len().checked_mul(MASTER_RATE as usize).ok_or("Audio too long")? / source_rate as usize;
    let mut out = Vec::with_capacity(n);
    // 64-tap Hann-windowed sinc, f64 accumulation, phase-normalized at the edges.
    for i in 0..n {
        let t = i as f64 * source_rate as f64 / MASTER_RATE as f64;
        let center = t.floor() as isize;
        let (mut sum, mut weights) = (0.0, 0.0);
        for j in center-31..=center+32 {
            if j < 0 || j >= input.len() as isize { continue; }
            let d = t-j as f64;
            if d.abs() >= 32.0 { continue; }
            let x = std::f64::consts::PI * d;
            let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin()/x };
            let w = sinc * (0.5+0.5*(x/32.0).cos());
            sum += input[j as usize]*w; weights += w;
        }
        out.push(sum/weights);
    }
    let peak = out.iter().fold(0.0_f64, |p, x| p.max(x.abs()));
    if !peak.is_finite() || peak == 0.0 { return Err("Silent or invalid audio".into()); }
    let gain = 0.70/peak;
    let fade = 240.min(out.len()/2);
    let end = out.len()-1;
    Ok(out.iter().enumerate().map(|(i,x)| {
        let w = (i.min(end-i) as f64 / fade.max(1) as f64).min(1.0);
        (x*gain*w) as f32
    }).collect())
}

/// PCM24 with deterministic TPDF dither and correct RIFF padding for odd frames.
pub fn wav24(samples: &[f32]) -> Result<Vec<u8>, String> {
    if samples.is_empty() || samples.iter().any(|x| !x.is_finite() || x.abs() >= 1.0) {
        return Err("Empty, clipped or non-finite master".into());
    }
    let bytes = samples.len().checked_mul(3).ok_or("WAV too large")?;
    let padded = bytes.checked_add(bytes & 1).ok_or("WAV too large")?;
    let size = u32::try_from(padded).ok().and_then(|v| v.checked_add(36)).ok_or("WAV too large")?;
    let mut out=Vec::with_capacity(44+padded);
    out.extend_from_slice(b"RIFF"); out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt "); out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&MASTER_RATE.to_le_bytes()); out.extend_from_slice(&(MASTER_RATE*3).to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes()); out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(b"data"); out.extend_from_slice(&(bytes as u32).to_le_bytes());
    let mut seed=0x7354_1245u32;
    let mut uniform=|| { seed^=seed<<13; seed^=seed>>17; seed^=seed<<5; seed as f64/u32::MAX as f64 };
    for &x in samples {
        let dither=if x==0.0 { 0.0 } else { uniform()-uniform() };
        let v=(x as f64*8_388_607.0+dither).round().clamp(-8_388_607.0,8_388_607.0) as i32;
        out.extend_from_slice(&v.to_le_bytes()[..3]);
    }
    if bytes&1!=0 { out.push(0); }
    Ok(out)
}
