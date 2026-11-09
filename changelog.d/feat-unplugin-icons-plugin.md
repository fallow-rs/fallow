### Added

- **Built-in unplugin-icons plugin.** An icon import such as `import IconX from '~icons/lucide/x'` or `virtual:icons/mdi/arrow-left` is no longer an unlisted dependency `~icons`. The import also credits the icon set package (`@iconify-json/lucide`), the full `@iconify/json` collection and `unplugin-icons`, when the project declares them. Before, fallow reported these packages as unused dev dependencies.
