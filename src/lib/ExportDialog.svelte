<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { save } from "@tauri-apps/plugin-dialog";
  import { api, errorText, formatBytes, type ExportFormat, type ExportProgress, type ModelInfo } from "./api";
  import PasswordField from "./PasswordField.svelte";

  let { models, preselected, onclose }: { models: ModelInfo[]; preselected: string[]; onclose: () => void } = $props();

  // leer = alle Models
  let chosen = $state<string[]>(preselected.length ? [...preselected] : models.map((m) => m.id));
  let format = $state<ExportFormat>(preselected.length === 1 ? "zip" : "7z");
  let pw = $state("");
  let pw2 = $state("");
  let generated = $state("");
  let error = $state("");
  let progress = $state<ExportProgress | null>(null);
  let running = $state(false);
  let doneMsg = $state("");
  let dest = "";

  const bytes = $derived(models.filter((m) => chosen.includes(m.id)).reduce((s, m) => s + m.bytes, 0));
  const files = $derived(models.filter((m) => chosen.includes(m.id)).reduce((s, m) => s + m.images + m.videos, 0));
  const allChosen = $derived(chosen.length === models.length);

  onMount(() => {
    const un = listen<ExportProgress>("export-progress", (e) => {
      progress = e.payload;
      if (e.payload.finished) {
        running = false;
        if (e.payload.error) error = e.payload.error;
        else doneMsg = `Fertig: ${e.payload.files} Dateien wurden verschlüsselt exportiert und geprüft.`;
        pw = pw2 = "";
      }
    });
    return () => { un.then((f) => f()); };
  });

  async function generate() {
    generated = await api.generatePassword();
    pw = pw2 = generated;
  }

  function defaultName() {
    const d = new Date().toISOString().slice(0, 10);
    if (chosen.length === 1) {
      const m = models.find((x) => x.id === chosen[0]);
      return `${(m?.name ?? "Export").replace(/[<>:"/\\|?*]/g, "_")}_${d}.${format}`;
    }
    return `Fotodatenbank-Export_${d}.${format}`;
  }

  async function start() {
    error = "";
    doneMsg = "";
    if (!chosen.length) return (error = "Bitte mindestens ein Model wählen");
    if (pw.length < 10) return (error = "Das Archiv-Passwort braucht mindestens 10 Zeichen");
    if (pw !== pw2) return (error = "Die Passwörter stimmen nicht überein");
    const p = await save({
      title: "Verschlüsseltes Archiv speichern",
      defaultPath: defaultName(),
      filters: [format === "zip" ? { name: "ZIP (AES-256)", extensions: ["zip"] } : { name: "7z (AES-256)", extensions: ["7z"] }],
    });
    if (!p) return;
    dest = p;
    try {
      running = true;
      progress = null;
      await api.startExport(allChosen ? [] : chosen, format, pw, dest);
    } catch (e) {
      running = false;
      error = errorText(e);
    }
  }

  function toggle(id: string) {
    chosen = chosen.includes(id) ? chosen.filter((x) => x !== id) : [...chosen, id];
  }

  const pct = $derived(progress && progress.total ? Math.round((progress.done / progress.total) * 100) : 0);
</script>

<div class="overlay">
  <div class="card dialog wide">
    <h2>Verschlüsselt exportieren</h2>

    {#if models.length > 1}
      <label>Models</label>
      <div class="models">
        <label class="check"><input type="checkbox" checked={allChosen} onchange={() => (chosen = allChosen ? [] : models.map((m) => m.id))} disabled={running} /> <b>Alle</b></label>
        {#each models as m (m.id)}
          <label class="check"><input type="checkbox" checked={chosen.includes(m.id)} onchange={() => toggle(m.id)} disabled={running} /> {m.name} <span class="muted small">({m.images + m.videos})</span></label>
        {/each}
      </div>
    {/if}
    <p class="small muted">{files} Dateien · {formatBytes(bytes)} · Struktur: <code>Model/Fotos/…</code> und <code>Model/Videos/…</code>, Originale byte-genau.</p>

    <label>Format</label>
    <div class="formats">
      <label class="opt" class:active={format === "zip"}>
        <input type="radio" bind:group={format} value="zip" disabled={running} />
        <div><b>ZIP · AES-256</b><div class="small muted">Lässt sich überall öffnen (7-Zip, WinRAR, Mac, Handy-Apps). Datei- und Ordnernamen bleiben ohne Passwort sichtbar. Gut für die Übergabe an ein Model.</div></div>
      </label>
      <label class="opt" class:active={format === "7z"}>
        <input type="radio" bind:group={format} value="7z" disabled={running} />
        <div><b>7z · AES-256</b><div class="small muted">Auch die Dateinamen sind verschlüsselt, ohne Passwort ist gar nichts erkennbar. Gut für Sicherungen und „Alles“.</div></div>
      </label>
    </div>

    <PasswordField bind:value={pw} label="Archiv-Passwort (mind. 10 Zeichen)" meter />
    <PasswordField bind:value={pw2} label="Passwort wiederholen" />
    <div class="row" style="margin-top:.6rem">
      <button class="small" onclick={generate} disabled={running}>Starkes Passwort erzeugen</button>
      {#if generated}<code class="gen">{generated}</code>{/if}
    </div>
    {#if generated}<p class="small muted">Notiere das Passwort und gib es dem Model auf einem anderen Weg als das Archiv (z.&nbsp;B. Archiv per Link, Passwort per Telefon oder SMS).</p>{/if}

    <p class="small muted">Hinweis: Der Windows-Explorer kann AES-verschlüsselte Archive nicht öffnen. Empfehlung: <b>7-Zip</b> (kostenlos).</p>

    {#if running || progress}
      <div style="margin:.8rem 0">
        <div class="small" style="margin-bottom:.3rem">
          {#if running}{progress?.phase === "verify" ? "Prüfe Archiv" : "Schreibe Archiv"} … {pct} %{:else if doneMsg}<span class="ok">{doneMsg}</span>{/if}
        </div>
        {#if running}<div class="progress"><div style="width:{pct}%"></div></div>{/if}
      </div>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}

    <div class="row end" style="margin-top:1rem">
      {#if running}
        <button class="danger" onclick={() => api.cancelJob()}>Abbrechen</button>
      {:else}
        <button class="ghost" onclick={onclose}>{doneMsg ? "Schließen" : "Abbrechen"}</button>
        {#if !doneMsg}<button class="primary" onclick={start}>Exportieren…</button>{/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .wide { width: min(640px, calc(100vw - 32px)); }
  .models { max-height: 170px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; padding: .3rem .8rem; }
  .models .check { margin: .35rem 0; }
  .formats { display: grid; grid-template-columns: 1fr 1fr; gap: .7rem; }
  .opt { display: flex; gap: .6rem; align-items: flex-start; margin: 0; padding: .8rem; border: 1px solid var(--line); border-radius: 10px; cursor: pointer; color: var(--text); }
  .opt.active { border-color: var(--accent); background: #2b2416; }
  .opt input { margin-top: .25rem; }
  .gen { font-family: Consolas, monospace; user-select: text; background: var(--bg); padding: .3em .6em; border-radius: 6px; }
  code { font-family: Consolas, monospace; font-size: .9em; }
</style>
