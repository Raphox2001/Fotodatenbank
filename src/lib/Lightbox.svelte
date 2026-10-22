<script lang="ts">
  import { formatBytes, formatDuration, formatDate, mediaUrl, type Media } from "./api";

  let {
    items,
    index,
    cover = null,
    readOnly = true,
    onsetcover,
    onclose,
  }: {
    items: Media[];
    index: number;
    cover?: string | null;
    readOnly?: boolean;
    onsetcover?: (id: string) => void;
    onclose: () => void;
  } = $props();

  let i = $state(index);
  let zoom = $state(false);
  let showInfo = $state(true);
  let loading = $state(true);
  let failed = $state(false);

  const m = $derived(items[i]);

  $effect(() => {
    m.id;
    loading = true;
    failed = false;
    zoom = false;
  });

  function go(d: number) {
    const n = i + d;
    if (n >= 0 && n < items.length) i = n;
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowRight") go(1);
    else if (e.key === "ArrowLeft") go(-1);
    else if (e.key === "i") showInfo = !showInfo;
    else if (e.key === " " && m.kind === "image") { e.preventDefault(); zoom = !zoom; }
    else return;
    e.stopPropagation();
  }
</script>

<svelte:window onkeydowncapture={keydown} />

<div class="lb">
  <div class="stage" class:zoom>
    {#key m.id}
      {#if m.kind === "image"}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <img
          src={mediaUrl(m.id)}
          alt={m.file_name}
          draggable="false"
          class:loaded={!loading}
          onload={() => (loading = false)}
          onerror={() => { loading = false; failed = true; }}
          onclick={() => (zoom = !zoom)}
        />
      {:else}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          src={mediaUrl(m.id)}
          controls
          autoplay
          controlslist="nodownload noremoteplayback"
          disablepictureinpicture
          oncanplay={() => (loading = false)}
          onerror={() => { loading = false; failed = true; }}
        ></video>
      {/if}
    {/key}
    {#if loading && !failed}<div class="center"><span class="spinner"></span></div>{/if}
    {#if failed}
      <div class="center card msg">
        <h2>Kann hier nicht angezeigt werden</h2>
        <p class="muted">
          {#if m.kind === "video"}
            Dieses Videoformat wird von der eingebauten Wiedergabe nicht unterstützt (z.&nbsp;B. H.265/HEVC ohne die
            Windows-Erweiterung „HEVC-Videoerweiterungen“, oder AVI/MKV-Codecs). Die Datei ist unverändert im Tresor und kann
            verschlüsselt exportiert werden.
          {:else}
            Das Bild konnte nicht dekodiert werden. Die Originaldatei ist unverändert im Tresor.
          {/if}
        </p>
      </div>
    {/if}
  </div>

  <div class="top row">
    <span class="small muted">{i + 1} / {items.length}</span>
    <span class="spacer"></span>
    {#if onsetcover && m.has_thumb}
      {#if cover === m.id}
        <span class="badge" style="color:var(--accent)">★ Titelbild</span>
      {:else}
        <button class="ghost" disabled={readOnly} onclick={() => onsetcover(m.id)} title="Als Titelbild des Models verwenden">☆ Als Titelbild</button>
      {/if}
    {/if}
    <button class="ghost" onclick={() => (showInfo = !showInfo)} title="Infos (I)">ⓘ</button>
    <button class="ghost" onclick={onclose} title="Schließen (Esc)">✕</button>
  </div>

  {#if i > 0}<button class="nav prev" onclick={() => go(-1)} title="Vorheriges (←)">‹</button>{/if}
  {#if i < items.length - 1}<button class="nav next" onclick={() => go(1)} title="Nächstes (→)">›</button>{/if}

  {#if showInfo}
    <div class="info small">
      <b>{m.file_name}</b>
      {#if m.width && m.height}<span>{m.width} × {m.height}</span>{/if}
      {#if m.duration}<span>{formatDuration(m.duration)}</span>{/if}
      <span>{formatBytes(m.size)}</span>
      {#if m.taken_at}<span>Aufnahme: {m.taken_at}</span>{/if}
      <span class="muted">Importiert: {formatDate(m.imported_at)}</span>
      {#if m.kind === "image"}<span class="muted">{zoom ? "100 %" : "Klick/Leertaste = 100 %"}</span>{/if}
    </div>
  {/if}
</div>

<style>
  .lb { position: fixed; inset: 0; background: #07080a; z-index: 100; }
  .stage { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; overflow: hidden; }
  .stage img, .stage video { max-width: 100%; max-height: 100%; object-fit: contain; display: block; }
  .stage img { opacity: 0; transition: opacity .15s; cursor: zoom-in; }
  .stage img.loaded { opacity: 1; }
  .stage.zoom { overflow: auto; display: block; }
  .stage.zoom img { max-width: none; max-height: none; cursor: zoom-out; margin: auto; }
  .center { position: absolute; inset: 0; margin: auto; display: grid; place-items: center; height: fit-content; width: fit-content; }
  .msg { max-width: 520px; text-align: center; }
  .top { position: absolute; top: 0; left: 0; right: 0; padding: .6rem 1rem; background: linear-gradient(#000a, transparent); }
  .top button { font-size: 1.15rem; }
  .nav {
    position: absolute; top: 50%; transform: translateY(-50%); font-size: 2.6rem; line-height: 1;
    background: rgba(0,0,0,.35); border: none; border-radius: 50%; width: 56px; height: 56px; padding: 0 0 6px;
    opacity: .55;
  }
  .nav:hover { opacity: 1; background: rgba(0,0,0,.6) !important; }
  .prev { left: 14px; }
  .next { right: 14px; }
  .info {
    position: absolute; left: 50%; top: 12px; transform: translateX(-50%);
    display: flex; gap: 1.1rem; flex-wrap: wrap; justify-content: center;
    background: rgba(18,19,24,.85); border: 1px solid var(--line); border-radius: 10px; padding: .5rem 1rem;
    max-width: calc(100% - 140px);
  }
</style>
