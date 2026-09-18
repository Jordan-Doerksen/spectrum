# Spectrum dev wrapper — prepends the toolchain dirs the GNU build needs
# (DECISIONS D-0001/D-0002), then runs cargo with whatever args you pass.
#
#   .\scripts\dev.ps1 check --workspace
#   .\scripts\dev.ps1 build --workspace
#   .\scripts\dev.ps1 test --workspace
#   .\scripts\dev.ps1 clippy --workspace -- -D warnings
#   .\scripts\dev.ps1 test -p spectrum-engine --no-run -- --list
#
# ⚑ DO NOT run the engine on this machine. `run -p spectrum-headless` and `run -p
#   spectrum-pro` poll live feeds, read a stale data\seen.json of ~2281 keys through
#   Ollama and then post to the LIVE Discord channels. check / build / test only.
#   [CR-1 chunk 0, hard limit · DECISIONS.md "Definition of Done — chunk 0", item 8]
#
# Without the winlibs\mingw64\bin prepend, the build dies at `dlltool.exe: program not found`.
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\winlibs\mingw64\bin;$env:PATH"

# ---------------------------------------------------------------- the `--` separator
# PowerShell strips a bare `--` from a script's arguments before the script ever sees
# them: `$args` for `dev.ps1 clippy --workspace -- -D warnings` is
# `clippy --workspace -D warnings`, and cargo rejects the orphaned flags. (Verified on
# PowerShell 7.6.6, 2026-09-18. Splatting is NOT the problem — an array element `--`
# forwards to cargo intact.)
#
# So the separator is put back here, from the raw command line, and only when the
# reconstruction is provably right: the tokens after `--` on that line must equal the
# tail of `$args` exactly. If they do not — a quoted argument holding spaces, a call
# through a variable, several statements on one line — nothing is inserted and the
# arguments pass through untouched, because a wrapper that guesses wrong would move a
# flag from one program to another in silence.
#
# Quoting the separator ('--') sidesteps all of this: it reaches `$args` as itself.
$cargoArgs = @($args)
if ($cargoArgs -notcontains '--') {
    $raw = [string]$MyInvocation.Line
    $me = [string]$MyInvocation.InvocationName
    $at = if ($raw -and $me) { $raw.LastIndexOf($me) } else { -1 }
    if ($at -ge 0) {
        $tokens = @(($raw.Substring($at + $me.Length) -split '\s+') |
            Where-Object { $_ } |
            ForEach-Object { $_.Trim("'", '"') })
        $sep = [array]::IndexOf($tokens, '--')
        if ($sep -ge 0) {
            $tail = @($tokens | Select-Object -Skip ($sep + 1))
            $split = $cargoArgs.Count - $tail.Count
            $same = $tail.Count -gt 0 -and $split -ge 0
            for ($i = 0; $same -and $i -lt $tail.Count; $i++) {
                if ($tail[$i] -ne [string]$cargoArgs[$split + $i]) { $same = $false }
            }
            if ($same) {
                $head = if ($split -gt 0) { $cargoArgs[0..($split - 1)] } else { @() }
                $cargoArgs = @($head) + @('--') + @($tail)
            }
        }
    }
}

& cargo @cargoArgs
exit $LASTEXITCODE
