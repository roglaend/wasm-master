#[allow(warnings)]
mod bindings;

// use bindings::exports::paxos::hosttest::adder::Guest;
use bindings::Guest;

struct Component;

impl Guest for Component {
    /// Say hello!
    fn add(a: u32, b: u32) -> u32 {
        a + b
    }
}

bindings::export!(Component with_types_in bindings);
