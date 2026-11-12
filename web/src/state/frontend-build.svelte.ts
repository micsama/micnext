/** 本页加载的前端构建是否已被服务端替换。入口脚本文件名带内容哈希，前端代码不变它就不变。 */
class FrontendBuild {
  stale = $state(false);
  readonly #entry = document.querySelector<HTMLScriptElement>('script[type="module"][src]')?.getAttribute("src") ?? null;

  /** 取一次最新的 index.html，看里面是否还引用本页的入口脚本。失败就跳过，等下次重连再查。 */
  async check(): Promise<void> {
    if (this.stale || this.#entry === null) return;
    try {
      const res = await fetch("/", { cache: "no-store" });
      if (res.ok && !(await res.text()).includes(`"${this.#entry}"`)) this.stale = true;
    } catch {
      // 服务还没起来；下次重连会再查。
    }
  }
}

export const frontendBuild = new FrontendBuild();
