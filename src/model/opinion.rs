use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use super::{Keypair, PublicKey, Signature, Statement, percent_encode, percent_decode};

#[derive(Clone,Debug)]
pub struct Opinion {
    pub statement: Statement,         // the statement being asserted
    pub date: u32,                    // day since the UNIX epoch
    pub valid: u16,                   // number of days this opinion is considered valid
    pub serial: u16, // to detect last opinion about a statement if more than one are made on a day
    pub certainty: i8, // positive or negative certainty in range -3..3.
    pub comment: String, // optional comment, may be empty
    pub signer: Option<PublicKey>, // public key of signer
    pub signature: Option<Signature>, // signature
}

impl Opinion {
    #[allow(dead_code)]
    pub fn sign_using(&self, keypair: Keypair) -> Self {
        // return a signed version. If original is already signed, strip signature first

        let mut result = self.clone();
        result.signer = Some(PublicKey {
            key: keypair.public(),
        });
        result.signature = Some(keypair.sign(&self.signable_bytes()).unwrap());
        result
    }

    fn strip_signature(&self) -> Self {
        let mut result = self.clone();
        result.signer = None;
        result.signature = None;
        result
    }

    #[allow(dead_code)]
    pub fn is_signature_ok(&self) -> bool {
        if let (Some(signer), Some(signature)) = (self.signer.as_ref(), self.signature.as_ref()) {
            signer.key.verify(&self.signable_bytes(), &signature)
        } else {
            false
        }
    }

    fn signable_bytes(&self) -> Vec<u8> {
        if self.signer.is_some() || self.signature.is_some() {
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
        if let (Some(pubkey), Some(sig)) = (&self.signer, &self.signature) {
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
            signer: None,
            signature: None,
        };
        if parts.len() >= 8 {
            result.signer = Some(parts[6].parse().unwrap());
            result.signature = Some(base64::decode(parts[7]).unwrap())
        }
        Ok(result)
    }
}
