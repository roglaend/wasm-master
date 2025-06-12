
## Repository Structure

This repository contains both early-stage experimental code and the final implementation of a Paxos-based system compiled to WebAssembly. Below is an overview of the file structure, with notes on what is relevant for the final implementation and what is legacy or experimental.

---

### Not Used in Final Implementation

These folders were part of early development, experimentation, or learning phases and are **not used in the final system**:

- **`archive/`**  
  Contains old, unusable code primarily from the very beginning of the project.

- **`paxos-rust/`**  
  Early Paxos prototyping in native Rust. This version was eventually abandoned in favor of the Wasm-based approach.

- **`pocs/`**  
  Proof of concepts used for testing various ideas during early development stages.

- **`wasm-examples/`**  
  Examples from WebAssembly tutorials and documentation used to understand capabilities. Not part of the production system.

---

### Used in Final Implementation

The following folders contain all the code and scripts used in the final system:

- **`cluster/`**  
  Bash scripts for deploying and running the system on the [UIS BBChain cluster](https://www.bbchain.no/).

- **`paxos-wasm/`**  
  Core of the final implementation. This directory contains all WebAssembly components and supporting infrastructure:

  - **`agents/`**  
    Wasm components for all the Paxos agent roles.

  - **`core/`**  
    Core logic implemented as Wasm components.

  - **`runners/`**  
    Includes both the coordinator and runner implementations.

  - **`network-ws/`** *(network – WASI sockets)*  
    Wasm components for networking: client-server, network server, serializers, TCP client, and UDP client (not used in final setup).  
    Also includes helpful scripts:
    - `run-local.sh`: Launches the full local cluster.
    - `run-demo-client`: Sends client requests to the system.

  - **`runner-ws/`**  
    Host-side logic for instantiating and executing Wasm components.

  - **`paxos-ws-client/`**  
    Simulates real client behavior by sending Paxos requests to the cluster.

  - **`shared/`**  
    Shared resources used across components — WIT interfaces, configuration files, and common types.

  - _Other folders inside `paxos-wasm/`_  
    Additional code that reflects the system’s evolution, including prior experiments (e.g., with gRPC networking). Not relevant to the final architecture.



## Prerequisites

Before building or running the project, ensure you have the following installed in your **WSL environment**:

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

### Build Native Binaries

```bash
cargo build --release
```

### Build WebAssembly Components

```bash
cargo build
```

---

## Demo & Running Instructions

The configuration is already set up for running the **standalone** model.

From the main folder (after building):

### Run Nodes

Launch 7 terminal tabs—each representing a Paxos node on `localhost`:

```bash
paxos-wasm/runner-ws/run-local.sh
```

### Run Clients

Start 500 logical clients across 10 physical processes. These clients send a total of **1,000,000 requests** to the leader node:

```bash
paxos-wasm/runner-ws/rund-demo-client.sh
```


