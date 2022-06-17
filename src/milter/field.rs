// Field values available to the milter.
// Field values may contain or lead to other field values, for example a domain name may lead to DNS records

use lazy_static::lazy_static;
use std::fmt::Display;
use tokio::task::JoinSet;

use trust_dns_resolver::{
    name_server::{GenericConnection, GenericConnectionProvider, TokioRuntime},
    AsyncResolver,
};

lazy_static! {
    static ref RESOLVER: AsyncResolver<GenericConnection, GenericConnectionProvider<TokioRuntime>> =
        AsyncResolver::tokio_from_system_conf().unwrap();
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum FieldValue {
    Str(String),
    Domain(String),
    Mail(String),
    #[allow(dead_code)]
    Url(String),
    Ipv4(String),
    Ipv6(String),
    #[allow(dead_code)]
    Header(String),
}

struct LookupTask {
    value: FieldValue,
    path: String,
}

impl LookupTask {
    async fn lookup(&self) -> Vec<Self> {
        let (first, rest) = match self.path.find(".") {
            Some(dot) => (&self.path[..dot], &self.path[dot + 1..]),
            None => (self.path.as_str(), ""),
        };
        self.value
            .lookup(first)
            .await
            .into_iter()
            .map(|x| Self {
                value: x,
                path: rest.to_owned(),
            })
            .collect()
    }
}

impl Display for FieldValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.data())
    }
}

impl FieldValue {
    pub async fn lookup_path(&self, path: &str) -> Vec<Self> {
        if path.is_empty() {
            return vec![self.clone()];
        }
        let path = &path[1..]; // first is a dot which we want to skip
        let mut tasks = JoinSet::new();
        let mut results = vec![];
        let first = LookupTask {
            value: self.clone(),
            path: path.to_owned(),
        };
        tasks.spawn(async move { first.lookup().await });
        while let Some(Ok(finished)) = tasks.join_one().await {
            for next in finished {
                if next.path.is_empty() {
                    results.push(next.value);
                } else {
                    tasks.spawn(async move { next.lookup().await });
                }
            }
        }
        results
    }

    async fn lookup(&self, part: &str) -> Vec<Self> {
        let result = match part {
            // Mail address parts
            "domain" => self.domain().await,
            "localpart" => self.localpart().await,
            // Domain name DNS records
            "a" => self.a().await,
            "aaaa" => self.aaaa().await,
            "mx" => self.mx().await,
            "ns" => self.ns().await,
            "txt" => self.txt().await,
            "ptr" => self.ptr().await,
            // other
            "cc" => self.cc().await,
            _ => {
                log::debug!("{} is not a valid field selector", part);
                vec![]
            }
        };
        log::debug!("Lookup {} {:?} -> {:?}", self, part, result);
        result
    }

    pub fn data(&self) -> &String {
        match self {
            FieldValue::Str(s) => s,
            FieldValue::Domain(s) => s,
            FieldValue::Mail(s) => s,
            FieldValue::Url(s) => s,
            FieldValue::Ipv4(s) => s,
            FieldValue::Ipv6(s) => s,
            FieldValue::Header(s) => s,
        }
    }

