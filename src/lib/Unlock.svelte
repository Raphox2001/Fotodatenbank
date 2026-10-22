<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText } from "./api";
  import PasswordField from "./PasswordField.svelte";

  let { vaultPath, ondone, onforget }: { vaultPath: string; ondone: () => void; onforget: () => void } = $props();

  let recovery = $state(false);
  let pw = $state("");
  let key = $state("");
  let newPw = $state("");
  let newPw2 = $state("");
  let busy = $state(false);
  let error = $state("");

  async function unlock() {
    if (!pw || busy) return;
    error = "";
    busy = true;
    try {
      await api.unlock(null, pw);
      pw = "";
      ondone();
    } catch (e) {
      error = errorText(e);
      pw = "";
    } finally {
      busy = false;
    }
  }

  async function useRecovery() {
    error = "";
    if (newPw.length < 10) return (error = "Das neue Passwort braucht mindestens 10 Zeichen");
    if (newPw !== newPw2) return (error = "Die Passwörter stimmen nicht überein");
    busy = true;
    try {
      await api.unlockRecovery(null, key, newPw);
      key = newPw = newPw2 = "";
      ondone();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function forget() {
    if (await ask("Einen anderen Tresor öffnen oder neu anlegen? Der aktuelle Tresor bleibt unverändert erhalten.", { title: "Anderer Tresor", kind: "info" })) {
      await api.forgetVault();
      onforget();
    }
  }
</script>

<div class="wrap">
  <div class="card dialog">
    <div class="head">
      <img src="/icon.svg" alt="" width="56" height="56" />
      <div>
        <h2 style="margin:0">Fotodatenbank ist gesperrt</h2>
        <div class="small muted path" title={vaultPath}>{vaultPath}</div>
      </div>
    </div>

    {#if !recovery}
      <PasswordField bind:value={pw} autofocus onenter={unlock} />
      {#if error}<p class="error">{error}</p>{/if}
      <div class="row" style="margin-top:1.2rem">
        <button class="link small" onclick={() => { recovery = true; error = ""; }}>Passwort vergessen?</button>
        <span class="spacer"></span>
        <button class="primary" onclick={unlock} disabled={busy || !pw}>
          {#if busy}<span class="spinner"></span> Entsperre…{:else}Entsperren{/if}
        </button>
      </div>
    {:else}
      <p class="muted small">Gib deinen Wiederherstellungsschlüssel ein und lege ein neues Passwort fest.</p>
      <label>Wiederherstellungsschlüssel</label>
      <textarea rows="3" bind:value={key} spellcheck="false" style="font-family:Consolas,monospace"></textarea>
      <PasswordField bind:value={newPw} label="Neues Passwort" meter />
      <PasswordField bind:value={newPw2} label="Neues Passwort wiederholen" onenter={useRecovery} />
      {#if error}<p class="error">{error}</p>{/if}
      <div class="row end" style="margin-top:1.2rem">
        <button class="ghost" onclick={() => { recovery = false; error = ""; }} disabled={busy}>Zurück</button>
        <button class="primary" onclick={useRecovery} disabled={busy || !key}>{#if busy}<span class="spinner"></span>{/if} Wiederherstellen</button>
      </div>
    {/if}
  </div>
  <button class="link small muted-link" onclick={forget}>Anderen Tresor öffnen oder neu anlegen</button>
</div>

<style>
  .wrap { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 1.2rem; padding: 2rem; }
  .head { display: flex; gap: 1rem; align-items: center; margin-bottom: .6rem; }
  .path { max-width: 400px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; margin-top: .2rem; }
  .muted-link { color: var(--muted); }
</style>
