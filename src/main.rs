use futures::{StreamExt, AsyncBufReadExt};
use std::iter::Iterator;

use async_std::{io, task::spawn};
use futures::{
    channel::mpsc::{channel, Receiver, Sender},
    select, SinkExt,
};
use log::{debug, info};
use std::error::Error;

use libp2p::{multiaddr::Protocol, swarm::SwarmEvent, Multiaddr, PeerId, Swarm};

mod milter;
mod model;
mod reputation_net;
mod storage;

use reputation_net::{NetworkMessage, ReputationNet};

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let (input_sender, input_receiver) = channel::<String>(5);
    let (event_sender, event_receiver) = channel::<(NetworkMessage, Option<PeerId>)>(100);

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
    /*
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        info!("Dialing {}", addr)
    }
    */

    let storage = swarm.behaviour().storage.clone();
    spawn(network_loop(swarm, input_receiver, event_receiver));

    if let Some(command) = std::env::args().nth(1) {
        if command == "milter" {
            spawn(milter::run_milter(("0.0.0.0", 21000), storage));
        }
    }
    
    input_reader(input_sender).await?;
    Ok(())
}

async fn input_reader(mut sender: Sender<String>) -> Result<(), std::io::Error> {
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    loop {
        match stdin.next().await {
            Some(result) => {
                let line = result?;
                sender.send(line).await.expect("could send");
            }
            None => {
                println!("EOF on stdin");
                return Ok(());
            }
        }
    }
}

async fn network_loop(
    mut swarm: Swarm<ReputationNet>,
    mut input_receiver: Receiver<String>,
    mut event_receiver: Receiver<(NetworkMessage, Option<PeerId>)>,
) -> Result<(), std::io::Error> {
    loop {
        select! {
            event = swarm.next() => {
                info!("swarm event: {:?}", event);
                match event {
                    Some(SwarmEvent::Behaviour(s)) => {
                        swarm.behaviour_mut().handle_behaviour_event(s).await;
                    }
                    Some(SwarmEvent::ConnectionEstablished{peer_id, endpoint: _, num_established, concurrent_dial_errors: _}) => {
                        info!("{:?}", event);
                        swarm.behaviour_mut().handle_connection_established(peer_id, u32::from(num_established)).await;
                    }
                    Some(SwarmEvent::ConnectionClosed{peer_id, endpoint: _, num_established, cause: _}) => {
                        info!("{:?}", event);
                        swarm.behaviour_mut().handle_connection_closed(peer_id, num_established).await;
                    }
                    _ => ()
                }
            },
            event = input_receiver.next() => {
                info!("input: {:?}", event);
                match event {
                    Some(s) => {
                        debug!("stdin event: {:?}", s);
                        swarm.behaviour_mut().handle_input(&s).await;
                    }
                    None => break Ok(())
                }
            }
            event = event_receiver.next() => {
                info!("internal event: {:?}", event);
                match event {
                    Some(e) => {
                        let (message, peer) = e;
                        debug!("network event: {:?}", message);
                        swarm.behaviour_mut().handle_event(message, peer).await;
                    }
                    None => panic!("end of network?")
                }
            }
        }
    }
}
