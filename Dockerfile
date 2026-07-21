FROM debian:bookworm-slim AS download

ARG FALLOW_VERSION=3.7.1
ARG TARGETARCH

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

# The sha256 pins below are bound to FALLOW_VERSION above; bump both together.
# release.yml's docker-lockstep job keeps them in sync automatically after
# every release by opening a PR here; a manual edit only needs to preserve
# the lockstep rule for local review.
RUN set -eux; \
  case "${TARGETARCH}" in \
    amd64) \
      asset="fallow-linux-x64-musl"; \
      sha256="ab024f50d9f8cb3dbe2b7d2b0926de98b5a4a51bd530c56662ab16efae06e0c3"; \
      ;; \
    arm64) \
      asset="fallow-linux-arm64-musl"; \
      sha256="e0bb27f1c4906ec92fdf00891bb3ac86842a29818e5113d047f4693a5352fc9b"; \
      ;; \
    *) \
      echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; \
      exit 1; \
      ;; \
  esac; \
  curl -fsSL "https://github.com/fallow-rs/fallow/releases/download/v${FALLOW_VERSION}/${asset}" -o /usr/local/bin/fallow; \
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
