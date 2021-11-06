// store entities, statements, opinions persistently
use std::str::FromStr;

use libp2p::identity::Keypair;
use log::info;
// library imports
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow},
    ConnectOptions, Error, Row,
};

// own imports
use super::model::{Entity, EntityType, SignedOpinion, Statement, Template, Trust};

const DATABASE_URL: &str = "sqlite:reputation.sqlite3?mode=rwc";

/// The database id type, i64 for PostgreSQL (the only supported database backend at the moment).
pub type Id = i64;

/// Status of a possibly pre-existing persistent item.
#[derive(PartialEq)]
pub enum PersistStatus {
    New,
    PartiallyNew,
    Old,
}

/// The result of persisting one database item.
pub struct PersistResult {
    /// Old or new Id of the persisted item.
    pub id: Id,
    /// Whether the item was completely new, partially new (some associated data was already present, or old)
    pub status: PersistStatus,
}
/// The storage menchanism for all data shared via the net.
/// Currently does not include caches.
pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    /// create a new initialized instance of the database.
    /// existing outdated entities, statements and opinions will be cleaned up
    pub async fn new() -> Self {
        let mut options = SqliteConnectOptions::from_str(DATABASE_URL).unwrap();
        options.log_statements(log::LevelFilter::Debug);
        let db = Self {
            pool: SqlitePoolOptions::new()
                .max_connections(5)
                .connect_with(options)
                .await
                .unwrap(),
        };
        db.initialize_database().await.expect("could initialize");
        db.cleanup().await.expect("could cleanup");
        db
    }


    /// initialize the database with the schema and well-known facts
    /// this should be idempotent, i.e. if the database is already initialized it should do nothing,
    /// but for a partially initialized database it should complete initialization.
    async fn initialize_database(&self) -> Result<(), Error> {
        let defs = vec![
            "create table entity(
                id integer primary key,
                kind integer not null,
                value text unique not null
            )",
            "create table statement(
                id integer primary key,
                template_id integer not null,
                first_entity_id integer not null,
                foreign key(template_id) references entity(id),
                foreign key(first_entity_id) references entity(id)
            )",
            "create table statement_entity(
                statement_id integer not null,
                position integer not null,
                entity_id integer not null,
                foreign key(statement_id) references statement(id),
                foreign key(entity_id) references entity(id),
                unique(statement_id,position)
            )",
            "create table opinion(
                id integer primary key,
                statement_id integer not null,
                signer_id integer not null,
                date integer not null,
                valid integer not null,
                serial integer not null,
                certainty integer not null,
                signature text,
                foreign key(statement_id) references statement(id),
                foreign key(signer_id) references entity(id)
            )",
            "create table trust(
                signer_id integer not null,
                level integer not null,
                key text,
                foreign key(signer_id) references entity(id)
            )",
        ];

        // create tables if necessary
        for def in defs {
            match sqlx::query(def).execute(&self.pool).await {
                Ok(result) => println!("{:?}", result),
                _ => (),
            }
        }

        // insert the root template, this is currently manual
        let entity = Entity::from_str("template(Template)").unwrap();
        self.persist_entity(&entity).await?;
        let statement = Statement::from_str("template(template(Template))").unwrap();
        self.persist_statement(&statement).await?;

        // make sure an owner trust entry exists
        self.owner_trust().await?;
        Ok(())
    }

    /// Select or insert an entity.
    pub async fn persist_entity(&self, entity: &Entity) -> Result<PersistResult, Error> {
        let string = entity.to_string();
        let kind = entity.entity_type() as i64;

        let result = sqlx::query("select id from entity where kind = ? and value = ?")
            .bind(kind)
            .bind(&string)
            .map(|row: SqliteRow| -> Id { row.get::<Id, &str>("id") })
            .fetch_optional(&self.pool)
            .await;
        match result {
            Ok(opt) => {
                if let Some(id) = opt {
                    Ok(PersistResult::old(id))
                } else {
                    let mut tx = self.pool.begin().await.unwrap();
                    sqlx::query("insert into entity(kind, value) values(?,?)")
                        .bind(kind)
                        .bind(string)
                        .execute(&mut tx)
                        .await
                        .expect("insert entity");
                    let id = sqlx::query("select last_insert_rowid()")
                        .map(|row: SqliteRow| -> Id { row.get::<Id, usize>(0) })
                        .fetch_one(&mut tx)
                        .await?;
                    tx.commit().await?;
                    Ok(PersistResult::new(id))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Return the entity with the given Id.
    pub async fn get_entity(&self, id: Id) -> Result<Option<Entity>, Error> {
        let result = sqlx::query("select value from entity where id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(if let Some(row) = result {
            let value = row.get::<String, usize>(0);
            Some(Entity::from_str(&value).expect("parseable entity"))
        } else {
            None
        })
    }

    // return all entities with a given kind
    pub async fn list_entities(&self, kind: EntityType) -> Result<Vec<Entity>, Error> {
        let result = sqlx::query("select value from entity where kind=?")
            .bind(kind as i64)
            .fetch_all(&self.pool)
            .await;
        match result {
            Ok(rows) => {
                let list = rows
                    .iter()
                    .map(|row| {
                        let value = row.get::<String, usize>(0);
                        info!("parsing entity {}", value);
                        Entity::from_str(&value).expect("parseable entity")
                    })
                    .collect();
                Ok(list)
            }
            Err(e) => Err(e),
        }
    }

    #[allow(dead_code)] // used in tests
    pub async fn persist_template(&self, template: &Template) -> Result<PersistResult, Error> {
        self.persist_entity(&Entity::Template(template.clone()))
            .await
    }

    pub async fn find_matching_template(
        &self,
        statement: &Statement,
    ) -> Result<PersistResult, Error> {
        let rows: Vec<SqliteRow> =
            sqlx::query("select id,value from entity where kind=? and value like ?")
                .bind(EntityType::Template as i64)
                .bind(format!("{}(%", statement.name))
                .fetch_all(&self.pool)
                .await?;
        for row in rows {
            let id = row.get::<Id, usize>(0);
            let template = Template::from_str(&row.get::<String, usize>(1)).unwrap();
            if statement.matches_template(&template) {
                return Ok(PersistResult::old(id));
            }
        }
        Err(Error::RowNotFound)
    }

    pub async fn list_templates(&self, _name: &str) -> Vec<Template> {
        vec![]
    }

    pub async fn persist_statement(&self, statement: &Statement) -> Result<PersistResult, Error> {
        // lookup entities before creating our own transaction, as we only want to use one transaction at a time

        let template_id = self.find_matching_template(statement).await?.id;
        let mut entity_ids: Vec<Id> = vec![];
        for entity in &statement.entities {
            let entity_id = self.persist_entity(entity).await?.id;
            entity_ids.push(entity_id);
        }
        let first_entity_id = entity_ids.remove(0);
        let mut query_s = "select s.id from statement s".to_string();
        for (pos, _entity_id) in entity_ids.iter().enumerate() {
            query_s.push_str(&format!(
                "
                join statement_entity e{}
                on e{}.statement_id=s.id
                and e{}.position={}
                and e{}.entity_id=?",
                pos, pos, pos, pos, pos
            ));
        }
        query_s.push_str(" where s.template_id=? and s.first_entity_id=?");
        let mut query = sqlx::query(&query_s);
        for entity_id in &entity_ids {
            query = query.bind(entity_id);
        }
        query = query.bind(template_id).bind(first_entity_id);
        let result = query
            .map(|row: SqliteRow| -> Id { row.get::<Id, &str>("id") })
            .fetch_optional(&self.pool)
            .await?;
        match result {
            Some(id) => Ok(PersistResult::old(id)),
            None => {
                let mut tx = self.pool.begin().await.unwrap();
                sqlx::query("insert into statement(template_id,first_entity_id) values(?,?)")
                    .bind(template_id)
                    .bind(first_entity_id)
                    .execute(&mut tx)
                    .await
                    .expect("could not insert statement");
                let id = sqlx::query("select last_insert_rowid()")
                    .map(|row: SqliteRow| -> Id { row.get::<Id, usize>(0) })
                    .fetch_one(&mut tx)
                    .await?;
                for (pos, entity_id) in entity_ids.iter().enumerate() {
                    sqlx::query("insert into statement_entity(statement_id, position, entity_id) values(?,?,?)")
                    .bind(id)
                    .bind(pos as Id)
                    .bind(entity_id)
                    .execute(&mut tx)
                    .await
                    .expect("could not insert statement_entity");
                }
                tx.commit().await?;
                Ok(PersistResult::new(id))
            }
        }
    }

    pub async fn persist_statement_hashing_emails(
        &mut self,
        statement: &Statement,
    ) -> Result<(PersistResult, Statement), Error> {
        // if the statement template can't be found, retry with hashed e-mails
        // the return value include the possibly translated statement
        match self.persist_statement(statement).await {
            Ok(result) => Ok((result, statement.clone())),
            Err(_) => {
                let hashed_statement = statement.hash_emails();
                let result = self.persist_statement(&hashed_statement).await;
                match result {
                    Ok(result) => Ok((result, hashed_statement)),
                    Err(e) => Err(e),
                }
            }
        }
    }

    pub async fn persist_opinion(
        &self,
        signed_opinion: &SignedOpinion,
        statement_id: Id,
    ) -> Result<PersistResult, Error> {
        // this actually persists a signed opinion. Raw opinions without signature are only used for temporary purposes.
        let signer_result = self
            .persist_entity(&Entity::Signer(signed_opinion.signer.clone()))
            .await
            .unwrap();
        let opinion = &signed_opinion.opinion;

        let prev_opinion_result = sqlx::query(
            "select id,date,serial from opinion where statement_id = ? and signer_id = ?",
        )
        .bind(statement_id)
        .bind(signer_result.id)
        .map(|row: SqliteRow| -> (Id, u32, u8) {
            (
                row.get::<Id, &str>("id"),
                row.get::<u32, &str>("date"),
                row.get::<u8, &str>("serial"),
            )
        })
        .fetch_optional(&self.pool)
        .await?;
        if let Some((old_id, date, serial)) = prev_opinion_result {
            if date < opinion.date || (date == opinion.date && serial < opinion.serial) {
                // delete old, overridden opinion
                sqlx::query("delete from opinion where id = ?")
                    .bind(old_id)
                    .execute(&self.pool)
                    .await
                    .expect("could delete old opinion");
            } else {
                return Ok(PersistResult::old(old_id));
            }
        }
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("insert into opinion(statement_id, signer_id, date, valid, serial, certainty, signature) values(?,?,?,?,?,?,?)")
            .bind(statement_id)
            .bind(signer_result.id)
            .bind(opinion.date)
            .bind(opinion.valid)
            .bind(opinion.serial)
            .bind(opinion.certainty)
            .bind(base64::encode(&signed_opinion.signature))
            .execute(&mut tx)
            .await
            .expect("insert signed opinion");
        let id = sqlx::query("select last_insert_rowid()")
            .map(|row: SqliteRow| -> Id { row.get::<Id, usize>(0) })
            .fetch_one(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(PersistResult::new(id))
    }

    pub async fn owner_trust(&self) -> Result<Trust, Error> {
        if let Ok(row) = sqlx::query("select signer_id, key from trust where level = 0")
            .fetch_one(&self.pool)
            .await
        {
            let entity_id = row.get::<Id, usize>(0);
            let key_bytes = base64::decode(row.get::<String, usize>(1)).expect("base64 decode");
            let privkey = libp2p::identity::secp256k1::SecretKey::from_bytes(key_bytes)
                .expect("secp256k1 decode");
            let signer = self.get_entity(entity_id).await?.unwrap();
            let keypair = Keypair::Secp256k1(libp2p::identity::secp256k1::Keypair::from(privkey));
            Ok(Trust {
                signer: signer,
                level: 0,
                key: Some(keypair),
            })
        } else {
            let trust = Trust::new();
            let persist_result = self.persist_entity(&trust.signer).await?;
            let privkey = trust.privkey_string();
            println!("trust {} {}", persist_result.id, privkey);
            let mut tx = self.pool.begin().await.unwrap();
            sqlx::query("insert into trust(signer_id, level, key) values(?,0,?)")
                .bind(persist_result.id)
                .bind(privkey)
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            Ok(trust)
        }
    }

    /* Clean up opinions which are not valid anymore. */
    pub async fn cleanup_opinions(&self) -> Result<(), Error> {
        sqlx::query("delete from opinion where date + valid < ?")
            .bind(super::model::today())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /* Clean up statements without opinions. */
    pub async fn cleanup_statements(&self) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("delete from statement_entity where statement_id not in (select statement_id from opinion)")
            .execute(&mut tx)
            .await?;
        sqlx::query("delete from statement where id not in (select statement_id from opinion)")
            .execute(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /* Cleanup entities not referenced from anywhere */
    pub async fn cleanup_entities(&self) -> Result<(), Error> {
        sqlx::query(
            "delete from entity
                        where id not in
                            (select template_id from statement
                            union select first_entity_id from statement
                            union select entity_id from statement_entity
                            union select signer_id from opinion
                            union select signer_id from trust)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cleanup(&self) -> Result<(), Error> {
        self.cleanup_opinions().await?;
        self.cleanup_statements().await?;
        self.cleanup_entities().await
    }
}

impl PersistResult {
    fn new(id: Id) -> Self {
        Self {
            id: id,
            status: PersistStatus::New,
        }
    }
    pub fn is_new(&self) -> bool {
        self.status == PersistStatus::New
    }

    fn partially_new(id: Id) -> Self {
        Self {
            id: id,
            status: PersistStatus::PartiallyNew,
        }
    }
    pub fn is_partially_new(&self) -> bool {
        self.status == PersistStatus::PartiallyNew
    }

    fn old(id: Id) -> Self {
        Self {
            id: id,
            status: PersistStatus::Old,
        }
    }
    pub fn is_old(&self) -> bool {
        self.status == PersistStatus::Old
    }

    pub fn wording(&self) -> &str {
        match self.status {
            PersistStatus::New => "new",
            PersistStatus::PartiallyNew => "partially new",
            PersistStatus::Old => "old",
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use std::str::FromStr;

    use super::*;

    #[test]
    fn lookup_entity() {
        let storage = block_on(Storage::new());
        block_on(storage.initialize_database()).expect("could initialize database");
        let persist_result =
            block_on(storage.persist_entity(&Entity::EMail("test@example.com".to_string())))
                .unwrap();
        assert!(persist_result.id >= 1);
    }

    #[test]
    fn lookup_template() {
        let storage = block_on(Storage::new());
        block_on(storage.initialize_database()).expect("could initialize database");
        let persist_result = block_on(storage.persist_template(&Template {
            name: "template".into(),
            entity_types: vec![vec![EntityType::Template]],
        }))
        .unwrap();
        assert!(persist_result.id >= 1);
    }

    #[test]
    fn lookup_statement() {
        let storage = block_on(Storage::new());
        block_on(storage.initialize_database()).expect("could initialize database");
        let statement = Statement::from_str("template(template(Template))").unwrap();
        let persist_result = block_on(storage.persist_statement(&statement)).unwrap();
        assert!(persist_result.id >= 1);
    }

    #[test]
    fn test_sqlite() {
        use sqlx::{sqlite::SqliteConnection, Connection};

        let res = block_on(SqliteConnection::connect(DATABASE_URL));
        match res {
            Ok(_conn) => assert!(true),
            _ => assert!(false, "{:?}", res),
        }
    }
}
