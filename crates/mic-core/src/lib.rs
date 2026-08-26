//! Session/Query/ExecutionContext/Agent 等核心原语与 execute_query 主路径。
//! 不依赖具体 Channel。空壳阶段——execute_query 签名、错误枚举等公开契约
//! 动工前先走 B2。
//! 设计依据：docs/brainstorm/next-gen-architecture.md §三/§四/§五/§七。
