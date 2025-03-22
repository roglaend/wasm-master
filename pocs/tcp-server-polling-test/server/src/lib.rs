use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures::{StreamExt, stream::FuturesUnordered};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, error::TryRecvError, unbounded_channel,
};

use wasi::clocks::monotonic_clock;
use wasi::io::poll::{Pollable, poll};
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::TcpSocket;
use wasi::sockets::tcp_create_socket::create_tcp_socket;

mod bindings {
    wit_bindgen::generate!({
        path: "../wit/tcp-server-polling.wit",
        world: "tcp-server-world",
        // async: true
    });
}

bindings::export!(Component with_types_in bindings);

use bindings::exports::poc::tcp_server_polling::tcp_server::Guest;
use bindings::poc::tcp_server_polling::handler;

struct Component;

impl Guest for Component {
    fn run() {
        futures_executor::block_on(run_tcp_server());
    }
}

async fn run_tcp_server() {
    println!("Server: initialized. Listening on port 8080...");
    let (pollables_tx, mut pollables_rx) = unbounded_channel::<(Pollable, Waker)>();
    let mut tasks = FuturesUnordered::new();

    let listener = create_listener().await.expect("failed to create listener");

    loop {
        // Accept new connection pollable
        let accept_pollable = listener.subscribe();
        PollableFuture {
            pollable: Some(accept_pollable),
            pollables_tx: pollables_tx.clone(),
        }
        .await
        .unwrap();

        // Accept connection (non-blocking)
        if let Ok((socket, input, output)) = listener.accept() {
            println!("Server: new connection accepted");
            tasks.push(handle_client(socket, input, output, pollables_tx.clone()));
        }

        // Poll one task to drive progress
        tokio::select! {
            Some(_) = tasks.next() => {},
            _ = poller_tick(&mut pollables_rx) => {},
        }
    }
}

async fn create_listener() -> Result<TcpSocket, wasi::sockets::tcp::ErrorCode> {
    let network = instance_network();
    let socket = create_tcp_socket(IpAddressFamily::Ipv4)?;
    let pollable = socket.subscribe();

    let ip = Ipv4Addr::new(0, 0, 0, 0).octets();
    let local_address = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        address: (ip[0], ip[1], ip[2], ip[3]),
        port: 8080,
    });

    socket.start_bind(&network, local_address)?;
    pollable.block();
    socket.finish_bind()?;

    socket.start_listen()?;
    pollable.block();
    socket.finish_listen()?;

    Ok(socket)
}

async fn handle_client(
    _socket: TcpSocket,
    input: InputStream,
    output: OutputStream,
    pollables_tx: UnboundedSender<(Pollable, Waker)>,
) {
    println!("Server: waiting for client message...");
    let pollable = input.subscribe();
    PollableFuture {
        pollable: Some(pollable),
        pollables_tx,
    }
    .await
    .unwrap();

    match input.read(1024) {
        Ok(buf) => {
            if buf.is_empty() {
                return;
            }
            let msg = String::from_utf8_lossy(&buf).to_string();
            println!("Server: received from client: {}", msg);

            // Parse "sleep: true | actual message" format
            let (should_sleep, actual_msg) = if let Some((flag, body)) = msg.split_once('|') {
                let should_sleep = flag.trim().to_lowercase().contains("true");
                (should_sleep, body.trim().to_string())
            } else {
                // default: no sleep
                (false, msg)
            };

            println!(
                "Server: Calling handler with message: \"{}\", should_sleep: {}",
                actual_msg, should_sleep
            );
            // TODO: THIS NEEDS TO BE ASYNC TO WORK PROPERLY!!!
            let response = handler::handle(&actual_msg, should_sleep); //* This is the imported wasm component */
            let _ = output.write(response.as_bytes());
            println!("Server: response sent back to client.");
        }
        Err(e) => {
            eprintln!("read error: {:?}", e);
        }
    }
}

async fn poller_tick(rx: &mut UnboundedReceiver<(Pollable, Waker)>) {
    let mut pollables = Vec::<(Pollable, Waker)>::new();

    match rx.try_recv() {
        Ok((pollable, waker)) => pollables.push((pollable, waker)),
        Err(TryRecvError::Empty) => {
            if let Some((pollable, waker)) = rx.recv().await {
                pollables.push((pollable, waker));
            }
        }
        Err(TryRecvError::Disconnected) => {}
    }

    wake_ready(&mut pollables, Some(Duration::from_millis(10)));
}

fn wake_ready(pollables: &mut Vec<(Pollable, Waker)>, timeout: Option<Duration>) {
    let timeout = timeout
        .map(|d| monotonic_clock::subscribe_duration(d.as_nanos().try_into().unwrap_or(u64::MAX)));
    let timeout_ref = timeout.as_ref();

    let poll_refs: Box<[_]> = match timeout_ref {
        Some(t) => pollables.iter().map(|(p, _)| p).chain(Some(t)).collect(),
        None => pollables.iter().map(|(p, _)| p).collect(),
    };

    let mut ready = poll(&poll_refs);
    ready.sort_by_key(|i| std::cmp::Reverse(*i));
    let mut ready = ready.as_slice();

    if let (Some(i), Some(_)) = (ready.first(), timeout_ref) {
        if *i as usize == pollables.len() {
            ready = &ready[1..];
        }
    }

    for i in ready {
        let (_, waker) = pollables.remove(*i as usize);
        waker.wake();
    }
}

struct PollableFuture {
    pollable: Option<Pollable>,
    pollables_tx: UnboundedSender<(Pollable, Waker)>,
}

impl Future for PollableFuture {
    type Output = Result<(), ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let pollable = match this.pollable.as_ref() {
            Some(p) if poll(&[p])[0] == 0 => return Poll::Ready(Ok(())),
            Some(_) => this.pollable.take().unwrap(),
            None => return Poll::Pending,
        };

        let _ = this.pollables_tx.send((pollable, cx.waker().clone()));
        Poll::Pending
    }
}
