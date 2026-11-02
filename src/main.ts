import "./theme.css";
import { mount } from "svelte";

// The website's demo answers the app's backend calls itself, and has to be in place first.
const ready = import.meta.env.MODE === "demo" ? import("./demoBackend") : Promise.resolve();

ready.then(async () => {
  const { default: App } = await import("./App.svelte");
  mount(App, { target: document.getElementById("app")! });
  document.getElementById("boot")?.remove();
});
