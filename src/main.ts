import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

// Kein Rechtsklick-Menü (z. B. "Bild speichern unter"), außer in Eingabefeldern.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea")) e.preventDefault();
});
// Keine Bilder aus dem Fenster herausziehen.
window.addEventListener("dragstart", (e) => e.preventDefault());

export default mount(App, { target: document.getElementById("app")! });
