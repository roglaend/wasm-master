mod bindings;

use clap::Parser;

use bindings::paxos::proposer::propose;

#[derive(Parser)]
#[clap(name = "calculator", version = env!("CARGO_PKG_VERSION"))]
struct Command {
    /// The first operand
    propose_value: String,
}
impl Command {
    fn run(self) {
        let res = propose::propose(&self.propose_value);
        println!("{res}");
    }
}

fn main() {
    Command::parse().run()
}