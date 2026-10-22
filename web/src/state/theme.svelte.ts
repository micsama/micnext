import { load, save } from "./storage";

export type ThemeChoice = "system" | "light" | "dark";

const KEY = "micnext.theme";
const ORDER: ThemeChoice[] = ["system", "light", "dark"];
const systemDark = matchMedia("(prefers-color-scheme: dark)");

function initial(): ThemeChoice {
  const t = load(KEY);
  return t === "light" || t === "dark" ? t : "system";
}

/** `data-theme` 恒为解析后的明暗；首帧由 index.html 写入同样的值。 */
class Theme {
  choice = $state<ThemeChoice>(initial());
  dark = $state(false);

  constructor() {
    this.dark = this.resolve();
    systemDark.addEventListener("change", () => this.apply());
  }

  cycle(): void {
    this.choice = ORDER[(ORDER.indexOf(this.choice) + 1) % ORDER.length]!;
    save(KEY, this.choice === "system" ? null : this.choice);
    this.apply();
  }

  private resolve(): boolean {
    return this.choice === "dark" || (this.choice === "system" && systemDark.matches);
  }

  private apply(): void {
    this.dark = this.resolve();
    document.documentElement.dataset.theme = this.dark ? "dark" : "light";
  }
}

export const theme = new Theme();
