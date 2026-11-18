### Fixed

- **A suppression marker inside a string no longer suppresses findings.** A line in a template literal that starts with `// fallow-ignore-file code-duplication` removed the whole file from `fallow dupes`. The file and next-line markers inside a string or template literal now have no effect, as the range markers already did. The same rule now applies to the scripts of Vue, Svelte, Astro, MDX and HTML files, and to the suppressions that `fallow audit` reads for changed lines.
  (Closes [#4751](https://github.com/fallow-rs/fallow/issues/4751))
- **`fallow security` marks GraphQL resolver args only in a resolver map.** Before, each object method with a destructured second parameter was a `graphql-resolver-args` source. Fallow now marks the args of a resolver under a `Query`, `Mutation` or `Subscription` map and its type keys, of a field config with `subscribe` or `resolve`, and of a binding typed `Resolvers` or `*Resolvers`.
  (Closes [#4718](https://github.com/fallow-rs/fallow/issues/4718))
- **`fallow flags` no longer reports a method of a value that the file declares as a flag SDK call.** In a file without a flag import, a generic SDK method such as `isEnabled` on an object literal, on a `new C()` of a project class, or on a binding typed with such a class is not a flag. A `medium` confidence SDK call also has no `sdk_name` in the output now, because no flag import confirms the SDK. The `--flag-state` vendor match still uses the SDK of the method name.
  (Closes [#4726](https://github.com/fallow-rs/fallow/issues/4726))
- **A CSS clone group no longer starts on a declaration that differs between the files.** The detector drops the leading `;` tokens of a clone, so the clone starts on the first token that is the same.
  (Closes [#4738](https://github.com/fallow-rs/fallow/issues/4738))
