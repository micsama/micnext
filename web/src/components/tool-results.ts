import type { Message, ToolCallRef, ToolResultOutcome } from "../api/types";

export const toolKey = (call: ToolCallRef) => `${call.reply}:${call.index}`;

export function toolResults(messages: Message[]): Map<string, ToolResultOutcome> {
  const results = new Map<string, ToolResultOutcome>();
  for (const { body } of messages) if (body.kind === "ToolResult") results.set(toolKey(body.call), body.outcome);
  return results;
}
