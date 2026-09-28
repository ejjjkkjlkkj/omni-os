# Run the complete boot-proof suite as NT AUTHORITY\SYSTEM (launched through PsExec64 -s).
param([string]$Log = 'C:\aw-kernel\target-proofs-system.log', [string[]]$Only = @())
$env:PATH = "C:\Users\adm\.cargo\bin;C:\Program Files\Python313;C:\Program Files\LLVM\bin;$env:PATH"
$env:CARGO_HOME = 'C:\Users\adm\.cargo'
$env:RUSTUP_HOME = 'C:\Users\adm\.rustup'
Set-Location C:\aw-kernel
"identity: $([Security.Principal.WindowsIdentity]::GetCurrent().Name)" | Set-Content $Log
$args2 = @('-NoProfile', '-File', 'scripts\Invoke-BootProofs.ps1')
if ($Only) { $args2 += @('-Only') + $Only }
& pwsh @args2 *>> $Log
"exit: $LASTEXITCODE" | Add-Content $Log
