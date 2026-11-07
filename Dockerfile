FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3 AS download

ARG FALLOW_VERSION=3.41.0
ARG TARGETARCH

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

# The sha256 pins below are bound to FALLOW_VERSION above; bump both together.
# The maintainer release flow refreshes all three after publication, via
# .github/scripts/update-dockerfile-pins.mjs. There is no CI job that does it:
# the docker-lockstep job that used to open a PR here was removed in v3.7.1.
RUN set -eux; \
  case "${TARGETARCH}" in \
    amd64) \
      asset="fallow-linux-x64-musl"; \
      sha256="f86e33152d5ef25e10b6259b38731cc494ceda9045f795a2d1b6c1368db25db4"; \
      ;; \
    arm64) \
      asset="fallow-linux-arm64-musl"; \
      sha256="e40edd176043ba926d1c4eff19b4f0bfe255bef56b63f197413f3216d4968e34"; \
      ;; \
    *) \
      echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; \
      exit 1; \
      ;; \
  esac; \
  curl -fsSL --retry 5 --retry-connrefused --retry-delay 2 \
    "https://github.com/fallow-rs/fallow/releases/download/v${FALLOW_VERSION}/${asset}" -o /usr/local/bin/fallow; \
  echo "${sha256}  /usr/local/bin/fallow" | sha256sum -c -; \
  chmod +x /usr/local/bin/fallow

FROM node:26-bookworm-slim@sha256:1e96b01216cab31612cd1f9480e83413472dee8686d423cb4c1ec62bf8a26e9e AS runtime

# tools/corepack/package-lock.json pins the corepack version and its hash.
COPY tools/corepack/package.json tools/corepack/package-lock.json /opt/corepack/

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates git \
  && npm ci --prefix /opt/corepack --ignore-scripts --no-audit --no-fund \
  && ln -sf /opt/corepack/node_modules/corepack/dist/corepack.js /usr/local/bin/corepack \
  && corepack enable --install-directory /usr/local/bin \
  && npm cache clean --force \
  && rm -rf /var/lib/apt/lists/*

COPY --from=download /usr/local/bin/fallow /usr/local/bin/fallow

WORKDIR /workspace
ENTRYPOINT ["fallow"]
CMD ["--help"]
