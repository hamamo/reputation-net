use std::error::Error;
use std::str::FromStr;
use std::time::Instant;

#[allow(unused_imports)]
use log::info;

use crate::model::Date;
use crate::model::Statement;

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
            if let Err(e) = self.local_query(&what[1..]).await {
                println!("{:?}", e);
            }
            return;
        }
        match what.parse::<Statement>() {
            Ok(statement) => {
                let template = statement.specific_template();
                let result = self
                    .storage
                    .write()
                    .await
                    .persist_statement_hashing_emails(statement)
                    .await;
                match result {
                    Ok(actual_statement) => {
                        println!(
                            "{} statement {} has id {}",
                            actual_statement.wording(),
                            actual_statement.data,
                            actual_statement.id
                        );
                        let signed_statement = self.sign_statement(actual_statement).await.unwrap();
                        self.publish_statement(signed_statement).await;
                    }
                    Err(_e) => {
                        println!("No matching template: {}", template);
                        println!("Available:");
                        for t in self
                            .storage
                            .read()
                            .await
                            .list_templates(&template.name)
                            .await
                            .iter()
                        {
                            println!("  {:?}", t)
                        }
                    }
                }
            }
            Err(e) => println!("Invalid statement format: {:?}", e),
        };
    }

    async fn local_command(&mut self, command: &str) {
        let words = command.split_ascii_whitespace().collect::<Vec<_>>();
        if words.len() == 0 {
            return;
        }
        match words[0] {
            "fix-cidr" => {
                if let Err(e) = self.storage.write().await.fix_cidr().await {
                    println!("error: {:?}", e);
                }
            }
            "sync" => {
                let date = if words.len() > 1 {
                    match Date::from_str(words[1]) {
                        Ok(d) => d,
                        _ => match u32::from_str(words[1]) {
                            Ok(u) => Date::from(u),
                            _ => {
                                println!("could not parse date: {}", words[1]);
                                Date::today()
                            }
                        },
                    }
                } else {
                    Date::today()
                };
                let sync_infos = self
                    .storage
                    .read()
                    .await
                    .get_sync_infos(date)
                    .await
                    .expect("sync infos");
                println!("{:?}", sync_infos);
            }
            _ => println!("unknown command: {}", command),
        }
    }

    async fn local_query(&mut self, query: &str) -> Result<(), Box<dyn Error>> {
        let entity = Entity::from_str(query)?;
        let instant = Instant::now();
        let statements = self
            .storage
            .read()
            .await
            .find_statements_about(&entity)
            .await?;
        let duration = instant.elapsed();
        println!("Execution time: {:?}", duration);
        if statements.len() == 0 {
            println!("No matches");
        }
        for statement in statements {
            println!("{}: {}", statement.id, statement.data);
            let opinions = self
                .storage
                .read()
                .await
                .list_opinions_on(statement.id)
                .await?;
            for opinion in opinions {
                let data = &opinion.data;
                println!(
                    "  {}: {}..{}{} {} {}",
                    opinion.id,
                    data.date,
                    data.last_date(),
                    (if data.serial > 0 {
                        format!(".{}", data.serial)
                    } else {
                        "".into()
                    }),
                    data.certainty,
                    data.signer
                );
            }
        }
        return Ok(());
    }
}
