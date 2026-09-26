//! Versioned handle ABI. Handles are serialized; callbacks must not re-enter a handle.
use crate::engine::{Backend, Engine, Options};
use std::{cell::RefCell, panic::{catch_unwind,AssertUnwindSafe}, sync::{Mutex,atomic::{AtomicBool,Ordering}}};
pub struct StEngine { inner:Mutex<Engine>, cancel:AtomicBool }
pub type Callback=unsafe extern "C" fn(*const f32,usize,u32,*mut std::ffi::c_void)->u8;

thread_local!(static LAST_ERROR:RefCell<String>=const{RefCell::new(String::new())});
fn fail(code:i32,message:impl Into<String>)->i32 {LAST_ERROR.with(|e|*e.borrow_mut()=message.into());code}
fn ok()->i32 {LAST_ERROR.with(|e|e.borrow_mut().clear());0}

unsafe fn input<'a>(ptr:*const u8,len:usize)->Result<&'a str,i32> {
    if ptr.is_null()||len==0||len>65536 {return Err(fail(1,"Input must be 1..65536 bytes"));}
    std::str::from_utf8(std::slice::from_raw_parts(ptr,len)).map_err(|_|fail(1,"Input is not valid UTF-8"))
}
fn options(text:&str)->Result<Options,String> {
    let j:serde_json::Value=serde_json::from_str(text).map_err(|e|format!("Invalid JSON: {e}"))?;
    let obj=j.as_object().ok_or("Config must be a JSON object")?;
    if let Some(k)=obj.keys().find(|k| !["backend","lang","voice","rate","pitch","neural_home"].contains(&k.as_str())) {return Err(format!("Unknown key: {k}"));}
    let mut o=Options::default();
    if let Some(x)=obj.get("backend") {o.backend=match x.as_str(){Some("compact")=>Backend::Compact,Some("neural")=>Backend::Neural,_=>return Err("backend must be compact or neural".into())};}
    if let Some(x)=obj.get("lang") {o.french=match x.as_str(){Some("fr")=>true,Some("en")=>false,_=>return Err("lang must be fr or en".into())};}
    o.voice=if o.backend==Backend::Neural {if o.french {"ff_siwis"}else{"af_heart"}}else{"male"}.into();
    if let Some(x)=obj.get("voice") {o.voice=x.as_str().ok_or("voice must be a string")?.into();}
    for (name,target) in [("rate",&mut o.rate),("pitch",&mut o.pitch)] {
        if let Some(x)=obj.get(name) {*target=x.as_u64().and_then(|v|u32::try_from(v).ok()).ok_or(format!("{name} must be an unsigned integer"))?;}
    }
    if let Some(x)=obj.get("neural_home") {o.neural_home=Some(x.as_str().ok_or("neural_home must be a string")?.into());}
    Ok(o)
}
/// JSON config: backend, lang, voice, rate, pitch, neural_home. Unknown keys fail.
/// Return 0 success, 1 invalid input, 2 backend failure, 3 busy/panic, 4 cancellation.
#[no_mangle]
pub unsafe extern "C" fn st_engine_create_v1(config:*const u8,len:usize,out:*mut *mut StEngine)->i32 {
    if out.is_null() {return fail(1,"out is NULL");} *out=std::ptr::null_mut();
    catch_unwind(AssertUnwindSafe(|| {
        let text=match input(config,len) {Ok(x)=>x,Err(c)=>return c};
        let o=match options(text) {Ok(o)=>o,Err(e)=>return fail(1,e)};
        match Engine::new(o) {Ok(e)=>{*out=Box::into_raw(Box::new(StEngine{inner:Mutex::new(e),cancel:AtomicBool::new(false)}));ok()},Err(e)=>fail(2,e)}
    })).unwrap_or_else(|_|fail(3,"Internal panic"))
}
#[no_mangle]
pub unsafe extern "C" fn st_engine_destroy_v1(handle:*mut StEngine) {
    if !handle.is_null() {drop(Box::from_raw(handle));}
}
#[no_mangle]
pub unsafe extern "C" fn st_engine_stream_v1(handle:*mut StEngine,text:*const u8,len:usize,callback:Option<Callback>,user:*mut std::ffi::c_void)->i32 {
    let Some(callback)=callback else {return fail(1,"callback is NULL")};
    if handle.is_null() {return fail(1,"handle is NULL");}
    catch_unwind(AssertUnwindSafe(||{
        let text=match input(text,len){Ok(s)=>s,Err(c)=>return c};
        let mut engine=match (*handle).inner.try_lock(){Ok(e)=>e,Err(_)=>return fail(3,"Engine busy or poisoned")};
        let flag=&(*handle).cancel;
        flag.store(false,Ordering::SeqCst);
        match engine.stream(text,|pcm| !flag.load(Ordering::SeqCst) && callback(pcm.as_ptr(),pcm.len(),48000,user)!=0) {
            Ok(())=>ok(),Err(e) if e=="Cancelled"=>fail(4,e),Err(e)=>fail(2,e)
        }
    })).unwrap_or_else(|_|fail(3,"Internal panic"))
}
/// Thread-safe: stops the utterance currently streaming on this handle (st_engine_stream_v1
/// then returns 4 before delivering further audio). No effect when idle.
#[no_mangle]
pub unsafe extern "C" fn st_engine_cancel_v1(handle:*const StEngine) {
    if !handle.is_null() {(*handle).cancel.store(true,Ordering::SeqCst);}
}
/// The returned buffer is released with st_free_wav(ptr, len).
#[no_mangle]
pub unsafe extern "C" fn st_engine_wav_v1(handle:*mut StEngine,text:*const u8,len:usize,out:*mut *mut u8,out_len:*mut usize)->i32 {
    if out.is_null()||out_len.is_null(){return fail(1,"out/out_len is NULL");}*out=std::ptr::null_mut();*out_len=0;
    if handle.is_null(){return fail(1,"handle is NULL");}
    catch_unwind(AssertUnwindSafe(||{
        let text=match input(text,len){Ok(s)=>s,Err(c)=>return c};
        let mut engine=match (*handle).inner.try_lock(){Ok(e)=>e,Err(_)=>return fail(3,"Engine busy or poisoned")};
        match engine.wav(text) {Ok(wav)=>{
            let mut data=wav.into_boxed_slice();*out_len=data.len();*out=data.as_mut_ptr();std::mem::forget(data);ok()
        },Err(e)=>fail(2,e)}
    })).unwrap_or_else(|_|fail(3,"Internal panic"))
}
/// Copies the calling thread's last v1 error as NUL-terminated UTF-8 (truncated to cap).
/// Returns the full message length in bytes, excluding the NUL; 0 means no error.
#[no_mangle]
pub unsafe extern "C" fn st_last_error_v1(buffer:*mut u8,cap:usize)->usize {
    LAST_ERROR.with(|e| {
        let e=e.borrow();
        if !buffer.is_null() && cap>0 {
            let mut n=e.len().min(cap-1);
            while !e.is_char_boundary(n) {n-=1;}
            std::ptr::copy_nonoverlapping(e.as_ptr(),buffer,n);*buffer.add(n)=0;
        }
        e.len()
    })
}
