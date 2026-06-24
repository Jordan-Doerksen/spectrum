# Assemble the Spectrum-for-Riley package: release exe + DLLs + config (with webhooks,
# a private deploy) + launchers, zipped to Downloads. Run AFTER `cargo build --release -p spectrum-pro`.

$root    = "C:\projects\spectrum"
$rel     = Join-Path $root "target\release"
$exe     = Join-Path $rel  "spectrum-pro.exe"
$winlibs = Join-Path $env:USERPROFILE "winlibs\mingw64\bin"
$stage   = Join-Path $env:TEMP "Spectrum-for-Riley"
$out     = Join-Path $env:USERPROFILE "Downloads\Spectrum-for-Riley.zip"

if (-not (Test-Path $exe)) { Write-Error "release exe not found - build it first"; exit 1 }

# fresh staging dir
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null

# 1) the app
Copy-Item $exe (Join-Path $stage "spectrum-pro.exe")

# 2) any sidecar DLLs cargo/Tauri dropped next to the exe (e.g. WebView2Loader.dll)
Get-ChildItem $rel -Filter *.dll -ErrorAction SilentlyContinue | ForEach-Object {
  Copy-Item $_.FullName (Join-Path $stage $_.Name)
}

# 3) (No MinGW runtime DLLs needed - objdump confirmed the GNU build statically links
#     libgcc/winpthread and there's no C++; only WebView2Loader.dll + system DLLs.)

# 4) config WITH webhooks (private deploy - keep it), launchers, the guide
Copy-Item (Join-Path $root "config.local.json") (Join-Path $stage "config.local.json")
Copy-Item (Join-Path $root "deploy\*.bat") $stage
Copy-Item (Join-Path $root "deploy\RILEY.md") $stage

# zip it (no pre-seeded data/ - Riley's first run seeds his own backlog)
if (Test-Path $out) { Remove-Item $out -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $out

Write-Output "Packaged -> $out"
Get-ChildItem $stage | Select-Object Name, @{n='KB';e={[math]::Round($_.Length/1KB,1)}}
Get-Item $out | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,1)}}
