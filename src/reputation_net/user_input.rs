use std::error::Error;
use std::str::FromStr;

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
                match self
                    .storage
                    .persist_statement_hashing_emails(statement)
                    .await
                {
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
                        for t in self.storage.list_templates(&template.name).await.iter() {
                            println!("  {:?}", t)
                        }
                    }
                }
            }
            Err(e) => println!("Invalid statement format: {:?}", e),
        };
    }

    async fn local_command(&mut self, command: &str) {
        match command {
            "fix-cidr" => {
                if let Err(e) = self.storage.fix_cidr().await {
                    println!("error: {:?}", e);
                }
            }
            _ => println!("unknown command: {}", command),
        }
    }

    async fn local_query(&mut self, query: &str) -> Result<(), Box<dyn Error>> {
        let entity = Entity::from_str(query)?;
        let statements = self.storage.find_statements_about(&entity).await?;
        if statements.len() == 0 {
            println!("No matches");
        }
        for statement in statements {
            println!("{}: {}", statement.id, statement.data);
            let opinions = self.storage.list_opinions_on(statement.id).await?;
            for opinion in opinions {
                let data = &opinion.data;
                println!(
                    "  {}: {}..{}{} {} {}",
                    opinion.id,
                    Date::from(data.date),
                    Date::from(data.last_date()),
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
