// store entities, statements, opinions persistently
use std::{collections::HashSet, str::FromStr};

use libp2p::identity::Keypair;

// library imports
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow},
    ConnectOptions, Error, Row, Sqlite,
};

use crate::model::{today, Opinion, PublicKey};

// own imports
use super::model::{Entity, OwnKey, SignedOpinion, Statement, Template};

const DATABASE_URL: &str = "sqlite:reputation.sqlite3?mode=rwc";

/// The database type, currently only Sqlite
pub type DB = Sqlite;

/// The database id type, i64 for PostgreSQL (the only supported database backend at the moment).
pub type Id = i64;

/// Status of a possibly pre-existing persistent item.
#[derive(PartialEq)]
pub enum PersistStatus {
    New,
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
    templates: HashSet<Template>,
    signers: HashSet<PublicKey>,
}

impl Storage {
    /// create a new initialized instance of the database.
    /// existing outdated entities, statements and opinions will be cleaned up
    pub async fn new() -> Self {
        let mut options = SqliteConnectOptions::from_str(DATABASE_URL).unwrap();
        options.log_statements(log::LevelFilter::Debug);
        let mut db = Self {
            pool: SqlitePoolOptions::new()
                .max_connections(5)
                .connect_with(options)
                .await
                .unwrap(),
            templates: HashSet::new(),
            signers: HashSet::new(),
        };
        db.initialize_database().await.expect("could initialize");
        db.cleanup().await.expect("could cleanup");
        db
    }

    /// initialize the database with the schema and well-known facts
    /// this should be idempotent, i.e. if the database is already initialized it should do nothing,
    /// but for a partially initialized database it should complete initialization.
    async fn initialize_database(&mut self) -> Result<(), Error> {
        let defs = vec![
            // statements must have no more than 4 entities (most likely 3 would be generous already)
            "create table statement(
                id integer primary key,
                name text not null,
                entity_1 text not null,
                entity_2 text,
                entity_3 text,
                entity_4 text,
                unique(name,entity_1,entity_2,entity_3,entity_4)
            )",
            "create table opinion(
                id integer primary key,
                statement_id integer not null,
                signer_id integer not null,
                date integer not null,
                valid integer not null,
                serial integer not null,
                certainty integer not null,
                signature text not null,
                unique(statement_id,signer_id,date,serial),
                foreign key(statement_id) references statement(id),
                foreign key(signer_id) references statement(id)
            )",
            "create table private_key(
                signer_id integer not null,
                key text not null,
                foreign key(signer_id) references statement(id)
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
        let template_statement = Statement::from_str("template(template(Template))").unwrap();
        self.persist_statement(&template_statement).await?;

        // insert the "signer" template
        let signer_statement = Statement::from_str("template(signer(Signer))").unwrap();
        self.persist_statement(&signer_statement).await?;

        // make sure an owner trust entry exists
        let own_key = self.own_key().await?;

        // sign the predefined statements with it
        self.sign_statement_default(&template_statement, &own_key)
            .await?;
        self.sign_statement_default(&signer_statement, &own_key)
            .await?;

        // fill templates and signers
        self.read_templates().await?;
        self.read_signers().await?;

