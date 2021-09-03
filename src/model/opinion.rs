use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use super::{percent_decode, percent_encode, Keypair, PublicKey, Signature, Statement};

#[derive(Clone, Debug)]
pub struct Opinion {
    pub statement: Statement, // the statement being asserted
    pub date: u32,            // day since the UNIX epoch
    pub valid: u16,           // number of days this opinion is considered valid
    pub serial: u8, // to detect last opinion about a statement if more than one are made on a day
    pub certainty: i8, // positive or negative certainty in range -3..3.
    pub comment: String, // optional comment, may be empty
    pub signature: Option<(PublicKey, Signature)>, // public key of signer and signature
}

impl Opinion {
    #[allow(dead_code)]
    pub fn sign_using(&self, keypair: Keypair) -> Self {
        // return a signed version. If original is already signed, strip signature first

        let mut result = self.clone();
        let signature = (
            PublicKey {
                key: keypair.public(),
            },
            keypair.sign(&self.signable_bytes()).unwrap(),
        );
        result.signature = Some(signature);
        result
    }

    fn strip_signature(&self) -> Self {
        let mut result = self.clone();
        result.signature = None;
        result
    }

    #[allow(dead_code)]
    pub fn is_signature_ok(&self) -> bool {
        if let Some((signer, signature)) = &self.signature {
            signer.key.verify(&self.signable_bytes(), signature)
        } else {
            false
        }
    }

    fn signable_bytes(&self) -> Vec<u8> {
        if self.signature.is_some() {
            self.strip_signature().signable_bytes()
        } else {
            self.to_string().as_bytes().to_vec()
        }
    }
}

impl Display for Opinion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{};{};{};{};{};{}",
            self.statement.to_string(),
            self.date,
            self.valid,
            self.serial,
            self.certainty,
            percent_encode(&self.comment),
        )?;
        if let Some((pubkey, sig)) = &self.signature {
            write!(f, ";{};{}", pubkey.to_string(), base64::encode(sig))?;
        }
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
        let mut result = Self {
            statement: parts[0].parse().unwrap(),
            date: parts[1].parse().unwrap(),
            valid: parts[2].parse().unwrap(),
            serial: parts[3].parse().unwrap(),
            certainty: parts[4].parse().unwrap(),
            comment: percent_decode(parts[5]),
            signature: None,
        };
        if parts.len() >= 8 {
            result.signature = Some((parts[6].parse().unwrap(), base64::decode(parts[7]).unwrap()))
        }
        Ok(result)
    }
}
