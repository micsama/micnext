use mic_message::PersonId;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::store::Store;
use crate::{Identity, Person, StoreError};

impl Store {
    pub async fn ensure_person(&self, name: &str, now: i64) -> Result<PersonId, StoreError> {
        let name = name.to_owned();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let id = ensure_person(&tx, &name, now)?;
            tx.commit()?;
            Ok(id)
        })
        .await
    }

    /// 返回改绑前的 person（原先未绑定则 None）。
    pub async fn bind_identity(
        &self,
        identity: Identity,
        person: PersonId,
        now: i64,
    ) -> Result<Option<PersonId>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let old = identity_person(&tx, &identity)?;
            // NOTE: 新建绑定时 display_name 留空，待该身份下次入站经 resolve_identity 刷新。
            tx.execute(
                "INSERT INTO core_person_identities
                   (channel, external_id, person_id, display_name, created_at)
                 VALUES (?1, ?2, ?3, '', ?4)
                 ON CONFLICT(channel, external_id) DO UPDATE SET person_id = excluded.person_id",
                params![identity.channel, identity.external_id, person.0, now],
            )?;
            tx.commit()?;
            Ok(old)
        })
        .await
    }

    /// 已绑定则刷新 display_name 并返回其 person；未绑定则绑到 `person`。
    pub async fn resolve_identity(
        &self,
        identity: Identity,
        display_name: &str,
        person: PersonId,
        now: i64,
    ) -> Result<PersonId, StoreError> {
        let display_name = display_name.to_owned();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let bound = match identity_person(&tx, &identity)? {
                Some(bound) => {
                    tx.execute(
                        "UPDATE core_person_identities SET display_name = ?3
                         WHERE channel = ?1 AND external_id = ?2",
                        params![identity.channel, identity.external_id, display_name],
                    )?;
                    bound
                }
                None => {
                    tx.execute(
                        "INSERT INTO core_person_identities
                           (channel, external_id, person_id, display_name, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            identity.channel,
                            identity.external_id,
                            person.0,
                            display_name,
                            now
                        ],
                    )?;
                    person
                }
            };
            tx.commit()?;
            Ok(bound)
        })
        .await
    }

    pub async fn person(&self, id: PersonId) -> Result<Person, StoreError> {
        self.call(move |conn| {
            Ok(conn.query_row(
                "SELECT id, name, created_at FROM core_persons WHERE id = ?1",
                [id.0],
                |r| {
                    Ok(Person {
                        id: PersonId(r.get(0)?),
                        name: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                },
            )?)
        })
        .await
    }
}

fn ensure_person(tx: &Transaction<'_>, name: &str, now: i64) -> Result<PersonId, StoreError> {
    tx.execute(
        "INSERT INTO core_persons (name, created_at) VALUES (?1, ?2) ON CONFLICT(name) DO NOTHING",
        params![name, now],
    )?;
    Ok(PersonId(tx.query_row(
        "SELECT id FROM core_persons WHERE name = ?1",
        [name],
        |r| r.get(0),
    )?))
}

fn identity_person(
    tx: &Transaction<'_>,
    identity: &Identity,
) -> Result<Option<PersonId>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT person_id FROM core_person_identities WHERE channel = ?1 AND external_id = ?2",
            params![identity.channel, identity.external_id],
            |r| r.get(0),
        )
        .optional()?
        .map(PersonId))
}
