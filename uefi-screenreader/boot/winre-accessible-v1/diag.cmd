@echo off
rem Accessible WinRE start-up diagnostic: writes results to <boot disk>\diag.
set D=X:
for %%L in (C D E F G H I J K) do if exist %%L:\sources\winre.wim set D=%%L:
set O=%D%\diag
mkdir %O% 2>nul
set ST=%SystemDrive%\Program Files\ST
echo boot=%D% > %O%\env.txt
"%ST%\speak.exe" --ps > %O%\ps-before.txt 2>&1
pnputil /enum-devices /class MEDIA > %O%\media.txt 2>&1
(sc qc AudioEndpointBuilder & sc qc Audiosrv & sc query AudioEndpointBuilder & sc query Audiosrv) > %O%\audio-services-before.txt 2>&1
(net start AudioEndpointBuilder & net start Audiosrv) > %O%\audio-services-start.txt 2>&1
ping -n 4 127.0.0.1 > nul
(sc query AudioEndpointBuilder & sc query Audiosrv) > %O%\audio-services-after.txt 2>&1
pnputil /enum-devices /class AudioEndpoint > %O%\endpoints.txt 2>&1
for /l %%i in (1,1,12) do ("%ST%\speak.exe" --audio >> %O%\audio-stack.txt 2>&1 & ping -n 6 127.0.0.1 > nul)
"%ST%\speak.exe" %O% "ST Siwis" Hortense > %O%\speak.txt 2>&1
start "" %SYSTEMROOT%\System32\Narrator.exe
ping -n 25 127.0.0.1 > nul
"%ST%\speak.exe" --ps > %O%\ps-after.txt 2>&1
echo DIAG_DONE > %O%\done.txt
