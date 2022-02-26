use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use itertools::Itertools;

use super::parser;

use super::entity::{Entity, EntityType};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Template {
    pub name: String,
    pub entity_types: Vec<Vec<EntityType>>,
}

pub struct SpecificTemplate {
    pub name: String,
    pub entity_types: Vec<EntityType>,
}

impl Display for Template {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{}({})",
            self.name,
            &self
                .entity_types
                .iter()
                .map(|x| {
                    (&x).iter()
                        .map(EntityType::to_string)
                        .collect_vec()
                        .join("|")
                        .to_string()
                })
                .collect_vec()
                .join(",")
        )
    }
}

impl Display for SpecificTemplate {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{}({})",
            self.name,
            &self
                .entity_types
                .iter()
                .map(EntityType::to_string)
                .collect_vec()
                .join(",")
        )
    }
}

#[derive(Debug)]
pub struct InvalidTemplate;
impl Display for InvalidTemplate {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "invalid template")
    }
}

impl FromStr for Template {
    type Err = InvalidTemplate;

    fn from_str(input: &str) -> Result<Self, InvalidTemplate> {
        match nom::combinator::all_consuming(parser::template)(input) {
            Ok((_, Entity::Template(template))) => Ok(template),
            _ => Err(InvalidTemplate {}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        let template = Template {
            name: "template".into(),
            entity_types: vec![vec![EntityType::Template]],
        };
        assert_eq!(template.to_string(), "template(Template)");
    }

    #[test]
    fn from_str() {
        let template = Template {
            name: "template".into(),
            entity_types: vec![vec![EntityType::Template]],
        };
        assert_eq!(template, Template::from_str("template(Template)").unwrap())
    }

    #[test]
    fn from_str_spammer() {
        let input = "spammer(HashValue|IPv4|IPv6)";
        let template: Template = Template::from_str(input).unwrap();
        assert_eq!(template.name, "spammer");
        assert_eq!(
            template.entity_types,
            vec![vec![
                EntityType::HashValue,
                EntityType::IPv4,
                EntityType::IPv6
            ]]
        );
    }
}
