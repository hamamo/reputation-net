use std::fmt;
use std::fmt::{Display, Formatter};
use std::num::ParseIntError;
use std::ops::Deref;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{percent_decode, percent_encode, Keypair, PublicKey, Signature, Statement, Date};

#[derive(Clone, Debug, PartialEq)]
pub struct UnsignedOpinion {
    pub date: Date,       // day since the UNIX epoch
    pub valid: u16,      // number of days this opinion is considered valid
    pub serial: u8, // to detect most recent opinion about a statement if more than one are made on one day
    pub certainty: i8, // positive or negative certainty in range -3..3.
    pub comment: String, // optional comment, may be empty
}

#[derive(Clone, Debug)]
pub struct Opinion {
    pub data: UnsignedOpinion,
    pub signer: PublicKey,
    pub signature: Signature,
}

// SignedStatement is actually a list of signed opinions about a single statement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedStatement {
    pub statement: Statement,
    pub opinions: Vec<Opinion>,
}

impl UnsignedOpinion {
    pub fn sign_using(self, statement_bytes: &Vec<u8>, keypair: &Keypair) -> Opinion {
        // return a signed version. If original is already signed, strip signature first

        let signature = keypair.sign(&self.signable_bytes(statement_bytes)).unwrap();
        Opinion {
            data: self,
            signer: PublicKey {
                key: keypair.public(),
            },
            signature: signature,
        }
    }

    pub fn last_date(&self) -> Date {
        self.date + self.valid
    }

    fn signable_bytes(&self, statement_bytes: &Vec<u8>) -> Vec<u8> {
        let mut bytes = self.to_string().as_bytes().to_vec();
        bytes.extend(statement_bytes);
        bytes
    }
}

impl Default for UnsignedOpinion {
    fn default() -> Self {
        Self {
            date: Date::today(),
            valid: 7,
            serial: 0,
            certainty: 3,
            comment: "".into(),
        }
    }
}

impl Display for UnsignedOpinion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{};{};{};{};{}",
            self.date.d,
            self.valid,
            self.serial,
            self.certainty,
            percent_encode(&self.comment),
        )?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct InvalidFormat {
    cause: String,
}

impl fmt::Display for InvalidFormat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid opinion format: {}", self.cause)
    }
}

impl From<ParseIntError> for InvalidFormat {
    fn from(err: ParseIntError) -> Self {
        Self {
            cause: err.to_string(),
        }
    }
}

impl FromStr for UnsignedOpinion {
    type Err = InvalidFormat;
    fn from_str(s: &str) -> Result<Self, InvalidFormat> {
        let parts: Vec<&str> = s.split(";").collect();
        if parts.len() != 5 {
            return Err(InvalidFormat {
                cause: format!("opinion should have 5 parts, this has {}", parts.len()),
            });
        }
        let d: u32 = parts[0].parse()?;
        let result = Self {
            date: Date::from(d),
            valid: parts[1].parse()?,
            serial: parts[2].parse()?,
            certainty: parts[3].parse()?,
            comment: percent_decode(parts[4]),
        };
        Ok(result)
    }
}

impl Opinion {
    #[allow(dead_code)]
    pub fn verify_signature(&self, statement_bytes: &Vec<u8>) -> bool {
        let signable_bytes = self.data.signable_bytes(statement_bytes);
        self.signer.key.verify(&signable_bytes, &self.signature)
    }
}

impl Display for Opinion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{};{};{}",
            self.data,
            self.signer,
            base64::encode(&self.signature)
        )
    }
}

impl FromStr for Opinion {
    type Err = InvalidFormat;
    fn from_str(s: &str) -> Result<Self, InvalidFormat> {
        let parts: Vec<&str> = s.split(";").collect();
        if parts.len() != 7 {
            return Err(InvalidFormat {
                cause: format!(
                    "signed opinion should have 7 parts, this has {}",
                    parts.len()
                ),
            });
        }
        let d: u32 = parts[0].parse()?;
        let opinion = UnsignedOpinion {
            date: Date::from(d),
            valid: parts[1].parse()?,
            serial: parts[2].parse()?,
            certainty: parts[3].parse()?,
            comment: percent_decode(parts[4]),
        };
        let result = Self {
            data: opinion,
            signer: parts[5].parse().unwrap(),
            signature: base64::decode(parts[6]).unwrap(),
        };
        Ok(result)
    }
}

impl Serialize for Opinion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Opinion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let s: &str = Deserialize::deserialize(deserializer)?;
        match Opinion::from_str(s) {
            Ok(e) => Ok(e),
            Err(_) => Err(D::Error::custom("a SignedOpinion")),
        }
    }
}

impl Deref for Opinion {
    type Target = UnsignedOpinion;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl SignedStatement {
    #[allow(dead_code)]
    pub fn verify_signatures(&self) -> bool {
        let statement_bytes = self.statement.signable_bytes();
        self.opinions.len() > 0
            && self
                .opinions
                .iter()
                .all(|x| x.verify_signature(&statement_bytes))
    }
}

impl Display for SignedStatement {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.statement.fmt(f)?;
        for op in &self.opinions {
            write!(f, "\n{}", op)?
        }
        Ok(())
    }
}

impl FromStr for SignedStatement {
    type Err = InvalidFormat;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lines: Vec<&str> = s.split("\n").collect();
        let statement = match lines[0].parse::<Statement>() {
            Ok(statement) => statement,
            _ => {
                return Err(InvalidFormat {
                    cause: "invalid statement".into(),
                })
            }
        };
        let opinions = lines[1..]
            .iter()
            .map(|s| Ok(s.parse::<Opinion>()?))
            .collect::<Result<Vec<Opinion>, InvalidFormat>>()?;

        Ok(Self {
            statement: statement,
            opinions: opinions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> UnsignedOpinion {
        UnsignedOpinion {
            date: Date::from(18924), // 2021-10-24
            valid: 7,
            serial: 0,
            certainty: 3,
            comment: "".to_string(),
        }
    }

    #[test]
    fn print() {
        let opinion = example();
        assert_eq!(opinion.to_string(), "18924;7;0;3;")
    }

    #[test]
    fn parse() {
        let mut opinion = example();
        opinion.comment = "hey;there".to_string();
        let parsed = UnsignedOpinion::from_str("18924;7;0;3;hey%3Bthere").unwrap();
        assert_eq!(opinion, parsed)
    }

    #[test]
    fn sign() {
        let opinion = example();
        let statement = super::super::statement::tests::example();
        let keypair = super::super::tests::example_keypair();
        let signed_opinion = opinion.sign_using(&statement.signable_bytes(), &keypair);
        let signer = super::super::tests::example_signer();
        let signature = "MEQCIGPqfQzTjTFWTHNPT+KIMqGDvN1VV5HF0S6JWgb8n+WnAiBFGcls4ZILhxP0GWvcLdkhbUwSkZ+TaO/lf+4Hs/bf2w==";
        assert_eq!(
            signed_opinion.to_string(),
            format!("18924;7;0;3;;{};{}", signer, signature)
        )
    }

    #[test]
    fn sign_statement() {
        let opinion = example();
        let statement = super::super::statement::tests::example();
        let keypair = super::super::tests::example_keypair();
        let signed_opinion = opinion.sign_using(&statement.signable_bytes(), &keypair);
        let signed_statement_string = format!("{}\n{}", statement, signed_opinion);
        let signed_statement = SignedStatement {
            statement: statement,
            opinions: vec![signed_opinion],
        };
        assert!(signed_statement.verify_signatures());
        assert_eq!(signed_statement.to_string(), signed_statement_string)
    }
}
