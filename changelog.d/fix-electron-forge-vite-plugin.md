### Fixed

- The Electron plugin reads the `build` and `renderer` items of the Electron Forge `VitePlugin` in `forge.config.*`. Each `config` path is an entry and its default export is used, and each `build` `entry` path is an entry. `vite` is used when the Forge config names `@electron-forge/plugin-vite`. Before, `fallow dead-code` reported these Vite configs as unused files and `vite` as an unused devDependency. (#5097)
