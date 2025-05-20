## Prerequisites

Make sure you have the following installed in your **WSL environment** before building or running this project:

### 1. Rust & Cargo

Install Rust using `rustup` (includes Cargo):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

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

### Native Binaries

```bash
cargo build --release
```

### WebAssembly Components

```bash
cargo build
```

