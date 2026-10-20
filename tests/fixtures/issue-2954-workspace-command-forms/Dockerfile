FROM node:22@sha256:0e5f906573693feaa1e21057ebdcfdb5bd5021f050b2dc7c9deceb629c7da2a8
COPY . .
RUN yarn prettier src/docker-input.ts
RUN yarn workspace web node scripts/docker-run.ts
RUN npm run -w web codegen -- scripts/docker-codegen.ts
