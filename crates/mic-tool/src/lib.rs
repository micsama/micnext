//! 静态 `Tool` trait（Args+name()+execute()->ToolOutcome）；不依赖 mic-store，
//! 需要的 port trait（如 CronRegistry）自带、由 mic-core 注入实现（依赖倒置）。
//! 空壳阶段——trait 签名、ToolError 分类等公开契约动工前先走 B2。
//! 设计依据：docs/brainstorm/next-gen-architecture.md §三/§六/§七.1。
