use axum::{
    extract::Path,
    routing::{get, post},
    Extension, Json, Router,
};

use itertools::Itertools;
use std::{
    net::{IpAddr, Ipv6Addr, SocketAddr},
    str::FromStr,
    sync::Arc,
};
use tokio::sync::RwLock;

use crate::{
    model::{Entity, Statement},
    storage::Storage,
};

async fn lookup(
    Path(entity): Path<String>,
    state: Extension<Arc<RwLock<Storage>>>,
) -> Json<Vec<Statement>> {
    let storage = state.read().await;
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

async fn insert_statement(statement: String, state: Extension<Arc<RwLock<Storage>>>) {
    let statement = Statement::from_str(&statement).expect("invalid statement");
    let mut storage = state.write().await;
    let key = storage.own_key().clone();
    storage
        .persist_statement_hashing_emails(&statement)
        .await
        .expect("could not insert statement");
    storage
        .sign_statement_default(&statement, &key)
        .await
        .expect("could not persist");
}

pub async fn api(port: u16, storage: Arc<RwLock<Storage>>) -> Result<(), anyhow::Error> {
    let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port);
    let routes = Router::new()
        .route("/entity/:ent", get(lookup))
        .route("/statement", post(insert_statement))
        .layer(Extension(Arc::clone(&storage)));
    let api = Router::new().nest("/api", routes);
    axum::Server::bind(&addr)
        .serve(api.into_make_service())
        .await?;
    Ok(())
}
