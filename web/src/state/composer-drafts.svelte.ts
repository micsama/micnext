/** 输入框的回复引用：`label` 显示在输入框内，发送时 `quote` 以引用块前置到正文。 */
export type ComposerReply = { label: string; quote: string };
/** 支持的图片带预览 `url`，按图片发送；其余按文件发送。 */
export type Attachment = { id: number; file: File; url: string | null };

let nextId = 0;

/** 未发送的输入；只在页面内存，刷新即消失。附件的对象 URL 归草稿所有。 */
export class ComposerDraft {
  text = $state("");
  attachments = $state<Attachment[]>([]);
  reply = $state<ComposerReply | null>(null);
  sending = $state(false);

  attach(file: File, image: boolean): void {
    this.attachments.push({ id: nextId++, file, url: image ? URL.createObjectURL(file) : null });
  }

  detach(i: number): void {
    const [a] = this.attachments.splice(i, 1);
    if (a?.url) URL.revokeObjectURL(a.url);
  }

  clear(): void {
    for (const a of this.attachments) if (a.url) URL.revokeObjectURL(a.url);
    this.attachments = [];
    this.text = "";
    this.reply = null;
  }
}

const drafts = new Map<number | "new", ComposerDraft>();

/** 按会话保留草稿；`"new"` 是新会话页的草稿。 */
export const composerDrafts = {
  get(key: number | "new"): ComposerDraft {
    let draft = drafts.get(key);
    if (!draft) drafts.set(key, (draft = new ComposerDraft()));
    return draft;
  },
};
