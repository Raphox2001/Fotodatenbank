<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import {
    api, errorText, formatBytes, formatDuration, thumbUrl,
    type ImportProgress, type Media, type ModelInfo,
  } from "./api";
  import Lightbox from "./Lightbox.svelte";

  let {
    model,
    models,
    readOnly,
    reload,
    importProgress,
    onback,
    onchanged,
    onexport,
    onerror,
  }: {
    model: ModelInfo;
    models: ModelInfo[];
    readOnly: boolean;
    reload: number;
    importProgress: ImportProgress | null;
    onback: () => void;
    onchanged: () => void;
    onexport: (ids: string[]) => void;
    onerror: (msg: string) => void;
  } = $props();

  const GAP = 6;
  let media = $state<Media[]>([]);
  let filter = $state<"all" | "image" | "video">("all");
  let sort = $state<"taken" | "imported" | "name">("taken");
  let desc = $state(false);
  let tile = $state(200);
  let scrollTop = $state(0);
  let width = $state(800);
  let height = $state(600);
  let dragging = $state(false);
  let lightbox = $state<number | null>(null);
  let moveTarget = $state("");
  const selected = new SvelteSet<string>();
  let lastClicked = -1;

  async function load() {
    try {
      media = await api.listMedia(model.id);
    } catch (e) {
      onerror(errorText(e));
    }
  }

  $effect(() => {
    reload;
    model.id;
    load();
  });

  const items = $derived.by(() => {
    const list = media.filter((m) => filter === "all" || m.kind === filter);
    const key = (m: Media) =>
      sort === "name" ? m.file_name.toLowerCase() : sort === "imported" ? String(m.imported_at).padStart(12, "0") : (m.taken_at ?? "~") + m.file_name;
    list.sort((a, b) => (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0));
    if (desc) list.reverse();
    return list;
  });

  const counts = $derived({
    image: media.filter((m) => m.kind === "image").length,
    video: media.filter((m) => m.kind === "video").length,
  });

  // Virtuelles Raster: nur sichtbare Kacheln werden gerendert.
  const cols = $derived(Math.max(1, Math.floor((width + GAP) / (tile + GAP))));
  const size = $derived((width - GAP * (cols - 1)) / cols);
  const rowH = $derived(size + GAP);
  const totalH = $derived(Math.ceil(items.length / cols) * rowH);
  const first = $derived(Math.max(0, Math.floor(scrollTop / rowH) - 2) * cols);
  const last = $derived(Math.min(items.length, (Math.ceil((scrollTop + height) / rowH) + 2) * cols));
  const visible = $derived(items.slice(first, last).map((m, i) => ({ m, i: first + i })));

  const importing = $derived(importProgress && !importProgress.finished && importProgress.model_id === model.id);

  onMount(() => {
    const un = getCurrentWebview().onDragDropEvent((ev) => {
      const t = ev.payload.type;
      if (t === "enter" || t === "over") dragging = !readOnly;
      else if (t === "leave") dragging = false;
      else if (t === "drop") {
        dragging = false;
        if (!readOnly && ev.payload.paths.length) startImport(ev.payload.paths);
      }
    });
    return () => { un.then((f) => f()); };
  });

  async function startImport(paths: string[]) {
    try {
      await api.startImport(model.id, paths);
    } catch (e) {
      onerror(errorText(e));
    }
  }

  async function pickFiles() {
    const p = await open({
      multiple: true,
      title: "Fotos und Videos hinzufügen",
      filters: [{ name: "Fotos & Videos", extensions: ["jpg", "jpeg", "png", "tif", "tiff", "webp", "gif", "bmp", "mp4", "mov", "m4v", "webm", "mkv", "avi"] }],
    });
    if (Array.isArray(p) && p.length) startImport(p);
  }

  async function pickFolder() {
    const p = await open({ directory: true, title: "Ordner hinzufügen (inkl. Unterordner)" });
    if (typeof p === "string") startImport([p]);
  }

  function click(e: MouseEvent, i: number, m: Media) {
    if (e.shiftKey && lastClicked >= 0) {
      const [a, b] = [Math.min(lastClicked, i), Math.max(lastClicked, i)];
      for (let k = a; k <= b; k++) selected.add(items[k].id);
    } else if (e.ctrlKey || selected.size > 0) {
      selected.has(m.id) ? selected.delete(m.id) : selected.add(m.id);
      lastClicked = i;
    } else {
      lightbox = i;
    }
  }

  function toggle(e: MouseEvent, i: number, m: Media) {
    e.stopPropagation();
    selected.has(m.id) ? selected.delete(m.id) : selected.add(m.id);
    lastClicked = i;
  }

  async function deleteSelected() {
    const n = selected.size;
    const ok = await ask(`${n} Datei${n === 1 ? "" : "en"} endgültig aus dem Tresor löschen?`, {
      title: "Löschen", kind: "warning", okLabel: "Endgültig löschen", cancelLabel: "Abbrechen",
    });
    if (!ok) return;
    try {
      await api.deleteMedia([...selected]);
      selected.clear();
      onchanged();
    } catch (e) {
      onerror(errorText(e));
    }
  }

  async function moveSelected() {
    if (!moveTarget) return;
    try {
      await api.moveMedia([...selected], moveTarget);
      selected.clear();
      moveTarget = "";
      onchanged();
    } catch (e) {
      onerror(errorText(e));
    }
  }

  async function setCover(id: string) {
    try {
      await api.setCover(model.id, id);
      selected.clear();
      onchanged();
    } catch (e) {
      onerror(errorText(e));
    }
  }

  function keydown(e: KeyboardEvent) {
    if (lightbox !== null || (e.target as HTMLElement).closest("input, select, textarea")) return;
    if (e.key === "a" && e.ctrlKey) {
      e.preventDefault();
      items.forEach((m) => selected.add(m.id));
    } else if (e.key === "Escape") {
      if (selected.size) selected.clear();
      else onback();
    } else if (e.key === "Delete" && selected.size && !readOnly) {
      deleteSelected();
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="page">
  <div class="toolbar">
    <div class="row">
      <button class="ghost" onclick={onback} title="Zurück (Esc)">← Models</button>
      <h1 class="title" title={model.name}>{model.name}</h1>
      <span class="muted small">{counts.image} Fotos · {counts.video} Videos</span>
      <span class="spacer"></span>
      <button onclick={() => onexport([model.id])}>Exportieren…</button>
      <button disabled={readOnly} onclick={pickFolder}>Ordner…</button>
      <button class="primary" disabled={readOnly} onclick={pickFiles}>+ Dateien</button>
    </div>
    <div class="row filters">
      <div class="seg">
        <button class:active={filter === "all"} onclick={() => (filter = "all")}>Alle</button>
        <button class:active={filter === "image"} onclick={() => (filter = "image")}>Fotos</button>
        <button class:active={filter === "video"} onclick={() => (filter = "video")}>Videos</button>
      </div>
      <select bind:value={sort} style="width:auto">
        <option value="taken">Aufnahmedatum</option>
        <option value="imported">Importdatum</option>
        <option value="name">Dateiname</option>
      </select>
      <button class="ghost" onclick={() => (desc = !desc)} title="Reihenfolge umkehren">{desc ? "↓" : "↑"}</button>
      <span class="spacer"></span>
      <span class="small muted">Größe</span>
      <input type="range" min="110" max="420" bind:value={tile} style="width:140px" />
    </div>
    {#if selected.size}
      <div class="row selbar">
        <b>{selected.size} ausgewählt</b>
        <button class="ghost" onclick={() => items.forEach((m) => selected.add(m.id))}>Alle</button>
        <button class="ghost" onclick={() => selected.clear()}>Auswahl aufheben</button>
        <span class="spacer"></span>
        {#if selected.size === 1}
          <button disabled={readOnly} onclick={() => setCover([...selected][0])}>Als Titelbild</button>
        {/if}
        <select bind:value={moveTarget} disabled={readOnly} style="width:auto">
          <option value="">Verschieben nach…</option>
          {#each models.filter((x) => x.id !== model.id) as other (other.id)}
            <option value={other.id}>{other.name}</option>
          {/each}
        </select>
        {#if moveTarget}<button onclick={moveSelected}>Verschieben</button>{/if}
        <button class="danger" disabled={readOnly} onclick={deleteSelected}>Löschen</button>
      </div>
    {/if}
    {#if importing && importProgress}
      <div class="importbar">
        <div class="row small">
          <span class="spinner"></span>
          <span>Importiere {importProgress.done} / {importProgress.total} – {importProgress.current}</span>
          <span class="spacer"></span>
          <button class="ghost small" onclick={() => api.cancelJob()}>Abbrechen</button>
        </div>
        <div class="progress"><div style="width:{importProgress.total ? (importProgress.done / importProgress.total) * 100 : 0}%"></div></div>
      </div>
    {/if}
  </div>

  <div class="scroller" bind:clientWidth={width} bind:clientHeight={height} onscroll={(e) => (scrollTop = (e.currentTarget as HTMLElement).scrollTop)}>
    {#if items.length === 0}
      <div class="empty">
        {#if media.length === 0}
          <div class="drop-hint">
            <div style="font-size:2.6rem">⬇</div>
            <h2>Fotos und Videos hierher ziehen</h2>
            <p class="muted">oder über „+ Dateien“ bzw. „Ordner…“ hinzufügen. Die Originale werden verlustfrei und verschlüsselt im Tresor abgelegt. Deine Quelldateien bleiben unverändert.</p>
          </div>
        {:else}
          <p class="muted">Keine Einträge für diesen Filter.</p>
        {/if}
      </div>
    {:else}
      <div class="canvas" style="height:{totalH}px">
        {#each visible as { m, i } (m.id)}
          <button
            class="tile"
            class:sel={selected.has(m.id)}
            style="width:{size}px;height:{size}px;transform:translate({(i % cols) * (size + GAP)}px,{Math.floor(i / cols) * rowH}px)"
            onclick={(e) => click(e, i, m)}
            title={m.file_name}
          >
            {#if m.has_thumb}
              <img src={thumbUrl(m.id)} alt="" loading="lazy" draggable="false" />
            {:else}
              <div class="nothumb">{m.kind === "video" ? "🎞" : "🖼"}<span>{m.ext.toUpperCase()}</span></div>
            {/if}
            {#if m.kind === "video"}<span class="vbadge">▶ {formatDuration(m.duration)}</span>{/if}
            {#if model.cover === m.id}<span class="cbadge" title="Titelbild">★</span>{/if}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <span class="check" onclick={(e) => toggle(e, i, m)}>{selected.has(m.id) ? "✓" : ""}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  {#if dragging}
    <div class="dropzone"><div>Loslassen, um in „{model.name}“ zu importieren</div></div>
  {/if}
</div>

{#if lightbox !== null}
  <Lightbox items={items} index={lightbox} cover={model.cover} {readOnly} onsetcover={setCover} onclose={() => (lightbox = null)} />
{/if}

<style>
  .page { height: 100%; display: flex; flex-direction: column; position: relative; }
  .toolbar { padding: 1rem 1.5rem .6rem; border-bottom: 1px solid var(--line); }
  .title { margin: 0 .4rem; max-width: 40vw; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .filters { margin-top: .7rem; }
  .seg { display: inline-flex; border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
  .seg button { border: none; border-radius: 0; background: transparent; }
  .seg button.active { background: var(--bg-3); color: var(--accent); }
  .selbar { margin-top: .7rem; padding: .5rem .8rem; background: #2b2416; border: 1px solid #5a4720; border-radius: 8px; }
  .importbar { margin-top: .7rem; }
  .importbar .row { margin-bottom: .35rem; }
  .scroller { flex: 1; overflow-y: auto; overflow-x: hidden; padding: 0; margin: .8rem .9rem 0 1.5rem; }
  .canvas { position: relative; }
  .tile {
    position: absolute; left: 0; top: 0; padding: 0; border: none; border-radius: 6px;
    overflow: hidden; background: var(--bg-3); outline: 3px solid transparent; outline-offset: -3px;
  }
  .tile img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .tile:hover img { filter: brightness(1.08); }
  .tile.sel { outline-color: var(--accent); }
  .tile.sel img { filter: brightness(.75); }
  .nothumb { width: 100%; height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: .3rem; font-size: 2rem; color: var(--muted); }
  .nothumb span { font-size: .8rem; }
  .vbadge { position: absolute; left: 6px; bottom: 6px; font-size: .75rem; background: rgba(0,0,0,.65); padding: .15em .5em; border-radius: 4px; }
  .cbadge { position: absolute; right: 8px; bottom: 6px; color: var(--accent); text-shadow: 0 0 4px #000; }
  .check {
    position: absolute; left: 7px; top: 7px; width: 22px; height: 22px; border-radius: 50%;
    border: 2px solid rgba(255,255,255,.85); background: rgba(0,0,0,.35); display: grid; place-items: center;
    font-size: .8rem; color: #17140d; opacity: 0; transition: opacity .12s;
  }
  .tile:hover .check, .tile.sel .check { opacity: 1; }
  .tile.sel .check { background: var(--accent); border-color: var(--accent); }
  .empty { display: grid; place-items: center; height: 100%; text-align: center; }
  .drop-hint { max-width: 460px; border: 2px dashed var(--line); border-radius: 16px; padding: 2.5rem; }
  .dropzone {
    position: absolute; inset: 10px; border: 3px dashed var(--accent); border-radius: 16px;
    background: rgba(18, 19, 24, .85); display: grid; place-items: center; font-size: 1.3rem; z-index: 20; pointer-events: none;
  }
</style>
