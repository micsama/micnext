//! 会话标签：受控词表与会话的对应。契约：docs/blueprints/archive/session-tags.md。

use mic_message::SessionId;
use rusqlite::{params, Connection};

use crate::limits::{TAGS_PER_SESSION, TAG_CHARS};
use crate::sessions::ensure_active;
use crate::{Store, StoreError, Tag, TagId};

/// 标签写入的业务错误；文案面向用户。
#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("标签名不能为空")]
    Empty,
    #[error("标签名最多 {TAG_CHARS} 个字")]
    TooLong,
    #[error("标签名不能含空白、逗号或顿号")]
    Separator,
    #[error("已有同名标签「{0}」")]
    NameTaken(String),
    #[error("标签不存在")]
    NotFound,
    #[error("每个会话最多 {TAGS_PER_SESSION} 个标签")]
    TooMany,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for TagError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.into())
    }
}

/// 标题输出第二行与标签名共用的分隔符。
pub fn is_tag_separator(c: char) -> bool {
    matches!(c, '、' | ',' | '，') || c.is_whitespace()
}

/// 校验过的标签名：去掉首尾空白、非空、不超长、不含分隔符。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagName(String);

impl TagName {
    pub fn parse(raw: &str) -> Result<Self, TagError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(TagError::Empty);
        }
        if name.chars().count() > TAG_CHARS {
            return Err(TagError::TooLong);
        }
        if name.chars().any(is_tag_separator) {
            return Err(TagError::Separator);
        }
        Ok(Self(name.to_owned()))
    }
}

impl Store {
    /// 词表，id 升序。
    pub async fn tags(&self) -> Result<Vec<Tag>, StoreError> {
        self.call(|conn| Ok(read_tags(conn)?)).await
    }

    pub async fn create_tag(&self, name: TagName, now: i64) -> Result<TagId, TagError> {
        self.blocking(move |conn| {
            ensure_free(conn, &name, None)?;
            conn.execute(
                "INSERT INTO core_tags (name, created_at) VALUES (?1, ?2)",
                params![name.0, now],
            )?;
            Ok(TagId(conn.last_insert_rowid()))
        })
        .await
    }

    pub async fn rename_tag(&self, id: TagId, name: TagName) -> Result<(), TagError> {
        self.blocking(move |conn| {
            ensure_free(conn, &name, Some(id))?;
            match conn.execute(
                "UPDATE core_tags SET name = ?2 WHERE id = ?1",
                params![id.0, name.0],
            )? {
                0 => Err(TagError::NotFound),
                _ => Ok(()),
            }
        })
        .await
    }

    /// 连同各会话上的这个标签一起删除。
    pub async fn delete_tag(&self, id: TagId) -> Result<(), TagError> {
        self.blocking(move |conn| {
            match conn.execute("DELETE FROM core_tags WHERE id = ?1", [id.0])? {
                0 => Err(TagError::NotFound),
                _ => Ok(()),
            }
        })
        .await
    }

    /// 手动设置 Root 会话的标签，之后标题生成不再改它。
    pub async fn set_session_tags(
        &self,
        session: SessionId,
        tags: Vec<TagId>,
    ) -> Result<(), TagError> {
        self.blocking(move |conn| {
            let mut tags = tags;
            tags.sort();
            tags.dedup();
            if tags.len() > TAGS_PER_SESSION {
                return Err(TagError::TooMany);
            }
            let tx = conn.transaction()?;
            ensure_active(&tx, session)?;
            for tag in &tags {
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM core_tags WHERE id = ?1)",
                    [tag.0],
                    |r| r.get(0),
                )?;
                if !exists {
                    return Err(TagError::NotFound);
                }
            }
            tx.execute(
                "UPDATE core_sessions SET tags_manual = 1 WHERE id = ?1",
                [session.0],
            )?;
            replace_session_tags(&tx, session, &tags)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 标题生成可选的标签；会话标签是手动定的则为空，这次只生成标题。
    pub async fn title_tag_choices(&self, session: SessionId) -> Result<Vec<Tag>, StoreError> {
        self.call(move |conn| {
            let manual: bool = conn.query_row(
                "SELECT tags_manual FROM core_sessions WHERE id = ?1",
                [session.0],
                |r| r.get(0),
            )?;
            Ok(if manual { Vec::new() } else { read_tags(conn)? })
        })
        .await
    }
}

fn read_tags(conn: &Connection) -> rusqlite::Result<Vec<Tag>> {
    conn.prepare(
        "SELECT t.id, t.name, (SELECT count(*) FROM core_session_tags st WHERE st.tag_id = t.id)
         FROM core_tags t ORDER BY t.id",
    )?
    .query_map([], |r| {
        Ok(Tag {
            id: TagId(r.get(0)?),
            name: r.get(1)?,
            sessions: r.get(2)?,
        })
    })?
    .collect()
}

fn ensure_free(conn: &Connection, name: &TagName, except: Option<TagId>) -> Result<(), TagError> {
    let taken: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM core_tags WHERE name = ?1 AND id IS NOT ?2)",
        params![name.0, except.map(|id| id.0)],
        |r| r.get(0),
    )?;
    if taken {
        return Err(TagError::NameTaken(name.0.clone()));
    }
    Ok(())
}

/// 生成期间被删的标签直接跳过。
pub(crate) fn replace_session_tags(
    conn: &Connection,
    session: SessionId,
    tags: &[TagId],
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM core_session_tags WHERE session_id = ?1",
        [session.0],
    )?;
    for tag in tags {
        conn.execute(
            "INSERT INTO core_session_tags (session_id, tag_id)
             SELECT ?1, id FROM core_tags WHERE id = ?2",
            params![session.0, tag.0],
        )?;
    }
    Ok(())
}

/// 备份导入：补上缺少的名字，已有标签和会话上的标签不动。
pub(crate) fn import_names(conn: &Connection, names: &[String], now: i64) -> Result<(), TagError> {
    for raw in names {
        let name = TagName::parse(raw)?;
        conn.execute(
            "INSERT INTO core_tags (name, created_at) VALUES (?1, ?2) ON CONFLICT(name) DO NOTHING",
            params![name.0, now],
        )?;
    }
    Ok(())
}

pub(crate) fn export_names(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT name FROM core_tags ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect()
}
