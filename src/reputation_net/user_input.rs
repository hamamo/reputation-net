use std::{error::Error, str::FromStr, time::Instant};

use futures::{channel::mpsc::Sender, SinkExt};
use itertools::Itertools;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};

use log::{error, info};

use crate::{model::{Date, Statement}, storage::Persist};

/// functions handling user input (currently simple stdin, will be changed to handle readline and proper output)
use super::{Entity, ReputationNet};

impl ReputationNet {
    pub async fn handle_user_input(&mut self, what: &str) {
        /* for now, create opinions with default values. I don't know yet how the UI should look finally */

        if what == "" {
            return;
        }
        if what.starts_with("!") {
            return self.local_command(&what[1..]).await;
        }
        if what.starts_with("?") {
            if let Err(e) = self.local_query(&what[1..]).await {
                error!("{:?}", e);
            }
            return;
        }
        match what.parse::<Statement>() {
            Ok(statement) => {
                let template = statement.specific_template();
                let statement = if self.storage.read().await.has_matching_template(&statement) {
                    statement
                } else {
                    statement.hash_emails()
                };
                let result = self.storage.write().await.persist(&statement).await;
                match result {
                    Ok(persist_result) => {
                        println!("{}", persist_result);
                        let signed_statement = self
                            .sign_statement(statement, persist_result.id)
                            .await
                            .unwrap();
                        self.publish_statement(signed_statement);
                    }
                    Err(_e) => {
                        error!("No matching template: {}", template);
                        error!("Available:");
                        for t in self
                            .storage
                            .read()
                            .await
                            .list_templates(&template.name)
                            .await
                            .iter()
                        {
                            error!("  {:?}", t)
                        }
                    }
                }
            }
            Err(e) => error!("Invalid statement format: {:?}", e),
        };
    }

    async fn local_command(&mut self, command: &str) {
        let words = command.split_ascii_whitespace().collect_vec();
        if words.len() == 0 {
            return;
        }
        match words[0] {
            "sync" => {
                let date = if words.len() > 1 {
                    match Date::from_str(words[1]) {
                        Ok(d) => d,
                        _ => match u32::from_str(words[1]) {
                            Ok(u) => Date::from(u),
                            _ => {
                                error!("could not parse date: {}", words[1]);
                                Date::today()
                            }
                        },
                    }
                } else {
                    Date::today()
                };
                // println!("sending announce for {}", date);
                self.announce_infos(date).await
            }
            _ => error!("unknown command: {}", command),
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
        info!("Execution time: {:?}", duration);
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

pub async fn input_reader(mut sender: Sender<String>) -> Result<(), std::io::Error> {
    let mut stdin = BufReader::new(stdin()).lines();
    loop {
        match stdin.next_line().await {
            Ok(result) => match result {
                Some(line) => {
                    sender.send(line).await.expect("could send");
                }
                None => {
                    println!("EOF on stdin");
                    return Ok(());
                }
            },
            Err(e) => {
                println!("Error {} on stdin", e);
                return Err(e);
            }
        }
    }
}
