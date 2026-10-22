<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type AppInfo, type ImportProgress, type ModelInfo, type Status } from "./lib/api";
  import Setup from "./lib/Setup.svelte";
  import Unlock from "./lib/Unlock.svelte";
  import Models from "./lib/Models.svelte";
  import Gallery from "./lib/Gallery.svelte";
  import ExportDialog from "./lib/ExportDialog.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";

  let info = $state<AppInfo | null>(null);
  let status = $state<Status | null>(null);
  let models = $state<ModelInfo[]>([]);
  let current = $state<string | null>(null);
  let reload = $state(0);
  let exportFor = $state<string[] | null>(null);
  let settingsOpen = $state(false);
  let importProgress = $state<ImportProgress | null>(null);
  let toast = $state<{ text: string; error: boolean } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout>;

  const screen = $derived(
    !info || !status ? "loading" : status.unlocked ? "app" : info.vault_found ? "unlock" : "setup"
  );
  const currentModel = $derived(models.find((m) => m.id === current) ?? null);

  function notify(text: string, error = false) {
    toast = { text, error };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), error ? 7000 : 4000);
  }

  async function refresh() {
    try {
      info = await api.appInfo();
      status = await api.status();
      if (status.unlocked) models = await api.listModels();
    } catch (e) {
      notify(errorText(e), true);
    }
  }

  async function dataChanged() {
    status = await api.status();
    if (status.unlocked) models = await api.listModels();
    reload++;
  }

  function locked() {
    current = null;
    exportFor = null;
    settingsOpen = false;
    models = [];
    importProgress = null;
    refresh();
  }

  async function lockNow() {
    if (status?.busy && !(await ask("Ein Import oder Export läuft noch und wird abgebrochen. Trotzdem sperren?", { title: "Sperren", kind: "warning" })))
      return;
    await api.lock();
    locked();
  }

  async function takeOver() {
    const host = status?.holder?.host ?? "dem anderen PC";
    const ok = await ask(
      `Die Sperre von „${host}“ übernehmen?\n\nNur tun, wenn dort die Fotodatenbank wirklich nicht mehr läuft (z. B. nach Absturz oder Stromausfall).`,
      { title: "Schreibzugriff übernehmen", kind: "warning" }
    );
    if (!ok) return;
    try {
      status = await api.takeOverLock();
      dataChanged();
      notify("Schreibzugriff übernommen.");
    } catch (e) {
      notify(errorText(e), true);
    }
  }

  // Bedienung melden (für die automatische Sperre), höchstens alle 20 s.
  let lastTouch = 0;
  function activity() {
    const now = Date.now();
    if (screen === "app" && now - lastTouch > 20000) {
      lastTouch = now;
      api.touch().catch(() => {});
    }
  }

  onMount(() => {
    refresh();
    const subs = [
      listen<string>("vault-locked", (e) => {
        locked();
        notify(e.payload === "windows" ? "Gesperrt, weil Windows gesperrt wurde." : "Automatisch gesperrt.");
      }),
      listen("status-changed", () => dataChanged()),
      listen<ImportProgress>("import-progress", (e) => {
        importProgress = e.payload;
        const p = e.payload;
        if (p.finished) {
          const parts = [`${p.imported} importiert`];
          if (p.duplicates) parts.push(`${p.duplicates} Duplikate übersprungen`);
          if (p.unsupported.length) parts.push(`${p.unsupported.length} nicht unterstützt`);
          if (p.errors.length) parts.push(`${p.errors.length} Fehler`);
          notify((p.cancelled ? "Import abgebrochen: " : "Import fertig: ") + parts.join(", "), p.errors.length > 0);
          if (p.errors.length) console.warn(p.errors);
        }
      }),
    ];
    return () => subs.forEach((s) => s.then((f) => f()));
  });
</script>

<svelte:window onmousemove={activity} onkeydown={activity} onwheel={activity} onmousedown={activity} />

{#if screen === "loading"}
  <div class="loading"><span class="spinner"></span></div>
{:else if screen === "setup"}
  <Setup ondone={refresh} />
{:else if screen === "unlock" && info}
  <Unlock vaultPath={info.settings.vault_path ?? ""} ondone={refresh} onforget={refresh} />
{:else if status}
  <div class="shell">
    <header class="row">
      <button class="ghost brand" onclick={() => (current = null)}>
        <img src="/icon.svg" alt="" width="26" height="26" /> Fotodatenbank
      </button>
      {#if status.read_only}
        <span class="badge warn" title="Ein anderer PC hat gerade Schreibzugriff">
          🔒 Nur lesen{#if status.holder} · wird an „{status.holder.host}“ bearbeitet{/if}
        </span>
        {#if status.holder?.stale}<button class="small" onclick={takeOver}>Schreibzugriff übernehmen…</button>{/if}
      {/if}
      <span class="spacer"></span>
      <button class="ghost" onclick={() => (exportFor = [])} disabled={!models.length}>Alles exportieren…</button>
      <button class="ghost" onclick={() => (settingsOpen = true)}>Einstellungen</button>
      <button onclick={lockNow} title="Tresor sperren">🔒 Sperren</button>
    </header>
    <main>
      {#if currentModel}
        <Gallery
          model={currentModel}
          {models}
          readOnly={status.read_only}
          {reload}
          {importProgress}
          onback={() => (current = null)}
          onchanged={dataChanged}
          onexport={(ids) => (exportFor = ids)}
          onerror={(m) => notify(m, true)}
        />
      {:else}
        <Models
          {models}
          readOnly={status.read_only}
          onopen={(m) => (current = m.id)}
          onchanged={dataChanged}
          onexport={(ids) => (exportFor = ids)}
          onerror={(m) => notify(m, true)}
        />
      {/if}
    </main>
  </div>
{/if}

{#if exportFor !== null}
  <ExportDialog {models} preselected={exportFor} onclose={() => (exportFor = null)} />
{/if}
{#if settingsOpen && info && status}
  <SettingsDialog {info} {status} onclose={() => (settingsOpen = false)} onsaved={refresh} />
{/if}
{#if toast}
  <div class="toast" class:err={toast.error}>{toast.text}</div>
{/if}

<style>
  .loading { height: 100%; display: grid; place-items: center; }
  .shell { height: 100%; display: flex; flex-direction: column; }
  header { padding: .45rem 1rem; border-bottom: 1px solid var(--line); background: var(--bg-2); }
  .brand { display: flex; align-items: center; gap: .5rem; font-weight: 600; }
  main { flex: 1; min-height: 0; }
  .toast {
    position: fixed; bottom: 22px; left: 50%; transform: translateX(-50%); z-index: 200;
    background: var(--bg-3); border: 1px solid var(--line); border-radius: 10px; padding: .7rem 1.2rem;
    box-shadow: 0 8px 30px #0008; max-width: 70vw;
  }
  .toast.err { border-color: var(--danger); color: #ffd9d4; }
</style>
