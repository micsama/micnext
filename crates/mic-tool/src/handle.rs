use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mic_message::ContentPart;
use serde::Deserialize;

use crate::schema::parameters_for;
use crate::{Tool, ToolContext, ToolError, ToolSpec};

type ToolResult = Result<Vec<ContentPart>, ToolError>;
type BoxFuture<'a> = Pin<Box<dyn Future<Output = ToolResult> + Send + 'a>>;

/// 类型擦除后的工具，core 以它汇总不同 `Args` 的工具。Clone 廉价。
#[derive(Clone)]
pub struct ToolHandle {
    tool: Arc<dyn Erased>,
    spec: Arc<ToolSpec>,
}

impl ToolHandle {
    pub fn new<T: Tool>(tool: T) -> Self {
        let spec = ToolSpec {
            name: tool.name().to_owned(),
            description: tool.description().to_owned(),
            parameters: parameters_for::<T::Args>(),
        };
        Self {
            tool: Arc::new(tool),
            spec: Arc::new(spec),
        }
    }

    pub fn name(&self) -> &str {
        &self.spec.name
    }

    pub fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    pub fn prompt_hint(&self) -> Option<&str> {
        self.tool.prompt_hint()
    }

    /// 模型输出边界：`args` 解析失败 → `ToolError::input`，不调用 `execute`。
    /// 丢弃 future = 取消，语义同 `Tool::execute`。
    pub fn invoke<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> impl Future<Output = ToolResult> + Send + 'a {
        self.tool.invoke(args, ctx)
    }
}

trait Erased: Send + Sync {
    fn prompt_hint(&self) -> Option<&str>;
    fn invoke<'a>(&'a self, args: &'a serde_json::Value, ctx: &'a ToolContext) -> BoxFuture<'a>;
}

impl<T: Tool> Erased for T {
    fn prompt_hint(&self) -> Option<&str> {
        Tool::prompt_hint(self)
    }

    fn invoke<'a>(&'a self, args: &'a serde_json::Value, ctx: &'a ToolContext) -> BoxFuture<'a> {
        Box::pin(async move {
            if !args.is_object() {
                return Err(ToolError::input("arguments must be a JSON object"));
            }
            let args = T::Args::deserialize(args)
                .map_err(|e| ToolError::input(format!("invalid arguments: {e}")))?;
            self.execute(args, ctx).await
        })
    }
}
