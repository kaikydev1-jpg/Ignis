# Copia as DLLs do Visual C++ (redistribuíveis) para app/src-tauri/recursos/vcrt,
# de onde o instalador do Windows as leva junto. Roda no CI (runner Windows).
$ErrorActionPreference = "Stop"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -property installationPath
$crt = Get-ChildItem "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT" -Directory |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $crt) { throw "Pasta Redist do Visual C++ não encontrada em $vs" }
$dest = Join-Path $PSScriptRoot "..\app\src-tauri\recursos\vcrt"
New-Item -ItemType Directory -Force $dest | Out-Null
foreach ($dll in "msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll") {
  Copy-Item (Join-Path $crt.FullName $dll) $dest -Force
  Write-Host "copiado $dll de $($crt.FullName)"
}
