
## Repository Structure

This repository contains both early-stage experimental code and the final implementation of a Paxos-based system compiled to WebAssembly. 
Below is an overview of the file structure, with notes on what is relevant for the final implementation and what is legacy or experimental.

---

### Not Used in Final Implementation

These folders were part of early development, experimentation, or learning phases and are **not used in the final system**:

- **`archive/`**  
  Contains exploratory code and previously working models from various stages of the project, including approaches and tools that were either superseded by the final design or no longer function due to the system's evolved architecture.  
  Where the ones of note are: "composed-grpc", "modular-grpc" and "modular-ws".

- **`paxos-rust/`**  
  Early Paxos prototype implemented in native Rust. It was not finished to match the modularity and feature set of the final Wasm-based system.

- **`pocs/`**  
  Proof of concepts used for testing various ideas during development stages, which still works.

- **`wasm-examples/`**  
  Examples from WebAssembly tutorials and documentation used to understand capabilities. Not part of the production system.

---

### Used in Final Implementation

The following folders contain all the code and scripts used in the final system:

- **`cluster/`**  
  Bash scripts for deploying and running the system on the [UIS BBChain cluster](https://www.bbchain.no/).

- **`paxos-wasm/`**  
  The final implementation. This directory contains all WebAssembly components, supporting infrastructure and custom host application:

    - **`core/`**  
    Core Wasm components implemented: proposer, acceptor, learner, kv-store, failure-detector, leader-detector and storage.

  - **`agents/`**  
    Wasm components for all the Paxos agent roles: proposer-agent, acceptor-agent and learner-agent.

  - **`runners/`**  
    Wasm components for both the Runner and Coordinator.

  - **`network-ws/`**
    Wasm components for networking using WASI Sockets: client-server, network-server, network-client, serializer.
   

  - **`runner-ws/`**  
    Host-side logic for instantiating and executing Wasm components.  
    Includes custom host implementations of WIT interfaces: host-control, network-server, network-client, serializer, storage.  
    Include `config.yaml` to configure the the Paxos system.  
    Also includes helpful scripts:  
    - `run-local.sh`: Launches the full local cluster using windows terminal windows.
    - `run-demo-client`: Sends client requests to the system.

  - **`paxos-ws-client/`**  
    Simulates real client behavior by sending Paxos requests to the cluster.

  - **`shared/`**  
    Shared resources used across components — WIT interfaces, custom build helpers, example configuration files, proto file etc.



## Prerequisites

Before building or running the project, ensure you have the following installed in your **Linux environment**:

### 1. Rust & Cargo

Install Rust using `rustup` (includes Cargo):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
````

### 2. WASI-P2 Target

Add the WebAssembly target for WASI Preview 2:

```bash
rustup target add wasm32-wasip2
```

### 3. WAC CLI

Install the [WAC CLI](https://github.com/bytecodealliance/wac):

```bash
cargo install wac-cli
```

### 4. (Optional) Rust Analyzer Extension

For a better development experience in VSCode, install the **Rust Analyzer** extension from the Extensions Marketplace.

---

## Build Instructions

From the root of the project:

### Build WebAssembly Components

```bash
cargo build
```

### Build Native Binaries

```bash
cargo build --release
```

---

## Demo & Running Instructions

The configuration is already set up for running the **standalone** model with real clients.

From the main folder (after building):

### Run Nodes (only for WSL deployments, for other Linux, see "cluster" folder)

Launch windows terminal tabs—each representing a Paxos node on `localhost`:

```bash
paxos-wasm/runner-ws/run-local.sh
```

### Run Clients

Start 500 logical clients across 10 physical processes. These clients send a total of **1,000,000 requests** to the leader node:

```bash
paxos-wasm/runner-ws/run-demo-client.sh
```


