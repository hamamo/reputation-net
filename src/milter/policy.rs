use std::{collections::HashMap, str::FromStr, sync::Arc};

use async_std::sync::RwLock;
use lazy_static::lazy_static;
use log::error;
use mailparse::{addrparse_header, parse_header, MailAddr};
use unicase::UniCase;

use crate::{
    model::{Entity, Statement},
    storage::Storage,
};

use super::packet::*;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum StatementLocation {
    Connect,
    Helo,
    MailFrom,
    RcptTo,
    HeaderReceived,
    HeaderFrom,
    HeaderReplyTo,
    HeaderSender,
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
pub enum Severity {
    None = 0,
    Quarantine = 1,
    Tempfail = 2,
    Reject = 3,
}

pub struct PolicyAccumulator {
    storage: Arc<RwLock<Storage>>,
    statements: Vec<(StatementLocation, Statement)>,
    macros: HashMap<String, String>,
    severity: Severity,
}

impl Statement {
    fn severity(&self) -> Severity {
        match self.name.as_str() {
            "spammer" => Severity::Reject,
            "exploited" => Severity::Reject,
            "spammer_friendly" => Severity::Quarantine,
            "dynamic" => Severity::Tempfail,
            _ => Severity::None,
        }
    }
}

impl PolicyAccumulator {
    pub fn new(storage: Arc<RwLock<Storage>>) -> Self {
        Self {
            storage: storage,
            statements: vec![],
            macros: HashMap::new(),
            severity: Severity::None,
        }
    }

    pub fn reset(&mut self) {
        self.statements = vec![];
        self.macros = HashMap::new();
        self.severity = Severity::None;
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn reason(&self) -> String {
        match self
            .statements
            .iter()
            .find(|(_location, statement)| statement.severity() == self.severity)
        {
            Some((location, statement)) => format!("{} in {:?}", statement, location),
            None => String::new(),
        }
    }

    async fn lookup(&mut self, location: StatementLocation, what: &str) {
        let statements = self.statements_about(what).await;
        for statement in statements {
            self.add(location, statement);
        }
    }

    fn add(&mut self, location: StatementLocation, statement: Statement) {
        self.severity = self.severity.max(statement.severity());
        self.statements.push((location, statement));
    }

    pub async fn macros(&mut self, _data: &SmficMacro) -> () {}

    pub async fn connect(&mut self, data: &SmficConnect) -> () {
        self.lookup(StatementLocation::Connect, &data.hostname.to_string())
            .await;
        self.lookup(StatementLocation::Connect, &data.address.to_string())
            .await;
    }

    pub async fn helo(&mut self, data: &SmficHelo) -> () {
        let helo = &data.helo.to_string();
        self.lookup(StatementLocation::Helo, strip_brackets(helo))
            .await;
    }

    pub async fn mail_from(&mut self, data: &SmficMail) -> () {
        let from = &data.args[0].to_string();
        self.lookup(StatementLocation::MailFrom, strip_brackets(from))
            .await;
    }

    pub async fn header(&mut self, data: &SmficHeader) -> () {
        lazy_static! {
            static ref FROM: UniCase<&'static str> = UniCase::new("from");
            static ref SENDER: UniCase<&'static str> = UniCase::new("sender");
            static ref REPLY_TO: UniCase<&'static str> = UniCase::new("reply-to");
        }
        let mut line = data.name.bytes.clone();
        line.extend(&b": ".to_vec());
        line.extend(&data.value.bytes);
        if let Ok((header, _)) = parse_header(&line) {
            let key = UniCase::new(header.get_key_ref());
            let location = if FROM.eq(&key) {
                Some(StatementLocation::HeaderFrom)
            } else if SENDER.eq(&key) {
                Some(StatementLocation::HeaderSender)
            } else if REPLY_TO.eq(&key) {
                Some(StatementLocation::HeaderReplyTo)
            } else {
                None
            };
            if let Some(location) = location {
                if let Ok(addrlist) = addrparse_header(&header) {
                    for addr in addrlist.iter() {
                        match addr {
                            MailAddr::Single(info) => {
                                self.lookup(location, &info.addr).await;
                            }
                            MailAddr::Group(info) => {
                                for single in &info.addrs {
                                    self.lookup(location, &single.addr).await;
                                }
                            }
                        }
                    }
                }
            }
        } else {
            error!("could not parse header {}", data);
        }
    }

    async fn statements_about(&self, s: &str) -> Vec<Statement> {
        let entity = Entity::from_str(s).unwrap();
        let storage = self.storage.read().await;
        storage
            .find_statements_about(&entity)
            .await
            .unwrap()
            .into_iter()
            .map(|ps| ps.data)
            .collect()
    }
}

fn strip_brackets(s: &str) -> &str {
    if s.starts_with("<") && s.ends_with(">") {
        &s[1..s.len() - 1]
    } else {
        s
    }
}
