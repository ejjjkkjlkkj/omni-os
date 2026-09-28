"""Boot the accessible WinRE copy in QEMU (WHPX), record audio, take screenshots."""
import json, socket, subprocess, sys, time, pathlib

out = pathlib.Path(r"C:\OMNI-BACKUPS\winre\vmrun")
out.mkdir(exist_ok=True)
for f in out.glob("*"):
    f.unlink()
seconds = int(sys.argv[1]) if len(sys.argv) > 1 else 300
keys = sys.argv[2:]  # optional keys sent after the boot wait
qemu = r"C:\Program Files\qemu\qemu-system-x86_64.exe"
ovmf = r"C:\OMNI-BACKUPS\stage-36278366143-20260927-011750\OVMF_CODE.fd"
port = 4470
p = subprocess.Popen([qemu, "-machine", "q35", "-accel", "whpx", "-accel", "tcg", "-cpu", "max", "-smp", "4", "-m", "4096",
    "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
    "-drive", __import__("os").environ.get("ST_VM_DISK", r"format=vpc,file=C:\OMNI-BACKUPS\winre\winre-vm.vhd"),
    *__import__("os").environ.get("ST_VM_AUDIO", "-device intel-hda -device hda-duplex,audiodev=a0").split(),
    "-audiodev", rf"wav,id=a0,path={out}\audio.wav",
    "-usb", "-device", "usb-kbd", "-device", "usb-tablet",
    "-display", "none", "-vga", "std", "-net", "none",
    "-serial", rf"file:{out}\serial.txt",
    "-qmp", f"tcp:127.0.0.1:{port},server,nowait"], stderr=open(out / "qemu-stderr.txt", "w"))
time.sleep(3)
s = socket.create_connection(("127.0.0.1", port))
f = s.makefile("rw")
f.readline()
def cmd(obj):
    f.write(json.dumps(obj) + "\n"); f.flush()
    while True:
        line = json.loads(f.readline())
        if "return" in line or "error" in line:
            return line
cmd({"execute": "qmp_capabilities"})
start = time.time()
shot = 0
while time.time() - start < seconds and p.poll() is None:
    time.sleep(30)
    shot += 1
    cmd({"execute": "screendump", "arguments": {"filename": str(out / f"screen{shot:02d}.ppm")}})
    print(f"t={int(time.time() - start)}s screenshot {shot}", flush=True)
for k in keys:
    cmd({"execute": "human-monitor-command", "arguments": {"command-line": f"sendkey {k}"}})
    time.sleep(4)
if keys:
    time.sleep(int(__import__("os").environ.get("ST_VM_POST", "15")))
    cmd({"execute": "screendump", "arguments": {"filename": str(out / "screen-final.ppm")}})
cmd({"execute": "quit"})
p.wait(30)
print("exit", p.returncode)
