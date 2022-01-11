use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

use libp2p::identity::Keypair;

mod entity;
mod opinion;
pub mod parser;
mod publickey;
mod statement;
mod template;
mod own_key;
mod date;
pub use entity::{Entity, EntityType};
pub use opinion::{Opinion,SignedOpinion,SignedStatement};
pub use publickey::{PublicKey, Signature};
pub use statement::Statement;
pub use template::Template;
pub use own_key::OwnKey;
pub use date::Date;

fn percent_encode(s: &str) -> String {
    const ESCAPE: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b',')
        .add(b';')
        .add(b'(')
        .add(b')');
    utf8_percent_encode(s, ESCAPE).to_string()
}

fn percent_decode(s: &str) -> String {
    percent_decode_str(s).decode_utf8().unwrap().to_string()
}

#[cfg(test)]

pub mod tests {

    use super::*;

    // a DER key created by me. This should be considered public knowledge now, never trust that key
    pub fn example_keypair() -> Keypair {
        let mut der_bytes = base64::decode(
            "MHQCAQEEIJjTd4ks9PIRt4pFOGdhUYnKIkDrep7mkI7Se8QII8xToAcGBSuBBAAKoUQDQgAEwIfR\
            9vu28FoqiEzu9iADY6gqnQfP8q9WzAcLQ0kwfVz5dnEOHKssuQV+DFHlHM33CHr8uPAShT7uazCf\
            H6poUw==",
        )
        .unwrap();
        Keypair::secp256k1_from_der(&mut der_bytes).unwrap()
    }

    // signer (public key with algorithm prefix) for the DER key above
    pub fn example_signer() -> String {
        "secp256k1:A8CH0fb7tvBaKohM7vYgA2OoKp0Hz/KvVswHC0NJMH1c".to_string()
    }
}
