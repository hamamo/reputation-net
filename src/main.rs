use std::error::Error;

use async_std::io;
use futures::{AsyncBufReadExt};
use libp2p::{
    identity,
    multiaddr::Protocol,
    Multiaddr, PeerId, Swarm,
};


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

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    loop {
        match combined.next().await.unwrap() {
            Evt::Stdin(line) => {
                println!("stdin: {}", line);
                swarm.behaviour_mut().handle_input(&line);
            },
            Evt::Swarm() => println!("event: {:?}", "?"),
        }
    }

    Ok(())
}
