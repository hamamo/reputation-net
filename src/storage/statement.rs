use std::str::FromStr;

use async_trait::async_trait;
use log::{debug, error};
use sqlx::Error;

use crate::model::{Entity, Statement};

use super::{
    Convert, DbStatement, Get, GetRaw, Id, Persist, PersistResult, Persistent, RowType, Storage, DB,
};

#[async_trait]
impl Persist<Statement> for Storage {
    async fn persist(&mut self, statement: Statement) -> Result<PersistResult<Statement>, Error> {
        // ensure that the statement matches an existing template
        if !self.has_matching_template(&statement) {
            error!("did not find matching template for {}", statement);
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
                self.templates.insert(result.data.id, template.clone());
            }
        }
        if result.name == "signer" {
            if let Entity::Signer(signer) = &result.entities[0] {
                self.signers.insert(result.data.id, signer.clone());
            }
        }
        Ok(result)
    }
}

#[async_trait]
impl GetRaw<DbStatement, Id<Statement>> for Storage {
    async fn get_raw(&self, id: Id<Statement>) -> Result<Option<DbStatement>, Error> {
        sqlx::query_as::<DB, DbStatement>(&format!(
            "select {} from {} where id = ?",
            DbStatement::COLUMNS,
            DbStatement::TABLE
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }
}

#[async_trait]
impl Get<Statement> for Storage {
    async fn get(&self, id: Id<Statement>) -> Result<Option<Persistent<Statement>>, Error> {
        debug!("DB: getting {} with id {}", DbStatement::TABLE, id);
        match sqlx::query_as::<DB, DbStatement>(&format!(
            "select {} from {} where id = ?",
            DbStatement::COLUMNS,
            DbStatement::TABLE
        ))
        .bind(id)
        .fetch_one(&self.pool)
        .await
        {
            Ok(db_row) => {
                debug!("got DB row {:?}", db_row);
                let row = self.convert(db_row).await?;
                return Ok(Some(row));
            }
            Err(e) => {
                error!(
                    "error fetching {} with id {}: {:?}",
                    DbStatement::TABLE,
                    id,
                    e
                );
                Ok(None)
            }
        }
    }

    async fn get_all(&self) -> Result<Vec<Persistent<Statement>>, Error> {
        let rows = sqlx::query_as::<DB, DbStatement>(&format!(
            "select {} from {}",
            DbStatement::COLUMNS,
            DbStatement::TABLE
        ))
        .fetch_all(&self.pool)
        .await?;
        let mut list = vec![];
        for db_row in rows {
            list.push(self.convert(db_row).await?)
        }
        Ok(list)
    }
}

#[async_trait]
impl Convert<DbStatement, Persistent<Statement>> for Storage {
    async fn convert(&self, from: DbStatement) -> Result<Persistent<Statement>, Error> {
        let mut entities = vec![Entity::from_str(from.entity_1.as_str()).unwrap()];
        if let Some(entity) = from.entity_2 {
            entities.push(Entity::from_str(entity.as_str()).unwrap())
        }
        if let Some(entity) = from.entity_3 {
            entities.push(Entity::from_str(entity.as_str()).unwrap())
        }
        if let Some(entity) = from.entity_4 {
            entities.push(Entity::from_str(entity.as_str()).unwrap())
        }
        Ok(Persistent {
            id: from.id,
            data: Statement {
                name: from.name.clone(),
                entities,
            },
        })
    }
}
