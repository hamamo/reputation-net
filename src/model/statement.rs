use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use super::{entity::Entity, parser, percent_decode, percent_encode, template::Template};

#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub name: String,
    pub entities: Vec<Entity>,
}

impl Statement {
    pub fn new(name: &str, entities: Vec<Entity>) -> Self {
        Statement {
            name: name.to_string(),
            entities,
        }
    }

    pub fn matches_template(&self, template: &Template) -> bool {
        if self.name != template.name {
            return false;
        }
        let entity_iterator = self.entities.iter();
        let type_iterator = template.entity_types.iter();
        for (entity, entity_type_list) in entity_iterator.zip(type_iterator) {
            let entity_type = &entity.entity_type();
            if !entity_type_list.contains(entity_type) {
                return false;
            }
        }
        true
    }
}

// within Statements (and following, within Opinions), entities are percent-encoded to simplify parsing
impl Display for Statement {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}(", self.name)?;
        let mut first = true;
        for entity in &self.entities {
            if first {
                first = false
            } else {
                write!(f, ",")?;
            }
            write!(f, "{}", percent_encode(&entity.to_string()))?;
        }
        write!(f, ")")
    }
}

#[derive(Debug)]
pub struct InvalidStatement;
impl fmt::Display for InvalidStatement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid statement format")
    }
}

impl FromStr for Statement {
    type Err = InvalidStatement;
    fn from_str(s: &str) -> Result<Self, InvalidStatement> {
        match nom::combinator::all_consuming(parser::statement)(s) {
            Ok((_, stmt)) => Ok(stmt),
            _ => Err(InvalidStatement),
        }
    }
}

#[test]
fn statement() {
    let stmt_src = "abuse(example.com,abuse@example.com)";
    let stmt = Statement::from_str(stmt_src).unwrap();
    println!("{}", stmt);
    assert_eq!(stmt_src, stmt.to_string());
}

#[test]
fn match_template() {
    let stmt = Statement::from_str("abuse(example.com,abuse@example.com)").unwrap();
    let template = Template::from_str("abuse(Domain,EMail|Url)").unwrap();
    assert!(stmt.matches_template(&template));
}
