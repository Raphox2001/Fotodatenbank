# Fotodatenbank

Verschlüsselte Foto- und Video-Datenbank für Model-Shootings: eine Windows-Desktop-App (Tauri 2 + Svelte 5, Kryptografie in Rust).

## Sicherheitskonzept

- **Alles verschlüsselt:** Originale, Vorschaubilder und der Index (Model-Namen, Dateinamen) liegen nur verschlüsselt im Tresor-Ordner, mit Zufalls-Dateinamen. Der Ordner kann lokal oder auf einer NAS liegen.
- **Passwort:** Aus dem Passwort wird mit Argon2id (256 MiB, 3 Durchläufe) ein Schlüssel abgeleitet, der einen zufälligen 256-bit-Master-Key verpackt. Ein Wiederherstellungsschlüssel verpackt ihn ein zweites Mal.
- **Dateien:** Sie werden blockweise mit XChaCha20-Poly1305 verschlüsselt. Jeder Block ist authentifiziert, sodass Manipulation, Vertauschen und Abschneiden erkannt werden.
- **Klartext nur im RAM:** keine temporären Dateien, WebView im InPrivate-Modus, Antworten mit `no-store`.
- **Sperren:** Die App sperrt automatisch bei Inaktivität, bei Win+L und bei Standby.
- **Mehrere PCs:** Nur ein PC darf schreiben. Das regelt eine verschlüsselte Sperrdatei mit Heartbeat, alle anderen PCs öffnen nur lesend.
- **Export:** nur als verschlüsseltes Archiv (ZIP AES-256 oder 7z AES-256 mit verschlüsselten Dateinamen), danach per SHA-256 geprüft.
- **Updates:** Gesucht wird nur auf Knopfdruck. Jedes Update ist signiert (minisign/Ed25519) und wird vor der Installation geprüft.

Die Sicherheit hängt nur am Passwort, nicht an der Geheimhaltung dieses Quellcodes.

## Entwickeln

Voraussetzungen: Rust (rustup), Node.js, Visual Studio Build Tools (C++), ffmpeg/ffprobe im `PATH`.

```powershell
npm install
npx tauri dev                       # App im Entwicklungsmodus
cd src-tauri; cargo test --release  # Tests
```

## Neue Version veröffentlichen

```powershell
.\scripts\release.ps1 -Version 0.3.0 -Notes "Was ist neu"
```

Das Skript setzt die Version, baut den signierten Installer, erstellt `latest.json` und legt ein GitHub-Release an. Benötigt werden `gh auth login` und der Signaturschlüssel unter `%USERPROFILE%\.tauri\fotodatenbank.key`. Diesen Schlüssel unbedingt sichern: Ohne ihn können installierte Apps keine Updates mehr erhalten.

## Drittanbieter

Der Installer enthält [FFmpeg](https://ffmpeg.org) (GPL, Build von [gyan.dev](https://www.gyan.dev/ffmpeg/builds/)) für Video-Vorschaubilder. Den Quellcode gibt es unter <https://ffmpeg.org/download.html>.