    async fn domain(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Mail(s) => match s.find("@") {
                Some(index) => return vec![Domain(s[index + 1..].to_owned())],
                None => log::debug!("{} does not have an @ sign", self),
            },
            Url(s) => match url::Url::parse(s) {
                Ok(u) => {
                    if let Some(host) = u.host() {
                        match host {
                            url::Host::Domain(s) => return vec![Domain(s.to_owned())],
                            url::Host::Ipv4(addr) => return Ipv4(addr.to_string()).ptr().await,
                            url::Host::Ipv6(addr) => return Ipv6(addr.to_string()).ptr().await,
                        }
                    }
                }
                Err(e) => log::debug!("{} URL parsing error: {:?}", self, e),
            },
            Header(_s) => todo!(),
            _ => log::debug!("{} does not have a domain", self),
        }
        vec![]
    }

    async fn localpart(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Mail(s) => match s.find("@") {
                Some(index) => return vec![Self::Str(s[0..index].to_owned())],
                None => log::error!("{} does not have an @ sign", self),
            },
            _ => log::debug!("{} does not have a localpart", self),
        }
        vec![]
    }

    async fn a(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Domain(d) => match RESOLVER.ipv4_lookup(format!("{}.", d)).await {
                Ok(result) => {
                    return result.iter().map(|addr| Ipv4(addr.to_string())).collect();
                }
                Err(e) => log::error!("Error looking up A record for {}: {:?}", d, e),
            },
            _ => log::debug!("{} can not be used for DNS lookup", self),
        }
        vec![]
    }

    async fn aaaa(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Domain(d) => match RESOLVER.ipv6_lookup(format!("{}.", d)).await {
                Ok(result) => {
                    return result.iter().map(|addr| Ipv6(addr.to_string())).collect();
                }
                Err(e) => log::error!("Error looking up AAAA record for {}: {:?}", d, e),
            },
            _ => log::debug!("{} can not be used for DNS lookup", self),
        }
        vec![]
    }

    async fn mx(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Domain(d) => match RESOLVER.mx_lookup(format!("{}.", d)).await {
                Ok(result) => {
                    return result
                        .iter()
                        .map(|mx| Domain(mx.exchange().to_conv_string()))
                        .collect();
                }
                Err(e) => log::error!("Error looking up MX record for {}: {:?}", d, e),
            },
            _ => log::debug!("{} can not be used for DNS lookup", self),
        }
        vec![]
    }

    async fn ns(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Domain(d) => match RESOLVER.ns_lookup(format!("{}.", d)).await {
                Ok(result) => {
                    return result
                        .iter()
                        .map(|name| Domain(name.to_conv_string()))
                        .collect();
                }
                Err(e) => log::error!("Error looking up MX record for {}: {:?}", d, e),
            },
            _ => log::debug!("{} can not be used for DNS lookup", self),
        }
        vec![]
    }

    async fn txt(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Domain(d) => match RESOLVER.txt_lookup(format!("{}.", d)).await {
                Ok(result) => {
                    return result.iter().map(|txt| Str(txt.to_string())).collect();
                }
                Err(e) => log::error!("Error looking up TXT record for {}: {:?}", d, e),
            },
            _ => log::debug!("{} can not be used for DNS lookup", self),
        }
        vec![]
    }

    async fn ptr(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Ipv4(ip) => match RESOLVER.reverse_lookup(ip.parse().unwrap()).await {
                Ok(result) => {
                    return result
                        .iter()
                        .map(|name| Domain(name.to_conv_string()))
                        .collect();
                }
                Err(e) => log::error!("Error looking up PTR record for {}: {:?}", ip, e),
            },
            _ => log::debug!("{} can not be used for DNS lookup", self),
        }
        vec![]
    }

    async fn cc(&self) -> Vec<Self> {
        use FieldValue::*;
        match self {
            Domain(string) => {
                let len = string.len();
                if len > 3 && &string[len - 3..len - 2] == "." {
                    return vec![Self::Str(string[len - 2..].into())];
                }
            }
            _ => log::debug!("{} can not be used for CC lookup", self),
        }
        vec![]
    }
}

trait ConventionalDomainName {
    fn to_conv_string(&self) -> String;
}

impl ConventionalDomainName for trust_dns_resolver::Name {
    fn to_conv_string(&self) -> String {
        let ascii = self.to_lowercase().to_ascii();
        ascii[0..ascii.len() - 1].to_owned()
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn lookup_localpart() {
        use super::FieldValue::*;
        let root = Mail("user@example.com".to_owned());
        assert_eq!(
            root.lookup_path(".localpart").await,
            vec![Str("user".to_owned())]
        );
    }
    #[tokio::test]
    async fn lookup_mail_domain() {
        use super::FieldValue::*;
        let root = Mail("user@example.com".to_owned());
        assert_eq!(
            root.lookup_path(".domain").await,
            vec![Domain("example.com".to_owned())]
        );
    }
    #[tokio::test]
    async fn lookup_domain_a() {
        use super::FieldValue::*;
        let root = Domain("example.com".to_owned());
        assert_eq!(
            root.lookup_path(".a").await,
            vec![Ipv4("93.184.216.34".to_owned())]
        );
    }
    #[tokio::test]
    async fn lookup_ptr() {
        use super::FieldValue::*;
        let root = Ipv4("74.125.143.26".to_owned());
        assert_eq!(
            root.lookup_path(".ptr").await,
            vec![Domain("ed-in-f26.1e100.net".to_owned())]
        );
    }
    #[tokio::test]
    async fn lookup_mail_domain_a() {
        use super::FieldValue::*;
        let root = Mail("user@example.com".to_owned());
        assert_eq!(
            root.lookup_path(".domain.a").await,
            vec![Ipv4("93.184.216.34".to_owned())]
        );
    }
    #[tokio::test]
    async fn lookup_domain_aaaa() {
        use super::FieldValue::*;
        let root = Domain("example.com".to_owned());
        assert_eq!(
            root.lookup_path(".aaaa").await,
            vec![Ipv6("2606:2800:220:1:248:1893:25c8:1946".to_owned())]
        );
    }
    #[tokio::test]
    async fn lookup_domain_mx() {
        use super::FieldValue::*;
        let root = Domain("google.com".to_owned());
        assert_eq!(
            root.lookup_path(".mx").await,
            vec![Domain("smtp.google.com".to_owned())]
        );
    }
}
