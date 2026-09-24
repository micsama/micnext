import { load, save } from "./storage";

export type ThemeChoice = "system" | "light" | "dark";

const KEY = "micnext.theme";
const ORDER: ThemeChoice[] = ["system", "light", "dark"];

function initial(): ThemeChoice {
  const t = load(KEY);
  return t === "light" || t === "dark" ? t : "system";
}

class Theme {
  choice = $state<ThemeChoice>(initial());

  cycle(): void {
    this.choice = ORDER[(ORDER.indexOf(this.choice) + 1) % ORDER.length]!;
    save(KEY, this.choice === "system" ? null : this.choice);
    if (this.choice === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = this.choice;
  }
}

export const theme = new Theme();
