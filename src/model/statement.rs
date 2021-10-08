use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use nom::combinator::all_consuming;

use super::{entity::Entity, parser, template::Template};

#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub name: String,
    pub entities: Vec<Entity>,
}

impl Statement {
    #[allow(dead_code)]
    pub fn new(name: &str, entities: Vec<Entity>) -> Self {
        Self {
            name: name.into(),
            entities,
        }
    }

    pub fn minimal_template(&self) -> Template {
        Template{
            name: self.name.clone(),
            entity_types: self.entities.iter().map(|x| vec![x.entity_type()]).collect()
        }
    }
    
    pub fn matches_template(&self, template: &Template) -> bool {
        self.name == template.name && {
            for (entity, entity_type_list) in self.entities.iter().zip(template.entity_types.iter())
            {
                let entity_type = &entity.entity_type();
                if !entity_type_list.contains(entity_type) {
                    return false;
                }
            }
            true
        }
    }

    // create a version of self where literal e-mail addresses are replaced by hashed e-mail addresses
    // hash function is SHA256
    pub fn hash_emails(&self) -> Self {
        Self{
            name: self.name.clone(),
            entities: self.entities.iter().map(|e| e.hash_emails()).collect()
        }
    }

    #[allow(dead_code)]
    // return a byte vector for signing
    pub fn signable_bytes(&self) -> Vec<u8> {
        self.to_string().as_bytes().to_vec()
    }
}

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
            write!(f, "{}", entity)?;
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
        match all_consuming(parser::statement)(s) {
            Ok((_, stmt)) => Ok(stmt),
            _ => Err(InvalidStatement),
        }
    }
}

impl From<&str> for Statement {
    fn from(s: &str) -> Self {
        match all_consuming(parser::statement)(s) {
            Ok((_, stmt)) => stmt,
            _ => panic!("expected a statement"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Statement,Template};
    use std::str::FromStr;

    #[test]
    fn statement() {
        let stmt_src = "abuse(example.com,abuse@example.com)";
        let stmt = Statement::from_str(stmt_src).unwrap();
        println!("{}", stmt);
        assert_eq!(stmt_src, stmt.to_string());
    }

    #[test]
    fn template_statement() {
        let stmt_src = "template(exploited_host(Domain|IPv4))";
        let stmt = Statement::from_str(stmt_src).unwrap();
        assert_eq!(stmt_src, stmt.to_string());
    }

    #[test]
    fn match_template() {
        let stmt = Statement::from_str("abuse(example.com,abuse@example.com)").unwrap();
        let template = Template::from_str("abuse(Domain,EMail|Url)").unwrap();
        assert!(stmt.matches_template(&template));
    }
}
