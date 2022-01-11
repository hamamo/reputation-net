use std::str::FromStr;

use async_trait::async_trait;
use sqlx::Error;

use crate::model::{Statement, Entity};

use super::{Repository, Storage, Id, Persistent, DB, PrimitiveId, PersistResult};

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