        Ok(())
    }

    pub async fn read_templates(&mut self) -> Result<(), Error> {
        let template_entries = sqlx::query_scalar::<DB, String>(
            "select entity_1 from statement where name='template'",
        )
        .fetch_all(&self.pool)
        .await?;
        for s in template_entries {
            if let Ok(template) = Template::from_str(&s) {
                self.templates.insert(template);
            }
        }
        Ok(())
    }

    pub async fn read_signers(&mut self) -> Result<(), Error> {
        let signer_entries =
            sqlx::query_scalar::<DB, String>("select entity_1 from statement where name='signer'")
                .fetch_all(&self.pool)
                .await?;
        for s in signer_entries {
            if let Ok(signer) = PublicKey::from_str(&s) {
                self.signers.insert(signer);
            }
        }
        Ok(())
    }

    pub async fn has_matching_template(&self, statement: &Statement) -> bool {
        if statement.name == "template" {
            // always accept templates to allow bootstrapping
            return true;
        }
        for template in &self.templates {
            if statement.matches_template(template) {
                return true;
            }
        }
        false
    }

    pub async fn list_templates(&self, _name: &str) -> Vec<Template> {
        vec![]
    }

    pub async fn list_all_templates(&self) -> Result<Vec<Entity>, Error> {
        let rows = sqlx::query_scalar::<DB, String>(
            "select
                entity_1
            from
                statement
            where
                name = 'template'",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|s| Entity::from_str(s).unwrap()).collect())
    }

    #[allow(dead_code)]
    pub async fn get_statement(&self, statement_id: Id) -> Result<Option<Statement>, Error> {
        match sqlx::query_as::<
            DB,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(
            "select
                name,
                entity_1,
                entity_2,
                entity_3,
                entity_4
            from
                statement
            where
                id = ?",
        )
        .bind(statement_id)
        .fetch_one(&self.pool)
        .await
        {
            Ok((name, entity_1, entity_2, entity_3, entity_4)) => {
                let mut entities = vec![Entity::from_str(&entity_1.as_str()).unwrap()];
                if let Some(entity) = entity_2 {
                    entities.push(Entity::from_str(&entity.as_str()).unwrap())
                }
                if let Some(entity) = entity_3 {
                    entities.push(Entity::from_str(&entity.as_str()).unwrap())
                }
                if let Some(entity) = entity_4 {
                    entities.push(Entity::from_str(&entity.as_str()).unwrap())
                }
                return Ok(Some(Statement {
                    name: name,
                    entities: entities,
                }));
            }
            _ => Ok(None),
        }
    }

    /// this currently does not handle multi-entity statements
    pub async fn find_statements_referencing(
        &self,
        entity: &Entity,
    ) -> Result<Vec<Statement>, Error> {
        let rows = sqlx::query_as::<
            DB,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(
            "select
                name,
                entity_1,
                entity_2,
                entity_3,
                entity_4
            from
                statement
            where
                entity_1 = ?",
        )
        .bind(entity.to_string())
        .fetch_all(&self.pool)
        .await?;
        let statements = rows
            .iter()
            .map(|(name, entity_1, entity_2, entity_3, entity_4)| {
                let mut entities = vec![Entity::from_str(entity_1).unwrap()];
                if let Some(entity) = entity_2 {
                    entities.push(Entity::from_str(entity).unwrap())
                }
                if let Some(entity) = entity_3 {
                    entities.push(Entity::from_str(entity).unwrap())
                }
                if let Some(entity) = entity_4 {
                    entities.push(Entity::from_str(entity).unwrap())
                }

                Statement {
                    name: name.to_string(),
                    entities: entities,
                }
            })
            .collect();
        Ok(statements)
    }

    async fn try_select_statement(
        &self,
        name: &str,
        entity_1: &str,
        entity_2: &Option<String>,
        entity_3: &Option<String>,
        entity_4: &Option<String>,
    ) -> Result<Option<Id>, Error> {
        let mut sql = "select id from statement where name=? and entity_1=?".to_owned();
        if let Some(_) = entity_2 {
            sql.push_str(" and entity_2=?");
        }
        if let Some(_) = entity_3 {
            sql.push_str(" and entity_3=?");
        }
        if let Some(_) = entity_4 {
            sql.push_str(" and entity_4=?");
        }
        let mut query = sqlx::query_scalar::<DB, Id>(&sql).bind(name).bind(entity_1);
        if let Some(s) = entity_2 {
            query = query.bind(s)
        }
        if let Some(s) = entity_3 {
            query = query.bind(s)
        }
        if let Some(s) = entity_3 {
            query = query.bind(s)
        }
        Ok(query.fetch_optional(&self.pool).await?)
    }

    async fn try_insert_statement(
        &self,
        name: &str,
        entity_1: &str,
        entity_2: &Option<String>,
        entity_3: &Option<String>,
        entity_4: &Option<String>,
    ) -> Result<Id, Error> {
        let mut tx = self.pool.begin().await?;
        let query = sqlx::query::<DB>(
            "insert into
            statement(name, entity_1, entity_2, entity_3, entity_4)
            values(?,?,?,?,?)
            ",
        )
        .bind(name)
        .bind(entity_1)
        .bind(entity_2)
        .bind(entity_3)
        .bind(entity_4);
        query.execute(&mut tx).await?;
        let id = sqlx::query_scalar::<DB, Id>("select last_insert_rowid()")
            .fetch_one(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn persist_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<PersistResult, Error> {
        let entity_1 = statement.entities[0].to_string();
        let entity_2 = statement.entities.get(1).and_then(|e| Some(e.to_string()));
        let entity_3 = statement.entities.get(2).and_then(|e| Some(e.to_string()));
        let entity_4 = statement.entities.get(3).and_then(|e| Some(e.to_string()));

        // ensure that the statement matches an existing template
        if !self.has_matching_template(statement).await {
            return Err(Error::RowNotFound);
        }

        // first try to find existing statement
        let result = self
            .try_select_statement(&statement.name, &entity_1, &entity_2, &entity_3, &entity_4)
            .await?;
        if let Some(id) = result {
            if let Entity::Template(template) = &statement.entities[0] {
                self.templates.insert(template.clone());
            }
            return Ok(PersistResult::old(id));
        }

        // if not found, try to insert
        let insert_result = self
            .try_insert_statement(&statement.name, &entity_1, &entity_2, &entity_3, &entity_4)
            .await;
        if let Ok(id) = insert_result {
            if let Entity::Template(template) = &statement.entities[0] {
                self.templates.insert(template.clone());
            }
            return Ok(PersistResult::new(id));
        }

        // if insert failed, it's possibly because a concurrent insert happened, so retry select
        let result = self
            .try_select_statement(&statement.name, &entity_1, &entity_2, &entity_3, &entity_4)
            .await?;
        if let Some(id) = result {
            return Ok(PersistResult::old(id));
        }

        // if not there, we might have a real problem on insert, so return that
        match insert_result {
            Ok(_) => panic!("can't happen"),
            Err(e) => Err(e),
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
        &mut self,
        signed_opinion: &SignedOpinion,
        statement_id: Id,
    ) -> Result<PersistResult, Error> {
        // this actually persists a signed opinion. Raw opinions without signature are only used for temporary purposes.
        let signer = Statement::signer(Entity::Signer(signed_opinion.signer.clone()));
        let signer_result = self.persist_statement(&signer).await.unwrap();
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

    pub async fn sign_statement_default(
        &mut self,
        statement: &Statement,
        own_key: &OwnKey,
    ) -> Result<PersistResult, Error> {
        let opinion = Opinion {
            date: today(),
            valid: 30,
            serial: 0,
            certainty: 3,
            comment: "".into(),
        };
        let signed_opinion = opinion.sign_using(&statement.signable_bytes(), &own_key.key);
        let statement_id = self.persist_statement(statement).await?.id;
        self.persist_opinion(&signed_opinion, statement_id).await
    }

    pub async fn find_statements_about(&self, entity: &Entity) -> Result<Vec<Statement>, Error> {
        // Naive implementation without using sql shortcuts.
        // We can't use map() because that doesn't work with async closures.
        // Need to find out how to do it with streams.
        let mut statements = vec![];
        for e in entity.all_lookup_keys() {
            let mut list = self.find_statements_referencing(&e).await?;
            statements.append(&mut list);
        }
        Ok(statements)
    }

    pub async fn own_key(&mut self) -> Result<OwnKey, Error> {
        match sqlx::query_as::<DB, (Id, String)>("select signer_id, key from private_key")
            .fetch_optional(&self.pool)
            .await?
        {
            Some((id, key)) => {
                let key_bytes = base64::decode(key).expect("base64 decode");
                let privkey = libp2p::identity::secp256k1::SecretKey::from_bytes(key_bytes)
                    .expect("secp256k1 decode");
                let statement = self.get_statement(id).await?.unwrap();
                let signer = statement.entities[0].clone();
                let keypair =
                    Keypair::Secp256k1(libp2p::identity::secp256k1::Keypair::from(privkey));
                Ok(OwnKey {
                    signer: signer,
                    level: 0,
                    key: keypair,
                })
            }
            _ => {
                let own_key = OwnKey::new();
                let statement = Statement::signer(own_key.signer.clone());
                let persist_result = self.persist_statement(&statement).await?;
                let privkey = own_key.privkey_string();
                println!("trust {} {}", persist_result.id, privkey);
                let mut tx = self.pool.begin().await.unwrap();
                sqlx::query("insert into private_key(signer_id, key) values(?,?)")
                    .bind(persist_result.id)
                    .bind(privkey)
                    .execute(&mut tx)
                    .await?;
                tx.commit().await?;
                Ok(own_key)
            }
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
        sqlx::query(
            "delete from statement
            where id not in (
                select statement_id from opinion
                union select signer_id from opinion
                union select signer_id from private_key)",
        )
        .execute(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn cleanup(&self) -> Result<(), Error> {
        self.cleanup_opinions().await?;
        self.cleanup_statements().await
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

    fn old(id: Id) -> Self {
        Self {
            id: id,
            status: PersistStatus::Old,
        }
    }
    #[allow(dead_code)]
    pub fn is_old(&self) -> bool {
        self.status == PersistStatus::Old
    }

    pub fn wording(&self) -> &str {
        match self.status {
            PersistStatus::New => "new",
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
    fn lookup_statement() {
        let mut storage = block_on(Storage::new());
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
