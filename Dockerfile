FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc AS download

ARG FALLOW_VERSION=3.43.2
ARG TARGETARCH

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

# The sha256 pins below are bound to FALLOW_VERSION above; bump both together.
# The release flow refreshes the version and pins with
# .github/scripts/update-dockerfile-pins.mjs.
RUN set -eux; \
  case "${TARGETARCH}" in \
    amd64) \
      asset="fallow-linux-x64-musl"; \
      sha256="6b1c11985302b500943848d23db49e74253eee6cf9ead1456b58df4a011ead6e"; \
      ;; \
    arm64) \
      asset="fallow-linux-arm64-musl"; \
      sha256="75baf8ee94e157a30e082b356e90f7e904d8e41daec8ba5fc90e0eab68afe4f1"; \
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
