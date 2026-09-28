#!/usr/bin/env python3
"""Emit a minimal static x86-64 ELF for the userland boot proofs.

Two tiny, hand-built programs (no toolchain, so the proofs have no build
dependency beyond Python, matching the other on-disk fixtures):

  * default: SYS_REPORT(0xC0DE) then SYS_EXIT - for the single-program loader
    proof. The kernel seeing 0xC0DE come back proves the on-disk program ran.
  * --spinner: increment a counter at BASE+0x1000 forever - for the init proof
    that preemptively schedules several userland programs. The kernel maps the
    counter page, runs the program at CPL3, and checks the counter advanced.

Usage:
  make-user-elf.py OUT.ELF [--base HEX]
  make-user-elf.py OUT.ELF --spinner --base HEX
"""

import struct
import sys

# Must match ring3.rs: SYS_REPORT = 3, SYS_EXIT = 0xff, EXPECTED_REPORT = 0xC0DE.
SYS_REPORT = 3
SYS_EXIT = 0xFF
REPORT_VALUE = 0xC0DE

EHDR_SIZE = 64
PHDR_SIZE = 56
CODE_OFFSET = EHDR_SIZE + PHDR_SIZE  # single program header, then code

# Counter page the init proof maps read-write, relative to the program's base.
WORK_OFFSET = 0x1000


def report_code():
    # mov rax, SYS_REPORT ; mov rdi, REPORT_VALUE ; syscall ; mov rax, SYS_EXIT ; syscall
    return bytes(
        [0x48, 0xC7, 0xC0, *struct.pack("<I", SYS_REPORT)]
        + [0x48, 0xC7, 0xC7, *struct.pack("<I", REPORT_VALUE)]
        + [0x0F, 0x05]
        + [0x48, 0xC7, 0xC0, *struct.pack("<I", SYS_EXIT)]
        + [0x0F, 0x05]
    )


def spinner_code(base):
    work = base + WORK_OFFSET
    # mov rax, imm64(work) ; loop: inc qword [rax] ; jmp loop
    return bytes(
        [0x48, 0xB8, *struct.pack("<Q", work)]
        + [0x48, 0xFF, 0x00]
        + [0xEB, 0xFB]
    )


# ---- IPC proof programs ------------------------------------------------------
# Must match ipc.rs: syscall numbers, error codes and the work-page layout the
# kernel fills with the initial handles before the programs start.
SYS_CHANNEL_SEND = 16
SYS_CHANNEL_RECV = 17
SYS_HANDLE_CLOSE = 18
IPC_MESSAGES = 32          # messages A sends; B must receive all, in order
IPC_MAGIC = 0xA11CE        # second qword of every message
W_PROGRESS = 0x00          # incremented by each program's idle loop
W_HANDLE = 0x08            # A: send end / B: receive end (written by the kernel)
W_FOREIGN = 0x10           # A: a handle value that is only valid in B's table
W_FORGED = 0x18            # A: its own handle with a wrong generation
W_TEMP = 0x20              # A: a spare handle it closes, then reuses
W_RESULTS = 0x40           # A: five negative-test return codes
W_COUNT = 0x80             # A: messages sent / B: messages received
W_SUM = 0x90               # B: sum of received sequence numbers
W_BAD_ORDER = 0x98         # B: messages received out of order or damaged
W_BUF = 0x100              # A: outgoing message / B: receive buffer
KERNEL_ADDR = 0xFFFF_8000_0000_1000  # never a user address: must fault (E_FAULT)

RAX, RCX, RDX, RBX, RSP, RBP, RSI, RDI = range(8)


