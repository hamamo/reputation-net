// parser using nom. Currently only Template with the associated parts is parsed here

use cidr::{Ipv4Cidr, Ipv6Cidr};
use nom::{
    self,
    branch::alt,
    bytes::complete::{is_a, tag},
    character::complete::{alpha1, alphanumeric1, digit1, space1},
    combinator::{map, map_res, opt, recognize},
    error::Error,
    multi::{many0, many1, separated_list1},
    sequence::{pair, tuple},
    IResult,
};
use std::str::FromStr;

use super::{Entity, EntityType, PublicKey, Statement, Template};

// nom parser utilities
fn entity_type(i: &str) -> nom::IResult<&str, EntityType> {
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("Domain")(i) {
        return Ok((rest, EntityType::Domain));
    }
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("EMail")(i) {
        return Ok((rest, EntityType::EMail));
    }
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("AS")(i) {
        return Ok((rest, EntityType::AS));
    }
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("IPv4")(i) {
        return Ok((rest, EntityType::IPv4));
    }
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("IPv6")(i) {
        return Ok((rest, EntityType::IPv6));
    }
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("Signer")(i) {
        return Ok((rest, EntityType::Signer));
    }
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("Url")(i) {
        return Ok((rest, EntityType::Url));
    }
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("HashValue")(i) {
        return Ok((rest, EntityType::HashValue));
    }
    match nom::bytes::complete::tag("Template")(i) {
        Ok((rest, _)) => Ok((rest, EntityType::Template)),
        Err(e) => Err(e),
    }
}

fn entity_type_alternatives(i: &str) -> nom::IResult<&str, Vec<EntityType>> {
    nom::multi::separated_list1(nom::character::complete::char('|'), entity_type)(i)
}

fn entity_types(i: &str) -> nom::IResult<&str, Vec<Vec<EntityType>>> {
    nom::multi::separated_list1(
        nom::character::complete::char(','),
        entity_type_alternatives,
    )(i)
}
fn name(i: &str) -> nom::IResult<&str, &str> {
    recognize(pair(alpha1, many0(alt((alpha1, tag("_"))))))(i)
}

// a domain name label
fn label(i: &str) -> nom::IResult<&str, &str> {
    recognize(tuple((
        alphanumeric1,
        many0(alt((alphanumeric1, tag("-")))),
        opt(alphanumeric1),
    )))(i)
}

// a domain name
fn domain(i: &str) -> IResult<&str, Entity> {
    map(
        recognize(tuple((many0(tuple((label, tag(".")))), alpha1))),
        |s| Entity::Domain(s.into()),
    )(i)
}

// localpart - does not handle quoted strings and comments yet
fn localpart(i: &str) -> IResult<&str, &str> {
    recognize(many1(alt((alphanumeric1, is_a(".!#$%&'*+-/=?^_`{|}~")))))(i)
}

// an email address
fn email(i: &str) -> IResult<&str, Entity> {
    map(recognize(tuple((localpart, tag("@"), domain))), |s| {
        Entity::EMail(s.into())
    })(i)
}

// base64 string - returns matched characters
fn base64(i: &str) -> IResult<&str, &str> {
    recognize(tuple((
        many1(alt((alphanumeric1, is_a("./+")))),
        many0(is_a("=")),
    )))(i)
}

// a hashed email address - can be either raw input or already hashed base64
fn hash_value(i: &str) -> IResult<&str, Entity> {
    let (i, _) = tag("#")(i)?;
    alt((
        map(recognize(email), |s| Entity::hash_string(s)),
        map(base64, |s| Entity::HashValue(s.into())),
    ))(i)
}

// an AS
fn asn(i: &str) -> IResult<&str, Entity> {
    let (i, _) = tag("AS")(i)?;
    map(map_res(digit1, |s: &str| s.parse::<u32>()), |num| {
        Entity::AS(num)
    })(i)
}

// an IPv4 CIDR value
fn ipv4(i: &str) -> IResult<&str, Entity> {
    map(
        map_res(
            recognize(tuple((
                digit1,
                tag("."),
                digit1,
                tag("."),
                digit1,
                tag("."),
                digit1,
                opt(tuple((tag("/"), digit1))),
            ))),
            |s: &str| s.parse::<Ipv4Cidr>(),
        ),
        |cidr| Entity::IPv4(cidr),
    )(i)
}

