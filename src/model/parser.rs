// parser using nom. Currently only Template with the associated parts is parsed here

use nom::{self, IResult, branch::alt, bytes::complete::tag, character::complete::{alpha1, alphanumeric1}, combinator::{map, opt, recognize}, error::Error, multi::{many0, separated_list1}, sequence::{pair, tuple}};

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
    recognize(pair(alpha1, many0(alt((alpha1,tag("_"))))))(i)
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
    recognize(separated_list1(tag("."), label))(i)
}

// an email address - localpart is incorrect!
fn email(i: &str) -> IResult<&str, &str> {
    recognize(tuple((name, tag("@"), domain)))(i)
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
    fn parse_email() {
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
    fn parse_statement() {
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
