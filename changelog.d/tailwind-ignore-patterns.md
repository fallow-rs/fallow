### Added

- **Exclude intentional Tailwind arbitrary values with `health.tailwindIgnorePatterns`.** These regular expressions match utility tokens without variant prefixes and exclude matching classes from token-drift findings and analytics, including CVA variants. The default excludes nothing. For example, `"^icon-\\["` excludes Iconify classes while preserving other arbitrary-value findings.
