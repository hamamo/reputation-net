use std::{collections::HashMap, fmt::Display, str::FromStr, sync::Arc};

use cidr::Cidr;
use lazy_static::lazy_static;
use log::error;
use mailparse::{addrparse_header, parse_header, MailAddr};
use regex::Regex;
use tokio::sync::RwLock;

use crate::{
    model::{Entity, Statement},
    storage::Storage,
};

use super::packet::*;

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Location {
    Connect,
    Helo,
    MailFrom,
    RcptTo,
    Header(String),
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
pub enum Severity {
    None = 0,
    #[allow(dead_code)]
    Quarantine = 1,
    Tempfail = 2,
    Reject = 3,
    Known = 4,
}

struct Match {
    location: Location,
    entity: Entity,
    statement: Statement,
}

pub struct PolicyAccumulator {
    storage: Arc<RwLock<Storage>>,
    statements: Vec<Match>,
    macros: HashMap<String, String>,
    severity: Severity,
}

impl Statement {
    fn severity(&self) -> Severity {
        match self.name.as_str() {
            "spammer" => Severity::Reject,
            "exploited" => Severity::Reject,
            "spammer_friendly" => Severity::Tempfail,
            "dynamic" => Severity::Tempfail,
            "known" => Severity::Known,
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
            .find(|m| m.statement.severity() == self.severity)
        {
            Some(m) => {
                if m.entity == m.statement.entities[0] {
                    format!("{}: {}", m.location, m.statement.reason())
                } else {
                    format!(
                        "{}: {} matches {}",
                        m.location,
                        m.entity.reason(),
                        m.statement.reason()
                    )
                }
            }
            None => String::new(),
        }
    }

    async fn lookup(&mut self, location: &Location, what: &str) {
        if let Ok(entity) = Entity::from_str(what) {
            let statements = self.statements_about(&entity).await;
            if statements.len() == 0 {
                // println!("milter no match for {} in {}", entity, location);
            }
            for statement in statements {
                println!(
                    "{}: {} in {} ({})",
                    match &self.macros.get("i") {
                        Some(s) => s,
                        None => "NOQUEUE",
                    },
                    entity,
                    location,
                    statement
                );
                self.severity = self.severity.max(statement.severity());
                self.statements.push(Match {
                    location: location.clone(),
                    entity: entity.clone(),
                    statement,
                });
            }
        } else {
            println!(
                "{}: milter could not parse {} as entity in {}",
                match &self.macros.get("i") {
                    Some(s) => s,
                    None => "NOQUEUE",
                },
                what,
                location
            );
        }
    }

    pub async fn macros(&mut self, data: &SmficMacro) -> () {
        for (key, value) in data.nameval.iter() {
            self.macros.insert(key.to_string(), value.to_string());
        }
    }

    pub async fn connect(&mut self, data: &SmficConnect) -> () {
        self.lookup(&Location::Connect, &data.hostname.to_string())
            .await;
        self.lookup(&Location::Connect, &data.address.to_string())
            .await;
    }

    pub async fn helo(&mut self, data: &SmficHelo) -> () {
        let helo = &data.helo.to_string();
        self.lookup(&Location::Helo, strip_brackets(helo)).await;
    }

    pub async fn mail_from(&mut self, data: &SmficMail) -> () {
        let from = &data.args[0].to_string();
        self.lookup(&Location::MailFrom, strip_brackets(from)).await;
    }

    pub async fn header(&mut self, data: &SmficHeader) -> () {
        lazy_static! {
            static ref IP_OR_DOMAIN_REGEX: Regex = Regex::new(
                r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}|([A-Za-z0-9-]{1, 63}\.)+[A-Za-z]{2,8}"
            )
            .unwrap();
        }
        let mut line = data.name.bytes.clone();
        line.extend(&b": ".to_vec());
        line.extend(&data.value.bytes);
        if let Ok((header, _)) = parse_header(&line) {
            let key = data.name.to_string();
            let lower_key = key.to_ascii_lowercase();
            let location = Location::Header(key.clone());
            match lower_key.as_str() {
                "received"
                | "arc-authentication-results"
                | "x-originatororg"
                | "x-ms-exchange-authentication-results"
                | "x-forefront-antispam-report"
                | "x-ms-exchange-crosstenant-originalattributedtenantconnectingip" => {
                    let header_value = header.get_value();
                    let value = header_value.as_str();
                    for m in IP_OR_DOMAIN_REGEX.find_iter(value) {
                        // println!("{}: {}", key, m.as_str());
                        self.lookup(&location, m.as_str()).await;
                    }
                }
                "from" | "reply-to" | "sender" => {
                    if let Ok(addrlist) = addrparse_header(&header) {
                        for addr in addrlist.iter() {
                            match addr {
                                MailAddr::Single(info) => {
                                    self.lookup(&location, &info.addr).await;
                                }
                                MailAddr::Group(info) => {
                                    for single in &info.addrs {
                                        self.lookup(&location, &single.addr).await;
                                    }
                                }
                            }
                        }
                    }
                }
                _ => (),
            }
        } else {
            error!("could not parse header {}", data);
        }
    }

    async fn statements_about(&self, entity: &Entity) -> Vec<Statement> {
        let storage = self.storage.read().await;
        storage
            .find_statements_about(entity)
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

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::Connect => write!(f, "CONNECT"),
            Location::Helo => write!(f, "HELO"),
            Location::MailFrom => write!(f, "MAIL FROM"),
            Location::RcptTo => write!(f, "RCPT TO"),
            Location::Header(key) => write!(f, "{:?} header", key),
        }
    }
}

impl Entity {
    fn reason(&self) -> String {
        match self {
            Entity::Domain(domain) => format!("domain {:?}", domain),
            Entity::EMail(address) => format!("address {:?}", address),
            Entity::AS(asn) => format!("autonomous system AS{}", asn),
            Entity::IPv4(addr) => {
                if addr.is_host_address() {
                    format!("IP address {}", addr)
                } else {
                    format!("IP range {}", addr)
                }
            }
            Entity::IPv6(addr) => {
                if addr.is_host_address() {
                    format!("IPv6 address {}", addr)
                } else {
                    format!("IPv6 range {}", addr)
                }
            }
            Entity::Signer(signer) => format!("signer {}", signer),
            Entity::Url(url) => format!("URL {:?}", url),
            Entity::HashValue(hash) => format!("hash value {:?}", hash),
            Entity::Template(template) => format!("template {}", template),
        }
    }
}

impl Statement {
    fn reason(&self) -> String {
        format!(
            "{} ({})",
            self.entities[0],
            match self.name.as_str() {
                "spammer" => "reported as spam source",
                "spammer_friendly" => "listed as spammer-friendly",
                "dynamic" => "listed as dynamic/anonymous network range",
                _ => &self.name.as_str(),
            }
        )
    }
}
