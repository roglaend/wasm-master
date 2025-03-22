use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let start = Instant::now();

    // First client
    thread::spawn(|| {
        println!("[Client 1] Connecting...");
        let mut stream = TcpStream::connect("127.0.0.1:8080").unwrap();
        stream
            .write_all(b"sleep: true | hello from client 1\n")
            .unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        println!(
            "[Client 1] Response: {}",
            String::from_utf8_lossy(&buf[..n])
        );
    });

    // Slight delay before second client
    thread::sleep(Duration::from_millis(500));

    println!("[Client 2] Connecting after slight delay...");
    let mut stream = TcpStream::connect("127.0.0.1:8080").unwrap();
    stream
        .write_all(b"sleep: true | hello from client 2\n")
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).unwrap();
    println!(
        "[Client 2] Response: {}",
        String::from_utf8_lossy(&buf[..n])
    );

    println!("All clients received responses");
    println!("Total time elapsed: {:?}", Instant::now() - start);
}
