//! 用户入站图片：字节随消息同事务落库，按会话归属读取。

use std::collections::HashMap;

use mic_message::{
    ContentPart, ImageData, ImageFormat, ImageId, ImageRef, Message, MessageBody, PersonId,
    SessionId,
};
use rusqlite::{params, OptionalExtension};

use crate::store::insert_message;
use crate::{InputDisposition, NewInputPart, NewNotice, Store, StoreError};

fn format_str(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::WebP => "webp",
    }
}

fn parse_format(s: &str) -> Option<ImageFormat> {
    Some(match s {
        "png" => ImageFormat::Png,
        "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        _ => return None,
    })
}

fn data(format: String, bytes: Vec<u8>) -> Result<ImageData, StoreError> {
    Ok(ImageData {
        format: parse_format(&format)
            .ok_or_else(|| StoreError::Payload(serde::de::Error::custom("未知图片格式")))?,
        bytes: bytes.into(),
    })
}

impl Store {
    /// 写一条用户输入，可附一条通知；图片行、消息、hold 行与通知同事务，
    /// 片段顺序保持。返回按 id 升序的输入与通知。
    pub async fn append_input(
        &self,
        session_id: SessionId,
        person: PersonId,
        parts: Vec<NewInputPart>,
        disposition: InputDisposition,
        notice: Option<NewNotice>,
        at: i64,
    ) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let mut out = Vec::with_capacity(parts.len());
            for part in parts {
                out.push(match part {
                    NewInputPart::Text(text) => ContentPart::Text { text },
                    NewInputPart::Image(img) => {
                        tx.execute(
                            "INSERT INTO core_images (session_id, format, size_bytes, bytes)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![
                                session_id.0,
                                format_str(img.format),
                                img.bytes.len() as i64,
                                &*img.bytes
                            ],
                        )?;
                        ContentPart::Image(ImageRef {
                            id: ImageId(tx.last_insert_rowid()),
                        })
                    }
                });
            }
            let body = MessageBody::UserInput { person, parts: out };
            let id = insert_message(&tx, session_id, None, &body, at)?;
            if let InputDisposition::Held = disposition {
                tx.execute(
                    "INSERT INTO core_input_holds (message_id, held_at) VALUES (?1, ?2)",
                    params![id.0, at],
                )?;
            }
            let mut messages = vec![Message {
                id,
                session_id,
                body,
                created_at: at,
                delivered_at: None,
            }];
            if let Some(NewNotice { source, text }) = notice {
                let body = MessageBody::Notification { source, text };
                messages.push(Message {
                    id: insert_message(&tx, session_id, None, &body, at)?,
                    session_id,
                    body,
                    created_at: at,
                    delivered_at: None,
                });
            }
            tx.commit()?;
            Ok(messages)
        })
        .await
    }

    /// 按会话归属读取；id 可能来自外部，不属于该会话或不存在返回 `None`。
    pub async fn image(
        &self,
        session_id: SessionId,
        id: ImageId,
    ) -> Result<Option<ImageData>, StoreError> {
        self.call(move |conn| {
            let found = conn
                .query_row(
                    "SELECT format, bytes FROM core_images WHERE id = ?1 AND session_id = ?2",
                    params![id.0, session_id.0],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
                )
                .optional()?;
            found.map(|(f, b)| data(f, b)).transpose()
        })
        .await
    }

    /// 消息里引用的图片必在库中；缺失是库损坏，返回 `ImageMissing`。
    pub async fn images(
        &self,
        session_id: SessionId,
        ids: Vec<ImageId>,
    ) -> Result<HashMap<ImageId, ImageData>, StoreError> {
        self.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT format, bytes FROM core_images WHERE id = ?1 AND session_id = ?2",
            )?;
            let mut out = HashMap::with_capacity(ids.len());
            for id in ids {
                let (f, b) = stmt
                    .query_row(params![id.0, session_id.0], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
                    })
                    .optional()?
                    .ok_or(StoreError::ImageMissing(id.0))?;
                out.insert(id, data(f, b)?);
            }
            Ok(out)
        })
        .await
    }
}
