### Added

- **Exclude intentional Tailwind arbitrary values with `health.tailwindIgnorePatterns`.** These regular expressions match normalized `prefix-[value]` tokens and exclude matching classes from token-drift findings and analytics, including CVA variants. Variants, negative signs, and suffix modifiers are omitted: `-mt-[7px]` and `mt-[7px]` both match as `mt-[7px]`, and `bg-[#abc]/50` matches as `bg-[#abc]`. Exclusions cannot distinguish these spellings. The default excludes nothing. For example, `"^icon-\\["` excludes Iconify classes while preserving other arbitrary-value findings.
