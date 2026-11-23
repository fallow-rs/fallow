### Fixed

- **`fallow trace-error` keeps scoped package paths in Firefox and Safari frames.** An at-sign in a path such as `node_modules/@scope/lib/index.js` no longer splits the function name from the wrong part of the file location.
- **`fallow trace-error` resolves development server URLs to project files.** It decodes the URL path and ignores the query and fragment when it looks for a source file.
