/// Structure definitions for the database tables
use std::str::FromStr;

use sqlx::{
    types::chrono::{DateTime, Utc},
    FromRow, Row,
};

use crate::model::{Entity, UnsignedOpinion};

use super::{Date, Get, Id, Opinion, Persistent, RowType, Statement, Storage};

#[derive(sqlx::FromRow, Debug)]
pub struct DbStatement {
    pub id: Id<Statement>,
    pub name: String,
    pub entity_1: String,
    pub entity_2: Option<String>,
    pub entity_3: Option<String>,
    pub entity_4: Option<String>,
    pub last_used: Option<DateTime<Utc>>,
    pub last_weight: Option<f32>,
}

#[derive(sqlx::FromRow, Debug)]
pub struct DbOpinion {
    pub id: Id<Opinion>,
    pub statement_id: Id<Statement>,
    pub signer_id: Id<Statement>,
    pub date: Date,
    pub valid: u16,
    pub serial: u8,
    pub certainty: i8,
    pub comment: String,
    pub signature: String,
}

#[derive(Debug)]
pub struct DbStatementWithOpinion {
    pub statement: DbStatement,
    pub opinion: DbOpinion,
}

#[derive(sqlx::FromRow, Debug)]
pub struct DbPrivateKey {
    pub signer_id: Id<Statement>,
    pub key: String,
}

impl RowType for DbStatement {
    const TABLE: &'static str = "statement";
    const COLUMNS: &'static str = "statement.id,
        statement.name,
        statement.entity_1,
        statement.entity_2,
        statement.entity_3,
        statement.entity_4,
        statement.last_used,
        statement.last_weight";
}

impl RowType for DbOpinion {
    const TABLE: &'static str = "opinion";
    const COLUMNS: &'static str = "opinion.id,
        opinion.statement_id,
        opinion.signer_id,
        opinion.date,
        opinion.valid,
        opinion.serial,
        opinion.certainty,
        opinion.comment,
        opinion.signature";
}

// this is ugly as the columns are repeated, I can't currently compute them at compile time
impl RowType for DbStatementWithOpinion {
    const TABLE: &'static str = "statement join opinion on statement.id = opinion.statement_id";
    const COLUMNS: &'static str = "statement.id,
        statement.name,
        statement.entity_1,
        statement.entity_2,
        statement.entity_3,
        statement.entity_4,
        statement.last_used,
        statement.last_weight,
        opinion.id,
        opinion.statement_id,
        opinion.signer_id,
        opinion.date,
        opinion.valid,
        opinion.serial,
        opinion.certainty,
        opinion.comment,
        opinion.signature";
}

impl<'r, R: Row + Send> FromRow<'r, R> for DbStatementWithOpinion
where
    i64: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    u32: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    u16: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    u8: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    i8: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    String: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    DateTime<Utc>: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    f32: sqlx::Type<<R as Row>::Database> + sqlx::Decode<'r, <R as Row>::Database>,
    usize: sqlx::ColumnIndex<R>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        let statement = DbStatement {
            id: row.get(0),
            name: row.get(1),
            entity_1: row.get(2),
            entity_2: row.get(3),
            entity_3: row.get(4),
            entity_4: row.get(5),
            last_used: row.get(6),
            last_weight: row.get(7),
        };
        let opinion = DbOpinion {
            id: row.get(6),
            statement_id: row.get(7),
            signer_id: row.get(8),
            date: row.get(9),
            valid: row.get(10),
            serial: row.get(11),
            certainty: row.get(12),
            comment: row.get(13),
            signature: row.get(14),
        };
        Ok(Self { statement, opinion })
    }
}

impl RowType for DbPrivateKey {
    const TABLE: &'static str = "private_key";
    const COLUMNS: &'static str = "private_key.signer_id,
        private_key.key";
}

impl From<DbStatement> for Statement {
    fn from(row: DbStatement) -> Statement {
        let mut entities = vec![Entity::from_str(&row.entity_1.as_str()).unwrap()];
        if let Some(entity) = row.entity_2 {
            entities.push(Entity::from_str(&entity.as_str()).unwrap())
        }
        if let Some(entity) = row.entity_3 {
            entities.push(Entity::from_str(&entity.as_str()).unwrap())
        }
        if let Some(entity) = row.entity_4 {
            entities.push(Entity::from_str(&entity.as_str()).unwrap())
        }
        Statement {
            name: row.name,
            entities,
        }
    }
}

impl From<DbStatement> for Persistent<Statement> {
    fn from(row: DbStatement) -> Persistent<Statement> {
        let id = row.id;
        Persistent {
            id,
            data: row.into(),
        }
    }
}

impl Opinion {
    pub async fn from_using_storage(row: DbOpinion, storage: &Storage) -> Opinion {
        let signer_statement: Persistent<Statement> = storage
            .get(row.signer_id)
            .await
            .expect("could find signer")
            .unwrap();
        let signer = match &signer_statement.entities[0] {
            Entity::Signer(key) => key,
            _ => {
                panic!("expected signer")
            }
        };
        Opinion {
            data: UnsignedOpinion {
                date: row.date,
                valid: row.valid,
                serial: row.serial,
                certainty: row.certainty,
                comment: row.comment,
            },
            signer: signer.clone(),
            signature: base64::decode(row.signature).unwrap(),
        }
    }
}
