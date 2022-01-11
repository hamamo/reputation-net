use std::{fmt::Display, io::Write};

use nom::{
    branch::alt,
    bytes::complete::{tag, take_till},
    combinator::map,
    multi::{many0, many1},
    number::streaming::{be_u16, be_u32, be_u8},
    sequence::{pair, tuple},
    IResult,
};

use bitflags::bitflags;

// a simple milter

#[derive(Debug, PartialEq)]
pub enum Command {
    Optneg(SmficOptneg),   // O
    Macro(SmficMacro),     // D
    Connect(SmficConnect), // C
    Helo(SmficHelo),       // H
    Mail(SmficMail),       // M
    Rcpt(SmficRcpt),       // R
    Header(SmficHeader),   // L
    Eoh,                   // N
    Body(SmficBody),       // B
    BodyEob,               // E
    Quit,                  // Q
    Abort,                 // A
    // after version 2
    Disconnect, // K
    Data,       // T
    Unknown,    // U
}

#[derive(Debug, PartialEq)]
#[allow(dead_code)]
pub enum Response {
    Optneg(SmficOptneg),
    Accept,
    Reject,
    Tempfail,
    Discard,
    Quarantine(SmficQuarantine),
    Replycode(SmficReplycode),
    Continue,
}

// the preferred milter version. If the MTA only offers a lower version, we try to accomodate that
pub const MILTER_VERSION: u32 = 6;

#[derive(PartialEq)]
pub struct CString {
    pub bytes: Vec<u8>,
}

// Packet contents are described using byte-level abstractions, the interpretation of strings is up to the client

bitflags! {
    pub struct Actions: u32 {
        const SMFIF_ADDHDRS = 0x0001;
        const SMFIF_CHGBODY = 0x0002;
        const SMFIF_ADDRCPT = 0x0004;
        const SMFIF_DELRCPT = 0x0008;
        const SMFIF_CHGHDRS = 0x0010;
        const SMFIF_QUARANTINE = 0x0020;
        const SMFIF_CHGFROM = 0x0040;
        const SMFIF_ADDRCPT_PAR = 0x0080;
        const SMFIF_SETSYMLIST = 0x0100;
    }
}

bitflags! {
    pub struct Protocol: u32 {
        const SMFIP_NOCONNECT = 0x000001;
        const SMFIP_NOHELO = 0x000002;
        const SMFIP_NOMAIL = 0x000004;
        const SMFIP_NORCPT = 0x000008;
        const SMFIP_NOBODY = 0x000010;
        const SMFIP_NOHDRS = 0x000020;
        const SMFIP_NOEOH = 0x000040;
        const SMFIP_NR_HDR = 0x000080;
        const SMFIP_NOUNKNOWN = 0x000100;
        const SMFIP_NODATA = 0x000200;
        const SMFIP_SKIP = 0x000400;
        const SMFIP_RCPT_REJ = 0x000800;
        const SMFIP_NR_CONN = 0x001000;
        const SMFIP_NR_HELO = 0x002000;
        const SMFIP_NR_MAIL = 0x004000;
        const SMFIP_NR_RCPT = 0x008000;
        const SMFIP_NR_DATA = 0x010000;
        const SMFIP_NR_UNKN = 0x020000;
        const SMFIP_NR_EOH = 0x040000;
        const SMFIP_NR_BODY = 0x080000;
        const SMFIP_HDR_LEADSPC = 0x100000;
    }
}

#[derive(Debug, PartialEq)]
pub struct SmficOptneg {
    pub version: u32,
    pub actions: Actions,
    pub protocol: Protocol,
}

#[derive(Debug, PartialEq)]
pub struct SmficMacro {
    pub cmdcode: u8,
    pub nameval: Vec<(CString, CString)>,
}

#[derive(Debug, PartialEq)]
pub struct SmficConnect {
    pub hostname: CString,
    pub family: u8,
    pub port: u16,
    pub address: CString,
}

#[derive(Debug, PartialEq)]
pub struct SmficHelo {
    pub helo: CString,
}

#[derive(Debug, PartialEq)]
pub struct SmficMail {
    pub args: Vec<CString>,
}

#[derive(Debug, PartialEq)]
pub struct SmficRcpt {
    pub args: Vec<CString>,
}

#[derive(Debug, PartialEq)]
pub struct SmficHeader {
    pub name: CString,
    pub value: CString,
}

#[derive(Debug, PartialEq)]
pub struct SmficBody {
    pub buf: CString,
}

#[derive(Debug, PartialEq)]
pub struct SmficQuarantine {
    pub reason: CString,
}

#[derive(Debug, PartialEq)]
pub struct SmficReplycode {
    pub smtpcode: u16,
    pub reason: CString,
}

fn string(input: &[u8]) -> IResult<&[u8], CString> {
    let (i, bytes) = take_till(|c| c == 0)(input)?;
    let (i, _) = tag([0u8])(i)?;
    Ok((i, CString::from(bytes)))
}

fn body_string(input: &[u8]) -> IResult<&[u8], CString> {
    let (i, bytes) = take_till(|c| c == 0)(input)?;
    Ok((i, CString::from(bytes)))
}

pub fn smfic_optneg(input: &[u8]) -> IResult<&[u8], Command> {
    map(
        tuple((tag(b"O"), be_u32, be_u32, be_u32)),
        |(_, version, actions, protocol)| {
            Command::Optneg(SmficOptneg {
                version,
                actions: Actions::from_bits_truncate(actions),
                protocol: Protocol::from_bits_truncate(protocol),
            })
        },
    )(input)
}