// an IPv6 CIDR value
fn ipv6(i: &str) -> IResult<&str, Entity> {
    map(
        map_res(
            recognize(tuple((
                many1(is_a("0123456789ABCDEFabcdef")),
                many1(is_a("0123456789ABCDEFabcdef:")),
                opt(tuple((tag("/"), digit1))),
            ))),
            |s: &str| s.parse::<Ipv6Cidr>(),
        ),
        |cidr| Entity::IPv6(cidr),
    )(i)
}

// an URL (only web URLs are handled, excluding query parameters)
// This should be fixed to use Url::parse() from rust-url
fn url(i: &str) -> IResult<&str, Entity> {
    map(
        recognize(tuple((
            alt((tag("https"), tag("http"))),
            tag("://"),
            domain,
            opt(tuple((tag(":"), digit1))),
            many0(tuple((tag("/"), alt((alphanumeric1, is_a("-_.%+")))))),
        ))),
        |url| Entity::Url(url.into()),
    )(i)
}

// a template
pub fn template(i: &str) -> IResult<&str, Entity> {
    map(
        tuple((name, tag("("), entity_types, tag(")"))),
        |(name, _, entity_types, _)| {
            Entity::Template(Template {
                name: name.into(),
                entity_types,
            })
        },
    )(i)
}

pub fn signer(i: &str) -> IResult<&str, Entity> {
    map(recognize(tuple((tag("secp256k1:"), base64))), |s| {
        Entity::Signer(PublicKey::from_str(&s).expect("public key"))
    })(i)
}

pub fn entity(i: &str) -> IResult<&str, Entity> {
    alt((email, hash_value, template, asn, signer, domain, url, ipv4, ipv6))(i)
}

pub fn statement(i: &str) -> IResult<&str, Statement> {
    // accept standard form, and optionally human-typeable form with spaces instead of parentheses and commas
    alt((
        map(
            tuple((name, tag("("), separated_list1(tag(","), entity), tag(")"))),
            |(name, _, entities, _)| Statement {
                name: name.into(),
                entities,
            },
        ),
        map(
            tuple((name, space1, separated_list1(space1, entity))),
            |(name, _, entities)| Statement {
                name: name.into(),
                entities,
            },
        ),
    ))(i)
}

/* 
pub fn opinion_parameters(i: &str) -> IResult<&str, OpinionParameters> {
    map(
        separated_list1(alt((
            
        )))
    )
}
*/

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email() {
        assert_eq!(
            ("", Entity::EMail("user@example.com".into())),
            super::email("user@example.com").unwrap()
        );
        assert_eq!(
            (",", Entity::EMail("user@example.com".into())),
            super::email("user@example.com,").unwrap()
        );
    }
    #[test]
    fn hashed_email() {
        let entity = Entity::HashValue("tMmiiTI7IaAcPpQPFQ65uMVCWH8av9jw4cwf/F5HVRQ=".into());
        assert_eq!(
            ("", entity.clone()),
            super::hash_value("#user@example.com").unwrap()
        );

        assert_eq!(
            (",", entity),
            super::hash_value("#user@example.com,").unwrap()
        );
    }
    #[test]
    fn asn() {
        assert_eq!(("", Entity::AS(123)), super::asn("AS123").unwrap());
        assert_eq!((",", Entity::AS(123)), super::asn("AS123,").unwrap());
    }
    #[test]
    fn domain() {
        // domain starting with a digit
        // not allowed according to RFC1035, but used anyway
        assert_eq!(
            super::entity("3gen.com.mx").unwrap(),
            ("", Entity::Domain("3gen.com.mx".into())),
        );
        // tld, marked by trailing dot
        assert_eq!(
            super::entity("biz").unwrap(),
            ("", Entity::Domain("biz".into())),
        )
    }
    #[test]
    fn url() {
        assert_eq!(
            super::url("https://bit.ly/3fA9rE8").unwrap(),
            ("", Entity::Url("https://bit.ly/3fA9rE8".into())),
        )
    }
    #[test]
    fn statement() {
        assert_eq!(
            (
                "",
                Statement {
                    name: "abuse".into(),
                    entities: vec![
                        Entity::Domain("example.com".into()),
                        Entity::EMail("abuse@example.com".into())
                    ]
                }
            ),
            super::statement("abuse(example.com,abuse@example.com)").unwrap()
        )
    }
}
