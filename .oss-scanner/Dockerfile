# syntax=docker/dockerfile:1
# Build image for the OSS Scanner (https://github.com/anthropics/oss-scanner).
# The scanner builds this file with the repository as the build context. The
# build has network access. The scan after it has none, so every crate and npm
# package that a build or a test needs must be fetched here.
# Keep the base image tag aligned with rust-toolchain.toml.
FROM rust:1.99.0-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=never \
    CARGO_NET_OFFLINE=false \
    RUST_BACKTRACE=1

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates git build-essential pkg-config python3 \
    && rm -rf /var/lib/apt/lists/*

# Node.js runs the type-aware sidecar and the mutation runner tests.
COPY --from=node:22-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=node:22-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx

# Scanner contract: the checkout lives inside the image, at /src.
COPY . /src
WORKDIR /src

# Tests create git repositories in temporary directories.
RUN git config --global user.email "scanner@localhost" \
    && git config --global user.name "scanner" \
    && git config --global init.defaultBranch main

# Fetch every crate and every npm package, so later builds work offline.
RUN cargo fetch --locked \
    && npm ci --prefix tools/type-aware-sidecar --no-audit --no-fund --ignore-scripts

# Debug builds keep debug info, assertions and overflow checks. The binaries
# land in target/debug: fallow (CLI), fallow-lsp and fallow-mcp.
RUN cargo build --locked --workspace --bins

# Compile every test target so tests run offline without a rebuild.
RUN cargo test --locked --workspace --lib --bins --tests --examples --no-run

ENV CARGO_NET_OFFLINE=true

# Run the unit tests as a smoke check. A failure does not stop the build.
RUN (timeout 1800 cargo test --locked --workspace --lib \
      || echo "WARNING: fallow unit tests reported failures")
