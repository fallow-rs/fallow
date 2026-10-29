#!/bin/sh
nested-cli --check
bash scripts/build.sh
cd packages
tsx src/after-cd.ts
cd-cli
