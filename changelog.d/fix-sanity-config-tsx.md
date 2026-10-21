### Fixed

- **Fallow now treats `sanity.config.tsx` and `sanity.config.jsx` as Sanity
  entry points.** Sanity Studio loads its config from
  `sanity.config.{ts,tsx,js,jsx}`, but fallow knew only the `.ts` and `.js`
  names. Modules that only a `.tsx` or `.jsx` config file imports, such as a
  studio logo or a custom structure, are no longer reported as unused files,
  and their exports are no longer reported as unused. Under
  `includeEntryExports`, the `default` export of these config files counts as
  used. This changes the output for projects with a `.tsx` or `.jsx` Sanity
  config file in all modes.
