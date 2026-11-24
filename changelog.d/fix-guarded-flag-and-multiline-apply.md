### Fixed

- The flag retirement report shows separate sites for a Vercel flag definition and a call that tests it later in the same file. Fallow rebuilds the parse cache once to refresh these sites.
- CSS health reports count Tailwind utilities on continuation lines of an `@apply` directive. A theme token used only on a continuation line now shows its consumer.
