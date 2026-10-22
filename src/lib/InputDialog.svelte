<script lang="ts">
  let {
    title,
    value = "",
    confirm = "OK",
    onsubmit,
    oncancel,
  }: { title: string; value?: string; confirm?: string; onsubmit: (v: string) => void; oncancel: () => void } = $props();

  let text = $state(value);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="overlay" onclick={(e) => e.target === e.currentTarget && oncancel()}>
  <div class="card dialog">
    <h2>{title}</h2>
    <!-- svelte-ignore a11y_autofocus -->
    <input
      type="text"
      bind:value={text}
      autofocus
      onkeydown={(e) => {
        if (e.key === "Enter" && text.trim()) onsubmit(text.trim());
        if (e.key === "Escape") oncancel();
      }}
    />
    <div class="row end" style="margin-top:1.2rem">
      <button class="ghost" onclick={oncancel}>Abbrechen</button>
      <button class="primary" disabled={!text.trim()} onclick={() => onsubmit(text.trim())}>{confirm}</button>
    </div>
  </div>
</div>
