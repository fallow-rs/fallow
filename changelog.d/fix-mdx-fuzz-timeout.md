### Fixed

- **A crafted TypeScript, JSX or MDX file can no longer make `fallow` hang.** Some sources with many nested `<` characters made the TypeScript parser try each nested part again and again, so one small file could keep `fallow dead-code`, `fallow dupes` or MDX extraction busy for minutes. Fallow now counts this nesting before it parses a file. A file past the cap is reported as a failed parse, the same as a file with too-deep bracket nesting. Real code stays well below the cap, so its results do not change.
