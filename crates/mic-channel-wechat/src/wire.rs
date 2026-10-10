use serde::{Deserialize, Serialize};

#[derive(Clone, Copy)]
pub(crate) struct MessageId(pub u64);

impl Serialize for MessageId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for MessageId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = MessageId;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("uint64 整数或十进制字符串")
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(MessageId(value))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(E::custom("消息 id 不是十进制 uint64"));
                }
                value
                    .parse()
                    .map(MessageId)
                    .map_err(|_| E::custom("消息 id 超出 uint64 范围"))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Serialize)]
pub(crate) struct BaseInfo {
    pub channel_version: &'static str,
    pub bot_agent: String,
}

#[derive(Serialize)]
pub(crate) struct GetUpdatesRequest<'a> {
    pub get_updates_buf: &'a str,
    pub base_info: BaseInfo,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetUpdatesResponse {
    pub ret: Option<i32>,
    pub errcode: Option<i32>,
    pub errmsg: Option<String>,
    pub msgs: Option<Vec<WechatMessage>>,
    pub sync_buf: Option<String>,
    pub get_updates_buf: Option<String>,
    pub longpolling_timeout_ms: Option<u32>,
}

#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct WechatMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<MessageId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_time_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_state: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_list: Option<Vec<MessageItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_id: Option<MessageId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<MessageId>,
}

#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageItem {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_completed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_msg: Option<RefMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_item: Option<TextItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_item: Option<ImageItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_item: Option<VoiceItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_item: Option<FileItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_item: Option<VideoItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_start_item: Option<ToolCallStartItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_result_item: Option<ToolCallResultItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_item_list: Option<Vec<ButtonItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_bot_username_list: Option<Vec<String>>,
}

/// NOTE: 实测只见过空数组，字段未知；出现非空内容时按报错路径补模型。
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ButtonItem {}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextItem {
    pub text: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefMessage {
    pub message_item: Option<Box<MessageItem>>,
    pub title: Option<String>,
    pub svr_id: Option<String>,
    pub partial_text: Option<PartialText>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialText {
    pub start: String,
    pub end: String,
    pub startindex: u64,
    pub endindex: u64,
    pub quotemd5: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CdnMedia {
    pub encrypt_query_param: Option<String>,
    pub aes_key: Option<String>,
    pub encrypt_type: Option<u32>,
    pub full_url: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageItem {
    pub media: Option<CdnMedia>,
    pub thumb_media: Option<CdnMedia>,
    pub aeskey: Option<String>,
    pub url: Option<String>,
    pub mid_size: Option<u64>,
    pub thumb_size: Option<u64>,
    pub thumb_height: Option<u64>,
    pub thumb_width: Option<u64>,
    pub hd_size: Option<u64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoiceItem {
    pub media: Option<CdnMedia>,
    pub encode_type: Option<u32>,
    pub bits_per_sample: Option<u32>,
    pub sample_rate: Option<u32>,
    pub playtime: Option<u64>,
    pub text: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileItem {
    pub media: Option<CdnMedia>,
    pub file_name: Option<String>,
    pub md5: Option<String>,
    pub len: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VideoItem {
    pub media: Option<CdnMedia>,
    pub video_size: Option<u64>,
    pub play_length: Option<u64>,
    pub video_md5: Option<String>,
    pub thumb_media: Option<CdnMedia>,
    pub thumb_size: Option<u64>,
    pub thumb_height: Option<u64>,
    pub thumb_width: Option<u64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolCallStartItem {
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolCallResultItem {
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SendMessageRequest {
    pub msg: WechatMessage,
    pub base_info: BaseInfo,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendMessageResponse {
    pub message_id: Option<MessageId>,
    pub ret: Option<i32>,
    #[serde(rename = "errmsg")]
    pub _errmsg: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct GetConfigRequest<'a> {
    pub ilink_user_id: &'a str,
    pub context_token: &'a str,
    pub base_info: BaseInfo,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetConfigResponse {
    pub ret: Option<i32>,
    #[serde(rename = "errmsg")]
    pub _errmsg: Option<String>,
    pub typing_ticket: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SendTypingRequest<'a> {
    pub ilink_user_id: &'a str,
    pub typing_ticket: &'a str,
    pub status: u8,
    pub base_info: BaseInfo,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendTypingResponse {
    pub ret: Option<i32>,
    #[serde(rename = "errmsg")]
    pub _errmsg: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct QrRequest<'a> {
    pub local_token_list: Vec<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QrResponse {
    pub ret: Option<i32>,
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QrStatus {
    Wait,
    Scaned,
    Confirmed,
    Expired,
    ScanedButRedirect,
    NeedVerifycode,
    VerifyCodeBlocked,
    BindedRedirect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QrStatusResponse {
    pub ret: Option<i32>,
    pub status: QrStatus,
    pub bot_token: Option<String>,
    pub ilink_bot_id: Option<String>,
    pub baseurl: Option<String>,
    pub ilink_user_id: Option<String>,
    pub redirect_host: Option<String>,
}
