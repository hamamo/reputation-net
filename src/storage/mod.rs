// store entities, statements, opinions persistently
use std::{collections::HashMap, str::FromStr};

use async_trait::async_trait;
use libp2p::identity::Keypair;

// library imports
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow},
    ConnectOptions, Error, Row, Sqlite,
};

// own imports
use crate::model::{today, Entity, Opinion, OwnKey, PublicKey, SignedOpinion, Statement, Template};

mod repository;
pub use repository::*;

const DATABASE_URL: &str = "sqlite:reputation.sqlite3?mode=rwc";

/// The database type, currently only Sqlite
pub type DB = Sqlite;

/// The storage menchanism for all data shared via the net.
/// Currently does not include caches.
pub struct Storage {
    pool: SqlitePool,
    templates: HashMap<Id<Statement>, Template>,
    signers: HashMap<Id<Statement>, PublicKey>,
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
        let own_key = self.own_key().await?;

        // sign the predefined statements with it
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
        let template_entries = sqlx::query_as::<DB, (PrimitiveId, String)>(
            "select id, entity_1 from statement where name='template'",
        )
        .fetch_all(&self.pool)
        .await?;
        for (id, s) in template_entries {
            if let Ok(template) = Template::from_str(&s) {
                self.templates.insert(Id::new(id), template);
            }
        }
        Ok(())
    }

    pub async fn read_signers(&mut self) -> Result<(), Error> {
        let signer_entries = sqlx::query_as::<DB, (PrimitiveId, String)>(
            "select id, entity_1 from statement where name='signer'",
        )
        .fetch_all(&self.pool)
        .await?;
        for (id, s) in signer_entries {
            if let Ok(signer) = PublicKey::from_str(&s) {
                self.signers.insert(Id::new(id), signer);
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
        let query = match entity.cidr_minmax() {
            (Some(min), Some(max)) => sqlx::query_as::<
                DB,
                (
                    PrimitiveId,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(
                "select id, name, entity_1, entity_2, entity_3, entity_4
                    from statement
                    where cidr_min <= ? and cidr_max >= ?",
            )
            .bind(min)
            .bind(max),
            _ => sqlx::query_as::<
                DB,
                (
                    PrimitiveId,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(
                "select id, name, entity_1, entity_2, entity_3, entity_4
                    from statement
                    where entity_1 = ?",
            )
            .bind(entity.to_string()),
        };
        let rows = query.fetch_all(&self.pool).await?;
        let statements = rows
            .iter()
            .map(|(id, name, entity_1, entity_2, entity_3, entity_4)| {
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

                Id::new(*id).with(Statement {
                    name: name.to_string(),
                    entities: entities,
                })
            })
            .collect();
        Ok(statements)
    }

    pub async fn list_opinions_on(
        &self,
        id: Id<Statement>,
    ) -> Result<Vec<Persistent<SignedOpinion>>, Error> {
        let rows = sqlx::query_as::<DB, (PrimitiveId, PrimitiveId, u32, u16, u8, i8, String)>(
            "select
                    id,
                    signer_id,
                    date,
                    valid,
                    serial,
                    certainty,
                    signature
                from opinion
                where
                    statement_id = ?
                ",
        )
        .bind(id.id)
        .fetch_all(&self.pool)
        .await?;
        let opinions = rows
            .iter()
            .map(
                |(id, signer_id, date, valid, serial, certainty, signature)| {
                    let signer = self.signers.get(&Id::new(*signer_id)).unwrap().clone();
                    let opinion = SignedOpinion {
                        opinion: Opinion {
                            date: *date,
                            valid: *valid,
                            serial: *serial,
                            certainty: *certainty,
                            comment: String::new(),
                        },
                        signer,
                        signature: base64::decode(signature).unwrap(),
                    };
                    Id::new(*id).with(opinion)
                },
            )
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
        let mut query = sqlx::query_scalar::<DB, PrimitiveId>(&sql)
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
            Some(primitive_id) => Ok(Some(Id::new(primitive_id))),
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
        let id = sqlx::query_scalar::<DB, PrimitiveId>("select last_insert_rowid()")
            .fetch_one(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(Id::new(id))
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
        signed_opinion: SignedOpinion,
        statement_id: &Id<Statement>,
    ) -> Result<PersistResult<SignedOpinion>, Error> {
        // this actually persists a signed opinion. Raw opinions without signature are only used for temporary purposes.
        let signer = Statement::signer(Entity::Signer(signed_opinion.signer.clone()));
        let signer_result = self.persist(signer).await.unwrap();
        let opinion = &signed_opinion.opinion;

        let prev_opinion_result = sqlx::query_as::<DB, (PrimitiveId, u32, u8)>(
            "select id,date,serial from opinion where statement_id = ? and signer_id = ?",
        )
        .bind(statement_id.id)
        .bind(signer_result.id.id)
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
                return Ok(PersistResult::old(Id::new(old_id), signed_opinion));
            }
        }
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("insert into opinion(statement_id, signer_id, date, valid, serial, certainty, signature) values(?,?,?,?,?,?,?)")
            .bind(statement_id.id)
            .bind(signer_result.id.id)
            .bind(opinion.date)
            .bind(opinion.valid)
            .bind(opinion.serial)
            .bind(opinion.certainty)
            .bind(base64::encode(&signed_opinion.signature))
            .execute(&mut tx)
            .await
            .expect("insert signed opinion");
        let id = sqlx::query("select last_insert_rowid()")
            .map(|row: SqliteRow| -> PrimitiveId { row.get::<PrimitiveId, usize>(0) })
            .fetch_one(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(PersistResult::new(Id::new(id), signed_opinion))
    }

    pub async fn sign_statement_default(
        &mut self,
        statement: Statement,
        own_key: &OwnKey,
    ) -> Result<PersistResult<SignedOpinion>, Error> {
        let opinion = Opinion {
            date: today(),
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

    pub async fn own_key(&mut self) -> Result<OwnKey, Error> {
        match sqlx::query_as::<DB, (PrimitiveId, String)>("select signer_id, key from private_key")
            .fetch_optional(&self.pool)
            .await?
        {
            Some((id, key)) => {
                let key_bytes = base64::decode(key).expect("base64 decode");
                let privkey = libp2p::identity::secp256k1::SecretKey::from_bytes(key_bytes)
                    .expect("secp256k1 decode");
                let statement = self.get(Id::new(id)).await?.unwrap();
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
                let persist_result = self.persist(statement).await?;
                let privkey = own_key.privkey_string();
                println!("trust {} {}", persist_result.id, privkey);
                let mut tx = self.pool.begin().await.unwrap();
                sqlx::query("insert into private_key(signer_id, key) values(?,?)")
                    .bind(persist_result.id.id)
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
            .bind(today())
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

    pub async fn fix_cidr(&self) -> Result<(), Error> {
        for s in self.get_all().await? {
            let (cidr_min, cidr_max) = s.entities[0].cidr_minmax();
            if let Some(cidr_min) = cidr_min {
                sqlx::query("update statement set cidr_min=?, cidr_max=? where id=?")
                .bind(cidr_min)
                .bind(cidr_max)
                .bind(s.id.id)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Repository<Statement> for Storage {
    type RowType = (
        PrimitiveId,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );

    async fn get(&self, id: Id<Statement>) -> Result<Option<Persistent<Statement>>, Error> {
        match sqlx::query_as::<DB, Self::RowType>(
            "select
                    id,
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
        .bind(id.id)
        .fetch_one(&self.pool)
        .await
        {
            Ok(tuple) => {
                return Ok(Some(Self::row_to_record(tuple)));
            }
            _ => Ok(None),
        }
    }

    async fn get_all(&self) -> Result<Vec<Persistent<Statement>>, Error> {
        // dummy implementation for now
        let rows = sqlx::query_as::<DB, Self::RowType>(
            "select
                    id,
                    name,
                    entity_1,
                    entity_2,
                    entity_3,
                    entity_4
                from
                    statement",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|tuple| Self::row_to_record(tuple))
            .collect())
    }

    async fn persist(&mut self, statement: Statement) -> Result<PersistResult<Statement>, Error> {
        // ensure that the statement matches an existing template
        if !self.has_matching_template(&statement) {
            println!("did not find matching template for {}", statement);
            return Err(Error::RowNotFound);
        }

        let entity_1 = statement.entities[0].to_string();
        let (cidr_min, cidr_max) = statement.entities[0].cidr_minmax();
        let entity_2 = statement.entities.get(1).and_then(|e| Some(e.to_string()));
        let entity_3 = statement.entities.get(2).and_then(|e| Some(e.to_string()));
        let entity_4 = statement.entities.get(3).and_then(|e| Some(e.to_string()));

        // first try to find existing statement
        let result = self
            .try_select_statement(&statement.name, &entity_1, &entity_2, &entity_3, &entity_4)
            .await?;

        let result = match result {
            Some(id) => PersistResult::old(id, statement),
            None => {
                let insert_result = self
                    .try_insert_statement(
                        &statement.name,
                        &entity_1,
                        &entity_2,
                        &entity_3,
                        &entity_4,
                        &cidr_min,
                        &cidr_max,
                    )
                    .await;
                match insert_result {
                    Ok(id) => PersistResult::new(id, statement),
                    Err(_) => {
                        let result = self
                            .try_select_statement(
                                &statement.name,
                                &entity_1,
                                &entity_2,
                                &entity_3,
                                &entity_4,
                            )
                            .await?;
                        match result {
                            Some(id) => PersistResult::old(id, statement),
                            None => panic!("could not insert statement"),
                        }
                    }
                }
            }
        };
        if result.name == "template" {
            if let Entity::Template(template) = &result.entities[0] {
                self.templates.insert(result.id.clone(), template.clone());
            }
        }
        Ok(result)
    }

    fn row_to_record(row: Self::RowType) -> Persistent<Statement> {
        let (id, name, entity_1, entity_2, entity_3, entity_4) = row;
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
        Persistent {
            id: Id::new(id),
            data: Statement { name, entities },
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
        let persist_result = block_on(storage.persist(statement)).unwrap();
        assert!(persist_result.id.id >= 1);
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
