import type { ContextKind } from "../api/types";

/** 上下文分类的展示名与色，构成浮层与 trace 系统提示共用；色值见 app.css `--ctx-*`。 */
export const CONTEXT_LABEL: Record<ContextKind, string> = {
  instructions: "系统提示词",
  persona: "人设与偏好",
  tools: "工具",
  environment: "运行环境",
  summary: "压缩摘要",
  conversation: "用户对话",
};

export const contextColor = (kind: ContextKind) => `var(--ctx-${kind})`;
