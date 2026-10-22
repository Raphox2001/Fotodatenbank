<script lang="ts">
  import { api, errorText, thumbUrl, type Media, type ModelInfo } from "./api";

  let { model, onclose, onpicked }: { model: ModelInfo; onclose: () => void; onpicked: () => void } = $props();

  let media = $state<Media[] | null>(null);
  let error = $state("");

  $effect(() => {
    api
      .listMedia(model.id)
      .then((m) => (media = m.filter((x) => x.has_thumb).sort((a, b) => (b.kind === "image" ? 1 : 0) - (a.kind === "image" ? 1 : 0))))
      .catch((e) => (error = errorText(e)));
  });

  async function pick(id: string) {
    try {
      await api.setCover(model.id, id);
      onpicked();
      onclose();
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="overlay" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="card dialog wide">
    <div class="row between">
      <h2 style="margin:0">Titelbild für „{model.name}“</h2>
      <button class="ghost" onclick={onclose}>✕</button>
    </div>
    <p class="small muted">Klicke auf das Bild, das auf der Model-Karte erscheinen soll.</p>
    {#if error}<p class="error">{error}</p>{/if}
    {#if media === null}
      <div style="padding:2rem;text-align:center"><span class="spinner"></span></div>
    {:else if media.length === 0}
      <p class="muted">Dieses Model hat noch keine Bilder mit Vorschau.</p>
    {:else}
      <div class="grid">
        {#each media as m (m.id)}
          <button class="pick" class:current={model.cover === m.id} onclick={() => pick(m.id)} title={m.file_name}>
            <img src={thumbUrl(m.id)} alt="" loading="lazy" draggable="false" />
            {#if model.cover === m.id}<span class="star">★ aktuell</span>{/if}
            {#if m.kind === "video"}<span class="vid">▶</span>{/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .wide { width: min(900px, calc(100vw - 32px)); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 8px; margin-top: .8rem; }
  .pick { position: relative; padding: 0; border: none; border-radius: 8px; overflow: hidden; aspect-ratio: 4 / 5; background: var(--bg-3); outline: 3px solid transparent; outline-offset: -3px; }
  .pick img { width: 100%; height: 100%; object-fit: cover; display: block; transition: transform .2s; }
  .pick:hover img { transform: scale(1.05); }
  .pick:hover { outline-color: var(--accent-2); }
  .pick.current { outline-color: var(--accent); }
  .star { position: absolute; left: 6px; bottom: 6px; font-size: .75rem; background: var(--accent); color: #17140d; padding: .1em .5em; border-radius: 4px; font-weight: 600; }
  .vid { position: absolute; right: 6px; top: 6px; font-size: .75rem; background: rgba(0,0,0,.65); padding: .1em .45em; border-radius: 4px; }
</style>
