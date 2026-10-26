### Fixed

- **Deeply nested defaults in an object pattern no longer make extraction hang.** In a declaration such as `const { k: { k: { m } = s } = s } = s`, each default walked its left pattern again against the fallback. The work doubled at each level of nesting, so a short file could stop the analysis. A step budget now bounds the walk of one declaration.
