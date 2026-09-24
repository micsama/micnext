use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema};

/// 所有工具共用的一套生成设置：子 schema 内联，无 `$schema`/`title`，`Option` 只表现为非必填。
pub(crate) fn parameters_for<T: JsonSchema>() -> serde_json::Value {
    let mut settings = SchemaSettings::draft2020_12();
    settings.inline_subschemas = true;
    settings.meta_schema = None;
    settings
        .transforms
        .push(Box::new(RecursiveTransform(drop_null_type)));
    let mut schema = settings.into_generator().into_root_schema_for::<T>();
    schema.remove("title");
    schema.to_value()
}

fn drop_null_type(schema: &mut Schema) {
    let Some(serde_json::Value::Array(types)) = schema.get_mut("type") else {
        return;
    };
    types.retain(|t| t != "null");
    if let [only] = types.as_slice() {
        let only = only.clone();
        schema.insert("type".into(), only);
    }
}
