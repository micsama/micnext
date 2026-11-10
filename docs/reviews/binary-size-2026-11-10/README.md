# 二进制体积审查

2026-11-10，HEAD `3524b2f`，macOS arm64，默认 features。仅评估，没有修改实现、依赖或构建选项。

## 实测基线

执行 `./scripts/size.sh --diff HEAD`、`./scripts/size.sh`，以及 cargo-bloat 的 crate 与完整函数排行。当前工作区相对 HEAD：机器码与去符号体积均为零差异。双方共用 web/dist，因此不是对某项优化的收益测试。

| 指标 | 大小 |
|---|---:|
| 带符号文件 | 27.981 MiB |
| 去符号副本 | 21.567 MiB |
| 机器码 | 14.150 MiB |
| `__eh_frame` | 2.073 MiB |
| `__gcc_except_tab` | 0.956 MiB |
| `__unwind_info` | 0.379 MiB |
| web/dist 文件合计 | 0.790 MiB |

cargo-bloat 近似归因：mic_gateway 2.2 MiB、mic_core 1.6 MiB、mic_channel_wechat 997.8 KiB；regex_automata 407.2 KiB、regex_syntax 196.4 KiB。SQLite C 符号多数可能在 Unknown，不能只读 libsqlite3_sys 的 94.8 KiB。源码所属 crate 与最终归因不总是一致。

## 具体结构证据

1. `crates/mic-media/src/lib.rs::normalize` 只输出 PNG/JPEG，PNG 分支使用 `DynamicImage::write_to`。发布产物却包含 WebP encoder 的 `write_image`（12,512 B），还有 GIF encoder 路径。明确 PNG encoder 有望消除通用格式分发带入的代码，同时保留所有输入格式。这是证据充分的实验候选，尚未证明可全部删除；相关依赖既有解码也有编码，不能把整个 image_webp 当收益。
2. `crates/mic-core/src/assembly.rs::run_once` 和 `drive_once` 接受 `impl FnMut`，回调类型穿过较长启动、输入入库与事件等待流程。符号名匹配 run_once 的六项合计 73,216 B，三项大函数为 22,940、22,940、22,104 B。相同显示名称不保证是相同实现，可能包含编译器生成的不同入口/实例；需通过实验确认重复来源。
3. `Engine::run` 显示名对应两项 23,692、22,192 B；`Exec::execute` 两项 14,208、12,596 B。微信 `service::inbound` 单项 40,108 B，比 core 单个大函数更值得一起检查。适合考察准备、执行、收尾的实际生命周期；不能把拆函数等同于减少状态机代码。
4. 全量符号中名字包含 `Handler` 的 318 项合计 451,160 B（440.6 KiB），最大约 10 KiB。此统计按名字匹配，含业务内联与必要提取代码，并非纯框架重复。当前 service.rs 没有自定义缩小 handler Future 的适配层。可以试少量固定 Response/装箱边界，保留强类型提取、错误语义与路由契约。
5. 名称含 `drop_in_place` 的全局符号共约 2.64 MiB，分散在 11,954 项。这是生成代码的观测，不代表可以删除；与 async 候选可能重叠，也不能与 crate 大小相加。必须保持取消后的资源释放和后台任务所有权。
6. `web/vite.config.ts` 已采用 brotli 单份嵌入及 KaTeX woff2 清理。最大文件图标 chunk 169,234 B（165.3 KiB），包含 manifest 引用的完整图标 data URL。可比较保持同一图标集合的编码方式；删减图标集合属于体验取舍。

## 依赖判断

`cargo tree -d --depth 1` 有 aes/cipher/digest 的两代依赖、base64 0.22/0.23、三个 getrandom 和两代 syn。syn 属于构建期，不应当作交付代码膨胀证据。AES 的两代分别服务微信 ECB 与存储 AES-GCM，base64 另一代来自 reqwest；统一版本涉及依赖与 API 成本，尚无证据表明收益优于上述候选。

HTTP 客户端已共用 reqwest 0.13，多个消费者不会各自带一份完整依赖。image、axum、reqwest 都已限制 feature。regex 由脱敏、文件检索、忽略规则等实际功能消费；不能删除 core 的一个直接依赖就把整个正则族大小算作节省。

候选排序与决策边界见 [B1](../../brainstorm/binary-size.md)。本次没有实现改动，不执行实现收尾检查。cargo-bloat 构建会覆盖 target/release/micnext 为带符号产物；本报告的去符号值来自副本，未替换正式产物。
