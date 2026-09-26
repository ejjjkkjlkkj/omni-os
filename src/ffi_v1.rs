//! Versioned handle ABI. Handles are serialized; callbacks must not re-enter a handle.
use crate::engine::{Backend, Engine, Options};
use std::{panic::{catch_unwind,AssertUnwindSafe}, sync::Mutex};
pub struct StEngine { inner:Mutex<Engine> }
type Callback=unsafe extern "C" fn(*const f32,usize,u32,*mut std::ffi::c_void)->u8;

unsafe fn input<'a>(ptr:*const u8,len:usize)->Result<&'a str,()> {
    if ptr.is_null()||len==0||len>65536 {return Err(());}
    std::str::from_utf8(std::slice::from_raw_parts(ptr,len)).map_err(|_|())
}
/// JSON config: backend, lang, voice, rate, pitch, neural_home. Unknown keys fail.
/// Return 0 success, 1 invalid input, 2 backend failure, 3 panic/poison, 4 cancellation.
#[no_mangle]
pub unsafe extern "C" fn st_engine_create_v1(config:*const u8,len:usize,out:*mut *mut StEngine)->i32 {
    if out.is_null() {return 1;} *out=std::ptr::null_mut();
    catch_unwind(AssertUnwindSafe(|| {
        let text=match input(config,len) {Ok(x)=>x,Err(_)=>return 1};
        let j:serde_json::Value=match serde_json::from_str(text) {Ok(x)=>x,Err(_)=>return 1};
        let obj=match j.as_object(){Some(x)=>x,None=>return 1};
        if obj.keys().any(|k| !["backend","lang","voice","rate","pitch","neural_home"].contains(&k.as_str())) {return 1;}
        let mut o=Options::default();
        if let Some(x)=obj.get("backend") {o.backend=match x.as_str(){Some("compact")=>Backend::Compact,Some("neural")=>Backend::Neural,_=>return 1};}
        if let Some(x)=obj.get("lang") {o.french=match x.as_str(){Some("fr")=>true,Some("en")=>false,_=>return 1};}
        o.voice=if o.backend==Backend::Neural {if o.french {"ff_siwis"}else{"af_heart"}}else{"male"}.into();
        if let Some(x)=obj.get("voice") {o.voice=match x.as_str(){Some(s)=>s.into(),None=>return 1};}
        for (name,target) in [("rate",&mut o.rate),("pitch",&mut o.pitch)] {
            if let Some(x)=obj.get(name) {*target=match x.as_u64().and_then(|v|u32::try_from(v).ok()){Some(n)=>n,None=>return 1};}
        }
        if let Some(x)=obj.get("neural_home") {o.neural_home=match x.as_str(){Some(s)=>Some(s.into()),None=>return 1};}
        match Engine::new(o) {Ok(e)=>{*out=Box::into_raw(Box::new(StEngine{inner:Mutex::new(e)}));0},Err(_)=>2}
    })).unwrap_or(3)
}
#[no_mangle]
pub unsafe extern "C" fn st_engine_destroy_v1(handle:*mut StEngine) {
    if !handle.is_null() {drop(Box::from_raw(handle));}
}
#[no_mangle]
pub unsafe extern "C" fn st_engine_stream_v1(handle:*mut StEngine,text:*const u8,len:usize,callback:Option<Callback>,user:*mut std::ffi::c_void)->i32 {
    if handle.is_null()||callback.is_none(){return 1;}
    catch_unwind(AssertUnwindSafe(||{
        let text=match input(text,len){Ok(s)=>s,Err(_)=>return 1};
        let mut engine=match (*handle).inner.try_lock(){Ok(e)=>e,Err(_)=>return 3};
        match engine.stream(text,|pcm|callback.unwrap()(pcm.as_ptr(),pcm.len(),48000,user)!=0) {
            Ok(())=>0,Err(e) if e=="Cancelled"=>4,Err(_)=>2
        }
    })).unwrap_or(3)
}
#[no_mangle]
pub unsafe extern "C" fn st_engine_wav_v1(handle:*mut StEngine,text:*const u8,len:usize,out:*mut *mut u8,out_len:*mut usize)->i32 {
    if out.is_null()||out_len.is_null(){return 1;}*out=std::ptr::null_mut();*out_len=0;
    if handle.is_null(){return 1;}
    catch_unwind(AssertUnwindSafe(||{
        let text=match input(text,len){Ok(s)=>s,Err(_)=>return 1};
        let mut engine=match (*handle).inner.try_lock(){Ok(e)=>e,Err(_)=>return 3};
        match engine.wav(text) {Ok(wav)=>{
            let mut data=wav.into_boxed_slice();*out_len=data.len();*out=data.as_mut_ptr();std::mem::forget(data);0
        },Err(_)=>2}
    })).unwrap_or(3)
}
