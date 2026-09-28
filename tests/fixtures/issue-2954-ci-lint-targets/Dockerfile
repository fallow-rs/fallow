FROM node:22
RUN npx eslint src/dead-docker.ts
RUN npm run lint -- src/dead-docker-call.ts
RUN npm run lint src/dead-docker-positional.ts
CMD ["node", "scripts/ci-tool.ts"]
