<script lang="ts">
  import { passwordStrength } from "./api";

  let {
    value = $bindable(""),
    label = "Passwort",
    meter = false,
    autofocus = false,
    onenter,
  }: { value?: string; label?: string; meter?: boolean; autofocus?: boolean; onenter?: () => void } = $props();

  let visible = $state(false);
  const s = $derived(passwordStrength(value));
  const colors = ["transparent", "var(--danger)", "var(--accent)", "var(--ok)"];
</script>

<label>{label}</label>
<div class="row">
  <!-- svelte-ignore a11y_autofocus -->
  <input
    type={visible ? "text" : "password"}
    bind:value
    {autofocus}
    autocomplete="off"
    spellcheck="false"
    onkeydown={(e) => e.key === "Enter" && onenter?.()}
  />
  <button type="button" class="ghost" title={visible ? "Verbergen" : "Anzeigen"} onclick={() => (visible = !visible)}>
    {visible ? "🙈" : "👁"}
  </button>
</div>
{#if meter}
  <div class="strength"><div style="width:{Math.min(100, s.bits)}%; background:{colors[s.level]}"></div></div>
  {#if s.label}<div class="small muted" style="margin-top:.3em">Stärke: {s.label}</div>{/if}
{/if}
