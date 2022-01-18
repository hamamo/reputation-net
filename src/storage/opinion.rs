use std::str::FromStr;

use async_trait::async_trait;
use log::{debug, error};
use sqlx::Error;

use crate::model::{Statement, Entity, SignedOpinion};

use super::{Repository, Storage, Id, Persistent, DB, PersistResult, DbOpinion};

struct OpinionFkType {
    statement_id: Id<Statement>, signer_id: Id<Statement>
}

#[async_trait]
impl Repository<SignedOpinion> for Storage {
    type MyRowType = DbOpinion;
    type FkType = OpinionFkType;

    async fn get(&self, id: Id<Statement>) -> Result<Option<Persistent<Statement>>, Error> {
        debug!("DB: getting {} with id {}", Self::MyRowType::TABLE, id);
        match sqlx::query_as::<DB, Self::MyRowType>(&format!(
            "select {} from {} where id = ?",
            Self::MyRowType::COLUMNS,
            Self::MyRowType::TABLE
        ))
        .bind(id.id)
        .fetch_one(&self.pool)
        .await
        {
            Ok(row) => {
                debug!("got row {:?}", row);
                return Ok(Some(Self::row_to_record(row)));
            }
            Err(e) => {
                error!("error fetching {} with id {}: {:?}", Self::MyRowType::TABLE, id, e);
                Ok(None)
            }
        }
    }

    async fn get_all(&self) -> Result<Vec<Persistent<SignedOpinion>>, Error> {
        // dummy implementation for now
        let rows = sqlx::query_as::<DB, Self::RowType>(
            "select
                    id,
                    statement_id,
                    signer_id,
                    date,
                    valid,
                    serial,
                    certainty,
                    signature
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

    async fn persist(&mut self, opinion: SignedOpinion) -> Result<PersistResult<SignedOpinion>, Error> {
        
        Ok(result)
    }

    fn row_to_record(row: Self::MyRowType) -> Persistent<Statement> {
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