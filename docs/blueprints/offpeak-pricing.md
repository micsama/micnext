# B2：服务商分时段计价

**状态：搁置，以后做；现按高峰价计（2026-10-15 human 定）。**

依据：DeepSeek 按空闲/高峰两档计价，高峰为空闲的 2 倍（2026-10-15 human 提供价格表）。human 已定：时段由服务商决定，挂在服务商上；单价仍按模型填。币种保持美元，不在本 B2 处理。

## 一、用户看到什么

- 设置 → 服务商编辑：「计费时段」可选开启，填空闲时段的起止时刻与所在时区（UTC 偏移，DeepSeek 预设默认 +08:00）。不开启 = 全天一个价，同现状。
- 模型计费设置：开启时段后，现有四项单价旁多一组「空闲价」；空闲价各项留空 = 与高峰同价。
- trace、看板的费用按每次调用的开始时间所在时段取价。悬停费用时注明「按空闲价」/「按高峰价」。仍是按当前单价估算，不存历史单价。

## 二、公开契约变更

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| schema `core_endpoints` | 增加 `offpeak_utc_offset`（分钟）、`offpeak_start`、`offpeak_end`（当地零点起的分钟，0–1439），三者同为空或同不为空；起止可跨零点 | 迁移新列初始为空 |
| schema `core_models` | 增加 `price_offpeak_input`、`price_offpeak_cache_read`、`price_offpeak_cache_write`、`price_offpeak_output`，可空、≥0 | 同上 |
| `mic-store` 类型 | `OffpeakWindow { utc_offset_minutes: i32, start: u16, end: u16 }`；`EndpointView` / `EndpointWrite` 增 `offpeak: Option<OffpeakWindow>`；`ModelPrice` 不变，模型增 `offpeak_price: ModelPrice` | 设置读写、Gateway DTO |
| `call_cost` | 签名改为 `call_cost(usage, price: &ModelPrice, offpeak: Option<(&OffpeakWindow, &ModelPrice)>, started_at: i64)`；空闲时段内逐项取空闲价，该项为空回落到高峰价 | trace 调用行与看板聚合，口径一处 |
| `GET/PUT` 服务商与模型接口 | 服务商 DTO 增 `offpeak: { utc_offset_minutes, start, end } \| null`；模型价格 DTO 增 `offpeak_price`（四项同现有） | Web 设置页 |
| trace call DTO | 增 `offpeak: bool`（是否按空闲价计） | 费用悬停提示 |

空闲价逐项回落到高峰价是该设置本身的语义（「空闲价没写的项不打折」），不是错误兜底；回落后仍无价，按现有规则费用未知。

## 三、验收

- store 测试：窗口内/外、跨零点窗口、空闲价部分为空的取价；未设窗口与现状一致。
- 设置页能开关时段、填空闲价；trace 同一模型在空闲与高峰时段的调用费用不同，悬停可见依据。
- `cargo fmt`、`cargo clippy -- -D warnings`、相关测试，`web/` 下 `bun run check`。
