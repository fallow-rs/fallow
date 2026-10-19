### Security

- **The Docker image now pins its base images and corepack.** `Dockerfile`
  and `Dockerfile.dev` pin each base image by its multi-platform digest and
  install corepack from a committed lockfile with an integrity hash.
  Dependabot keeps the digests and the corepack version current.
