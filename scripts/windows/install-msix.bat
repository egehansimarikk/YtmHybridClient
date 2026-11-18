@echo off
title YTM Hybrid v0.4.3 MSIX Kurulumu

:: Yonetici yetkisi kontrolu (yoksa UAC penceresi acar)
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Yonetici yetkisi isteniyor...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

echo === YTM Hybrid v0.4.3 MSIX Kurulumu Baslatiliyor ===
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference = 'Stop';" ^
  "$dirs = @('%~dp0', \"$env:USERPROFILE\Downloads\", \"$env:USERPROFILE\Desktop\") | Where-Object { Test-Path $_ };" ^
  "$msix = $dirs | ForEach-Object { Get-ChildItem -Path $_ -Filter 'ytm-hybrid-v0.4.3*.msix' -File -ErrorAction SilentlyContinue } | Sort-Object LastWriteTime -Descending | Select-Object -First 1;" ^
  "if (-not $msix) { throw 'ytm-hybrid-v0.4.3*.msix dosyasi bulunamadi! Lutfen .bat ve .msix dosyasini ayni klasore koyun.' };" ^
  "Write-Host ('Paket bulundu: ' + $msix.FullName) -ForegroundColor Green;" ^
  "$sig = Get-AuthenticodeSignature -FilePath $msix.FullName;" ^
  "if (-not $sig.SignerCertificate) { throw 'MSIX paketinde dijital imza bulunamadi!' };" ^
  "Write-Host 'Sertifika guvenilen depolara ekleniyor...' -ForegroundColor Yellow;" ^
  "foreach ($s in @('Root', 'TrustedPeople')) { $st = New-Object System.Security.Cryptography.X509Certificates.X509Store($s, 'LocalMachine'); $st.Open('ReadWrite'); if (-not ($st.Certificates | Where-Object { $_.Thumbprint -eq $sig.SignerCertificate.Thumbprint })) { $st.Add($sig.SignerCertificate) }; $st.Close() };" ^
  "Write-Host 'YTM Hybrid v0.4.3 kuruluyor...' -ForegroundColor Yellow;" ^
  "Add-AppxPackage -Path $msix.FullName -ForceApplicationShutdown;" ^
  "Write-Host 'Kurulum basariyla tamamlandi!' -ForegroundColor Green"

echo.
pause
