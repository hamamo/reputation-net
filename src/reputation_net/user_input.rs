use std::str::FromStr;

/// functions handling user input (currently simple stdin, will be changed to handle readline and proper output)
use super::ReputationNet;

use super::Entity;

impl ReputationNet {
    pub async fn handle_input(&mut self, what: &str) {
        /* for now, create opinions with default values. I don't know yet how the UI should look finally */

        if what.starts_with("!") {
            return self.local_command(&what[1..]).await;
        }
        if what.starts_with("?") {
            return self.local_query(&what[1..]).await;
        }
        match what.parse() {
            Ok(statement) => match self
                .storage
                .persist_statement_hashing_emails(&statement)
                .await
            {
                Ok((persist_result, actual_statement)) => {
                    println!(
                        "{} statement {} has id {}",
                        persist_result.wording(),
                        actual_statement,
                        persist_result.id
                    );
                    let signed_statement = self
                        .sign_statement(actual_statement, persist_result.id)
                        .await
                        .unwrap();
                    if persist_result.is_new() {
                        // we ignore the possible error when no peers are currently connected
                        self.publish_statement(&signed_statement).await
                    };
                }
                Err(_e) => {
                    println!("No matching template: {}", statement.specific_template());
                    println!("Available:");
                    for t in self.storage.list_templates(&statement.name).await.iter() {
                        println!("  {}", t)
                    }
                }
            },
            Err(e) => println!("Invalid statement format: {:?}", e),
        };
    }

    async fn local_command(&mut self, command: &str) {
        println!("local: {}", command);
    }

    async fn local_query(&mut self, query: &str) {
        match Entity::from_str(query) {
            Ok(entity) => {
                let statements = self.storage.find_statements_about(&entity).await;
                for s in statements {
                    println!("{:?}", s);
                }
            }
            Err(e) => println!("{:?}", e),
        }
    }
}
