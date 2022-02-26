use itertools::Itertools;
use rocket::{get, serde::json::Json, Config, State};
use std::{str::FromStr, sync::Arc};
use tokio::sync::RwLock;

use crate::{
    model::{Entity, Statement},
    storage::Storage,
};

struct ManagedStorage {
    storage: Arc<RwLock<Storage>>,
}

#[get("/<entity>")]
async fn lookup(entity: String, state: &State<ManagedStorage>) -> Json<Vec<Statement>> {
    let storage = state.storage.read().await;
    let entity = Entity::from_str(&entity).unwrap();
    let statements = storage
        .find_statements_about(&entity)
        .await
        .expect("could read statements")
        .into_iter()
        .map(|ps| ps.data)
        .collect_vec();
    Json(statements)
}

pub async fn api(port: u16, storage: Arc<RwLock<Storage>>) -> Result<(), anyhow::Error> {
    let managed_storage = ManagedStorage { storage };
    let config = Config {
        address: "127.0.0.1".parse().unwrap(),
        port,
        ..Config::default()
    };
    rocket::build()
        .configure(config)
        .manage(managed_storage)
        .mount("/", routes![lookup])
        .launch()
        .await?;
    Ok(())
}
