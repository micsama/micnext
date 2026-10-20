# B2：前端 Markdown CJK 强调修正

**状态：CLOSED（2026-10-20）。**

## 一、问题与方向

中文标点紧贴 `**` 时加粗失败，例如 `**所以不用担心。**你们` 原样显示星号。原因是 CommonMark 的 flanking 规则只认空格和标点，不认 CJK 字符。

改用现成插件 `markdown-it-cjk-friendly` 修正，删除工作区里手写的 `chinese_strong` 规则，不并存两套。Markdown 渲染仍只在 Web 展示层，Rust、HTTP、数据库无变更。

## 二、契约

| 边界 | 决定 |
|---|---|
| Web 运行依赖 | 新增 `markdown-it-cjk-friendly@^3.0.0`（传递依赖 `get-east-asian-width`），peer 与现有 markdown-it 15、`@types/markdown-it` 14 兼容 |
| `web/src/lib/markdown.ts` | 单例解析器 `md.use(cjkFriendly)`，删除 `chinese_strong`；`renderMarkdown(source: string): string` 签名不变 |
| 其余渲染配置 | `html: false`、linkify、公式、代码高亮、链接属性均不变 |

插件做法：在该 `md` 实例上把 `inline.State` 换成子类，重写 `scanDelims`，只影响此实例，不改全局原型。

## 三、行为

- `*`、`**`、`***`、`~~` 采用插件的 CJK 规则：紧贴 CJK 字符或全角标点的定界符可开可闭。
- `_`、`__` 不受插件影响，保持 CommonMark。
- 非 CJK 文本输出与 CommonMark 0.31.2 一致（作者以其测试集保证）。
- 不做空格补齐、文本预处理或私有补丁；插件规则不符合预期时报告，不追加手写规则。

## 四、验收

`web/tests/markdown.test.ts` 是这个模块的功能回归测试：一张“输入 → 期望片段”表，用一个用例跑完，覆盖原始反馈、CJK 标点边界、单星号/嵌套/删除线、`__` 不变、英文强调、行内代码和 HTML 转义。新问题往表里加一行即可。放在 `src/` 外是为了不进 svelte-check，也就不需要 `@types/bun`；在 `web/` 下用 `bun test` 运行。

自动检查（已通过）：`bun test`、`bun run check`、`bun run build`、`cargo fmt`、`cargo clippy -- -D warnings`。Human 在页面上确认原始案例和流式输出，通过后标 CLOSED 并归档。

## 五、参考

- [插件](https://github.com/tats-u/markdown-cjk-friendly/tree/main/packages/markdown-it-cjk-friendly)
- [CJK 扩展规则](https://github.com/tats-u/markdown-cjk-friendly/blob/main/specification.md)
