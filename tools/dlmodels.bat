@echo off && chcp 65001

echo working dir is %cd%
echo downloading requirement aria2 check.
echo=

dir /a:d/b | findstr "aria2" > flag.txt
findstr "aria2" flag.txt >nul
if %errorlevel% ==0 (
    echo aria2 checked.
    echo=
) else (
    echo failed. please downloading aria2 from webpage!
    echo unzip it and put in this directory!
    timeout /T 5
    start https://github.com/aria2/aria2/releases/tag/release-1.36.0
    echo=
    goto end
)

for /f %%x in ('findstr /i /c:"aria2" "flag.txt"') do (set aria2=%%x)&goto endSch
:endSch

REM 仅需 hubert_base.pt + rmvpe.pt
REM HDEMUCS 由 torchaudio 自动下载
REM 预训练模型不再使用

set hb=hubert_base.pt
set rmvpe=rmvpe.pt

set dlhb=https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/main/hubert_base.pt
set dlrmvpe=https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/main/rmvpe.pt

echo envfiles checking start.
echo=

echo dir check start.
if exist "%~dp0assets\hubert" (
    echo dir .\assets\hubert checked.
) else (
    echo failed. generating dir .\assets\hubert.
    mkdir assets\hubert
)
if exist "%~dp0assets\rmvpe" (
    echo dir .\assets\rmvpe checked.
) else (
    echo failed. generating dir .\assets\rmvpe.
    mkdir assets\rmvpe
)
echo dir check finished.

echo=
echo required files check start.

echo checking %hb%
if exist "%~dp0assets\hubert\%hb%" (
    echo %hb% in .\assets\hubert checked.
    echo=
) else (
    echo failed. starting download from huggingface.
    %~dp0%aria2%\aria2c --console-log-level=error -c -x 16 -s 16 -k 1M %dlhb% -d %~dp0assets\hubert\ -o %hb%
    if exist "%~dp0assets\hubert\%hb%" (echo download successful.) else (echo please try again!
    echo=)
)

echo checking %rmvpe%
if exist "%~dp0assets\rmvpe\%rmvpe%" (
    echo %rmvpe% in .\assets\rmvpe checked.
    echo=
) else (
    echo failed. starting download from huggingface.
    %~dp0%aria2%\aria2c --console-log-level=error -c -x 16 -s 16 -k 1M %dlrmvpe% -d %~dp0assets\rmvpe\ -o %rmvpe%
    if exist "%~dp0assets\rmvpe\%rmvpe%" (echo download successful.) else (echo please try again!
    echo=)
)

echo required files check finished.
echo envfiles check complete.
pause
:end
del flag.txt
