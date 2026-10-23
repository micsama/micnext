use mic_message::SessionId;
use rusqlite::{params, Connection, OptionalExtension};

use crate::row::{self, PERSONA_COLS};
use crate::store::Store;
use crate::{CompactionSettings, Persona, PersonaId, Settings, SettingsError, StoreError};

impl Store {
    pub async fn settings(&self) -> Result<Settings, StoreError> {
        self.call(|conn| Ok(read_settings(conn)?)).await
    }

    pub async fn update_settings(&self, s: Settings) -> Result<(), SettingsError> {
        self.call_settings(move |conn| {
            let tx = conn.transaction()?;
            live_persona(&tx, s.default_persona)?;
            tx.execute(
                "UPDATE core_settings
                 SET default_persona_id = ?1, general_prompt = ?2, default_workdir = ?3,
                     max_turns = ?4, compaction_trigger_percent = ?5, compaction_keep_percent = ?6",
                params![
                    s.default_persona.0,
                    s.general_prompt,
                    s.default_workdir,
                    s.max_turns,
                    s.compaction.trigger_percent,
                    s.compaction.keep_percent
                ],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 未删除的人设，内置在前，其余按 id。
    pub async fn personas(&self) -> Result<Vec<Persona>, StoreError> {
        self.call(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {PERSONA_COLS} FROM core_personas
                 WHERE deleted_at IS NULL ORDER BY builtin DESC, id"
            ))?;
            let rows = stmt.query_map([], row::persona)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    /// 含已删除；id 可能来自外部，不存在返回 `None`。
    pub async fn persona(&self, id: PersonaId) -> Result<Option<Persona>, StoreError> {
        self.call(move |conn| Ok(find_persona(conn, id)?)).await
    }

    pub async fn create_persona(
        &self,
        name: String,
        prompt: String,
        now: i64,
    ) -> Result<PersonaId, SettingsError> {
        self.call_settings(move |conn| {
            let tx = conn.transaction()?;
            ensure_name_free(&tx, &name, None)?;
            tx.execute(
                "INSERT INTO core_personas (name, prompt, builtin, created_at, updated_at)
                 VALUES (?1, ?2, 0, ?3, ?3)",
                params![name, prompt, now],
            )?;
            let id = PersonaId(tx.last_insert_rowid());
            tx.commit()?;
            Ok(id)
        })
        .await
    }

    pub async fn update_persona(
        &self,
        id: PersonaId,
        name: String,
        prompt: String,
        now: i64,
    ) -> Result<(), SettingsError> {
        self.call_settings(move |conn| {
            let tx = conn.transaction()?;
            editable_persona(&tx, id)?;
            ensure_name_free(&tx, &name, Some(id))?;
            tx.execute(
                "UPDATE core_personas SET name = ?1, prompt = ?2, updated_at = ?3 WHERE id = ?4",
                params![name, prompt, now, id.0],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 软删除：已选它的会话与调用记录仍可读到原行。
    pub async fn delete_persona(&self, id: PersonaId, now: i64) -> Result<(), SettingsError> {
        self.call_settings(move |conn| {
            let tx = conn.transaction()?;
            editable_persona(&tx, id)?;
            if read_settings(&tx)?.default_persona == id {
                return Err(SettingsError::IsDefault);
            }
            tx.execute(
                "UPDATE core_personas SET deleted_at = ?1 WHERE id = ?2",
                params![now, id.0],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 会话存在由调用方保证。
    pub async fn set_session_persona(
        &self,
        session: SessionId,
        persona: PersonaId,
    ) -> Result<(), SettingsError> {
        self.call_settings(move |conn| {
            let tx = conn.transaction()?;
            match find_persona(&tx, persona)? {
                None => return Err(SettingsError::PersonaNotFound),
                Some(p) if p.deleted => return Err(SettingsError::Deleted),
                Some(_) => {}
            }
            tx.execute(
                "UPDATE core_sessions SET persona_id = ?1 WHERE id = ?2",
                params![persona.0, session.0],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

pub(crate) fn read_settings(conn: &Connection) -> rusqlite::Result<Settings> {
    conn.query_row(
        "SELECT default_persona_id, general_prompt, default_workdir, max_turns, compaction_trigger_percent, compaction_keep_percent FROM core_settings",
        [],
        |r| {
            Ok(Settings {
                default_persona: PersonaId(r.get(0)?),
                general_prompt: r.get(1)?,
                default_workdir: r.get(2)?,
                max_turns: r.get(3)?,
                compaction: CompactionSettings { trigger_percent: r.get(4)?, keep_percent: r.get(5)? },
            })
        },
    )
}

fn find_persona(conn: &Connection, id: PersonaId) -> rusqlite::Result<Option<Persona>> {
    conn.query_row(
        &format!("SELECT {PERSONA_COLS} FROM core_personas WHERE id = ?1"),
        [id.0],
        row::persona,
    )
    .optional()
}

/// 存在且未删除。
fn live_persona(conn: &Connection, id: PersonaId) -> Result<Persona, SettingsError> {
    match find_persona(conn, id)? {
        Some(p) if !p.deleted => Ok(p),
        _ => Err(SettingsError::PersonaNotFound),
    }
}

/// 存在、未删除且非内置。
fn editable_persona(conn: &Connection, id: PersonaId) -> Result<(), SettingsError> {
    if live_persona(conn, id)?.builtin {
        return Err(SettingsError::Builtin);
    }
    Ok(())
}

fn ensure_name_free(
    conn: &Connection,
    name: &str,
    except: Option<PersonaId>,
) -> Result<(), SettingsError> {
    let taken: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM core_personas
                       WHERE name = ?1 AND deleted_at IS NULL AND id IS NOT ?2)",
        params![name, except.map(|id| id.0)],
        |r| r.get(0),
    )?;
    if taken {
        return Err(SettingsError::NameTaken(name.to_owned()));
    }
    Ok(())
}
