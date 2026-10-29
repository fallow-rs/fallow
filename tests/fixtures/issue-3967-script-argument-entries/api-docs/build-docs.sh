#!/bin/sh
node api-docs/build-openapi.js --definition api-docs/definition.json src/lib/helper-arg.js src/**/*.js
pnpm watchee -f './src/lib/watch*.js' -c 'echo hi'
