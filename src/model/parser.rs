// parser using nom. Currently only Template with the associated parts is parsed here

use cidr::{Ipv4Cidr, Ipv6Cidr};
use nom::{
    self,
    branch::alt,
    bytes::complete::{is_a, tag},
    character::complete::{alpha1, alphanumeric1, digit1},
    combinator::{map, map_res, opt, recognize},
    error::Error,
    multi::{many0, many1, separated_list1},
    sequence::{pair, tuple},
    IResult,
};

use super::{Entity, EntityType, Statement, Template};

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
    if let Ok((rest, _)) = nom::bytes::complete::tag::<&str, &str, Error<&str>>("HashedEMail")(i) {
        return Ok((rest, EntityType::HashedEMail));
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
        alpha1,
        many0(alt((alpha1, tag("-")))),
        opt(alphanumeric1),
    )))(i)
}

// a domain name
fn domain(i: &str) -> IResult<&str, &str> {
    recognize(tuple((label, tag("."), separated_list1(tag("."), label))))(i)
}

// localpart - does not handle quoted strings and comments yet
fn localpart(i: &str) -> IResult<&str, &str> {
    recognize(many1(alt((alphanumeric1, is_a(".!#$%&'*+-/=?^_`{|}~")))))(i)
}

// an email address
fn email(i: &str) -> IResult<&str, &str> {
    recognize(tuple((localpart, tag("@"), domain)))(i)
}

// an AS number
fn asn(i: &str) -> IResult<&str, u32> {
    let (i, _) = tag("AS")(i)?;
    map_res(digit1, |s: &str| s.parse::<u32>())(i)
}

// an IPv4 CIDR value
fn ipv4(i: &str) -> IResult<&str, Ipv4Cidr> {
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
    )(i)
}

// an IPv6 CIDR value
fn ipv6(i: &str) -> IResult<&str, Ipv6Cidr> {
    map_res(
        recognize(many1(is_a("0123456789ABCDEFabcdef:/"))),
        |s: &str| s.parse::<Ipv6Cidr>(),
    )(i)
}

// top level rules
pub fn template(i: &str) -> IResult<&str, Template> {
    let (i, name) = name(i)?;
    let (i, _) = tag("(")(i)?;
    let (i, entity_types) = entity_types(i)?;
    let (i, _) = tag(")")(i)?;
    Ok((
        i,
        Template {
            name: name.into(),
            entity_types,
        },
    ))
}

pub fn entity(i: &str) -> IResult<&str, Entity> {
    alt((
        map(email, |s: &str| Entity::EMail(s.into())),
        map(template, |t: Template| Entity::Template(t)),
        map(asn, |num: u32| Entity::AS(num)),
        map(ipv4, |s: Ipv4Cidr| Entity::IPv4(s)),
        map(ipv6, |s: Ipv6Cidr| Entity::IPv6(s)),
        map(domain, |s: &str| Entity::Domain(s.into())),
    ))(i)
}

pub fn statement(i: &str) -> IResult<&str, Statement> {
    let (i, name) = name(i)?;
    let (i, _) = tag("(")(i)?;
    let (i, entities) = separated_list1(tag(","), entity)(i)?;
    let (i, _) = tag(")")(i)?;
    Ok((
        i,
        Statement {
            name: name.into(),
            entities,
        },
    ))
}

mod tests {
    use crate::model::{Entity, Statement};

    #[test]
    fn email() {
        assert_eq!(
            ("", "user@example.com"),
            super::email("user@example.com").unwrap()
        );
        assert_eq!(
            (",", "user@example.com"),
            super::email("user@example.com,").unwrap()
        );
    }
    #[test]
    fn asn() {
        assert_eq!(("", 123), super::asn("AS123").unwrap());
        assert_eq!((",", 123), super::asn("AS123,").unwrap());
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
