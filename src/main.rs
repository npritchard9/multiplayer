use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, StreamExt, TryStreamExt};
use std::{
    io::Error as IoError,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

pub mod state;
use state::*;

#[derive(Debug)]
struct Room {
    r1: Option<SocketAddr>,
    r1tx: Option<Tx>,
    r2: Option<SocketAddr>,
    r2tx: Option<Tx>,
    state: State,
}

impl Room {
    pub fn new(r1: Option<SocketAddr>) -> Self {
        Room {
            r1,
            r1tx: None,
            r2: None,
            r2tx: None,
            state: State::Waiting,
        }
    }
}

type Tx = UnboundedSender<Message>;
type Rooms = Arc<Mutex<Vec<Room>>>;

async fn hc2(rooms: Rooms, raw_stream: TcpStream, addr: SocketAddr) {
    println!("Incoming connection from: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(raw_stream)
        .await
        .expect("Error during the websocket handshake");
    println!("websocket connection established: {}", addr);

    let (tx, rx) = unbounded();

    let mut able_to_insert = false;

    for room in rooms.lock().unwrap().iter_mut() {
        if room.r1.is_none() {
            room.r1 = Some(addr);
            room.r1tx = Some(tx.clone());
            able_to_insert = true;
            break;
        } else if room.r2.is_none() {
            room.r2 = Some(addr);
            room.r2tx = Some(tx.clone());
            able_to_insert = true;
            room.state = State::Ready;
            break;
        }
    }

    if !able_to_insert {
        let mut r = rooms.lock().unwrap();
        r.push(Room::new(Some(addr)));
        let last = r.last_mut().unwrap();
        last.r1tx = Some(tx.clone());
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
            let room = r
                .iter()
                .find(|room| room.r1 == Some(addr) || room.r2 == Some(addr))
                .unwrap();
            let (recp, tx) = if room.r1 == Some(addr) {
                (room.r2, room.r2tx.clone())
            } else if room.r2 == Some(addr) {
                (room.r1, room.r1tx.clone())
            } else {
                (None, None)
            };

            println!("rooms: {r:#?}");
            println!("recp: {recp:?}");
            room.state.game_status();
            if let Some(_r) = recp {
                tx.unwrap().unbounded_send(msg.clone()).unwrap();
            }
        }

        future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    println!("{} disconnected", &addr);
    let mut to_remove = vec![];
    let mut r = rooms.lock().unwrap();
    for (i, room) in r.iter_mut().enumerate() {
        if let Some(r1) = room.r1 {
            if r1 == addr {
                room.r1 = room.r2;
                room.r1tx = room.r2tx.clone();
                room.r2 = None;
                room.r2tx = None;
                room.state = State::Waiting;
            }
        }
        if let Some(r2) = room.r2 {
            if r2 == addr {
                room.r2 = None;
                room.r2tx = None;
                room.state = State::Waiting;
            }
        }
        if room.r1.is_none() && room.r2.is_none() {
            to_remove.push(i);
        }
    }
    // would i even want to do this
    // or would it make sense to leave
    // a certain number of rooms open
    // to reduce allocations
    for i in to_remove {
        println!("removing {:#?}", r[i]);
        r.swap_remove(i);
    }
}

#[tokio::main]
async fn main() -> Result<(), IoError> {
    let rooms = Rooms::new(Mutex::new(Vec::with_capacity(8)));

    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(hc2(Arc::clone(&rooms), stream, addr));
    }
    Ok(())
}
