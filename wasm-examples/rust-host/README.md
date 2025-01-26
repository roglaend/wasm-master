# Rust Host with component

## Overview
The host imports the component from `host-test` and initializes it. It can then execute the functions that the component exports.

## How to Run

### 1. Compile the `host-test` Component
Navigate to the `host-test` folder and run the following command:

```
cargo component build --release
```

### 2. Compile and Run the Host Program
Navigate to the `host` folder and execute:

```
cargo run
```

This will compile and run the host program, utilizing the `host-test` component.

