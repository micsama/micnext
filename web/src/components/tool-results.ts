import type { Message, ToolCallRef, ToolResultOutcome } from "../api/types";

export const toolKey = (call: ToolCallRef) => `${call.reply}:${call.index}`;

/** `ms` 为执行耗时（与 Trace 同口径），转入后台或无开始时间时为 null。 */
export type ToolResult = { outcome: ToolResultOutcome; ms: number | null };

export const toolResult = (outcome: ToolResultOutcome, startedAt: number | null, createdAt: number): ToolResult => ({
  outcome,
  ms: startedAt !== null && "Terminal" in outcome ? createdAt - startedAt : null,
});

export function toolResults(messages: Message[]): Map<string, ToolResult> {
  const results = new Map<string, ToolResult>();
  for (const { body, created_at } of messages)
    if (body.kind === "ToolResult") results.set(toolKey(body.call), toolResult(body.outcome, body.started_at, created_at));
  return results;
}
