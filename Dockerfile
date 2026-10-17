FROM debian:bookworm-slim AS download

ARG FALLOW_VERSION=3.34.0
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
      sha256="34ff1737fd8bb58ef979e409b6a0ca60f3eb611d9e0feba6fda45674731d3f2c"; \
      ;; \
    arm64) \
      asset="fallow-linux-arm64-musl"; \
      sha256="58c20be859f8590ea8de586ef95244dc2e462c1164d871cfdfce524739754096"; \
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

FROM node:26-bookworm-slim AS runtime

ARG COREPACK_VERSION=0.35.0

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates git \
  && npm install -g "corepack@${COREPACK_VERSION}" \
  && corepack enable \
  && npm cache clean --force \
  && rm -rf /var/lib/apt/lists/*

COPY --from=download /usr/local/bin/fallow /usr/local/bin/fallow

WORKDIR /workspace
ENTRYPOINT ["fallow"]
CMD ["--help"]
