### Fixed

- **A crafted stylesheet can no longer make `fallow dead-code` or `fallow health --css` hang.** The CSS parser parsed the content of some unclosed color functions such as `light-dark(` twice at each level, so the parse time doubled with each nested function. Fallow now skips the structural CSS parse of a stylesheet that nests more than 8 color functions. CSS Modules classes in such a file come from the plain class scan, and the file gets no CSS analytics. Real stylesheets are not affected.
