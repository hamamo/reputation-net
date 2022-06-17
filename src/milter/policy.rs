use std::{collections::HashMap, fmt::Display, str::FromStr, sync::Arc};

use cidr::Cidr;
use lazy_static::lazy_static;
use mailparse::{addrparse_header, parse_header, MailAddr};
use regex::Regex;
use tokio::sync::RwLock;

use crate::{
    model::{Entity, Statement},
    storage::Storage,
};

use super::{config::Config, packet::*, FieldValue};

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum Location {
    ConnectAddress,
    ConnectName,
    Helo,
    MailFrom,
    RcptTo,
    Header(String),
    Body,
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
    #[allow(dead_code)]
    path: String,
    entity: Entity,
    statement: Statement,
}

pub struct PolicyAccumulator {
    storage: Arc<RwLock<Storage>>,
    pub config: Arc<Config>,
    macros: HashMap<String, String>,
    matches: Vec<Match>,
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
    pub fn new(storage: Arc<RwLock<Storage>>, config: Arc<Config>) -> Self {
        Self {
            storage,
            config,
            matches: vec![],
            macros: HashMap::new(),
            severity: Severity::None,
        }
    }

    pub fn reset(&mut self) {
        self.matches = vec![];
        self.macros = HashMap::new();
        self.severity = Severity::None;
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn reason(&self) -> String {
        match self
            .matches
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

    async fn lookup(&mut self, location: &Location, value: FieldValue) {
        log::debug!("looking up {} in {}", value, location);
        let prefix = location.prefix();
        for (rulename, rule) in &self.config.rules {
            for path in rule.paths_matching_prefix(&prefix) {
                let values = value.lookup_path(path).await;
                log::debug!(
                    "Found values {:?} in rule {} path {}",
                    values,
                    rulename,
                    path
                );
                let storage = &*self.storage.read().await;
                for v in values {
                    if let Some(result) = rule.match_value(&v, &self.config, storage).await {
                        println!(
                            "Rule {} matched {} in {}: {:?}",
                            rulename, value, location, result
                        );
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    async fn old_lookup(&mut self, location: &Location, what: &str) {
        if let Ok(entity) = Entity::from_str(what) {
            let statements = self.statements_about(&entity).await;
            if statements.len() == 0 {
                // println!("milter no match for {} in {}", entity, location);
            }
            for statement in statements {
                let qid = match self.macros.get("i") {
                    Some(s) => s.clone(),
                    None => "NOQUEUE".to_string(),
                };
                match statement.name.as_str() {
                    "known" | "asn" => (),
                    _ => println!("{}: {} in {} ({})", qid, entity, location, statement),
                }
                // ignore dynamic IPs anywhere else than in CONNECT
                if location == &Location::ConnectName
                    || location == &Location::ConnectAddress
                    || statement.name != "dynamic"
                {
                    self.severity = self.severity.max(statement.severity());
                    self.matches.push(Match {
                        location: location.clone(),
                        path: "".into(),
                        entity: entity.clone(),
                        statement,
                    });
                }
            }
        } else {
            /*
            println!(
                "{}: milter could not parse {} as entity in {}",
                match &self.macros.get("i") {
                    Some(s) => s,
                    None => "NOQUEUE",
                },
                what,
                location
            );
             */
        }
    }

    pub async fn macros(&mut self, data: &SmficMacro) -> () {
        for (key, value) in data.nameval.iter() {
            self.macros.insert(key.to_string(), value.to_string());
        }
    }

    pub async fn connect(&mut self, data: &SmficConnect) -> () {
        self.lookup(
            &Location::ConnectName,
            FieldValue::Domain(data.hostname.to_string()),
        )
        .await;
        self.lookup(
            &Location::ConnectAddress,
            FieldValue::Ipv4(data.address.to_string()),
        )
        .await;
    }

    pub async fn helo(&mut self, data: &SmficHelo) -> () {
        let helo = &data.helo.to_string();
        self.lookup(
            &Location::Helo,
            FieldValue::Domain(strip_brackets(helo).into()),
        )
        .await;
    }

    pub async fn mail_from(&mut self, data: &SmficMail) -> () {
        let value = data.args[0].to_string();
        let from = strip_brackets(&value);
        self.lookup(&Location::MailFrom, FieldValue::Mail(from.into()))
            .await;
        if let Some(srs_from) = srs_unpack(&from) {
            self.lookup(&Location::MailFrom, FieldValue::Mail(srs_from))
                .await;
        }
    }

    pub async fn header(&mut self, data: &SmficHeader) -> () {
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
                    let regex = Regex::new(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}").unwrap();
                    for m in regex.find_iter(value) {
                        // println!("{}: {}", key, m.as_str());
                        self.lookup(&location, FieldValue::Ipv4(m.as_str().into()))
                            .await;
                    }
                    let regex = Regex::new(r"([A-Za-z0-9-]{1, 63}\.)+[A-Za-z]{2,8}").unwrap();
                    for m in regex.find_iter(value) {
                        // println!("{}: {}", key, m.as_str());
                        self.lookup(&location, FieldValue::Domain(m.as_str().into()))
                            .await;
                    }
                }
                "from" | "reply-to" | "sender" => {
                    if let Ok(addrlist) = addrparse_header(&header) {
                        for addr in addrlist.iter() {
                            match addr {
                                MailAddr::Single(info) => {
                                    self.lookup(&location, FieldValue::Mail(info.addr.to_owned()))
                                        .await;
                                }
                                MailAddr::Group(info) => {
                                    for single in &info.addrs {
                                        self.lookup(
                                            &location,
                                            FieldValue::Mail(single.addr.to_owned()),
                                        )
                                        .await;
                                    }
                                }
                            }
                        }
                    }
                }
                _ => (),
            }
        } else {
            log::error!("could not parse header {}", data);
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

/// Strip angle brackets from an address.
fn strip_brackets(s: &str) -> &str {
    if s.starts_with("<") && s.ends_with(">") {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Unpack an SRS-encoded address.
/// This only handles SRS0 format, the deeper nested SRS1 is almost never seen in the wild.
fn srs_unpack(address: &str) -> Option<String> {
    lazy_static! {
        static ref SRS0_RE: Regex =
            Regex::new("SRS0[+=][^=]+=[^=]+=([^=]+)=([^@]+)@([^@]+)").unwrap();
    }
    if let Some(cap) = SRS0_RE.captures(address) {
        let inner_domain = cap.get(1).unwrap().as_str().to_string();
        let inner_localpart = cap.get(2).unwrap().as_str().to_string();
        Some(format!("{}@{}", inner_localpart, inner_domain))
    } else {
        None
    }
}

impl Location {
    fn prefix(&self) -> String {
        match self {
            Location::ConnectName => "connect.client-name".to_owned(),
            Location::ConnectAddress => "connect.client-addr".to_owned(),
            Location::Helo => "envelope.helo".to_owned(),
            Location::MailFrom => "envelope.mail-from".to_owned(),
            Location::RcptTo => "envelope.rcpt-to".to_owned(),
            Location::Header(key) => format!("header.{}", key.to_ascii_lowercase()),
            Location::Body => "body".to_owned(),
        }
    }
}

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::ConnectName | Location::ConnectAddress => write!(f, "CONNECT"),
            Location::Helo => write!(f, "HELO"),
            Location::MailFrom => write!(f, "MAIL FROM"),
            Location::RcptTo => write!(f, "RCPT TO"),
            Location::Header(key) => write!(f, "{:?} header", key),
            Location::Body => write!(f, "BODY"),
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
            // the following cases probably never appear in rejection reasons, but are handled for completeness
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
