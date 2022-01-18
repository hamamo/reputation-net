// store entities, statements, opinions persistently
use std::{collections::HashMap, str::FromStr};

use itertools::Itertools;
use libp2p::identity::Keypair;

use log::{debug, info};
// library imports
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow},
    ConnectOptions, Error, Row, Sqlite,
};

// own imports
use crate::model::{Date, Entity, UnsignedOpinion, OwnKey, PublicKey, Opinion, Statement, Template};

mod schema;
pub use schema::*;
mod repository;
pub use repository::*;
mod statement;
mod sync_info;
pub use sync_info::*;

const DATABASE_URL: &str = "sqlite:reputation.sqlite3?mode=rwc";

/// The database type, currently only Sqlite
pub type DB = Sqlite;

/// The storage mechanism for all data shared via the net.
/// Currently does not include caches.
pub struct Storage {
    pool: SqlitePool,
    templates: HashMap<Id<Statement>, Template>,
    signers: HashMap<Id<Statement>, PublicKey>,
    own_key: OwnKey,
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
            templates: HashMap::new(),
            signers: HashMap::new(),
            own_key: OwnKey::new(),
        };
        db.initialize_database().await.expect("could initialize");
        db.cleanup().await.expect("could cleanup");
        db
    }

    /// initialize the database with the schema and well-known facts
    /// this should be idempotent, i.e. if the database is already initialized it should do nothing,
    /// but for a partially initialized database it should complete initialization.
    async fn initialize_database(&mut self) -> Result<(), Error> {
        // perform migrations as necessary
        let migration = sqlx::migrate!();
        migration.run(&self.pool).await.expect("could migrate");

        // insert the root template, this is currently manual
        let template_statement = Statement::from_str("template(template(Template))").unwrap();
        let template_statement = self.persist(template_statement).await?;

        // insert the "signer" template
        let signer_statement = Statement::from_str("template(signer(Signer))").unwrap();
        let signer_statement = self.persist(signer_statement).await?;

        // make sure an owner trust entry exists
        self.ensure_own_key().await?;

        // sign the predefined statements with it
        let own_key = self.own_key.clone();
        self.sign_statement_default(template_statement.data, &own_key)
            .await?;
        self.sign_statement_default(signer_statement.data, &own_key)
            .await?;

        // fill templates and signers
        self.read_templates().await?;
        self.read_signers().await?;

        Ok(())
    }

    pub async fn read_templates(&mut self) -> Result<(), Error> {
        let template_entries = sqlx::query_as::<DB, (Id<Statement>, String)>(
            "select id, entity_1 from statement where name='template'",
        )
        .fetch_all(&self.pool)
        .await?;
        for (id, s) in template_entries {
            if let Ok(template) = Template::from_str(&s) {
                self.templates.insert(id, template);
            }
        }
        Ok(())
    }

    pub async fn read_signers(&mut self) -> Result<(), Error> {
        let signer_entries = sqlx::query_as::<DB, (Id<Statement>, String)>(
            "select id, entity_1 from statement where name='signer'",
        )
        .fetch_all(&self.pool)
        .await?;
        for (id, s) in signer_entries {
            if let Ok(signer) = PublicKey::from_str(&s) {
                self.signers.insert(id, signer);
            }
        }
        Ok(())
    }

    pub fn has_matching_template(&self, statement: &Statement) -> bool {
        if statement.name == "template" {
            // always accept templates to allow bootstrapping
            return true;
        }
        for (_id, template) in &self.templates {
            if statement.matches_template(&template) {
                return true;
            }
        }
        false
    }

    pub async fn list_templates(&self, name: &str) -> Result<Vec<Template>, Error> {
        let all = self.list_all_templates().await?;
        Ok(all
            .iter()
            .filter_map(|e| match e {
                Entity::Template(t) => {
                    if t.name == name {
                        Some(t.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect())
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

    /// this currently does not handle multi-entity statements
    pub async fn find_statements_referencing(
        &self,
        entity: &Entity,
    ) -> Result<Vec<Persistent<Statement>>, Error> {
        let rows = match entity.cidr_minmax() {
            (Some(min), Some(max)) => {
                sqlx::query_as::<DB, DbStatement>(&format!(
                    "select {} from {} where cidr_min <= ? and cidr_max >= ?",
                    DbStatement::COLUMNS,
                    DbStatement::TABLE
                ))
                .bind(min)
                .bind(max)
                .fetch_all(&self.pool)
                .await?
            }
            _ => {
                sqlx::query_as::<DB, DbStatement>(&format!(
                    "select {} from {} where entity_1 = ?",
                    DbStatement::COLUMNS,
                    DbStatement::TABLE
                ))
                .bind(entity.to_string())
                .fetch_all(&self.pool)
                .await?
            }
        };
        let statements = rows
            .iter()
            .map(|row| {
                let mut entities = vec![Entity::from_str(&row.entity_1).unwrap()];
                if let Some(entity) = &row.entity_2 {
                    entities.push(Entity::from_str(&entity).unwrap())
                }
                if let Some(entity) = &row.entity_3 {
                    entities.push(Entity::from_str(&entity).unwrap())
                }
                if let Some(entity) = &row.entity_4 {
                    entities.push(Entity::from_str(&entity).unwrap())
                }

                row.id.with(Statement {
                    name: row.name.to_string(),
                    entities: entities,
                })
            })
            .collect();
        Ok(statements)
    }

    pub async fn list_opinions_on(
        &self,
        id: Id<Statement>,
    ) -> Result<Vec<Persistent<Opinion>>, Error> {
        let rows = sqlx::query_as::<DB, DbOpinion>(&format!(
            "select {} from {} where statement_id = ?",
            DbOpinion::COLUMNS,
            DbOpinion::TABLE
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let opinions = rows
            .iter()
            .map(|row| {
                let signer = self.signers.get(&row.signer_id).unwrap().clone();
                let opinion = Opinion {
                    data: UnsignedOpinion {
                        date: row.date.clone(),
                        valid: row.valid,
                        serial: row.serial,
                        certainty: row.certainty,
                        comment: String::new(),
                    },
                    signer,
                    signature: base64::decode(&row.signature).unwrap(),
                };
                row.id.with(opinion)
            })
            .collect();
        Ok(opinions)
    }

    async fn try_select_statement(
        &self,
        name: &str,
        entity_1: &str,
        entity_2: &Option<String>,
        entity_3: &Option<String>,
        entity_4: &Option<String>,
    ) -> Result<Option<Id<Statement>>, Error> {
        let mut sql = "select id from statement where name=? and entity_1=?".to_owned();
        if let Some(_) = entity_2 {
            sql.push_str(" and entity_2=?");
            if let Some(_) = entity_3 {
                sql.push_str(" and entity_3=?");
                if let Some(_) = entity_4 {
                    sql.push_str(" and entity_4=?");
                }
            }
        }
        let mut query = sqlx::query_scalar::<DB, Id<Statement>>(&sql)
            .bind(name)
            .bind(entity_1);
        if let Some(s) = entity_2 {
            query = query.bind(s);
            if let Some(s) = entity_3 {
                query = query.bind(s);
                if let Some(s) = entity_3 {
                    query = query.bind(s)
                }
            }
        }
        match query.fetch_optional(&self.pool).await? {
            Some(id) => Ok(Some(id)),
            None => Ok(None),
        }
    }

    async fn try_insert_statement(
        &self,
        name: &str,
        entity_1: &str,
        entity_2: &Option<String>,
        entity_3: &Option<String>,
        entity_4: &Option<String>,
        cidr_min: &Option<String>,
        cidr_max: &Option<String>,
    ) -> Result<Id<Statement>, Error> {
        let mut tx = self.pool.begin().await?;
        let query = sqlx::query::<DB>(
            "insert into
            statement(name, entity_1, entity_2, entity_3, entity_4, cidr_min, cidr_max)
            values(?,?,?,?,?,?,?)
            ",
        )
        .bind(name)
        .bind(entity_1)
        .bind(entity_2)
        .bind(entity_3)
        .bind(entity_4)
        .bind(cidr_min)
        .bind(cidr_max);
        query.execute(&mut tx).await?;
        let id = sqlx::query_scalar::<DB, Id<Statement>>("select last_insert_rowid()")
            .fetch_one(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    fn requires_email_hashing(&self, statement: &Statement) -> bool {
        !self.has_matching_template(statement)
    }

    pub async fn persist_statement_hashing_emails(
        &mut self,
        statement: Statement,
    ) -> Result<PersistResult<Statement>, Error> {
        // if the statement template can't be found, retry with hashed e-mails
        // the return value include the possibly translated statement
        if self.requires_email_hashing(&statement) {
            self.persist(statement.hash_emails()).await
        } else {
            self.persist(statement).await
        }
    }

    pub async fn persist_opinion(
        &mut self,
        opinion: Opinion,
        statement_id: &Id<Statement>,
    ) -> Result<PersistResult<Opinion>, Error> {
        // this actually persists a signed opinion. Raw opinions without signature are only used for temporary purposes.
        let signer = Statement::signer(Entity::Signer(opinion.signer.clone()));
        let signer_result = self.persist(signer).await.unwrap();
        let opinion_data = &opinion.data;

        let prev_opinion_result = sqlx::query_as::<DB, (Id<Opinion>, Date, u8)>(
            "select id,date,serial from opinion where statement_id = ? and signer_id = ?",
        )
        .bind(statement_id)
        .bind(signer_result.id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((old_id, date, serial)) = prev_opinion_result {
            if date < opinion_data.date || (date == opinion_data.date && serial < opinion_data.serial) {
                // delete old, overridden opinion
                sqlx::query("delete from opinion where id = ?")
                    .bind(old_id)
                    .execute(&self.pool)
                    .await
                    .expect("could delete old opinion");
            } else {
                return Ok(PersistResult::old(old_id, opinion));
            }
        }
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("insert into opinion(statement_id, signer_id, date, valid, serial, certainty, signature) values(?,?,?,?,?,?,?)")
            .bind(statement_id)
            .bind(signer_result.id)
            .bind(opinion_data.date)
            .bind(opinion_data.valid)
            .bind(opinion_data.serial)
            .bind(opinion_data.certainty)
            .bind(base64::encode(&opinion.signature))
            .execute(&mut tx)
            .await
            .expect("insert signed opinion");
        let id = sqlx::query("select last_insert_rowid()")
            .map(|row: SqliteRow| -> Id<Opinion> { row.get::<Id<Opinion>, usize>(0) })
            .fetch_one(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(PersistResult::new(id, opinion))
    }

    pub async fn sign_statement_default(
        &mut self,
        statement: Statement,
        own_key: &OwnKey,
    ) -> Result<PersistResult<Opinion>, Error> {
        let opinion = UnsignedOpinion {
            date: Date::today(),
            valid: 30,
            serial: 0,
            certainty: 3,
            comment: "".into(),
        };
        let signed_opinion = opinion.sign_using(&statement.signable_bytes(), &own_key.key);
        let statement_id = self.persist(statement).await?.id;
        self.persist_opinion(signed_opinion, &statement_id).await
    }

    pub async fn find_statements_about(
        &self,
        entity: &Entity,
    ) -> Result<Vec<Persistent<Statement>>, Error> {
        // Naive implementation without using sql shortcuts.
        // We can't use map() because that doesn't work with async closures.
        // Need to find out how to do it with streams.
        let mut statements = vec![];
        for e in entity.all_lookup_keys() {
            let mut list = self.find_statements_referencing(&e).await?;
            statements.append(&mut list);
        }
        let asns = statements
            .iter()
            .filter_map(|x| {
                if x.name == "asn" {
                    Some(x.entities[1].clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for e in asns {
            let mut list = self.find_statements_referencing(&e).await?;
            statements.append(&mut list);
        }
        Ok(statements)
    }

    pub async fn ensure_own_key(&mut self) -> Result<(), Error> {
        self.own_key = match sqlx::query_as::<DB, DbPrivateKey>(&format!(
            "select {} from {}",
            DbPrivateKey::COLUMNS,
            DbPrivateKey::TABLE
        ))
        .fetch_optional(&self.pool)
        .await?
        {
            Some(private_key) => {
                let key_bytes = base64::decode(private_key.key).expect("base64 decode");
                let privkey = libp2p::identity::secp256k1::SecretKey::from_bytes(key_bytes)
                    .expect("secp256k1 decode");
                debug!("getting statement with id {}", private_key.signer_id);
                let statement = self.get(private_key.signer_id).await?.unwrap();
                let signer = statement.entities[0].clone();
                let keypair =
                    Keypair::Secp256k1(libp2p::identity::secp256k1::Keypair::from(privkey));
                OwnKey {
                    signer: signer,
                    level: 0,
                    key: keypair,
                }
            }
            _ => {
                let own_key = OwnKey::new();
                let statement = Statement::signer(own_key.signer.clone());
                let persist_result = self.persist(statement).await?;
                let privkey = own_key.privkey_string();
                info!("trust {} {}", persist_result.id, privkey);
                let mut tx = self.pool.begin().await.unwrap();
                sqlx::query(&format!(
                    "insert into {} (signer_id, key) values(?,?)",
                    DbPrivateKey::TABLE
                ))
                .bind(persist_result.id)
                .bind(privkey)
                .execute(&mut tx)
                .await?;
                tx.commit().await?;
                own_key
            }
        };
        Ok(())
    }

    pub fn own_key<'a>(&'a self) -> &'a OwnKey {
        &self.own_key
    }

    /* Clean up opinions which are not valid anymore. */
    pub async fn cleanup_opinions(&self) -> Result<(), Error> {
        sqlx::query("delete from opinion where date + valid < ?")
            .bind(Date::today())
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

    /// utility method to fix the cidr database columns, to be used if there are old IP entries
    pub async fn fix_cidr(&self) -> Result<(), Error> {
        for s in self.get_all().await? {
            let (cidr_min, cidr_max) = s.entities[0].cidr_minmax();
            if let Some(cidr_min) = cidr_min {
                sqlx::query("update statement set cidr_min=?, cidr_max=? where id=?")
                    .bind(cidr_min)
                    .bind(cidr_max)
                    .bind(s.id)
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn get_sync_infos(&self, date: Date) -> Result<SyncInfos, Error> {
        let rows = sqlx::query_as::<DB, (String, String)>(
            "select s.name, o.signature
            from statement s join opinion o on s.id = o.statement_id
            where o.date=?
            order by s.name, o.signature",
        )
        .bind(date)
        .fetch_all(&self.pool)
        .await?;
        let mut result = SyncInfos {
            date,
            infos: HashMap::new(),
        };
        for (name, hash_strings) in &rows.into_iter().group_by(|tuple| tuple.0.to_string()) {
            let hashes: Vec<Vec<u8>> = hash_strings
                .map(|tuple| base64::decode(tuple.1).unwrap())
                .collect();
            result.infos.insert(name, SyncInfo::new(hashes));
        }
        Ok(result)
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
        let persist_result = block_on(storage.persist(statement)).unwrap();
        assert!(persist_result.id >= Id::new(1));
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
