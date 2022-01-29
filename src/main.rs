#![feature(never_type)]

use std::error::Error;

use async_std::{io, task::spawn};
use clap::{Parser, Subcommand};
use futures::{
    channel::mpsc::{channel, Receiver, Sender},
    select, AsyncBufReadExt, SinkExt, StreamExt,
};
use log::{debug, info};

use libp2p::{multiaddr::Protocol, swarm::SwarmEvent, Multiaddr, Swarm};

mod milter;
mod model;
mod reputation_net;
mod storage;

use reputation_net::{Message, ReputationNet};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    #[clap(short, long)]
    peer: Option<String>,
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Milter { port: Option<u16> },
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let args = Args::parse();

    let (input_sender, input_receiver) = channel::<String>(5);
    let (message_sender, message_receiver) = channel::<Message>(100);

    let mut swarm = {
        let behaviour = ReputationNet::new(message_sender).await;
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

    // Dial the peer identified by the multi-address given on the command line.

    if let Some(addr) = args.peer {
        let remote: Multiaddr = addr.parse()?;
        println!("Dialing {}", remote);
        swarm.dial(remote)?;
    }

    let storage = swarm.behaviour().storage.clone();
    spawn(network_loop(swarm, input_receiver, message_receiver));

    if let Some(cmd) = args.command {
        match cmd {
            Commands::Milter { port } => {
                let port = port.or(Some(21000)).unwrap();
                println!("Running milter on port {}", port);
                spawn(milter::run_milter(("0.0.0.0", port), storage));
            }
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
    mut message_receiver: Receiver<Message>,
) -> Result<(), std::io::Error> {
    loop {
        select! {
            event = swarm.next() => {
                info!("swarm event: {:?}", event);
                match event {
                    Some(SwarmEvent::Behaviour(s)) => {
                        swarm.behaviour_mut().handle_behaviour_event(s);
                    }
                    Some(SwarmEvent::ConnectionEstablished{peer_id, endpoint: _, num_established, concurrent_dial_errors: _}) => {
                        swarm.behaviour_mut().handle_connection_established(peer_id, u32::from(num_established));
                    }
                    Some(SwarmEvent::ConnectionClosed{peer_id, endpoint: _, num_established, cause: _}) => {
                        swarm.behaviour_mut().handle_connection_closed(peer_id, num_established);
                    }
                    _ => ()
                }
            },
            event = input_receiver.next() => {
                match event {
                    Some(s) => {
                        swarm.behaviour_mut().handle_user_input(&s).await;
                    }
                    None => break Ok(())
                }
            }
            event = message_receiver.next() => {
                info!("network message: {:?}", event);
                match event {
                    Some(message) => {
                        debug!("network event: {:?}", message);
                        swarm.behaviour_mut().handle_message(message).await;
                    }
                    None => panic!("end of network?")
                }
            }
        }
    }
}
