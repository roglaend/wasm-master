use paxos_wasm_utils::build_helpers::build_and_plug;

fn main() {
    let target = "wasm32-wasip2";

    let build_list: &[&str] = &[];
    let plugs = &["composed_tcp_client"];
    let socket = "composed_proposer_agent";
    let output = "tcp_composed_proposer_agent";
    build_and_plug(target, build_list, plugs, socket, output);

    let build_list: &[&str] = &[];
    let plugs = &["composed_tcp_client"];
    let socket = "composed_acceptor_agent";
    let output = "tcp_composed_acceptor_agent";
    build_and_plug(target, build_list, plugs, socket, output);

    let build_list: &[&str] = &[];
    let plugs = &["composed_tcp_client"];
    let socket = "composed_learner_agent";
    let output = "tcp_composed_learner_agent";
    build_and_plug(target, build_list, plugs, socket, output);

    // Now plug the composed agents into the composed TCP server.
    // Here, we add the modules for the composed agents and the serializer as plug modules.
    // The composed TCP server is treated as the 'socket' module.
    // The final composite module is named "final_composed_tcp_server".
    let plugs = &[
        "tcp_composed_proposer_agent",
        "tcp_composed_acceptor_agent",
        "tcp_composed_learner_agent",
    ];
    let socket = "composed_tcp_server";
    let output = "final_composed_tcp_server";
    build_and_plug(&target, &[], plugs, socket, output);
}
