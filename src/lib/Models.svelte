<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText, formatBytes, thumbUrl, type ModelInfo } from "./api";
  import InputDialog from "./InputDialog.svelte";
  import CoverPicker from "./CoverPicker.svelte";

  let {
    models,
    readOnly,
    onopen,
    onchanged,
    onexport,
    onerror,
  }: {
    models: ModelInfo[];
    readOnly: boolean;
    onopen: (m: ModelInfo) => void;
    onchanged: () => void;
    onexport: (ids: string[]) => void;
    onerror: (msg: string) => void;
  } = $props();

  let query = $state("");
  let creating = $state(false);
  let renaming = $state<ModelInfo | null>(null);
  let coverFor = $state<ModelInfo | null>(null);
  let menuFor = $state<string | null>(null);

  const filtered = $derived(
    models.filter((m) => m.name.toLowerCase().includes(query.trim().toLowerCase()))
  );

  async function create(name: string) {
    creating = false;
    try {
      const m = await api.createModel(name);
      onchanged();
      onopen(m);
    } catch (e) {
      onerror(errorText(e));
    }
  }

  async function rename(name: string) {
    const m = renaming!;
    renaming = null;
    try {
      await api.renameModel(m.id, name);
      onchanged();
    } catch (e) {
      onerror(errorText(e));
    }
  }

  async function remove(m: ModelInfo) {
    menuFor = null;
    const n = m.images + m.videos;
    const ok = await ask(
      `„${m.name}“ mit ${n} Datei${n === 1 ? "" : "en"} endgültig aus dem Tresor löschen?\n\nDas kann nicht rückgängig gemacht werden.`,
      { title: "Model löschen", kind: "warning", okLabel: "Endgültig löschen", cancelLabel: "Abbrechen" }
    );
    if (!ok) return;
    try {
      await api.deleteModel(m.id);
      onchanged();
    } catch (e) {
      onerror(errorText(e));
    }
  }
</script>

<svelte:window onclick={() => (menuFor = null)} />

<div class="page">
  <div class="toolbar row">
    <h1 style="margin:0">Models</h1>
    <span class="muted">{models.length}</span>
    <span class="spacer"></span>
    <input type="search" placeholder="Suchen…" bind:value={query} style="width:240px" />
    <button class="primary" disabled={readOnly} onclick={() => (creating = true)}>+ Neues Model</button>
  </div>

  {#if models.length === 0}
    <div class="empty">
      <h2>Noch keine Models</h2>
      <p class="muted">Lege ein Model an und ziehe danach Fotos und Videos in die Galerie.</p>
      <button class="primary" disabled={readOnly} onclick={() => (creating = true)}>+ Erstes Model anlegen</button>
    </div>
  {:else}
    <div class="grid">
      {#each filtered as m (m.id)}
        <div class="model">
          <button class="cover" onclick={() => onopen(m)}>
            {#if m.cover}
              <img src={thumbUrl(m.cover)} alt="" loading="lazy" draggable="false" />
            {:else}
              <div class="placeholder">{m.name.slice(0, 1).toUpperCase()}</div>
            {/if}
          </button>
          <div class="meta row">
            <div class="text">
              <div class="name" title={m.name}>{m.name}</div>
              <div class="small muted">
                {m.images} Foto{m.images === 1 ? "" : "s"}{#if m.videos} · {m.videos} Video{m.videos === 1 ? "" : "s"}{/if} · {formatBytes(m.bytes)}
              </div>
            </div>
            <div class="menu-wrap">
              <button class="ghost dots" title="Mehr" onclick={(e) => { e.stopPropagation(); menuFor = menuFor === m.id ? null : m.id; }}>⋯</button>
              {#if menuFor === m.id}
                <div class="menu card">
                  <button class="ghost" disabled={readOnly || m.images + m.videos === 0} onclick={() => (coverFor = m)}>Titelbild wählen…</button>
                  <button class="ghost" onclick={() => onexport([m.id])}>Verschlüsselt exportieren…</button>
                  <button class="ghost" disabled={readOnly} onclick={() => (renaming = m)}>Umbenennen…</button>
                  <button class="ghost danger" disabled={readOnly} onclick={() => remove(m)}>Löschen…</button>
                </div>
              {/if}
            </div>
          </div>
        </div>
      {/each}
    </div>
    {#if filtered.length === 0}<p class="muted" style="padding:0 1.5rem">Kein Model gefunden.</p>{/if}
  {/if}
</div>

{#if creating}
  <InputDialog title="Neues Model" confirm="Anlegen" onsubmit={create} oncancel={() => (creating = false)} />
{/if}
{#if coverFor}
  <CoverPicker model={coverFor} onclose={() => (coverFor = null)} onpicked={onchanged} />
{/if}
{#if renaming}
  <InputDialog title="Model umbenennen" value={renaming.name} confirm="Speichern" onsubmit={rename} oncancel={() => (renaming = null)} />
{/if}

<style>
  .page { height: 100%; overflow: auto; }
  .toolbar { padding: 1.2rem 1.5rem; position: sticky; top: 0; background: var(--bg); z-index: 2; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 1.2rem; padding: 0 1.5rem 2rem; }
  .model { background: var(--bg-2); border: 1px solid var(--line); border-radius: 12px; overflow: visible; }
  .cover { display: block; width: 100%; aspect-ratio: 4 / 5; padding: 0; border: none; border-radius: 12px 12px 0 0; overflow: hidden; background: var(--bg-3); }
  .cover img { width: 100%; height: 100%; object-fit: cover; display: block; transition: transform .3s; }
  .cover:hover img { transform: scale(1.04); }
  .placeholder { width: 100%; height: 100%; display: grid; place-items: center; font-size: 3.5rem; color: #4a4f5c; }
  .meta { padding: .7rem .5rem .8rem .9rem; }
  .text { flex: 1; min-width: 0; }
  .name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .menu-wrap { position: relative; }
  .dots { font-size: 1.2rem; padding: .1em .5em; }
  .menu { position: absolute; right: 0; bottom: 110%; padding: .4rem; z-index: 5; display: flex; flex-direction: column; min-width: 230px; }
  .menu button { text-align: left; }
  .empty { text-align: center; margin-top: 18vh; }
</style>
