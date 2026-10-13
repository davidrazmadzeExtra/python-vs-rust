# python-vs-rust

Log analyzer benchmark: Python vs Rust.

```sh
# 1. Generate logs (written to data/server.log, which git ignores)
python3 generate_logs.py 10000000

# 2. Python
time python3 analyze_logs.py

# 3. Rust
cargo build --release --manifest-path rust-analyzer/Cargo.toml
time ./rust-analyzer/target/release/log_analyzer
```
