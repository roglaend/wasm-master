import graphviz

# Create a new Graphviz Digraph object with SVG format
dot = graphviz.Digraph('Paxos Architecture', format='svg')

# Set graph attributes for better resolution, large size, and no cropping
dot.attr(rankdir='TB', nodesep='3.5', ranksep='5.0', splines='polyline', size="20,20", dpi="300")

# Define Rust Host (Wasmtime) Cluster with more space
with dot.subgraph(name='cluster_host') as host:
    host.attr(label='Rust Host (Wasmtime)\nRuns Paxos-Coordinator & gRPC', color='lightgray', style='filled', fontsize='14')

    # Define Paxos Coordinator (WASM) Cluster with more space
    with host.subgraph(name='cluster_paxos') as paxos:
        paxos.attr(label='Paxos-Coordinator (WASM)\nHandles Paxos Algorithm', color='lightgreen', style='filled', fontsize='14')

        paxos.node('proposer', 'Proposer (WASM)\nInitiates Paxos rounds', shape='box', style='filled', fillcolor='lightblue', width='3.0', height='1.0', fontsize='12')
        paxos.node('acceptor', 'Acceptor (WASM)\nValidates proposals', shape='box', style='filled', fillcolor='lightblue', width='3.0', height='1.0', fontsize='12')
        paxos.node('learner', 'Learner (WASM)\nLearns final value', shape='box', style='filled', fillcolor='lightblue', width='3.0', height='1.0', fontsize='12')
        paxos.node('kv_store', 'KV Store (WASM)\nStores consensus data', shape='box', style='filled', fillcolor='lightyellow', width='3.0', height='1.0', fontsize='12')

    # Define WASI Interface and gRPC with more space
    host.node('wasi_interface', 'WASI Interface\n(network.send_message())\nFacilitates communication', shape='box', style='filled', fillcolor='orange', width='4.0', height='1.2', fontsize='12')
    host.node('grpc_server', 'gRPC Client / Server (Rust)\nHandles Paxos messaging', shape='box', style='filled', fillcolor='gray', width='4.0', height='1.2', fontsize='12')

# Define gRPC Network and Other Nodes with more space
dot.node('grpc_network', 'gRPC Network\nRelays Paxos messages', shape='ellipse', style='filled', fillcolor='lightgray', width='4.5', height='1.2', fontsize='12')
dot.node('other_nodes', 'Other Paxos Nodes\n(Same Architecture)', shape='box', style='filled', fillcolor='lightgray', width='4.5', height='1.2', fontsize='12')

# Improved Paxos Flow with Phase Labels and more spacing
dot.edge('proposer', 'paxos_coordinator', xlabel='Phase 1:\nstart_paxos_round(value)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('paxos_coordinator', 'wasi_interface', xlabel='Phase 2:\nnetwork.send_message(Prepare)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('wasi_interface', 'grpc_server', xlabel='Phase 3:\nsend_paxos_message(Prepare)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('grpc_server', 'grpc_network', xlabel='Phase 4:\ngRPC: Prepare Message', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('grpc_network', 'other_nodes', xlabel='Phase 5:\nDeliver Prepare(proposal_id)', penwidth='3', fontsize='10', arrowsize='1.2')

# Other nodes process messages
dot.edge('other_nodes', 'paxos_coordinator', xlabel='Phase 6:\nhandle_paxos_message(Prepare)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('acceptor', 'paxos_coordinator', xlabel='Phase 7:\nhandle_prepare() → Promise', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('paxos_coordinator', 'wasi_interface', xlabel='Phase 8:\nnetwork.send_message(Promise)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('grpc_network', 'proposer', xlabel='Phase 9:\nDeliver Promise(proposal_id)', penwidth='3', fontsize='10', arrowsize='1.2')

# Proposer sends Accept messages
dot.edge('proposer', 'paxos_coordinator', xlabel='Phase 10:\ncollect_promises()', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('paxos_coordinator', 'wasi_interface', xlabel='Phase 11:\nnetwork.send_message(Accept)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('grpc_network', 'other_nodes', xlabel='Phase 12:\nDeliver Accept(proposal_id, value)', penwidth='3', fontsize='10', arrowsize='1.2')

# Acceptors respond
dot.edge('acceptor', 'paxos_coordinator', xlabel='Phase 13:\nhandle_accept() → Accepted', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('paxos_coordinator', 'wasi_interface', xlabel='Phase 14:\nnetwork.send_message(Accepted)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('grpc_network', 'proposer', xlabel='Phase 15:\nDeliver Accepted(proposal_id, value)', penwidth='3', fontsize='10', arrowsize='1.2')

# Learning Phase
dot.edge('proposer', 'learner', xlabel='Phase 16:\nnotify_learners(value)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('learner', 'kv_store', xlabel='Phase 17:\nstore_value(value)', penwidth='3', fontsize='10', arrowsize='1.2')
dot.edge('kv_store', 'paxos_coordinator', xlabel='Phase 18:\nConsensus Reached!', penwidth='3', fontsize='10', arrowsize='1.2')


# Save and render the diagram in SVG format
dot_path = "./model_graph"
dot.render(dot_path)

# Render the diagram in both SVG and PNG formats
dot.format = 'svg'
dot.render(dot_path)

dot.format = 'png'
dot.attr(dpi='600')  # Ensure high-resolution PNG output
dot.render(dot_path)
