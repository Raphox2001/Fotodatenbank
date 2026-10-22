<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, errorText } from "./api";
  import PasswordField from "./PasswordField.svelte";

  let { ondone }: { ondone: () => void } = $props();

  let mode = $state<"choose" | "create" | "open">("choose");
  let folder = $state("");
  let pw = $state("");
  let pw2 = $state("");
  let busy = $state(false);
  let error = $state("");
  let recoveryKey = $state("");
  let confirmed = $state(false);

  async function pick() {
    const p = await open({ directory: true, title: mode === "create" ? "Ordner für den neuen Tresor" : "Tresor-Ordner wählen" });
    if (typeof p === "string") folder = p;
  }

  async function create() {
    error = "";
    if (!folder) return (error = "Bitte einen Ordner wählen");
    if (pw.length < 10) return (error = "Bitte mindestens 10 Zeichen verwenden");
    if (pw !== pw2) return (error = "Die Passwörter stimmen nicht überein");
    busy = true;
    try {
      const r = await api.createVault(folder, pw);
      recoveryKey = r.recovery_key;
      folder = r.vault_path;
      pw = pw2 = "";
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function openExisting() {
    error = "";
    if (!folder) return (error = "Bitte den Tresor-Ordner wählen");
    busy = true;
    try {
      await api.unlock(folder, pw);
      pw = "";
      ondone();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap">
  {#if recoveryKey}
    <div class="card dialog stack">
      <h1>Tresor angelegt ✓</h1>
      <p>
        Das ist dein <b>Wiederherstellungsschlüssel</b>. Wenn du dein Passwort vergisst, kommst du nur damit wieder an deine
        Bilder. <b>Ohne Passwort und ohne diesen Schlüssel sind die Daten unwiederbringlich verloren.</b>
      </p>
      <div class="printable">
        <h2 class="print-only">Fotodatenbank – Wiederherstellungsschlüssel</h2>
        <div class="recovery-key">{recoveryKey}</div>
        <p class="small muted">Tresor: {folder}</p>
      </div>
      <p class="small muted">Drucke ihn aus oder schreibe ihn ab und bewahre ihn getrennt vom PC auf (z. B. im Tresor). Speichere ihn nicht unverschlüsselt am PC.</p>
      <div class="row">
        <button onclick={() => window.print()}>Drucken…</button>
        <span class="spacer"></span>
      </div>
      <label class="check"><input type="checkbox" bind:checked={confirmed} /> Ich habe den Schlüssel sicher aufbewahrt.</label>
      <div class="row end">
        <button class="primary" disabled={!confirmed} onclick={() => { recoveryKey = ""; ondone(); }}>Weiter zur Fotodatenbank</button>
      </div>
    </div>
  {:else if mode === "choose"}
    <div class="intro">
      <img src="/icon.svg" alt="" width="84" height="84" />
      <h1>Willkommen bei der Fotodatenbank</h1>
      <p class="muted">Alle Bilder, Videos und Model-Namen werden verschlüsselt gespeichert, lokal oder auf deiner NAS.</p>
    </div>
    <div class="choices">
      <button class="card choice" onclick={() => { mode = "create"; folder = ""; error = ""; }}>
        <h2>Neuen Tresor anlegen</h2>
        <p class="muted">Beim ersten Mal: Speicherort wählen (z.&nbsp;B. ein Ordner auf der NAS) und ein Passwort festlegen.</p>
      </button>
      <button class="card choice" onclick={() => { mode = "open"; folder = ""; error = ""; }}>
        <h2>Vorhandenen Tresor öffnen</h2>
        <p class="muted">Auf einem weiteren PC: denselben Tresor-Ordner auf der NAS wählen und dasselbe Passwort eingeben.</p>
      </button>
    </div>
  {:else}
    <div class="card dialog">
      <h2>{mode === "create" ? "Neuen Tresor anlegen" : "Vorhandenen Tresor öffnen"}</h2>
      <label>Speicherort</label>
      <div class="row">
        <input type="text" bind:value={folder} placeholder={mode === "create" ? "z. B. \\\\DiskStation\\Fotos oder D:\\Fotodatenbank" : "Ordner mit vault.json"} />
        <button onclick={pick}>Durchsuchen…</button>
      </div>
      {#if mode === "create"}
        <p class="small muted">Ist der Ordner nicht leer, wird darin ein Unterordner „Fotodatenbank-Tresor“ angelegt.</p>
        <PasswordField bind:value={pw} label="Passwort (mind. 10 Zeichen, besser ein ganzer Satz)" meter />
        <PasswordField bind:value={pw2} label="Passwort wiederholen" onenter={create} />
      {:else}
        <PasswordField bind:value={pw} onenter={openExisting} />
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
      <div class="row end" style="margin-top:1.2rem">
        <button class="ghost" onclick={() => (mode = "choose")} disabled={busy}>Zurück</button>
        {#if mode === "create"}
          <button class="primary" onclick={create} disabled={busy}>{#if busy}<span class="spinner"></span>{/if} Tresor anlegen</button>
        {:else}
          <button class="primary" onclick={openExisting} disabled={busy}>{#if busy}<span class="spinner"></span>{/if} Öffnen</button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .wrap { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2rem; padding: 2rem; overflow: auto; }
  .intro { text-align: center; max-width: 560px; }
  .choices { display: grid; grid-template-columns: repeat(2, minmax(0, 320px)); gap: 1.2rem; }
  .choice { text-align: left; padding: 1.6rem; }
  .choice:hover { border-color: var(--accent) !important; }
  .print-only { display: none; }
  @media print { .print-only { display: block; } }
</style>
