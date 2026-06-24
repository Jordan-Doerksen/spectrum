# Spectrum dev wrapper — prepends the toolchain dirs the GNU build needs
# (DECISIONS D-0001/D-0002), then runs cargo with whatever args you pass.
#
#   .\scripts\dev.ps1 run -p spectrum-headless
#   .\scripts\dev.ps1 build --workspace
#   .\scripts\dev.ps1 clippy --workspace -- -D warnings
#
# Without the winlibs\mingw64\bin prepend, the build dies at `dlltool.exe: program not found`.
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\winlibs\mingw64\bin;$env:PATH"
& cargo @args
