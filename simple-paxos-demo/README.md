# Paxos Algorithm with WebAssembly and Component Model

This project implements the Paxos consensus algorithm using WebAssembly's new Component Model. By defining each component of Paxos as a WebAssembly interface, this approach supports modular, language-agnostic implementations. Developers can implement components like the Proposer, Acceptor, and Learner in different programming languages, compile them to WebAssembly, and compose them into a cohesive system.

## Overview

The Paxos algorithm is a consensus protocol that enables distributed systems to agree on a single value, even in the presence of faults. It comprises the following roles:

- **Proposer**: Proposes values and ensures a majority of acceptors agree.
- **Acceptors** (Multiple): Maintain state and collectively decide whether to accept proposals.
- **Learners** (Multiple): Learn the agreed value after consensus is reached.

Optionally, you can include:
- **Client**: Sends values to proposers for consensus.
- **Coordinator**: Orchestrates communication between components for testing or monitoring purposes.

### Architecture

The Paxos system is implemented as a collection of WebAssembly modules, where each module conforms to interfaces defined in WebAssembly Interface Types (WIT). These modules can be written in any language that compiles to WebAssembly and interact seamlessly via the defined interfaces.

---

### Component Structure

Below is the high-level structure of the Paxos system, showing interactions between components:

```text
+-----------+      +-------------+      +-----------------+
|   Client  | ---> |   Proposer  | ---> |  Acceptors (N)  |
+-----------+      +-------------+      +-----------------+
                                             |         |
                                             v         v
                                      [Learners (M)]  ...
```

1. **Client**: Initiates requests by submitting values to the Proposer.
2. **Proposer**: Orchestrates the proposal process, interacting with multiple Acceptors.
3. **Acceptors (N)**: Respond to proposals and collectively maintain consensus state.
4. **Learners (M)**: Receive the agreed-upon value from Acceptors or other Learners.

---

### Interfaces and Worlds

Each role is defined using WIT files. For example:

- `proposer.wit`: Defines the functions for initiating and submitting proposals.
- `acceptor.wit`: Handles requests to prepare and accept proposals.
- `learner.wit`: Learns the consensus outcome from acceptors.

Refer to the `interfaces/` folder for the complete WIT definitions.

---

### Deployment and Composition

#### 1. Module Implementation
Each role can be implemented in the programming language of your choice, as long as it complies with its WIT interface. For example:
- **Rust**: Efficient memory handling and safety.
- **Go**: Simplified concurrency.
- **JavaScript**: Easy integration with web applications.

#### 2. Composition
WebAssembly's Component Model allows seamless composition of these modules. For example:
- Import multiple Acceptor modules into the Proposer.
- Import multiple Learner modules into the system.

#### 3. Integration World
The `paxos-system.wit` world integrates all the modules into a functional system:
```wit
world paxos-system {
    import paxos:client/client@0.1.0;
    import paxos:proposer/proposer@0.1.0;
    import paxos:acceptor/acceptor@0.1.0;  // Multiple acceptors
    import paxos:learner/learner@0.1.0;    // Multiple learners
}
```

---

### Figures

#### Component Interaction Diagram

```plaintext
[Client]
   |
   v
[Proposer] ---> [Acceptor 1]
                   |        \
                   |         \
                   v          v
              [Acceptor 2]  [Acceptor N]
                   |             |
                   |             v
                   v        [Learner 1]
              [Learner M]
```

This diagram highlights:
1. **Multiple Acceptors**: The Proposer interacts with multiple acceptors to reach a quorum.
2. **Multiple Learners**: Each learner can learn the agreed value from acceptors or other learners.

---

### Getting Started

1. Clone the repository:
   ```bash
   git clone https://github.com/your-repo/paxos-wasm.git
   cd paxos-wasm
   ```

2. Install WebAssembly toolchains:
   - For Rust: `rustup target add wasm32-unknown-unknown`
   - For Go: Use `tinygo` or similar tools.

3. Implement modules:
   - Write code for each role (e.g., Proposer, Acceptor, Learner).
   - Ensure compliance with WIT interfaces.

4. Compile modules to Wasm:
   ```bash
   cargo build --target wasm32-unknown-unknown
   ```

5. Run the system:
   Use a WebAssembly runtime or a Component Model orchestrator to compose and execute the modules.

---