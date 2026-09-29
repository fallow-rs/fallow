FROM node:22
COPY . .
RUN yarn prettier src/docker-input.ts
RUN yarn workspace web node scripts/docker-run.ts
RUN npm run -w web codegen -- scripts/docker-codegen.ts
