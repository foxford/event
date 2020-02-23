# Running

1. Start up the development environment: `docker-compose -d`.
2. Start up the service: `cargo run`.
3. Go to this directory: `cd ./benchmarks`
4. Run benchmark with `cargo run --bin BENCHMARK_NAME -- OPTIONS`.
   For example `cargo run --bin create_events -- --rate 10 --tag example`

For available benchmark names see `src/bin` directory.

To list all available options run `cargo run --bin BENCHMARK_NAME -- --help`.
