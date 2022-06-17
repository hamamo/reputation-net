// the milter policy config structure

use async_recursion::async_recursion;
use std::{collections::HashMap, str::FromStr};

use serde_derive::Deserialize;

use crate::{model::Entity, storage::Storage};

use super::FieldValue;

#[derive(Deserialize, Debug, Default)]
pub struct Config {
    pub contact: Option<String>,
    #[serde(default)]
    pub rules: HashMap<String, Rule>,
    #[serde(default)]
    pub lists: HashMap<String, List>,
    #[serde(default)]
    pub conditions: HashMap<String, Condition>,
}

#[derive(Deserialize, Debug)]
pub struct Rule {
    pub priority: Option<u8>,
    pub field: Option<FieldRef>,
    #[serde(rename = "match")]
    pub list: Option<List>,
    pub condition: Option<Condition>,
    pub reject: Option<String>,
    pub defer: Option<String>,
    pub hold: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum List {
    Single(String),
    Multi(Vec<List>),
    Named { list: String },
    Reputation { reputation: String },
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum FieldRef {
    Single(String),
    Multi(Vec<String>),
}

#[derive(Deserialize, Debug)]
pub struct Condition {}

#[derive(Debug)]
pub struct MatchResult {
    pub field: String,
    pub priority: u8,
    pub action: Action,
}

#[derive(Debug)]
pub enum Action {
    Hold(String),
    Reject(String),
    Defer(String),
}

impl Rule {
    pub fn paths_matching_prefix(&self, prefix: &str) -> Vec<&str> {
        match &self.field {
            Some(field_ref) => field_ref.paths_matching_prefix(prefix),
            None => vec![],
        }
    }

    pub async fn match_value(
        &self,
        value: &FieldValue,
        config: &Config,
        storage: &Storage,
    ) -> Option<MatchResult> {
        match &self.list {
            Some(list) => {
                if list.matches_value(value, config, storage).await {
                    let action = self.action();
                    Some(MatchResult {
                        field: self.field_name(),
                        priority: self.priority.unwrap_or(5),
                        action,
                    })
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn action(&self) -> Action {
        use Action::*;
        if let Some(reason) = &self.reject {
            Reject(reason.into())
        } else if let Some(reason) = &self.defer {
            Defer(reason.into())
        } else if let Some(reason) = &self.hold {
            Hold(reason.into())
        } else {
            Hold("no reason given".into())
        }
    }

    fn field_name(&self) -> String {
        format!("{:?}", self.field)
    }
}

impl List {
    #[async_recursion]
    async fn matches_value(&self, value: &FieldValue, config: &Config, storage: &Storage) -> bool {
        match self {
            List::Single(string) => value.data() == string,
            List::Multi(lists) => {
                for list in lists {
                    if list.matches_value(value, config, storage).await {
                        return true;
                    }
                }
                false
            }
            List::Named { list } => match config.lists.get(list) {
                Some(list) => list.matches_value(value, config, storage).await,
                None => {
                    log::error!("No such list in milter config: {}", list);
                    false
                }
            },
            List::Reputation { reputation } => {
                let data = value.data();
                log::debug!(
                    "Reputation check: is {:?} in repnet-list {:?}?",
                    data,
                    reputation
                );
                match Entity::from_str(&data) {
                    Ok(entity) => {
                        log::debug!("Entity: {:?}", entity);
                        let reputation_results =
                            storage.find_statements_about(&entity).await.unwrap();
                        log::debug!("Reputation Results: {:?}", reputation_results);
                        for statement in reputation_results {
                            if &statement.name == reputation {
                                return true;
                            }
                        }
                        false
                    }
                    Err(_) => false,
                }
            }
        }
    }
}

impl FieldRef {
    fn paths_matching_prefix(&self, prefix: &str) -> Vec<&str> {
        use FieldRef::*;
        match self {
            Single(s) => {
                if s.starts_with(prefix) {
                    vec![&s[prefix.len()..]]
                } else {
                    vec![]
                }
            }
            Multi(m) => m
                .iter()
                .filter_map(|s| {
                    if s.starts_with(prefix) {
                        Some(&s[prefix.len()..])
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, anyhow::Error> {
        let toml = std::fs::read_to_string(path)?;
        Ok(Self::from_str(&toml)?)
    }

    fn finish_up(&mut self) {
        // this should populate the `rules_by_path` map
    }
}
impl FromStr for Config {
    type Err = toml::de::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut config: Self = toml::from_str(s)?;
        config.finish_up();
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_contact() {
        let toml = r#"
            contact = "please visit http://postmaster.example.com"
            "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.contact.as_ref().unwrap(),
            "please visit http://postmaster.example.com"
        );
    }

    #[test]
    fn parse_file() {
        let config = Config::from_file("src/milter/milter.toml").unwrap();
        assert_eq!(
            config.rules["reject_dynamic"].reject.as_ref().unwrap(),
            "Mail from dynamic network range"
        );
        assert!(config.rules["reject_dynamic"].hold.is_none());
    }
}
