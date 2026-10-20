### Added

- **Experimental tsrs backend for type-aware analysis.** Set `FALLOW_TYPE_AWARE_BACKEND=tsrs` to run the semantic pass on [tsrs](https://github.com/oxc-project/tsrs), the Rust port of the TypeScript 7 checker, instead of TypeScript-Go. Install the pinned `tsrs` version next to `fallow-type-aware`. `fallow type-aware status` and `_meta.type_aware` report the backend that ran. TypeScript-Go stays the default.
