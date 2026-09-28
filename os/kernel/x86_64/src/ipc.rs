//! IPC and the handle/object model (roadmap Phase 2, last item).
//!
//! Kernel objects live in one table and are reference counted; a process never
//! sees an object, only a *handle* into its own handle table. A handle value is
//! `generation << 16 | slot`: the generation is global and never reused, so a
//! handle that was closed, copied from another process or forged does not name a
//! live entry and is refused. Each entry carries *rights*; the two ends of a
//! channel differ only by rights (send or receive).
//!
//! The first object type is a channel: a bounded queue of small messages. Send
//! and receive never block - an empty or full queue is reported to the caller,
//! which retries while the timer runs the other side. That keeps every syscall
//! short and non-preemptible, as the single syscall stack requires.
//!
//! Every user pointer is checked against the calling process's registered user
//! region before any copy, and copies honour SMAP with `stac`/`clac`.

use core::sync::atomic::{AtomicU64, Ordering};

/// Syscall numbers (the ABI stays additive: version 1 keeps its meaning).
pub const SYS_CHANNEL_SEND: u64 = 16;
pub const SYS_CHANNEL_RECV: u64 = 17;
pub const SYS_HANDLE_CLOSE: u64 = 18;

/// Error codes returned in `rax` (values no successful call can produce).
pub const E_BAD_HANDLE: u64 = u64::MAX - 1;
pub const E_ACCESS: u64 = u64::MAX - 2;
pub const E_FAULT: u64 = u64::MAX - 3;
pub const E_EMPTY: u64 = u64::MAX - 4;
pub const E_FULL: u64 = u64::MAX - 5;
pub const E_TOO_BIG: u64 = u64::MAX - 6;
pub const E_NO_PROCESS: u64 = u64::MAX - 7;

pub const RIGHT_SEND: u8 = 1;
pub const RIGHT_RECV: u8 = 2;

const MAX_PROCESSES: usize = 4;
const HANDLES_PER_PROCESS: usize = 16;
const MAX_OBJECTS: usize = 8;
const QUEUE_DEPTH: usize = 8;
pub const MAX_MESSAGE: usize = 64;

#[derive(Clone, Copy)]
struct Message {
    len: usize,
    data: [u8; MAX_MESSAGE],
}

#[derive(Clone, Copy)]
struct Channel {
    refs: u32,
    head: usize,
    len: usize,
    queue: [Message; QUEUE_DEPTH],
}

#[derive(Clone, Copy)]
struct HandleEntry {
    object: u16,
    generation: u16,
    rights: u8,
}

#[derive(Clone, Copy)]
struct Process {
    live: bool,
    user_start: u64,
    user_end: u64,
    handles: [Option<HandleEntry>; HANDLES_PER_PROCESS],
}

const EMPTY_MESSAGE: Message = Message {
    len: 0,
    data: [0; MAX_MESSAGE],
};
const EMPTY_CHANNEL: Channel = Channel {
    refs: 0,
    head: 0,
    len: 0,
    queue: [EMPTY_MESSAGE; QUEUE_DEPTH],
};
const EMPTY_PROCESS: Process = Process {
    live: false,
    user_start: 0,
    user_end: 0,
    handles: [None; HANDLES_PER_PROCESS],
};

struct State {
    objects: [Option<Channel>; MAX_OBJECTS],
    processes: [Process; MAX_PROCESSES],
    /// scheduler slot -> process id
    slot_process: [Option<usize>; 8],
    next_generation: u16,
}

// Single-CPU state touched only at CPL0 with interrupts masked (syscalls clear
// IF through IA32_FMASK; setup runs before the user programs are scheduled).
static mut STATE: State = State {
    objects: [None; MAX_OBJECTS],
    processes: [EMPTY_PROCESS; MAX_PROCESSES],
    slot_process: [None; 8],
    next_generation: 1,
};

