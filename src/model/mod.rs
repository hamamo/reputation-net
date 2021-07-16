use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

use libp2p::identity::Keypair;

mod entity;
mod opinion;
mod publickey;
mod template;
mod statement;
pub mod parser;
pub use entity::{Entity, EntityType};
pub use template::Template;
pub use opinion::Opinion;
pub use publickey::{PublicKey, Signature};
pub use statement::Statement;

fn percent_encode(s: &str) -> String {
    const ESCAPE: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b',').add(b';').add(b'(').add(b')');
    utf8_percent_encode(s,ESCAPE).to_string()
}

fn percent_decode(s: &str) -> String {
    percent_decode_str(s).decode_utf8().unwrap().to_string()
}

#[cfg(test)]

mod tests {
    use chrono::Utc;

    use super::*;
    use entity::Entity;
    use opinion::Opinion;

    // a DER key created by me. This should be considered public knowledge now, never trust that key
    const KEY_BASE64: &str =
        "MHQCAQEEIJjTd4ks9PIRt4pFOGdhUYnKIkDrep7mkI7Se8QII8xToAcGBSuBBAAKoUQDQgAEwIfR\
         9vu28FoqiEzu9iADY6gqnQfP8q9WzAcLQ0kwfVz5dnEOHKssuQV+DFHlHM33CHr8uPAShT7uazCf\
         H6poUw==";

    #[test]
    fn signing() {
        let mut der_bytes = base64::decode(KEY_BASE64).unwrap();
        let keypair = Keypair::secp256k1_from_der(&mut der_bytes).unwrap();
        let opinion = Opinion {
            statement: Statement::new(
                "abuse_contact",
                vec![
                    Entity::Domain("example.com".into()),
                    Entity::EMail("abuse@example.com".into()),
                ],
            ),
            date: (Utc::now().timestamp() / 86400) as u32,
            valid: 365,
            serial: 0,
            certainty: 3,
            comment: "as per whois info".to_string(),
            signer: None,
            signature: None,
        };
        // print unsigned opinion
        println!("{}", opinion.to_string());
        let opinion = opinion.sign_using(keypair);
        // print signed opinion
        let opinion_string = opinion.to_string();
        println!("{}", opinion_string);
        // decode from string
        let decoded: Opinion = opinion_string.parse().unwrap();
        println!("{:?}", decoded);
        let sig_ok = decoded.is_signature_ok();
        println!("Signature is {}", if sig_ok { "ok" } else { "not ok" });
    }
}
