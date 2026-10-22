<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { api, errorText, formatBytes, type AppInfo, type Status } from "./api";
  import PasswordField from "./PasswordField.svelte";

  let { info, status, onclose, onsaved }: { info: AppInfo; status: Status; onclose: () => void; onsaved: () => void } = $props();

  let autoLock = $state(info.settings.auto_lock_minutes);
  let protect = $state(info.settings.content_protection);
  let winLock = $state(info.settings.lock_on_windows_lock);

  let version = $state("");
  getVersion().then((v) => (version = v));
  let update = $state<Update | null>(null);
  let upd = $state<"idle" | "checking" | "none" | "found" | "downloading" | "installing" | "error">("idle");
  let updError = $state("");
  let loaded = $state(0);
  let size = $state(0);

  async function checkUpdate() {
    upd = "checking";
    updError = "";
    try {
      update = await check();
      upd = update ? "found" : "none";
    } catch (e) {
      upd = "error";
      updError = errorText(e);
    }
  }

  async function installUpdate() {
    if (!update) return;
    if (status.busy) return (updError = "Bitte warte, bis Import oder Export fertig sind.");
    upd = "downloading";
    loaded = size = 0;
    try {
      // Download wird vor der Installation per Signatur geprüft.
      await update.download((ev) => {
        if (ev.event === "Started") size = ev.data.contentLength ?? 0;
        else if (ev.event === "Progress") loaded += ev.data.chunkLength;
      });
      upd = "installing";
      // Vor der Installation sperren: Schlüssel löschen und Schreibsperre freigeben.
      await api.lock();
      await update.install();
    } catch (e) {
      upd = "error";
      updError = errorText(e);
    }
  }
  let oldPw = $state("");
  let newPw = $state("");
  let newPw2 = $state("");
  let msg = $state("");
  let error = $state("");
  let busy = $state(false);

  async function saveSettings() {
    error = msg = "";
    try {
      await api.saveSettings(Number(autoLock), protect, winLock);
      msg = "Einstellungen gespeichert.";
      onsaved();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function changePw() {
    error = msg = "";
    if (newPw.length < 10) return (error = "Das neue Passwort braucht mindestens 10 Zeichen");
    if (newPw !== newPw2) return (error = "Die neuen Passwörter stimmen nicht überein");
    busy = true;
    try {
      await api.changePassword(oldPw, newPw);
      oldPw = newPw = newPw2 = "";
      msg = "Passwort geändert. Es gilt ab sofort an allen PCs.";
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="overlay">
  <div class="card dialog wide">
    <div class="row between"><h2 style="margin:0">Einstellungen</h2><button class="ghost" onclick={onclose}>✕</button></div>

    <h3 class="sec">Sicherheit</h3>
    <label>Automatisch sperren nach</label>
    <select bind:value={autoLock}>
      {#each [1, 5, 10, 15, 30, 60] as m}<option value={m}>{m} Minute{m === 1 ? "" : "n"} ohne Bedienung</option>{/each}
      <option value={0}>Nie (nicht empfohlen)</option>
    </select>
    <label class="check" style="margin-top:.9rem"><input type="checkbox" bind:checked={winLock} /> Sperren, wenn Windows gesperrt wird (Win+L) oder in den Standby geht</label>
    <label class="check" style="margin-top:.5rem"><input type="checkbox" bind:checked={protect} /> Fenster vor Screenshots und Bildschirmübertragung schützen</label>
    <div class="row end" style="margin-top:.8rem"><button onclick={saveSettings}>Speichern</button></div>

    <h3 class="sec">Passwort ändern</h3>
    {#if status.read_only}
      <p class="small muted">Nur möglich, solange dieser PC Schreibzugriff hat.</p>
    {:else}
      <PasswordField bind:value={oldPw} label="Aktuelles Passwort" />
      <PasswordField bind:value={newPw} label="Neues Passwort" meter />
      <PasswordField bind:value={newPw2} label="Neues Passwort wiederholen" onenter={changePw} />
      <p class="small muted">Der Wiederherstellungsschlüssel bleibt gültig.</p>
      <div class="row end"><button onclick={changePw} disabled={busy || !oldPw}>{#if busy}<span class="spinner"></span>{/if} Passwort ändern</button></div>
    {/if}

    {#if msg}<p class="ok">{msg}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}

    <h3 class="sec">Updates</h3>
    <div class="row">
      <span>Installierte Version: <b>{version}</b></span>
      <span class="spacer"></span>
      {#if upd !== "downloading" && upd !== "installing"}
        <button onclick={checkUpdate} disabled={upd === "checking"}>{#if upd === "checking"}<span class="spinner"></span>{/if} Nach Updates suchen</button>
      {/if}
    </div>
    {#if upd === "none"}<p class="small ok">Du hast die neueste Version.</p>{/if}
    {#if upd === "found" && update}
      <div class="update">
        <b>Version {update.version} ist verfügbar.</b>
        {#if update.body}<p class="small notes">{update.body}</p>{/if}
        <p class="small muted">Der Tresor wird für die Installation gesperrt. Danach startet die App neu und du entsperrst wie gewohnt. Deine Daten bleiben unverändert.</p>
        <div class="row end"><button class="primary" onclick={installUpdate}>Jetzt installieren</button></div>
      </div>
    {/if}
    {#if upd === "downloading"}
      <p class="small">Lade herunter… {size ? `${formatBytes(loaded)} / ${formatBytes(size)}` : formatBytes(loaded)}</p>
      <div class="progress"><div style="width:{size ? (loaded / size) * 100 : 0}%"></div></div>
    {/if}
    {#if upd === "installing"}<p class="small"><span class="spinner"></span> Installiere… die App wird gleich neu gestartet.</p>{/if}
    {#if updError}<p class="small error">Update fehlgeschlagen: {updError}</p>{/if}
    <p class="small muted">Gesucht wird nur, wenn du den Knopf drückst. Updates sind signiert; eine manipulierte Datei wird abgelehnt.</p>

    <h3 class="sec">Info</h3>
    <div class="small kv">
      <span class="muted">Tresor</span><span class="sel">{status.vault_path}</span>
      <span class="muted">Dieser PC</span><span>{info.hostname}</span>
      <span class="muted">Zugriff</span><span>{status.read_only ? "nur lesend" : "lesen und schreiben"}</span>
      <span class="muted">Video-Vorschau</span><span>{info.ffmpeg ? "ffmpeg gefunden ✓" : "ffmpeg fehlt, Videos werden ohne Vorschaubild importiert"}</span>
    </div>
  </div>
</div>

<style>
  .wide { width: min(600px, calc(100vw - 32px)); }
  .sec { margin-top: 1.6rem; font-size: 1rem; color: var(--accent); }
  .kv { display: grid; grid-template-columns: 130px 1fr; gap: .4rem .8rem; }
  .sel { user-select: text; word-break: break-all; }
  .update { margin-top: .7rem; padding: .8rem 1rem; border: 1px solid #5a4720; background: #2b2416; border-radius: 10px; }
  .notes { white-space: pre-wrap; }
</style>
