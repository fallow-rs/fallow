FROM node:22
RUN my-codegen src/docker-input.ts
RUN npm run gen -- src/docker-call-input.ts
RUN npm run gen -- --input=src/docker-flag-input.ts
