FROM node:22@sha256:0e5f906573693feaa1e21057ebdcfdb5bd5021f050b2dc7c9deceb629c7da2a8
RUN npx eslint src/dead-docker.ts
RUN npm run lint -- src/dead-docker-call.ts
RUN npm run lint src/dead-docker-positional.ts
CMD ["node", "scripts/ci-tool.ts"]
