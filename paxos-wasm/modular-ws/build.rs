use paxos_wasm_utils::build_helpers::{build_and_plug, build_wasm_components};

fn main() {
    let target = "wasm32-wasip2";

    let base_components = &[
        "serializer",
        "tcp-server",
        "tcp-client",
        "udp-server",
        "udp-client",
    ];
    build_wasm_components(target, base_components);

    // --- Build and Plug WS Components ---

    let build_list_server: &[&str] = &[];
    let plugs_server = &["serializer"];
    let socket_server = "tcp_server";
    let output_server = "composed_tcp_server";
    build_and_plug(
        target,
        build_list_server,
        plugs_server,
        socket_server,
        output_server,
    );

    let build_list_client: &[&str] = &[];
    let plugs_client = &["serializer"];
    let socket_client = "tcp_client";
    let output_client = "composed_tcp_client";
    build_and_plug(
        target,
        build_list_client,
        plugs_client,
        socket_client,
        output_client,
    );

    let build_list_server_udp: &[&str] = &[];
    let plugs_server_udp = &["serializer"];
    let socket_server_udp = "udp_server";
    let output_server_udp = "composed_udp_server";
    build_and_plug(
        target,
        build_list_server_udp,
        plugs_server_udp,
        socket_server_udp,
        output_server_udp,
    );

    let build_list_client_udp: &[&str] = &[];
    let plugs_client_udp = &["serializer"];
    let socket_client_udp = "udp_client";
    let output_client_udp = "composed_udp_client";
    build_and_plug(
        target,
        build_list_client_udp,
        plugs_client_udp,
        socket_client_udp,
        output_client_udp,
    );

    // --- Build Agent Components ---

    let build_list_agents: &[&str] = &[];
    let plugs_agents = &["composed_tcp_client"];
    let socket_proposer = "composed_proposer_agent";
    let output_proposer = "tcp_composed_proposer_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs_agents,
        socket_proposer,
        output_proposer,
    );

    let socket_acceptor = "composed_acceptor_agent";
    let output_acceptor = "tcp_composed_acceptor_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs_agents,
        socket_acceptor,
        output_acceptor,
    );

    let socket_learner = "composed_learner_agent";
    let output_learner = "tcp_composed_learner_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs_agents,
        socket_learner,
        output_learner,
    );

    let build_list_agents: &[&str] = &[];
    let plugs_agents = &["composed_udp_client"];
    let socket_proposer = "composed_proposer_agent";
    let output_proposer = "udp_composed_proposer_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs_agents,
        socket_proposer,
        output_proposer,
    );

    let socket_acceptor = "composed_acceptor_agent";
    let output_acceptor = "udp_composed_acceptor_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs_agents,
        socket_acceptor,
        output_acceptor,
    );

    let socket_learner = "composed_learner_agent";
    let output_learner = "udp_composed_learner_agent";
    build_and_plug(
        target,
        build_list_agents,
        plugs_agents,
        socket_learner,
        output_learner,
    );

    // --- Compose the Final Server ---

    let plugs_final_tcp = &[
        "tcp_composed_proposer_agent",
        "tcp_composed_acceptor_agent",
        "tcp_composed_learner_agent",
    ];
    let socket_final_tcp = "composed_tcp_server";
    let output_final_tcp = "final_composed_tcp_server";
    build_and_plug(
        target,
        &[],
        plugs_final_tcp,
        socket_final_tcp,
        output_final_tcp,
    );

    let plugs_final_udp = &[
        "udp_composed_proposer_agent",
        "udp_composed_acceptor_agent",
        "udp_composed_learner_agent",
    ];
    let socket_final_udp = "composed_udp_server";
    let output_final_udp = "final_composed_udp_server";
    build_and_plug(
        target,
        &[],
        plugs_final_udp,
        socket_final_udp,
        output_final_udp,
    );
}
