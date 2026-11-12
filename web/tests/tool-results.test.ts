import { expect, test } from "bun:test";
import type { Message, ToolResultOutcome } from "../src/api/types";
import { toolKey, toolResult, toolResults } from "../src/components/tool-results";

function result(reply: number, index: number, text: string): Message {
  return {
    id: reply + 1, session_id: 1, created_at: 0, delivered_at: null,
    body: { kind: "ToolResult", call: { reply, index }, tool_name: "read", tool_call_id: "call_0",
      outcome: { Terminal: { Completed: { output: [{ Text: { text } }] } } }, started_at: 0 },
  };
}

const completed = (text: string): ToolResultOutcome => ({ Terminal: { Completed: { output: [{ Text: { text } }] } } });

test("reused upstream ids keep results attached to their own replies", () => {
  const results = toolResults([result(1, 0, "first"), result(3, 0, "second")]);
  expect(results.get(toolKey({ reply: 1, index: 0 }))).toEqual({ outcome: completed("first"), ms: 0 });
  expect(results.get(toolKey({ reply: 3, index: 0 }))).toEqual({ outcome: completed("second"), ms: 0 });
});

test("two blocks with the same upstream id remain distinct", () => {
  const results = toolResults([result(1, 0, "first"), result(1, 2, "second")]);
  expect(results.size).toBe(2);
  expect(results.get(toolKey({ reply: 1, index: 2 }))).toEqual({ outcome: completed("second"), ms: 0 });
});

test("a paginated result retains its source before the reply is loaded", () => {
  const message = result(1, 2, "page");
  const source = toolKey({ reply: 1, index: 2 });
  expect(toolResults([message]).get(source)).toEqual({ outcome: completed("page"), ms: 0 });
  const reply: Message = { id: 1, session_id: 1, created_at: 0, delivered_at: null,
    body: { kind: "Reply", model: "test", stats: { model_ms: 0, first_output_ms: null, usage: null }, blocks: [] } };
  expect(toolResults([reply, message]).get(source)).toEqual({ outcome: completed("page"), ms: 0 });
});

test("elapsed time belongs to terminal execution with a known start", () => {
  expect(toolResult(completed("done"), 100, 350).ms).toBe(250);
  expect(toolResult(completed("done"), null, 350).ms).toBeNull();
  expect(toolResult({ Dispatched: { exec_id: "job-1" } }, 100, 350).ms).toBeNull();
});
