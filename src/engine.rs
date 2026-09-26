//! Instance-based desktop API. Compact and neural engines share one audio contract.
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use crate::{audio, frontend, synth};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend { Compact, Neural }
#[derive(Clone)]
pub struct Options {
    pub backend: Backend,
    pub french: bool,
    pub voice: String,
    pub rate: u32,
    /// Compact only; zero uses the voice default. Neural rejects pitch overrides.
    pub pitch: u32,
    pub quality: synth::VoiceQuality,
    /// Directory containing worker.py, models/ and python/python.exe.
    pub neural_home: Option<PathBuf>,
}
impl Default for Options {
    fn default() -> Self { Self { backend: Backend::Compact, french:true, voice:"male".into(), rate:100, pitch:0, quality:synth::VoiceQuality::Modal, neural_home:None } }
}
pub struct Engine { options: Options, worker: Option<Worker> }
impl Engine {
    pub fn new(options: Options) -> Result<Self,String> {
        let max=if options.backend==Backend::Neural {200}else{300};
        if !(50..=max).contains(&options.rate) {return Err(format!("Rate must be 50..{max}"));}
        if options.pitch!=0 && !(50..=350).contains(&options.pitch) {return Err("Pitch must be 50..350".into());}
        if options.backend==Backend::Neural {
            if options.pitch!=0 || options.quality!=synth::VoiceQuality::Modal {return Err("Neural backend does not support pitch or compact voice quality controls".into());}
            let allowed=if options.french {vec!["ff_siwis"]}else{vec!["af_heart","af_bella","am_michael","bf_emma","bm_george"]};
            if !allowed.contains(&options.voice.as_str()) {return Err(format!("Voice incompatible with language: choose {allowed:?}"));}
        } else if !["male","female","child"].contains(&options.voice.as_str()) {return Err("Compact voice must be male, female or child".into());}
        let worker=if options.backend==Backend::Neural {Some(Worker::start(&options)?)}else{None};
        Ok(Self {options,worker})
    }
    /// Synchronous streaming with backpressure. Return false to cancel.
    /// Chunks are borrowed mono f32 at 48 kHz. Neural generation is sentence/window based.
    pub fn stream(&mut self, text:&str, mut receive:impl FnMut(&[f32])->bool) -> Result<(),String> {
        if text.len()>64*1024 || !text.chars().any(char::is_alphanumeric) {return Err("Text must contain speech and be at most 64 KiB".into());}
        let text=if text.contains('<') {
            if self.options.backend==Backend::Neural {return Err("Neural SSML is not supported; supply plain text".into());}
            text.to_owned()
        }else{frontend::normalize(text,self.options.french)};
        if self.options.backend==Backend::Neural {
            if self.worker.is_none() {self.worker=Some(Worker::start(&self.options)?);}
            let result=self.worker.as_mut().unwrap().stream(&text,&self.options,&mut receive);
            if result.is_err() {self.worker.take();}
            return result;
        }
        let voice=match self.options.voice.as_str() {"female"=>synth::Voice::Female,"child"=>synth::Voice::Child,_=>synth::Voice::Male};
        let config=synth::Config {rate:self.options.rate,pitch:if self.options.pitch==0 {voice.f0() as u32}else{self.options.pitch},voice,quality:self.options.quality};
        // SSML keeps its full scope; plain text renders incrementally by sentence.
        let clauses=if text.contains('<') {vec![text.as_str()]} else {text.split_inclusive(['.','?','!']).collect()};
        for clause in clauses {
            if !clause.chars().any(char::is_alphanumeric) {continue;}
            let raw=synth::render_samples(clause,self.options.french,config);
            let master=audio::master(&raw,synth::SAMPLE_RATE as u32)?;
            for chunk in master.chunks(2048) {if !receive(chunk) {return Err("Cancelled".into());}}
        }
        Ok(())
    }
    pub fn synthesize(&mut self,text:&str)->Result<Vec<f32>,String> {
        let mut samples=Vec::new();
        self.stream(text,|chunk| {samples.extend_from_slice(chunk);true})?;
        Ok(samples)
    }
    pub fn wav(&mut self,text:&str)->Result<Vec<u8>,String> {audio::wav24(&self.synthesize(text)?)}
}

struct Worker { child:Child, input:ChildStdin, output:ChildStdout }
impl Drop for Worker {fn drop(&mut self){let _=self.child.kill();let _=self.child.wait();}}
impl Worker {
    fn start(options:&Options)->Result<Self,String> {
        let home=options.neural_home.clone().or_else(||std::env::var_os("ST_NEURAL_HOME").map(PathBuf::from))
            .unwrap_or_else(||std::env::current_exe().unwrap_or_default().parent().unwrap_or(std::path::Path::new(".")).join("neural"));
        let python=std::env::var_os("ST_PYTHON").map(PathBuf::from).unwrap_or_else(||home.join("python/python.exe"));
        let mut command=Command::new(&python);
        // -I: ignore PYTHONPATH/PYTHONHOME and user site-packages of the host machine.
        command.arg("-I").arg("-u").arg(home.join("worker.py")).arg("--home").arg(&home)
            .env("PYTHONUTF8","1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        let mut child=command.spawn().map_err(|e|format!("Cannot start neural runtime at {}: {e}",python.display()))?;
        let input=child.stdin.take().unwrap(); let output=child.stdout.take().unwrap();
        let mut worker=Self{child,input,output};
        let (kind,payload)=worker.frame()?;
        if kind!=3 {return Err(format!("Neural initialization failed: {}",String::from_utf8_lossy(&payload)));}
        Ok(worker)
    }
    fn frame(&mut self)->Result<(u32,Vec<u8>),String> {
        let mut h=[0u8;16];self.output.read_exact(&mut h).map_err(|e|format!("Neural worker stopped: {e}"))?;
        if &h[..4]!=b"STN1" {return Err("Invalid neural protocol".into());}
        let kind=u32::from_le_bytes(h[4..8].try_into().unwrap());
        let rate=u32::from_le_bytes(h[8..12].try_into().unwrap());
        let n=u32::from_le_bytes(h[12..16].try_into().unwrap()) as usize;
        if rate!=48000 || n>1024*1024 {return Err("Invalid neural frame size/rate".into());}
        let mut data=vec![0;n];self.output.read_exact(&mut data).map_err(|e|e.to_string())?;
        Ok((kind,data))
    }
    fn stream(&mut self,text:&str,o:&Options,receive:&mut impl FnMut(&[f32])->bool)->Result<(),String> {
        let request=serde_json::json!({"text":text,"lang":if o.french {"fr-fr"}else{"en-us"},"voice":o.voice,"speed":o.rate as f64/100.0});
        writeln!(self.input,"{request}").map_err(|e|e.to_string())?;self.input.flush().map_err(|e|e.to_string())?;
        let mut frames=0usize;
        loop {
            let (kind,data)=self.frame()?;
            match kind {
                0=>{
                    if data.is_empty() || data.len()%4!=0 {return Err("Invalid PCM frame".into());}
                    let samples=data.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();
                    if samples.iter().any(|x|!x.is_finite()||x.abs()>=1.0) {return Err("Neural audio failed finite/peak gate".into());}
                    frames+=samples.len();
                    if frames>48000*3600 {return Err("Audio exceeds one hour".into());}
                    if !receive(&samples) {return Err("Cancelled".into());}
                }
                1=>return if frames>0 {Ok(())}else{Err("Neural backend returned no audio".into())},
                2=>return Err(String::from_utf8_lossy(&data).into()),
                _=>return Err("Unexpected neural frame".into())
            }
        }
    }
}
