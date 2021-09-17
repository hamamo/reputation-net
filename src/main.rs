use async_std::io;
use async_std::task::spawn;
use futures::prelude::*;
use futures::{channel::mpsc, select, AsyncBufReadExt, StreamExt};
use std::error::Error;

use libp2p::{identity, multiaddr::Protocol, Multiaddr, PeerId, Swarm};

mod model;
mod reputation_net;
mod storage;

use reputation_net::ReputationNet;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    let mut swarm = {
        let transport = libp2p::development_transport(local_key).await?;

        let behaviour = reputation_net::ReputationNet::new(local_peer_id).await;

        Swarm::new(transport, behaviour, local_peer_id)
    };

    // Tell the swarm to listen on all interfaces and the first available port
    // in range 10000..10100
    for port in 10000..10100 {
        let mut addr: Multiaddr = "/ip4/0.0.0.0".parse()?;
        addr.push(Protocol::Tcp(port));
        match swarm.listen_on(addr) {
            Ok(_) => break,
            _ => continue,
        }
    }

    // Dial the peer identified by the multi-address given as the second
    // command-line argument, if any.
    if let Some(addr) = std::env::args().nth(1) {
        let remote = addr.parse()?;
        swarm.dial_addr(remote)?;
        println!("Dialing {}", addr)
    }

    let (mut sender, mut receiver) = mpsc::channel::<String>(0);

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    spawn(async move {
        loop {
            let line = stdin.next().await.expect("could read line");
            match line {
                Ok(s) => {
                    sender.send(s).await.expect("could send");
                }
                _ => {
                    panic!("huh?");
                }
            }
        }
    });

    loop {
        select! {
            event = swarm.next() => {
                println!("swarm event: {:?}", event);
            },
            event = receiver.next() => {
                match event {
                    Some(s) => {
                        println!("stdin event: {:?}", s);
                        swarm.behaviour_mut().handle_input(&s).await;
                    }
                    None => panic!("end of input?")
                }
                
            }
        }
    }
}
