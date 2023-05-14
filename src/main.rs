use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, StreamExt, TryStreamExt};
use std::{
    collections::HashMap,
    io::Error as IoError,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

type Tx = UnboundedSender<Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;
type Room = [Option<SocketAddr>; 2];
type Rooms = Arc<Mutex<Vec<Room>>>;

async fn hc2(peer_map: PeerMap, rooms: Rooms, raw_stream: TcpStream, addr: SocketAddr) {
    println!("Incoming connection from: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(raw_stream)
        .await
        .expect("Error during the websocket handshake");
    println!("websocket connection established: {}", addr);

    let (tx, rx) = unbounded();
    peer_map.lock().unwrap().insert(addr, tx);

    let mut able_to_insert = false;

    for room in rooms.lock().unwrap().iter_mut() {
        if room[0].is_none() {
            room[0] = Some(addr);
            able_to_insert = true;
            break;
        } else if room[1].is_none() {
            room[1] = Some(addr);
            able_to_insert = true;
            break;
        }
    }

    if !able_to_insert {
        rooms.lock().unwrap().push([Some(addr), None]);
    }

    let (outgoing, incoming) = ws_stream.split();

    let broadcast_incoming = incoming.try_for_each(|msg| {
        println!(
            "received a message from {}: {}",
            addr,
            msg.to_text().unwrap()
        );

        let r = rooms.lock().unwrap();

        if msg.len() > 0 {
            let recps = r
                .iter()
                .filter_map(|room| {
                    if room[0] == Some(addr) {
                        room[1]
                    } else if room[1] == Some(addr) {
                        room[0]
                    } else {
                        None
                    }
                })
                .collect::<Vec<SocketAddr>>();

            println!("rooms: {r:?}");
            println!("recps: {recps:?}");
            for rcp in recps {
                peer_map
                    .lock()
                    .unwrap()
                    .get(&rcp)
                    .unwrap()
                    .unbounded_send(msg.clone())
                    .unwrap();
            }
        }

        future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    println!("{} disconnected", &addr);
    peer_map.lock().unwrap().remove(&addr);
    let mut r = rooms.lock().unwrap();
    for room in r.iter_mut() {
        if let Some(r0) = room[0] {
            if r0 == addr {
                room[0] = room[1];
                room[1] = None;
                break;
            }
        }
        if let Some(r1) = room[1] {
            if r1 == addr {
                room[1] = None;
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), IoError> {
    let state = PeerMap::new(Mutex::new(HashMap::new()));
    let rooms = Rooms::new(Mutex::new(Vec::with_capacity(8)));

    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(hc2(state.clone(), rooms.clone(), stream, addr));
    }
    Ok(())
}