/// Per-error rejection counters, for the proof and for diagnostics.
pub static REJECTED_BAD_HANDLE: AtomicU64 = AtomicU64::new(0);
pub static REJECTED_ACCESS: AtomicU64 = AtomicU64::new(0);
pub static REJECTED_FAULT: AtomicU64 = AtomicU64::new(0);
pub static DELIVERED: AtomicU64 = AtomicU64::new(0);

fn state() -> &'static mut State {
    // SAFETY: see STATE; never touched concurrently.
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

/// Reset everything (a fresh object/handle universe for one proof run).
pub fn reset() {
    let s = state();
    s.objects = [None; MAX_OBJECTS];
    s.processes = [EMPTY_PROCESS; MAX_PROCESSES];
    s.slot_process = [None; 8];
    for counter in [
        &REJECTED_BAD_HANDLE,
        &REJECTED_ACCESS,
        &REJECTED_FAULT,
        &DELIVERED,
    ] {
        counter.store(0, Ordering::Relaxed);
    }
}

/// Register a process running in `slot` whose user memory is `[start, end)`.
pub fn create_process(pid: usize, slot: usize, user_start: u64, user_end: u64) -> bool {
    let s = state();
    if pid >= MAX_PROCESSES || slot >= s.slot_process.len() || user_start >= user_end {
        return false;
    }
    s.processes[pid] = Process {
        live: true,
        user_start,
        user_end,
        handles: [None; HANDLES_PER_PROCESS],
    };
    s.slot_process[slot] = Some(pid);
    true
}

/// Create a channel object with no references yet.
pub fn create_channel() -> Option<usize> {
    let s = state();
    let index = s.objects.iter().position(Option::is_none)?;
    s.objects[index] = Some(EMPTY_CHANNEL);
    Some(index)
}

/// Grant `pid` a handle to `object` with `rights`; returns the handle value.
pub fn grant(pid: usize, object: usize, rights: u8) -> Option<u64> {
    let s = state();
    let process = s.processes.get_mut(pid).filter(|p| p.live)?;
    let channel = s.objects.get_mut(object)?.as_mut()?;
    let slot = process.handles.iter().position(Option::is_none)?;
    let generation = s.next_generation;
    s.next_generation = s.next_generation.checked_add(1)?; // never reuse a generation
    process.handles[slot] = Some(HandleEntry {
        object: object as u16,
        generation,
        rights,
    });
    channel.refs += 1;
    Some((u64::from(generation) << 16) | slot as u64)
}

fn caller_pid(slot: usize) -> Option<usize> {
    state().slot_process.get(slot).copied().flatten()
}

/// Resolve a handle value in `pid`'s table; counts and returns the refusal.
fn resolve(pid: usize, handle: u64, needed: u8) -> Result<(usize, usize), u64> {
    let s = state();
    let index = (handle & 0xffff) as usize;
    let generation = (handle >> 16) as u16;
    let entry = if handle >> 32 == 0 {
        s.processes[pid]
            .handles
            .get(index)
            .copied()
            .flatten()
            .filter(|e| e.generation == generation)
    } else {
        None
    };
    let Some(entry) = entry else {
        REJECTED_BAD_HANDLE.fetch_add(1, Ordering::Relaxed);
        return Err(E_BAD_HANDLE);
    };
    if entry.rights & needed != needed {
        REJECTED_ACCESS.fetch_add(1, Ordering::Relaxed);
        return Err(E_ACCESS);
    }
    Ok((index, entry.object as usize))
}

/// The whole `[ptr, ptr+len)` must lie inside the caller's user region.
fn check_user(pid: usize, ptr: u64, len: u64) -> Result<(), u64> {
    let p = &state().processes[pid];
    match ptr.checked_add(len) {
        Some(end) if ptr >= p.user_start && end <= p.user_end => Ok(()),
        _ => {
            REJECTED_FAULT.fetch_add(1, Ordering::Relaxed);
            Err(E_FAULT)
        }
    }
}

