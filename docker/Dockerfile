## -----------------------------------------------------------------------------
## Build
## -----------------------------------------------------------------------------
FROM rust:1.46.0-slim-buster as build-stage

RUN apt update && apt install -y --no-install-recommends \
  pkg-config \
  libssl-dev \
  libcurl4-openssl-dev \
  libpq-dev \
  postgresql

# Set up and postgres for passwordless local connection
RUN echo "host all postgres 0.0.0.0/0 trust" > /etc/postgresql/11/main/pg_hba.conf
ENV DATABASE_URL postgresql://postgres@0.0.0.0/postgres

# Install diesel_cli to run migrations
RUN cargo install diesel_cli --no-default-features --features "postgres"

WORKDIR "/build"

# Install and build crates
COPY Cargo.* /build/
RUN mkdir /build/src && echo "fn main() {}" > /build/src/main.rs
RUN cargo build --release

# Run migrations
COPY migrations /build/migrations
RUN service postgresql start && diesel migration run

# Build app
COPY src/ /build/src/
RUN service postgresql start && touch src/main.rs && cargo build --release

## -----------------------------------------------------------------------------
## Package
## -----------------------------------------------------------------------------
FROM debian:buster

# Install runtime dependencies
RUN apt update && apt install -y --no-install-recommends \
  ca-certificates \
  libssl1.1 \
  libcurl4 \
  libpq5

COPY --from=build-stage "/build/target/release/event" "/app/event"

WORKDIR "/app"
ENTRYPOINT ["/app/event"]
