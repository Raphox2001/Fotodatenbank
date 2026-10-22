<#
  Veröffentlicht eine neue Version der Fotodatenbank auf GitHub.
  Installierte Apps finden sie danach über Einstellungen -> "Nach Updates suchen".

  Aufruf (im Projektordner):
    .\scripts\release.ps1 -Version 0.3.0 -Notes "Was ist neu"

  Voraussetzungen:
    - gh auth login  (einmalig)
    - Signaturschlüssel unter %USERPROFILE%\.tauri\fotodatenbank.key
      (ohne diesen Schlüssel können keine Updates mehr veröffentlicht werden -> sichern!)
#>
param(
  [Parameter(Mandatory = $true)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
  [string]$Notes = ""
)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

$repo = "Raphox2001/Fotodatenbank"
$key = Join-Path $env:USERPROFILE ".tauri\fotodatenbank.key"
if (-not (Test-Path $key)) { throw "Signaturschlüssel fehlt: $key" }
if (git status --porcelain) { throw "Es gibt nicht eingecheckte Änderungen. Bitte zuerst committen." }
gh auth status | Out-Null
if ($LASTEXITCODE) { throw "Bitte zuerst 'gh auth login' ausführen." }

# ffmpeg/ffprobe werden mit dem Installer ausgeliefert (nicht im Repo).
New-Item -ItemType Directory -Force "src-tauri\bin" | Out-Null
foreach ($tool in "ffmpeg", "ffprobe") {
  $dest = "src-tauri\bin\$tool.exe"
  if (-not (Test-Path $dest)) { Copy-Item (Get-Command $tool).Source $dest }
}

node scripts/set-version.mjs $Version
if ($LASTEXITCODE) { throw "Version konnte nicht gesetzt werden" }

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_INCREMENTAL = "0"
# Der Schlüssel hat kein Passwort (mit "tauri signer generate --ci" erzeugt).
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $key -Raw
npx tauri build
if ($LASTEXITCODE) { throw "Build fehlgeschlagen" }

$dir = "src-tauri\target\release\bundle\nsis"
$exe = "Fotodatenbank_${Version}_x64-setup.exe"
if (-not (Test-Path "$dir\$exe.sig")) { throw "Signatur $exe.sig fehlt" }

$latest = [ordered]@{
  version   = $Version
  notes     = $Notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = (Get-Content "$dir\$exe.sig" -Raw).Trim()
      url       = "https://github.com/$repo/releases/download/v$Version/$exe"
    }
  }
}
[IO.File]::WriteAllText((Join-Path (Get-Location) "$dir\latest.json"), ($latest | ConvertTo-Json -Depth 5))

git add -A
git commit -m "Version $Version"
git tag "v$Version"
git push origin HEAD
git push origin "v$Version"

$notesText = if ($Notes) { $Notes } else { "Version $Version" }
gh release create "v$Version" "$dir\$exe" "$dir\$exe.sig" "$dir\latest.json" --repo $repo --title "Fotodatenbank $Version" --notes $notesText
if ($LASTEXITCODE) { throw "GitHub-Release fehlgeschlagen" }
Write-Host "Fertig: Version $Version ist veröffentlicht." -ForegroundColor Green
