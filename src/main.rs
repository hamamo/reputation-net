use async_std::io;
use async_std::task::spawn;
use futures::prelude::*;
use futures::{channel::mpsc, select, AsyncBufReadExt, StreamExt};
use log::{debug, error, info};
use std::error::Error;

use libp2p::{multiaddr::Protocol, Multiaddr, Swarm};

mod model;
mod reputation_net;
mod storage;

use reputation_net::{Event, ReputationNet};

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let (mut input_sender, mut input_receiver) = mpsc::channel::<String>(3);
    let (event_sender, mut event_receiver) = mpsc::channel::<Event>(100);

    let mut swarm = {
        let behaviour = ReputationNet::new(event_sender).await;
        let transport = libp2p::development_transport(behaviour.local_key.clone()).await?;
        let local_peer_id = behaviour.local_peer_id();

        println!("Local peer id: {:?}", local_peer_id);

        Swarm::new(transport, behaviour, local_peer_id)
    };

    // Tell the swarm to listen on all interfaces and the first available port
    // in range 10000..10100
    for port in 10000..10100 {
        let mut addr: Multiaddr = "/ip4/0.0.0.0".parse()?;
        addr.push(Protocol::Tcp(port));
        match swarm.listen_on(addr) {
            Ok(_) => {
                println!("Listening on port {}", port);
                break;
            }
            _ => continue,
        }
    }

    // Dial the peer identified by the multi-address given as the second
    // command-line argument, if any.
    if let Some(addr) = std::env::args().nth(1) {
        let remote = addr.parse()?;
        swarm.dial_addr(remote)?;
        info!("Dialing {}", addr)
    }

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    spawn(async move {
        loop {
            let line = stdin.next().await;
            match line {
                Some(Ok(s)) => {
                    input_sender.send(s).await.expect("could send");
                }
                Some(Err(e)) => {
                    error!("input: {:?}", e);
                    break;
                }
                _ => {
                    break;
                }
            }
        }
    });

    loop {
        select! {
            event = swarm.next() => {
                debug!("swarm event: {:?}", event);
            },
            event = input_receiver.next() => {
                match event {
                    Some(s) => {
                        debug!("stdin event: {:?}", s);
                        swarm.behaviour_mut().handle_input(&s).await;
                    }
                    None => break Ok(())
                }
            }
            event = event_receiver.next() => {
                match event {
                    Some(e) => {
                        debug!("network event: {:?}", e);
                        swarm.behaviour_mut().handle_event(&e).await;
                    }
                    None => panic!("end of network?")
                }
            }
        }
    }
}