pub fn smfic_macro(input: &[u8]) -> IResult<&[u8], Command> {
    map(
        tuple((tag(b"D"), be_u8, many0(pair(string, string)))),
        |(_, cmdcode, nameval)| Command::Macro(SmficMacro { cmdcode, nameval }),
    )(input)
}

pub fn smfic_connect(input: &[u8]) -> IResult<&[u8], Command> {
    map(
        tuple((tag(b"C"), string, be_u8, be_u16, string)),
        |(_, hostname, family, port, address)| {
            Command::Connect(SmficConnect {
                hostname,
                family: family,
                port,
                address,
            })
        },
    )(input)
}

pub fn smfic_helo(input: &[u8]) -> IResult<&[u8], Command> {
    map(tuple((tag(b"H"), string)), |(_, helo)| {
        Command::Helo(SmficHelo { helo })
    })(input)
}

pub fn smfic_mail(input: &[u8]) -> IResult<&[u8], Command> {
    map(tuple((tag(b"M"), many1(string))), |(_, args)| {
        Command::Mail(SmficMail { args })
    })(input)
}

pub fn smfic_rcpt(input: &[u8]) -> IResult<&[u8], Command> {
    map(tuple((tag(b"R"), many1(string))), |(_, args)| {
        Command::Rcpt(SmficRcpt { args })
    })(input)
}

pub fn smfic_header(input: &[u8]) -> IResult<&[u8], Command> {
    map(
        tuple((tag(b"L"), pair(string, string))),
        |(_, (name, value))| Command::Header(SmficHeader { name, value }),
    )(input)
}

pub fn smfic_eoh(input: &[u8]) -> IResult<&[u8], Command> {
    map(tag(b"N"), |_| Command::Eoh)(input)
}

pub fn smfic_body(input: &[u8]) -> IResult<&[u8], Command> {
    map(pair(tag(b"B"), body_string), |(_, buf)| {
        Command::Body(SmficBody { buf })
    })(input)
}

pub fn smfic_bodyeob(input: &[u8]) -> IResult<&[u8], Command> {
    map(tag(b"E"), |_| Command::BodyEob)(input)
}

pub fn smfic_quit(input: &[u8]) -> IResult<&[u8], Command> {
    map(tag(b"Q"), |_| Command::Quit)(input)
}

pub fn smfic_abort(input: &[u8]) -> IResult<&[u8], Command> {
    map(tag(b"A"), |_| Command::Abort)(input)
}

pub fn smfic_disconnect(input: &[u8]) -> IResult<&[u8], Command> {
    map(tag(b"K"), |_| Command::Disconnect)(input)
}

pub fn smfic_data(input: &[u8]) -> IResult<&[u8], Command> {
    map(tag(b"T"), |_| Command::Data)(input)
}

pub fn smfic_unknown(input: &[u8]) -> IResult<&[u8], Command> {
    map(tag(b"U"), |_| Command::Unknown)(input)
}

impl Command {
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        alt((
            smfic_optneg,
            smfic_macro,
            smfic_connect,
            smfic_helo,
            smfic_mail,
            smfic_rcpt,
            smfic_header,
            smfic_eoh,
            smfic_body,
            smfic_bodyeob,
            smfic_quit,
            smfic_abort,
            smfic_disconnect,
            smfic_data,
            smfic_unknown,
        ))(i)
    }
}

impl Response {
    pub fn data(&self) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::with_capacity(16);
        match self {
            Response::Optneg(optneg) => {
                data.write(b"O").unwrap();
                data.write(&optneg.version.to_be_bytes()).unwrap();
                data.write(&optneg.actions.bits.to_be_bytes()).unwrap();
                data.write(&optneg.protocol.bits.to_be_bytes()).unwrap();
            }
            Response::Accept => {
                data.write(b"a").unwrap();
            }
            Response::Reject => {
                data.write(b"r").unwrap();
            }
            Response::Tempfail => {
                data.write(b"t").unwrap();
            }
            Response::Discard => {
                data.write(b"d").unwrap();
            }
            Response::Quarantine(quarantine) => {
                data.write(b"q").unwrap();
                data.write(format!("{}\0", quarantine.reason).as_bytes())
                    .unwrap();
            }
            Response::Continue => {
                data.write(b"c").unwrap();
            }
            Response::Replycode(replycode) => {
                data.write(b"y").unwrap();
                data.write(format!("{:03} {}\0", replycode.smtpcode, replycode.reason).as_bytes())
                    .unwrap();
            }
        }
        [(data.len() as u32).to_be_bytes().to_vec(), data].concat()
    }
}

impl core::fmt::Debug for CString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", String::from_utf8_lossy(&self.bytes))
    }
}

impl Display for CString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.bytes))
    }
}

impl From<&[u8]> for CString {
    fn from(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
        }
    }
}

impl From<String> for CString {
    fn from(s: String) -> Self {
        Self {
            bytes: s.as_bytes().to_vec(),
        }
    }
}

impl Display for SmficHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string0() {
        let data = hex::decode("41424300").unwrap();
        let residue: Vec<u8> = b"".to_vec();
        assert_eq!(
            string(&data),
            Ok((&residue[..], CString::from(&b"ABC"[..]),))
        );
    }

    #[test]
    fn test_macro() {
        let data = hex::decode("44436A004100").unwrap();
        let residue: Vec<u8> = b"".to_vec();
        assert_eq!(
            smfic_macro(&data),
            Ok((
                &residue[..],
                Command::Macro(SmficMacro {
                    cmdcode: 0x43,
                    nameval: vec![(CString::from(&b"j"[..]), CString::from(&b"A"[..]))],
                }),
            )),
        );
    }
}
