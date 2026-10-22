<script lang="ts">
  import { untrack } from "svelte";
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

  // Wie weit über die Originalgröße (100 %) hinaus vergrößert werden darf.
  const MAX_OVER_NATIVE = 6;

  let i = $state(index);
  // Bildinfo nur auf Wunsch (ⓘ bzw. Taste I)
  let showInfo = $state(false);
  let loading = $state(true);
  let failed = $state(false);

  let stage = $state<HTMLDivElement>();
  let stageW = $state(0);
  let stageH = $state(0);
  // Originalmaße des geladenen Bildes
  let natW = $state(0);
  let natH = $state(0);
  // Zoom relativ zur eingepassten Größe (1 = ganzes Bild sichtbar) und Verschiebung in px
  let scale = $state(1);
  let tx = $state(0);
  let ty = $state(0);
  let dragging = $state(false);
  let dragFrom = { x: 0, y: 0, tx: 0, ty: 0 };

  const m = $derived(items[i]);
  // Eingepasst wie bisher: nie über 100 % hochskalieren.
  const fit = $derived(natW && stageW ? Math.min(1, stageW / natW, stageH / natH) : 1);
  const maxScale = $derived(Math.max(1, MAX_OVER_NATIVE / fit));
  const dispW = $derived(natW * fit * scale);
  const dispH = $derived(natH * fit * scale);
  const percent = $derived(Math.round(fit * scale * 100));
  const zoomed = $derived(scale > 1.001);

  $effect(() => {
    m.id;
    loading = true;
    failed = false;
    natW = natH = 0;
    reset();
  });

  function reset() {
    scale = 1;
    tx = ty = 0;
  }

  /** Verschiebung so begrenzen, dass das Bild nicht aus dem Fenster wandert. */
  function clamp() {
    const mx = Math.max(0, (dispW - stageW) / 2);
    const my = Math.max(0, (dispH - stageH) / 2);
    tx = Math.min(mx, Math.max(-mx, tx));
    ty = Math.min(my, Math.max(-my, ty));
  }

  /** Zoomt auf `next`, wobei der Bildpunkt unter (cx, cy) an seiner Stelle bleibt. */
  function zoomAt(next: number, cx: number, cy: number) {
    next = Math.min(maxScale, Math.max(1, next));
    if (next === 1) return reset();
    const rx = cx - stageW / 2 - tx;
    const ry = cy - stageH / 2 - ty;
    const k = next / scale;
    tx -= rx * (k - 1);
    ty -= ry * (k - 1);
    scale = next;
    clamp();
  }

  function local(e: MouseEvent) {
    const r = stage!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  // Wheel-Listener muss aktiv (nicht passiv) sein, damit preventDefault wirkt.
  $effect(() => {
    const el = stage;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (m.kind !== "image" || !natW) return;
      e.preventDefault();
      // Stufenlos: Faktor hängt von der Drehweite ab (Mausrad, Touchpad, Pinch).
      const unit = e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? 400 : 1;
      const factor = Math.exp(-e.deltaY * unit * 0.0015);
      const p = local(e);
      zoomAt(scale * factor, p.x, p.y);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  });

  // Fenstergröße geändert -> Verschiebung neu begrenzen
  $effect(() => {
    stageW;
    stageH;
    untrack(clamp);
  });

  function pointerdown(e: PointerEvent) {
    if (e.button !== 0 || !zoomed) return;
    dragging = true;
    dragFrom = { x: e.clientX, y: e.clientY, tx, ty };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function pointermove(e: PointerEvent) {
    if (!dragging) return;
    tx = dragFrom.tx + (e.clientX - dragFrom.x);
    ty = dragFrom.ty + (e.clientY - dragFrom.y);
    clamp();
  }

  function pointerup() {
    dragging = false;
  }

  /** Doppelklick: zwischen eingepasst und 100 % (Originalpixel) wechseln. */
  function dblclick(e: MouseEvent) {
    if (m.kind !== "image" || !natW) return;
    const p = local(e);
    if (zoomed) reset();
    else zoomAt(1 / fit > 1.01 ? 1 / fit : 2, p.x, p.y);
  }

  function go(d: number) {
    const n = i + d;
    if (n >= 0 && n < items.length) i = n;
  }

  function keydown(e: KeyboardEvent) {
    const isImg = m.kind === "image" && natW > 0;
    if (e.key === "Escape") {
      if (zoomed) reset();
      else onclose();
    } else if (e.key === "ArrowRight") go(1);
    else if (e.key === "ArrowLeft") go(-1);
    else if (e.key === "i") showInfo = !showInfo;
    else if (isImg && (e.key === "+" || e.key === "=")) zoomAt(scale * 1.25, stageW / 2, stageH / 2);
    else if (isImg && e.key === "-") zoomAt(scale / 1.25, stageW / 2, stageH / 2);
    else if (isImg && e.key === "0") reset();
    else if (isImg && e.key === " ") {
      e.preventDefault();
      if (zoomed) reset();
      else zoomAt(1 / fit > 1.01 ? 1 / fit : 2, stageW / 2, stageH / 2);
    } else return;
    e.stopPropagation();
  }
</script>

<svelte:window onkeydowncapture={keydown} />

<div class="lb">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stage"
    class:zoomable={m.kind === "image" && natW > 0}
    class:zoomed
    class:dragging
    bind:this={stage}
    bind:clientWidth={stageW}
    bind:clientHeight={stageH}
    onpointerdown={pointerdown}
    onpointermove={pointermove}
    onpointerup={pointerup}
    onpointercancel={pointerup}
    ondblclick={dblclick}
  >
    {#key m.id}
      {#if m.kind === "image"}
        <img
          src={mediaUrl(m.id)}
          alt={m.file_name}
          draggable="false"
          class:loaded={!loading}
          style={natW
            ? `width:${dispW}px;height:${dispH}px;left:${(stageW - dispW) / 2 + tx}px;top:${(stageH - dispH) / 2 + ty}px`
            : ""}
          onload={(e) => {
            const img = e.currentTarget as HTMLImageElement;
            natW = img.naturalWidth;
            natH = img.naturalHeight;
            loading = false;
          }}
          onerror={() => { loading = false; failed = true; }}
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
    {#if m.kind === "image" && natW}
      <button class="ghost small zoomlabel" onclick={reset} disabled={!zoomed} title="Einpassen (0)">{percent} %</button>
    {/if}
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
    <div class="info small" class:vid={m.kind === "video"}>
      <b>{m.file_name}</b>
      {#if m.width && m.height}<span>{m.width} × {m.height}</span>{/if}
      {#if m.duration}<span>{formatDuration(m.duration)}</span>{/if}
      <span>{formatBytes(m.size)}</span>
      {#if m.taken_at}<span>Aufnahme: {m.taken_at}</span>{/if}
      <span class="muted">Importiert: {formatDate(m.imported_at)}</span>
      {#if m.kind === "image"}<span class="muted">Mausrad = Zoom · Ziehen = Verschieben · Doppelklick = 100 %</span>{/if}
    </div>
  {/if}
</div>

<style>
  .lb { position: fixed; inset: 0; background: #07080a; z-index: 100; }
  .stage { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; overflow: hidden; touch-action: none; }
  .stage video { max-width: 100%; max-height: 100%; object-fit: contain; display: block; }
  .stage img { max-width: 100%; max-height: 100%; display: block; opacity: 0; transition: opacity .15s; }
  .stage img.loaded { opacity: 1; }
  /* Nach dem Laden: echte Größe setzen und frei positionieren (scharf bei jedem Zoom). */
  .stage.zoomable img { position: absolute; max-width: none; max-height: none; }
  .stage.zoomable { cursor: zoom-in; }
  .stage.zoomed { cursor: grab; }
  .stage.dragging { cursor: grabbing; }
  .center { position: absolute; inset: 0; margin: auto; display: grid; place-items: center; height: fit-content; width: fit-content; }
  .msg { max-width: 520px; text-align: center; }
  .top { position: absolute; top: 0; left: 0; right: 0; padding: .6rem 1rem; background: linear-gradient(#000a, transparent); }
  .top button { font-size: 1.15rem; }
  .top .zoomlabel { font-size: .85rem; min-width: 4.5em; font-variant-numeric: tabular-nums; }
  .top .zoomlabel:disabled { opacity: .7; }
  .nav {
    position: absolute; top: 50%; transform: translateY(-50%); font-size: 2.6rem; line-height: 1;
    background: rgba(0,0,0,.35); border: none; border-radius: 50%; width: 56px; height: 56px; padding: 0 0 6px;
    opacity: .55;
  }
  .nav:hover { opacity: 1; background: rgba(0,0,0,.6) !important; }
  .prev { left: 14px; }
  .next { right: 14px; }
  .info {
    position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%);
    display: flex; gap: 1.1rem; flex-wrap: wrap; justify-content: center;
    background: rgba(18,19,24,.85); border: 1px solid var(--line); border-radius: 10px; padding: .5rem 1rem;
    max-width: calc(100% - 140px); pointer-events: none;
  }
  /* Bei Videos oben, damit die Wiedergabeleiste frei bleibt */
  .info.vid { top: 56px; bottom: auto; }
</style>
