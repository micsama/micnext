import type { ContentPart, ExecOutcome } from "../api/types";

export function partsText(parts: ContentPart[]): string {
  return parts.map((p) => ("Text" in p ? p.Text.text : "File" in p ? `[文件] ${p.File.path}` : "Skill" in p ? `[skill] ${p.Skill.name}` : "[图片]")).join("\n");
}

export function outcomeText(o: ExecOutcome): string {
  if ("Completed" in o) return partsText(o.Completed.output) || "（无输出）";
  if ("TimedOut" in o) return `超时\n${partsText(o.TimedOut.output)}`;
  if ("Failed" in o) return o.Failed.message;
  return "已取消";
}
