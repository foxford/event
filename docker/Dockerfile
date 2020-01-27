## -----------------------------------------------------------------------------
## Build
## -----------------------------------------------------------------------------
# Using beta 1.41.0 because of this bugfix https://github.com/rust-lang/rust/pull/64856.
# Stable release is scheduled on January 30, 2020.
FROM instrumentisto/rust:1.41.0-beta-buster-slim as build-stage

WORKDIR "/build"

# Install and build crates
COPY Cargo.* /build/
RUN mkdir /build/src && echo "fn main() {}" > /build/src/main.rs
RUN cargo build --release

# Build app
COPY src/ /build/src/
RUN touch src/main.rs && cargo build --release

## -----------------------------------------------------------------------------
## Package
## -----------------------------------------------------------------------------
FROM debian:buster

# Install runtime dependencies
RUN apt update && apt install -y --no-install-recommends \
  libssl1.1 \
  libcurl4 \
  libpq5

COPY --from=build-stage "/build/target/release/event" "/app/event"

WORKDIR "/app"
ENTRYPOINT ["/app/event"]
