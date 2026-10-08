//! 内置人设：源码为唯一真相，每次打开库时按固定 id 同步，覆盖名字与提示词。
//! id 1 是兜底人设，永不下线。

use rusqlite::{params, Connection};

use crate::StoreError;

struct Builtin {
    id: i64,
    name: &'static str,
    prompt: &'static str,
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        id: 1,
        name: "默认",
        prompt: include_str!("personas/default.md"),
    },
    Builtin {
        id: 2,
        name: "大肥鱼",
        prompt: include_str!("personas/whale.md"),
    },
    Builtin {
        id: 3,
        name: "理性大脑",
        prompt: include_str!("personas/rational.md"),
    },
];

/// 同名自建人设让位（加后缀）；源码里已没有的旧内置人设软删除；默认人设失效则回到 id 1。
pub(crate) fn sync(conn: &mut Connection, now: i64) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    for b in BUILTINS {
        let prompt = b.prompt.trim_end();
        tx.execute(
            "UPDATE core_personas SET name = name || '（自建）', updated_at = ?3
             WHERE name = ?1 AND id != ?2 AND deleted_at IS NULL",
            params![b.name, b.id, now],
        )?;
        tx.execute(
            "INSERT INTO core_personas (id, name, prompt, builtin, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name, prompt = excluded.prompt, builtin = 1, deleted_at = NULL,
               updated_at = CASE WHEN name = excluded.name AND prompt = excluded.prompt
                                  AND deleted_at IS NULL THEN updated_at ELSE excluded.updated_at END",
            params![b.id, b.name, prompt, now],
        )?;
    }
    let ids = BUILTINS
        .iter()
        .map(|b| b.id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    tx.execute(
        &format!(
            "UPDATE core_personas SET deleted_at = ?1
             WHERE builtin = 1 AND deleted_at IS NULL AND id NOT IN ({ids})"
        ),
        [now],
    )?;
    tx.execute(
        "UPDATE core_settings SET default_persona_id = 1
         WHERE default_persona_id NOT IN (SELECT id FROM core_personas WHERE deleted_at IS NULL)",
        [],
    )?;
    tx.commit()?;
    Ok(())
}
