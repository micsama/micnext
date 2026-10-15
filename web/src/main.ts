import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { installTooltips } from "./lib/tooltip";

installTooltips();
mount(App, { target: document.getElementById("app")! });
