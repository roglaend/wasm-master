use std::thread;
use std::time::Duration;
use wasi::clocks::monotonic_clock;

mod bindings {
    wit_bindgen::generate! {{
        path: "../wit/tcp-server-polling.wit",
        world: "handler-world",
        // async: true,
    }}
}

bindings::export!(Component with_types_in bindings);

use bindings::exports::poc::tcp_server_polling::handler::Guest;

struct Component;

impl Guest for Component {
    fn handle(msg: String, should_sleep: bool) -> String {
        println!("Handler: received message: {}", msg);

        if should_sleep {
            busy_wait(Duration::from_secs(2)); // burn CPU instead of sleeping
        }

        println!("Handler: responding after delay");
        format!("Processed: {}", msg)
    }
}

fn busy_wait(duration: Duration) {
    let start = monotonic_clock::now();
    let target = start + duration.as_nanos() as u64;

    while monotonic_clock::now() < target {
        // spin
    }
}