class Asm:
    """Just enough x86-64 for the IPC programs; rbx always holds the work page."""

    def __init__(self):
        self.code = bytearray()
        self.labels = {}
        self.fixups = []

    def mov_imm(self, reg, value):          # mov r64, imm64
        self.code += bytes([0x48, 0xB8 + reg]) + struct.pack("<Q", value & (2**64 - 1))

    def load(self, reg, disp):              # mov r64, [rbx+disp32]
        self.code += bytes([0x48, 0x8B, 0x80 | (reg << 3) | RBX]) + struct.pack("<i", disp)

    def store(self, disp, reg):             # mov [rbx+disp32], r64
        self.code += bytes([0x48, 0x89, 0x80 | (reg << 3) | RBX]) + struct.pack("<i", disp)

    def store_imm(self, disp, value):       # mov qword [rbx+disp32], imm32
        self.code += bytes([0x48, 0xC7, 0x80 | RBX]) + struct.pack("<ii", disp, value)

    def inc(self, disp):                    # inc qword [rbx+disp32]
        self.code += bytes([0x48, 0xFF, 0x80 | RBX]) + struct.pack("<i", disp)

    def add_mem(self, disp, reg):           # add [rbx+disp32], r64
        self.code += bytes([0x48, 0x01, 0x80 | (reg << 3) | RBX]) + struct.pack("<i", disp)

    def lea(self, reg, disp):               # lea r64, [rbx+disp32]
        self.code += bytes([0x48, 0x8D, 0x80 | (reg << 3) | RBX]) + struct.pack("<i", disp)

    def cmp_imm(self, value):               # cmp rax, imm32
        self.code += bytes([0x48, 0x3D]) + struct.pack("<i", value)

    def cmp_reg(self, a, b):                # cmp a, b
        self.code += bytes([0x48, 0x39, 0xC0 | (b << 3) | a])

    def inc_reg(self, reg):                 # inc r64
        self.code += bytes([0x48, 0xFF, 0xC0 | reg])

    def syscall(self):
        self.code += b"\x0F\x05"

    def label(self, name):
        self.labels[name] = len(self.code)

    def _jump(self, opcode, name):
        self.code += opcode
        self.fixups.append((len(self.code), name))
        self.code += b"\0\0\0\0"

    def jmp(self, name):
        self._jump(b"\xE9", name)

    def je(self, name):
        self._jump(b"\x0F\x84", name)

    def jne(self, name):
        self._jump(b"\x0F\x85", name)

    def jae(self, name):                    # unsigned >=: error codes are huge
        self._jump(b"\x0F\x83", name)

    def assemble(self):
        for at, name in self.fixups:
            struct.pack_into("<i", self.code, at, self.labels[name] - (at + 4))
        return bytes(self.code)


def sys3(a, number, arg0_reg_disp=None, arg0=None, arg1=None, arg2=None):
    """rax=number; rdi=arg0 (from the work page or an immediate); rsi/rdx immediates or regs."""
    a.mov_imm(RAX, number)
    if arg0_reg_disp is not None:
        a.load(RDI, arg0_reg_disp)
    elif arg0 is not None:
        a.mov_imm(RDI, arg0)
    if arg1 is not None:
        a.mov_imm(RSI, arg1)
    if arg2 is not None:
        a.mov_imm(RDX, arg2)
    a.syscall()