fn smap_enabled() -> bool {
    let cr4: u64;
    // SAFETY: reading CR4 at CPL0 has no side effects.
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags))
    };
    cr4 & (1 << 21) != 0
}

/// Copy between kernel and a validated user range with SMAP opened only for it.
///
/// # Safety
/// The user range was validated by `check_user` and is mapped.
unsafe fn user_copy(dst: *mut u8, src: *const u8, len: usize) {
    let smap = smap_enabled();
    if smap {
        // SAFETY: open supervisor access to user pages for this copy only.
        unsafe { core::arch::asm!("stac", options(nomem, nostack)) };
    }
    for i in 0..len {
        // SAFETY: both ranges are valid for `len` bytes (caller contract).
        unsafe { dst.add(i).write_volatile(src.add(i).read_volatile()) };
    }
    if smap {
        // SAFETY: close it again.
        unsafe { core::arch::asm!("clac", options(nomem, nostack)) };
    }
}

fn send(pid: usize, handle: u64, ptr: u64, len: u64) -> u64 {
    let object = match resolve(pid, handle, RIGHT_SEND) {
        Ok((_, object)) => object,
        Err(e) => return e,
    };
    if len as usize > MAX_MESSAGE {
        return E_TOO_BIG;
    }
    if let Err(e) = check_user(pid, ptr, len) {
        return e;
    }
    let Some(channel) = state().objects[object].as_mut() else {
        return E_BAD_HANDLE;
    };
    if channel.len == QUEUE_DEPTH {
        return E_FULL;
    }
    let slot = (channel.head + channel.len) % QUEUE_DEPTH;
    let message = &mut channel.queue[slot];
    // SAFETY: user range validated; the kernel buffer holds MAX_MESSAGE bytes.
    unsafe { user_copy(message.data.as_mut_ptr(), ptr as *const u8, len as usize) };
    message.len = len as usize;
    channel.len += 1;
    len
}

fn recv(pid: usize, handle: u64, ptr: u64, capacity: u64) -> u64 {
    let object = match resolve(pid, handle, RIGHT_RECV) {
        Ok((_, object)) => object,
        Err(e) => return e,
    };
    let Some(channel) = state().objects[object].as_mut() else {
        return E_BAD_HANDLE;
    };
    if channel.len == 0 {
        return E_EMPTY;
    }
    let message = channel.queue[channel.head];
    if message.len as u64 > capacity {
        return E_TOO_BIG;
    }
    if let Err(e) = check_user(pid, ptr, message.len as u64) {
        return e;
    }
    // SAFETY: user range validated for message.len bytes.
    unsafe { user_copy(ptr as *mut u8, message.data.as_ptr(), message.len) };
    channel.head = (channel.head + 1) % QUEUE_DEPTH;
    channel.len -= 1;
    DELIVERED.fetch_add(1, Ordering::Relaxed);
    message.len as u64
}

fn close(pid: usize, handle: u64) -> u64 {
    let (index, object) = match resolve(pid, handle, 0) {
        Ok(found) => found,
        Err(e) => return e,
    };
    let s = state();
    s.processes[pid].handles[index] = None;
    if let Some(channel) = s.objects[object].as_mut() {
        channel.refs -= 1;
        if channel.refs == 0 {
            s.objects[object] = None; // last reference: the object dies with it
        }
    }
    0
}

/// Syscall entry for the IPC numbers. `slot` is the scheduler slot that trapped.
pub fn dispatch(slot: usize, number: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let Some(pid) = caller_pid(slot) else {
        return E_NO_PROCESS;
    };
    match number {
        SYS_CHANNEL_SEND => send(pid, a0, a1, a2),
        SYS_CHANNEL_RECV => recv(pid, a0, a1, a2),
        SYS_HANDLE_CLOSE => close(pid, a0),
        _ => u64::MAX,
    }
}

/// Live objects (for the proof: closing the spare handle must not kill the channel).
pub fn live_objects() -> usize {
    state().objects.iter().filter(|o| o.is_some()).count()
}
