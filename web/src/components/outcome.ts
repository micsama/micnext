import type { ContentPart, ExecOutcome } from "../api/types";

export function partsText(parts: ContentPart[]): string {
  return parts.map((p) => ("Text" in p ? p.Text.text : "File" in p ? `[文件] ${p.File.path}` : "[图片]")).join("\n");
}

export function outcomeText(o: ExecOutcome): string {
  if ("Completed" in o) return partsText(o.Completed.output) || "（无输出）";
  if ("Failed" in o) return o.Failed.message;
  return "已取消";
}
