FROM ubuntu:24.04

RUN apt update && apt install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY ./target/release/wasm-overhead-native /usr/local/bin/wasm-overhead-native

RUN chmod +x /usr/local/bin/wasm-overhead-native

ENTRYPOINT ["/usr/local/bin/wasm-overhead-native"]
