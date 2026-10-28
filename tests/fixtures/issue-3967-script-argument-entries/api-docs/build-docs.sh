#!/bin/sh
node api-docs/build-openapi.js --definition api-docs/definition.json src/lib/helper-arg.js src/**/*.js
pnpm watchee -f './src/lib/watched.js' -c 'echo hi'
