import MarkdownIt from "markdown-it";
import texmath from "markdown-it-texmath";
import katex from "katex";
import hljs from "highlight.js/lib/common";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import nginx from "highlight.js/lib/languages/nginx";
import powershell from "highlight.js/lib/languages/powershell";
import "katex/dist/katex.min.css";

hljs.registerLanguage("dockerfile", dockerfile);
hljs.registerLanguage("nginx", nginx);
hljs.registerLanguage("powershell", powershell);

// 模型输出不可信：原始 HTML 一律转义（html: false），公式不开放可执行扩展（trust: false）。
const md = new MarkdownIt({ html: false, linkify: true, breaks: false });

// texmath 的块级 `\[…\]` 不能打断段落；模型常把它写在句中或紧跟正文行，补一条行内显示公式规则。
texmath.rules.bracketsDisplay = {
  inline: [{ name: "math_inline_bracket", rex: /\\\[([^`]+?)\\\]/gy, tmpl: "$1", tag: "\\[", displayMode: true }],
  block: [],
};

md.use(texmath, {
  engine: katex,
  delimiters: ["dollars", "brackets", "bracketsDisplay"],
  katexOptions: { throwOnError: false, trust: false, strict: "ignore" },
});

md.renderer.rules.fence = (tokens, idx) => {
  const token = tokens[idx]!;
  const lang = token.info.trim().split(/\s+/)[0] ?? "";
  const code = token.content;
  const body =
    lang && hljs.getLanguage(lang)
      ? hljs.highlight(code, { language: lang, ignoreIllegals: true }).value
      : md.utils.escapeHtml(code);
  return (
    `<div class="code-block"><div class="code-head"><span>${md.utils.escapeHtml(lang)}</span>` +
    `<button type="button" class="copy-code">复制</button></div>` +
    `<pre><code class="hljs">${body}</code></pre></div>`
  );
};

const defaultLinkOpen =
  md.renderer.rules.link_open ?? ((tokens, idx, options, _env, self) => self.renderToken(tokens, idx, options));
md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  tokens[idx]!.attrSet("target", "_blank");
  tokens[idx]!.attrSet("rel", "noopener noreferrer");
  return defaultLinkOpen(tokens, idx, options, env, self);
};

export function renderMarkdown(source: string): string {
  return md.render(source);
}
