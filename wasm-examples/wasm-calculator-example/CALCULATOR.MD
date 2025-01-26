# WASM Calculator Project

### How to Run

1. **Build the components:**

   - For the `calculator`:
     ```bash
     cd calculator
     cargo component build --release
     ```

   - For the `adder`:
     ```bash
     cd adder
     cargo component build --release
     ```

   - For the `subtractor`:
     ```bash
     cd subtractor
     cargo component build --release
     ```

   - For the `command`:
     ```bash
     cd command
     cargo component build --release
     ```

2. **Create the composed module:**
   ```bash
   wac plug calculator/target/wasm32-wasip1/release/calculator.wasm --plug adder/target/wasm32-wasip1/release/adder.wasm --plug subtractor/target/wasm32-wasip1/release/subtractor.wasm -o composed.wasm
   ```

3. **Create the final module:**
   ```bash
   wac plug command/target/wasm32-wasip1/release/command.wasm --plug composed.wasm -o final.wasm
   ```

4. **Run the final module:**

   - For addition (`2 + 1`):
     ```bash
     wasmtime run final.wasm 2 1 add
     ```

   - For subtraction (`2 - 1`):
     ```bash
     wasmtime run final.wasm 2 1 sub
     ```