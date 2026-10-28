//! 杂活模型：标题与标签生成、SQL 写入把关等旁路用途共用的一份模型选择。

use mic_store::ClaimedModel;

use crate::provider::{BuiltProvider, Factories};
use crate::ConfigError;

pub(crate) fn provider(
    factories: &Factories,
    model: ClaimedModel,
) -> Result<BuiltProvider, ConfigError> {
    match model {
        ClaimedModel::Missing => Err(ConfigError {
            field: "model_id".into(),
            message: "没有默认模型，请在设置中选择杂活模型".into(),
        }),
        ClaimedModel::Deleted { name, .. } => Err(ConfigError {
            field: "model_id".into(),
            message: format!("杂活模型「{name}」已删除，请重新选择"),
        }),
        ClaimedModel::Selected(m) => factories.build(m),
    }
}
