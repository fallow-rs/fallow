FROM node:22@sha256:0e5f906573693feaa1e21057ebdcfdb5bd5021f050b2dc7c9deceb629c7da2a8
RUN my-codegen src/docker-input.ts
RUN npm run gen -- src/docker-call-input.ts
RUN npm run gen -- --input=src/docker-flag-input.ts
