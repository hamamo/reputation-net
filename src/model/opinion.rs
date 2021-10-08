use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use super::{percent_decode, percent_encode, Keypair, PublicKey, Signature, Statement};

#[derive(Clone, Debug)]
pub struct Opinion {
    pub date: u32,       // day since the UNIX epoch
    pub valid: u16,      // number of days this opinion is considered valid
    pub serial: u8, // to detect last opinion about a statement if more than one are made on a day
    pub certainty: i8, // positive or negative certainty in range -3..3.
    pub comment: String, // optional comment, may be empty
}

pub struct SignedOpinion {
    pub opinion: Opinion,
    pub signer: PublicKey,
    pub signature: Signature,
}

#[allow(dead_code)]
pub struct SignedStatement {
    pub statement: Statement,
    pub opinions: Vec<SignedOpinion>,
}

impl Opinion {
    #[allow(dead_code)]
    pub fn sign_using(&self, statement_bytes: &Vec<u8>, keypair: Keypair) -> SignedOpinion {
        // return a signed version. If original is already signed, strip signature first

        SignedOpinion {
            opinion: self.clone(),
            signer: PublicKey {
                key: keypair.public(),
            },
            signature: keypair.sign(&self.signable_bytes(statement_bytes)).unwrap(),
        }
    }

    fn signable_bytes(&self, statement_bytes: &Vec<u8>) -> Vec<u8> {
        let mut bytes = self.to_string().as_bytes().to_vec();
        bytes.extend(statement_bytes);
        bytes
    }
}

impl Display for Opinion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{};{};{};{};{}",
            self.date,
            self.valid,
            self.serial,
            self.certainty,
            percent_encode(&self.comment),
        )?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct InvalidOpinion;
impl fmt::Display for InvalidOpinion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid opinion format")
    }
}

impl FromStr for Opinion {
    type Err = InvalidOpinion;
    fn from_str(s: &str) -> Result<Self, InvalidOpinion> {
        let parts: Vec<&str> = s.split(";").collect();
        let result = Self {
            date: parts[0].parse().unwrap(),
            valid: parts[1].parse().unwrap(),
            serial: parts[2].parse().unwrap(),
            certainty: parts[3].parse().unwrap(),
            comment: percent_decode(parts[4]),
        };
        Ok(result)
    }
}

impl SignedOpinion {
    #[allow(dead_code)]
    pub fn verify_signature(&self, statement_bytes: &Vec<u8>) -> bool {
        let signable_bytes = self.opinion.signable_bytes(statement_bytes);
        self.signer.key.verify(&signable_bytes, &self.signature)
    }
}

impl Display for SignedOpinion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{};{};{}", self.opinion, self.signer, base64::encode(&self.signature))
    }
}