def ipc_sender_code(base):
    work = base + WORK_OFFSET
    a = Asm()
    a.mov_imm(RBX, work)
    buf = work + W_BUF
    # Negative tests first; each return code is recorded for the kernel to check.
    sys3(a, SYS_CHANNEL_SEND, arg0_reg_disp=W_FOREIGN, arg1=buf, arg2=16)      # foreign handle
    a.store(W_RESULTS + 0, RAX)
    sys3(a, SYS_CHANNEL_SEND, arg0_reg_disp=W_FORGED, arg1=buf, arg2=16)       # forged generation
    a.store(W_RESULTS + 8, RAX)
    sys3(a, SYS_CHANNEL_RECV, arg0_reg_disp=W_HANDLE, arg1=buf, arg2=64)       # send end lacks RECV
    a.store(W_RESULTS + 16, RAX)
    sys3(a, SYS_CHANNEL_SEND, arg0_reg_disp=W_HANDLE, arg1=KERNEL_ADDR, arg2=16)  # kernel pointer
    a.store(W_RESULTS + 24, RAX)
    sys3(a, SYS_HANDLE_CLOSE, arg0_reg_disp=W_TEMP)
    sys3(a, SYS_CHANNEL_SEND, arg0_reg_disp=W_TEMP, arg1=buf, arg2=16)         # use after close
    a.store(W_RESULTS + 32, RAX)
    # Send IPC_MESSAGES messages [seq, IPC_MAGIC], retrying while the queue is full.
    a.store_imm(W_BUF + 8, IPC_MAGIC)
    a.label("next")
    a.load(RAX, W_COUNT)
    a.cmp_imm(IPC_MESSAGES)
    a.je("idle")
    a.inc_reg(RAX)
    a.store(W_BUF, RAX)
    a.label("retry")
    sys3(a, SYS_CHANNEL_SEND, arg0_reg_disp=W_HANDLE, arg1=buf, arg2=16)
    a.cmp_imm(16)
    a.jne("retry")                           # E_FULL: the receiver has not drained yet
    a.inc(W_COUNT)
    a.jmp("next")
    a.label("idle")
    a.inc(W_PROGRESS)
    a.jmp("idle")
    return a.assemble()


def ipc_receiver_code(base):
    work = base + WORK_OFFSET
    a = Asm()
    a.mov_imm(RBX, work)
    a.label("loop")
    a.inc(W_PROGRESS)
    sys3(a, SYS_CHANNEL_RECV, arg0_reg_disp=W_HANDLE, arg1=work + W_BUF, arg2=64)
    a.cmp_imm(16)
    a.jne("loop")                            # E_EMPTY (or anything else): try again
    # Check order and payload: seq must be count+1 and the magic intact.
    a.load(RAX, W_COUNT)
    a.inc_reg(RAX)
    a.load(RDX, W_BUF)
    a.cmp_reg(RAX, RDX)
    a.jne("bad")
    a.load(RCX, W_BUF + 8)
    a.mov_imm(RSI, IPC_MAGIC)
    a.cmp_reg(RCX, RSI)
    a.jne("bad")
    a.label("good")
    a.store(W_COUNT, RAX)
    a.add_mem(W_SUM, RDX)
    a.jmp("loop")
    a.label("bad")
    a.inc(W_BAD_ORDER)
    a.jmp("loop")
    return a.assemble()


def main():
    out = sys.argv[1]
    args = sys.argv[2:]
    spinner = "--spinner" in args
    base = 0x4_0000_0000
    if "--base" in args:
        base = int(args[args.index("--base") + 1], 0)

    if "--ipc-sender" in args:
        code = ipc_sender_code(base)
    elif "--ipc-receiver" in args:
        code = ipc_receiver_code(base)
    else:
        code = spinner_code(base) if spinner else report_code()
    total = CODE_OFFSET + len(code)
    entry = base + CODE_OFFSET

    ehdr = struct.pack(
        "<16sHHIQQQIHHHHHH",
        b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        2, 0x3E, 1, entry, EHDR_SIZE, 0, 0, EHDR_SIZE, PHDR_SIZE, 1, 0, 0, 0,
    )
    phdr = struct.pack(
        "<IIQQQQQQ",
        1,      # PT_LOAD
        5,      # R + X
        0, base, base, total, total, 0x1000,
    )
    image = ehdr + phdr + code
    assert len(image) == total, (len(image), total)
    with open(out, "wb") as handle:
        handle.write(image)
    kind = ("ipc-sender" if "--ipc-sender" in args else "ipc-receiver" if "--ipc-receiver" in args
            else "spinner" if spinner else "report")
    print(f"wrote {out}: {total} bytes ({kind}, base={base:#x}, entry={entry:#x})")


if __name__ == "__main__":
    main()
