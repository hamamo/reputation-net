// store entities, statements, opinions persistently
use std::str::FromStr;

// library imports
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow},
    ConnectOptions, Error, Row,
};

// own imports
use super::model::{parser, Entity, EntityType, Statement, Template};

const DATABASE_URL: &str = "sqlite:reputation.sqlite3?mode=rwc";

// the database id type
pub type Id = i64;

pub type LookupResult = (Id, bool);

pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    pub async fn new() -> Self {
        let mut options = SqliteConnectOptions::from_str(DATABASE_URL).unwrap();
        options.log_statements(log::LevelFilter::Debug);
        let mut db = Self {
            pool: SqlitePoolOptions::new()
                .max_connections(5)
                .connect_with(options)
                .await
                .unwrap(),
        };
        db.initialize_database().await;
        db
    }

    async fn initialize_database(&mut self) {
        // initialize the database with the schema and well-known facts
        // this should be idempotent, i.e. if the database is already initialized it should do nothing,
        // but for a partially initialized database it should complete initialization.

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
                certainty integer not null,
                foreign key(statement_id) references statement(id),
                foreign key(signer_id) references entity(id)
            )",
            "create table trust(
                signer_id integer not null,
                level integer not null,
                key text not null,
                foreign key(signer_id) references entity(id)
            )",
        ];

        // create tables if necessary
        for def in defs {
            match sqlx::query(def).execute(&self.pool).await {
                Ok(result) => println!("{:?}", result),
                Err(problem) => println!("{:?}", problem),
            }
        }

        // insert the root template, this is currently manual
        let entity = Entity::from_str("template(Template)").unwrap();
        self.lookup_entity(&entity).await.unwrap();
        let statement = Statement::from_str("template(template(Template))").unwrap();
        self.lookup_statement(&statement).await.unwrap();
    }

    pub async fn lookup_entity(&mut self, entity: &Entity) -> Result<LookupResult, Error> {
        // select or insert an entity
        let string = entity.to_string();
        let kind = entity.entity_type() as i64;

        let mut tx = self.pool.begin().await.unwrap();
        let result = sqlx::query("select id from entity where kind = ? and value = ?")
            .bind(kind)
            .bind(&string)
            .map(|row: SqliteRow| -> Id { row.get::<Id, &str>("id") })
            .fetch_optional(&mut tx)
            .await;
        match result {
            Ok(opt) => {
                if let Some(id) = opt {
                    tx.rollback().await?;
                    Ok((id, false))
                } else {
                    sqlx::query("insert into entity(kind, value) values(?,?)")
                        .bind(kind)
                        .bind(string)
                        .execute(&mut tx)
                        .await
                        .expect("could insert");
                    let id = sqlx::query("select last_insert_rowid()")
                        .map(|row: SqliteRow| -> Id { row.get::<Id, usize>(0) })
                        .fetch_one(&mut tx)
                        .await?;
                    tx.commit().await?;
                    Ok((id, true))
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn lookup_template(&mut self, template: &Template) -> Result<LookupResult, Error> {
        self.lookup_entity(&Entity::Template(template.clone()))
            .await
    }

    pub async fn find_matching_template(
        &mut self,
        statement: &Statement,
    ) -> Result<LookupResult, Error> {
        let mut tx = self.pool.begin().await.unwrap();
        let rows: Vec<SqliteRow> =
            sqlx::query("select id,value from entity where kind=? and value like ?")
                .bind(EntityType::Template as i64)
                .bind(format!("{}(%", statement.name))
                .fetch_all(&mut tx)
                .await?;
        for row in rows {
            let id = row.get::<Id, usize>(0);
            let template = Template::from_str(&row.get::<String, usize>(1)).unwrap();
            if statement.matches_template(&template) {
                return Ok((id, true));
            }
        }
        Err(Error::RowNotFound)
    }

    pub async fn lookup_statement(&mut self, statement: &Statement) -> Result<LookupResult, Error> {
        // lookup entities before creating our own transaction, as we only want to use one transaction at a time

        let template_id = self.find_matching_template(statement).await?.0;
        let mut entity_ids: Vec<Id> = vec![];
        for entity in &statement.entities {
            let entity_id = self.lookup_entity(entity).await?.0;
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
        let mut tx = self.pool.begin().await.unwrap();
        let result = query
            .map(|row: SqliteRow| -> Id { row.get::<Id, &str>("id") })
            .fetch_optional(&mut tx)
            .await?;
        match result {
            Some(id) => {
                tx.rollback().await?;
                Ok((id, false))
            }
            None => {
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
                Ok((id, true))
            }
        }
    }
}

mod tests {
    use futures::executor::block_on;
    use std::str::FromStr;

    use crate::{
        model::{Entity, EntityType, Statement, Template},
        storage::{Storage, DATABASE_URL},
    };

    #[test]
    fn lookup_entity() {
        let mut storage = block_on(Storage::new());
        block_on(storage.initialize_database());
        let (id, _) =
            block_on(storage.lookup_entity(&Entity::EMail("test@example.com".to_string())))
                .unwrap();
        assert!(id >= 1);
    }

    #[test]
    fn lookup_template() {
        let mut storage = block_on(Storage::new());
        block_on(storage.initialize_database());
        let (id, _) = block_on(storage.lookup_template(&Template {
            name: "template".into(),
            entity_types: vec![vec![EntityType::Template]],
        }))
        .unwrap();
        assert!(id >= 1);
    }

    #[test]
    fn lookup_statement() {
        let mut storage = block_on(Storage::new());
        block_on(storage.initialize_database());
        let statement = Statement::from_str("template(template(Template))").unwrap();
        let (id, _) = block_on(storage.lookup_statement(&statement)).unwrap();
        assert!(id >= 1);
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
