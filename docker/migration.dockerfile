FROM rust:1.46.0-slim-buster

RUN apt update && apt install -y --no-install-recommends \
  pkg-config \
  libssl-dev \
  libcurl4-openssl-dev \
  libpq-dev

RUN cargo install sqlx-cli --version 0.1.0-beta.1
WORKDIR /app
CMD ["cargo", "sqlx", "migrate", "run"]
COPY ./migrations /app/migrations
