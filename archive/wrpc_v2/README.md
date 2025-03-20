# Simplified Paxos Architecture

This architecture consists of three fundamental WASM components—**Proposer**, **Acceptor**, and **Learner**—each defined by its own WIT interface. A higher-level **Paxos WASM Component** imports these three modules, aggregates their functionality, and both exports and proxies the relevant functions needed by the Paxos protocol. Multiple instances of the Paxos WASM Component run in different processes (each on its own port). When one Paxos node (for example, the leader proposer) needs to invoke functionality on remote nodes, it uses wrpc_transport RPC calls to invoke the exported functions of its peer Paxos WASM components.

---

## Diagram

```plaintext
                             ┌───────────────────────────────┐
                             │   Paxos WASM Component (Node) │
                             │                               │
                             │   ┌───────────────────────┐   │
                             │   │  Proposer Module      │   │
                             │   ├───────────────────────┤   │
                             │   │  Acceptor Module      │   │
                             │   ├───────────────────────┤   │
                             │   │  Learner Module       │   │
                             │   └───────────────────────┘   │
                             │                               │
                             │  Exports Paxos RPCs (e.g.,    │
                             │  prepare, accept, learn)      │
                             └───────────────┬───────────────┘
                                             │
                               (Internal proxy calls)
                                             │
────────────────────────────────────────────────────────────────────────────
          │                           │                           │
          ▼                           ▼                           ▼
┌─────────────────┐       ┌─────────────────┐       ┌─────────────────┐
│ Paxos WASM Node │       │ Paxos WASM Node │       │ Paxos WASM Node │
│    (port 7001)  │       │    (port 7002)  │       │    (port 7003)  │
└─────────────────┘       └─────────────────┘       └─────────────────┘
          │                           │                           │
          └─────────┬───────────────┬───────────────┬─────────────┘
                    │               │               │
                    ▼               ▼               ▼
         RPC calls via wrpc_transport between Paxos nodes
           (Leader proposer calls remote acceptor functions,
            learners are notified, etc.)
                    
                    
                   ┌─────────────────────────────┐
                   │   Command WASM Component    │
                   │    (Client Submitting       │
                   │       Proposals)            │
                   └─────────────────────────────┘
                                 │
                                 ▼
                   Sends proposals to one Paxos WASM Node
```

---

## Explanation

1. **WASM Role Components:**
   - **Proposer Module:**  
     Implements the logic for initiating proposals.
   - **Acceptor Module:**  
     Implements the logic for handling the prepare/accept phases.
   - **Learner Module:**  
     Implements the logic for learning and finalizing the chosen value.
     
   Each of these modules is defined by its own WIT file so that its interface is clearly specified.

2. **Paxos WASM Component:**
   - **Imports & Aggregation:**  
     This component imports the Proposer, Acceptor, and Learner modules and aggregates their functionalities.
   - **Internal Proxying:**  
     When a Paxos node receives an RPC (for example, a prepare or accept request), it proxies the call to its internal module responsible for that role.
   - **Exporting Functions:**  
     The component exports functions such as `prepare`, `accept`, and `learn` so that other Paxos nodes can invoke them via RPC (using wrpc_transport).

3. **Distributed Paxos Nodes:**
   - **Multiple Processes:**  
     Multiple instances of the Paxos WASM Component run on different processes and ports (e.g., 7001, 7002, 7003). Each node runs the same logic.
   - **Peer Communication:**  
     When one Paxos node (especially the leader) needs to invoke functionality on other nodes (for example, to trigger the accept phase), it uses wrpc_transport RPC calls to call the corresponding exported functions on its peer Paxos nodes.
   - **Abstraction of Addresses:**  
     Instead of having individual addresses for each role (proposer, acceptor, learner), each Paxos node knows only about the addresses of other Paxos nodes.

4. **Command WASM Component (Client):**
   - **Submits Proposals:**  
     A separate WASM module acts as the client. Its sole purpose is to submit proposals (or commands) into the system by calling one of the Paxos WASM Components.
   - **Simplicity:**  
     The client is unaware of the internal Paxos protocol details. It simply makes a call, and the Paxos node (acting as a leader) takes over to coordinate the protocol.

5. **Flow of Execution:**
   - **Step 1:**  
     The Command WASM Component sends a proposal to one of the Paxos WASM nodes.
   - **Step 2:**  
     The Paxos node uses its internal Proposer module to initiate the proposal.
   - **Step 3:**  
     The Paxos node (acting as leader) coordinates with its internal Acceptor module and then uses RPC calls to invoke `prepare` (and later `accept`) on other Paxos nodes.
   - **Step 4:**  
     Once consensus is achieved, the Learner modules on all nodes learn the chosen value.
   - **Step 5:**  
     The outcome of the proposal is then relayed back to the client if necessary.

---

This simplified architecture supports running multiple, identical Paxos nodes in parallel, with each node encapsulating all Paxos roles and exposing the necessary RPC endpoints to coordinate distributed consensus. The Paxos component is responsible for both serving RPC calls from its peers and for proxying calls to remote nodes to ensure the Paxos protocol is executed seamlessly across the network.