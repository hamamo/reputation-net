use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

use libp2p::identity::Keypair;

mod entity;
mod opinion;
pub mod parser;
mod publickey;
mod statement;
mod template;
mod trust;
pub use entity::{Entity, EntityType};
pub use opinion::Opinion;
pub use publickey::{PublicKey, Signature};
pub use statement::Statement;
pub use template::Template;
pub use trust::Trust;

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

mod tests {

    use super::*;
    use entity::Entity;
    use opinion::Opinion;

    // a DER key created by me. This should be considered public knowledge now, never trust that key
    const KEY_BASE64: &str =
        "MHQCAQEEIJjTd4ks9PIRt4pFOGdhUYnKIkDrep7mkI7Se8QII8xToAcGBSuBBAAKoUQDQgAEwIfR\
         9vu28FoqiEzu9iADY6gqnQfP8q9WzAcLQ0kwfVz5dnEOHKssuQV+DFHlHM33CHr8uPAShT7uazCf\
         H6poUw==";

    fn test_signer() -> Entity {
        let mut der_bytes = base64::decode(KEY_BASE64).unwrap();
        let keypair = Keypair::secp256k1_from_der(&mut der_bytes).unwrap();
        let pk = keypair.public();
        Entity::Signer(PublicKey { key: pk })
    }

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
            date: 12345 as u32, /* constant value to make unit test possible */
            valid: 365,
            serial: 0,
            certainty: 3,
            comment: "as per whois info".to_string(),
            signature: None,
        };
        // print unsigned opinion
        assert_eq!(
            opinion.to_string(),
            "abuse_contact(example.com,abuse@example.com);12345;365;0;3;as%20per%20whois%20info"
        );
        let opinion = opinion.sign_using(keypair);
        // print signed opinion
        let opinion_string = opinion.to_string();
        assert_eq!(opinion.to_string(), "abuse_contact(example.com,abuse@example.com);12345;365;0;3;as%20per%20whois%20info;secp256k1:A8CH0fb7tvBaKohM7vYgA2OoKp0Hz/KvVswHC0NJMH1c;MEUCIQDR1vz5mbXsqh7q/0ktW+WIpmekDdZ9m2nfa5UvSt0GNgIgL5sHzi2vkIu7OAMzX2AzRUAfe+cy8glF87TYbE8cMB8=");
        // decode from string
        let decoded: Opinion = opinion_string.parse().unwrap();
        println!("{:?}", decoded);
        let sig_ok = decoded.is_signature_ok();
        println!("Signature is {}", if sig_ok { "ok" } else { "not ok" });
    }

    #[test]
    fn signer_display() {
        let signer = test_signer();
        assert_eq!(
            signer.to_string(),
            "secp256k1:A8CH0fb7tvBaKohM7vYgA2OoKp0Hz/KvVswHC0NJMH1c"
        );
    }
}
