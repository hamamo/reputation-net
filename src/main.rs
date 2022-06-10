use std::{error::Error, time::Duration};

use clap::Parser;
use console_subscriber;
use futures::{
    channel::mpsc::{channel, Receiver},
    StreamExt,
};
use log::{debug, info};

use libp2p::{
    core::upgrade,
    mplex,
    multiaddr::Protocol,
    noise::{Keypair, NoiseConfig, X25519Spec},
    swarm::{SwarmBuilder, SwarmEvent},
    tcp::TokioTcpConfig,
    Multiaddr, Swarm, Transport,
};

mod api;
mod milter;
mod model;
mod reputation_net;
mod storage;

use reputation_net::{input_reader, Message, ReputationNet};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    #[clap(short, long)]
    dial: Option<String>,
    #[clap(short, long)]
    milter: Option<u16>,
    #[clap(short, long)]
    api: Option<u16>,
    #[clap(short, long)]
    interactive: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(debug_assertions)]
    console_subscriber::init();
    
    env_logger::init();
    let args = Args::parse();

    let (input_sender, input_receiver) = channel::<String>(5);
    let (message_sender, message_receiver) = channel::<Message>(100);

    let mut swarm = {
        let behaviour = ReputationNet::new(message_sender).await;
        let auth_keys = Keypair::<X25519Spec>::new()
            .into_authentic(&behaviour.local_key)
            .expect("can create auth keys");
        let transport = TokioTcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())
            .multiplex(mplex::MplexConfig::new())
            .boxed();
        let local_peer_id = behaviour.local_peer_id();

        println!("Local peer id: {:?}", local_peer_id);

        SwarmBuilder::new(transport, behaviour, local_peer_id)
            .executor(Box::new(|fut| {
                tokio::task::Builder::new()
                    .name("libp2p swarm")
                    .spawn(fut);
            }))
            .build()
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

    if let Some(addr) = args.dial {
        let remote: Multiaddr = addr.parse()?;
        println!("Dialing {}", remote);
        swarm.dial(remote)?;
    }

    if let Some(port) = args.milter {
        println!("Running milter on port {}", port);
        let storage = swarm.behaviour().storage.clone();
        tokio::task::Builder::new()
            .name("milter")
            .spawn(milter::run_milter(("0.0.0.0", port), storage));
    }

    if let Some(port) = args.api {
        println!("Running REST api on port {}", port);
        let storage = swarm.behaviour().storage.clone();
        tokio::task::Builder::new()
            .name("web api")
            .spawn(api::api(port, storage));
    }

    if args.interactive {
        tokio::task::Builder::new()
            .name("command input")
            .spawn(input_reader(input_sender));
    }

    network_loop(swarm, input_receiver, message_receiver).await?;

    Ok(())
}

async fn network_loop(
    mut swarm: Swarm<ReputationNet>,
    mut input_receiver: Receiver<String>,
    mut message_receiver: Receiver<Message>,
) -> Result<(), std::io::Error> {
    loop {
        tokio::select! {
            event = swarm.next() => {
                info!("swarm event: {:?}", event);
                match event {
                    Some(SwarmEvent::Behaviour(s)) => {
                        swarm.behaviour_mut().handle_behaviour_event(s);
                    }
                    Some(SwarmEvent::ConnectionEstablished{peer_id, endpoint, num_established, concurrent_dial_errors}) => {
                        println!("connection established: {}, {:?}, {}, {:?}", peer_id, endpoint, num_established, concurrent_dial_errors);
                        let num_total = swarm.network_info().num_peers();
                        swarm.behaviour_mut().handle_connection_established(peer_id, u32::from(num_established) as usize, num_total);
                    }
                    Some(SwarmEvent::ConnectionClosed{peer_id, endpoint, num_established, cause}) => {
                        println!("connection closed: {}, {:?}, {}, {:?}", peer_id, endpoint, num_established, cause);
                        let num_total = swarm.network_info().num_peers();
                        swarm.behaviour_mut().handle_connection_closed(peer_id, num_established as usize, num_total);
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
            else => {
                println!("nothing to do in main loop");
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
}
