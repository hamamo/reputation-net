use super::{Date, Id, Opinion, Statement, RowType};

/// Structure definitions for the database tables

#[derive(sqlx::FromRow, Debug)]
pub struct DbStatement {
    pub id: Id<Statement>,
    pub name: String,
    pub entity_1: String,
    pub entity_2: Option<String>,
    pub entity_3: Option<String>,
    pub entity_4: Option<String>,
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


#[derive(sqlx::FromRow, Debug)]
pub struct DbPrivateKey {
    pub signer_id: Id<Statement>,
    pub key: String,
}

impl RowType for DbStatement {
    const TABLE: &'static str = "statement";
    const COLUMNS: &'static str =
        "statement.id,
        statement.name,
        statement.entity_1,
        statement.entity_2,
        statement.entity_3,
        statement.entity_4";
}

impl RowType for DbOpinion {
    const TABLE: &'static str = "opinion";
    const COLUMNS: &'static str =
        "opinion.id,
        opinion.statement_id,
        opinion.signer_id,
        opinion.date,
        opinion.valid,
        opinion.serial,
        opinion.certainty,
        opinion.comment,
        opinion.signature";
}

impl RowType for DbPrivateKey {
    const TABLE: &'static str = "private_key";
    const COLUMNS: &'static str =
        "private_key.signer_id,
        private_key.key";
}