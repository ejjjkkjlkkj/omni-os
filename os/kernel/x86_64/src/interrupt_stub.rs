//! The bare interrupt entry sequence shared by every device vector.
//!
//! There is exactly one save/restore sequence in this kernel for external
//! interrupts, and it lives here. Each vector gets its own entry symbol but the
//! same audited body: a hand-written stub per device would be a hand-written
//! chance to forget a register.

/// Define a bare interrupt entry point that preserves every register, calls a
/// Rust dispatch function, and returns with `iretq`.
///
/// External interrupts push no error code, so the frame the CPU builds is
/// exactly what `iretq` expects; the only requirement is that nothing the
/// handler runs is visible to the interrupted context.
#[macro_export]
macro_rules! device_interrupt_stub {
    ($stub:ident, $dispatch:path) => {
        core::arch::global_asm!(
            concat!(".global ", stringify!($stub)),
            concat!(".type ", stringify!($stub), ",@function"),
            concat!(stringify!($stub), ":"),
            "push rax",
            "push rcx",
            "push rdx",
            "push rsi",
            "push rdi",
            "push r8",
            "push r9",
            "push r10",
            "push r11",
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "cld",
            // Keep the exact frame position in RBX while the System V ABI's
            // 16-byte stack alignment is forced for the call.
            "mov rbx, rsp",
            "and rsp, -16",
            "call {dispatch}",
            "mov rsp, rbx",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbp",
            "pop rbx",
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdi",
            "pop rsi",
            "pop rdx",
            "pop rcx",
            "pop rax",
            "iretq",
            concat!(".size ", stringify!($stub), ", .-", stringify!($stub)),
            dispatch = sym $dispatch,
        );

        unsafe extern "C" {
            fn $stub();
        }
    };
}
