import { load, save } from "./storage";

const KEY = "micnext.sidebar";
const desktop = matchMedia("(min-width: 48rem)");

/** 窄屏是抽屉（不记忆）；宽屏常驻，可收起，收起状态按浏览器记忆。 */
class Sidebar {
  drawer = $state(false);
  docked = $state(load(KEY) !== "collapsed");

  toggle(): void {
    if (!desktop.matches) {
      this.drawer = !this.drawer;
      return;
    }
    this.docked = !this.docked;
    save(KEY, this.docked ? null : "collapsed");
  }
}

export const sidebar = new Sidebar();
