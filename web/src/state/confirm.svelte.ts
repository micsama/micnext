export type Ask = { title: string; description?: string; action: string };

/** 破坏性操作的二次确认；全局只有一个确认框，由 ConfirmDialog 渲染。 */
class Confirm {
  pending = $state<(Ask & { resolve: (ok: boolean) => void }) | null>(null);

  ask(a: Ask): Promise<boolean> {
    this.pending?.resolve(false);
    return new Promise((resolve) => (this.pending = { ...a, resolve }));
  }

  settle(ok: boolean): void {
    this.pending?.resolve(ok);
    this.pending = null;
  }
}

export const confirm = new Confirm();
