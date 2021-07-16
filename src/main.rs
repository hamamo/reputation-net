use std::{
    error::Error,
    task::{Context, Poll},
};

use async_std::io;
use futures::{executor::block_on, future, prelude::*};
use log;
use libp2p::{core::multiaddr::Protocol, identity, Multiaddr, PeerId, Swarm};

mod model;
mod reputation_net;
mod storage;

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

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    block_on(future::poll_fn(move |cx: &mut Context<'_>| {
        loop {
            let poll = swarm.poll_next_unpin(cx);
            match poll {
                Poll::Ready(Some(event)) => {
                    println!("{:?}", event);
                }
                Poll::Ready(None) => return poll,
                Poll::Pending => break,
            }
        }
        loop {
            let poll = stdin.poll_next_unpin(cx);
            if let Poll::Ready(evt) = &poll {
                println!("stdin: {:?}", evt);
            }
            match poll {
                Poll::Ready(Some(line)) => {
                    swarm.behaviour_mut().handle_input(&line.unwrap());
                }
                Poll::Ready(None) => panic!("Stdin closed"),
                Poll::Pending => break,
            }
        }
        Poll::Pending
    }));

    Ok(())
}
