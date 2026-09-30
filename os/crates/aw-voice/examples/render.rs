fn main() {
    let text = std::env::args().nth(1).unwrap_or_else(|| "Session d'administration d'omni-os.".into());
    let pcm = aw_voice::speak(&text, true, 48_000);
    std::fs::write(std::env::args().nth(2).unwrap_or_else(|| "out.pcm".into()), pcm).unwrap();
}
