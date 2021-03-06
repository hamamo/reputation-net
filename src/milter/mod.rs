use std::{
    io::{Error, ErrorKind},
    sync::Arc,
};

use async_std::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::RwLock,
    task::spawn,
};
use futures::{
    io::{BufReader, BufWriter},
    AsyncReadExt, AsyncWriteExt, StreamExt,
};
use log::{debug, error, info};

use crate::storage::Storage;

mod packet;
mod policy;

use packet::*;
use policy::*;

pub struct Milter {
    input: BufReader<TcpStream>,
    output: BufWriter<TcpStream>,
    policy: PolicyAccumulator,
}

pub async fn run_milter(
    addr: impl ToSocketAddrs + std::fmt::Debug,
    storage: Arc<RwLock<Storage>>,
) -> Result<(), Error> {
    info!("starting milter listener on {:?}", addr);
    let listener = TcpListener::bind(addr).await?;
    info!("got listener: {:?}", listener);
    let mut incoming = listener.incoming();
    info!("got incoming stream: {:?}", incoming);
    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        let peer_addr = stream.peer_addr()?;
        info!("accepted connection from {:?}", peer_addr);
        spawn(Milter::run_on(stream, storage.clone()));
    }
    Ok(())
}

impl Milter {
    async fn run_on(stream: TcpStream, storage: Arc<RwLock<Storage>>) -> Result<(), Error> {
        let mut milter = Self {
            input: BufReader::new(stream.clone()),
            output: BufWriter::new(stream),
            policy: PolicyAccumulator::new(storage),
        };
        let result = milter.run().await;
        info!("milter run result: {:?}", result);
        result
    }

    async fn run(&mut self) -> Result<(), Error> {
        while let Ok(command) = self.read_command().await {
            debug!("--> {:?}", command);
            self.handle_command(&command).await?;
        }
        Ok(())
    }

    async fn read_command(&mut self) -> Result<Command, Error> {
        let mut len = [0u8; 4];
        self.input.read_exact(&mut len).await?;
        let len = u32::from_be_bytes(len);
        let mut data = vec![0u8; len as usize];
        self.input.read_exact(&mut data).await?;
        match Command::parse(&data) {
            Ok((_i, packet)) => Ok(packet),
            Err(_) => {
                error!("unable to parse {:?}", data);
                Err(Error::new(ErrorKind::InvalidData, "invalid milter format"))
            }
        }
    }

    async fn write_response(&mut self, response: &Response) -> Result<(), Error> {
        debug!("<-- {:?}", response);
        let data = response.data();
        self.output.write_all(&data).await?;
        self.output.flush().await?;
        Ok(())
    }

    async fn write_policy_response(&mut self) -> Result<(), Error> {
        let response = match self.policy.severity() {
            Severity::Known => Response::Accept,
            Severity::Reject => Response::Replycode(SmficReplycode {
                smtpcode: 554,
                reason: CString::from(self.policy.reason()),
            }),
            Severity::None => Response::Continue,
            Severity::Quarantine => Response::Quarantine(SmficQuarantine {
                reason: CString::from(self.policy.reason()),
            }),
            Severity::Tempfail => Response::Replycode(SmficReplycode {
                smtpcode: 457,
                reason: CString::from(self.policy.reason()),
            }),
        };
        self.write_response(&response).await
    }

    fn reset(&mut self) {
        self.policy.reset();
    }

    async fn handle_command(&mut self, command: &Command) -> Result<(), Error> {
        match command {
            Command::Optneg(optneg) => {
                self.reset();
                return self
                    .write_response(&Response::Optneg(SmficOptneg {
                        version: optneg.version.min(MILTER_VERSION),
                        actions: optneg.actions.intersection(Actions::SMFIF_QUARANTINE),
                        protocol: Protocol::empty(),
                    }))
                    .await;
            }
            Command::Macro(macros) => {
                self.policy.macros(macros).await;
                return Ok(());
            }
            Command::Connect(connect) => self.policy.connect(connect).await,
            Command::Helo(helo) => self.policy.helo(helo).await,
            Command::Mail(mail) => self.policy.mail_from(mail).await,
            Command::Rcpt(_rcpt) => {
                return self.write_policy_response().await;
            }
            Command::Header(header) => self.policy.header(header).await,
            Command::Eoh => {
                return self.write_policy_response().await;
            }
            Command::BodyEob => self.reset(),
            Command::Quit => {
                return self.output.close().await;
            }
            Command::Abort => {
                self.reset();
                return Ok(());
            }
            _ => (),
        }
        self.write_response(&Response::Continue).await?;
        Ok(())
    }
}
