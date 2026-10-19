import { expect, test } from "bun:test";
import { renderMarkdown } from "../src/lib/markdown";

// 输入 → 渲染结果须包含的片段
const cases: [string, string][] = [
  ["**所以不用担心。**你们", "<strong>所以不用担心。</strong>你们"],
  ["**结论：**这里", "<strong>结论：</strong>这里"],
  ["正文**「重点」**继续", "正文<strong>「重点」</strong>继续"],
  ["*斜体。*正文", "<em>斜体。</em>正文"],
  ["***重点。***正文", "<em><strong>重点。</strong></em>正文"],
  ["~~删除。~~正文", "<s>删除。</s>正文"],
  ["__下划线。__正文", "__下划线。__正文"],
  ["a **b** c", "a <strong>b</strong> c"],
  ["`**x。**y`", "<code>**x。**y</code>"],
  ["<script>alert(1)</script>", "&lt;script&gt;alert(1)&lt;/script&gt;"],
];

test("renderMarkdown 回归", () => {
  for (const [input, expected] of cases) expect(renderMarkdown(input)).toContain(expected);
});
