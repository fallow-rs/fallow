# Styling Corpus Smoke

Use the styling corpus smoke before release reviews that touch CSS, CSS-in-JS,
or audit styling gates.

```bash
npm run smoke:styling-corpus -- --out-dir target/styling-corpus-smoke
```

The harness clones a curated public corpus into
`~/.cache/fallow/styling-corpus` by default, reuses those clones on later runs,
and writes:

- `styling-corpus-smoke.json`: bounded machine summary
- `styling-corpus-smoke.md`: human review summary

It runs these commands per project:

- `fallow health --css`
- `fallow health --css --production`
- `fallow audit --css-deep --base HEAD~1`

The corpus covers Tailwind, StyleX, vanilla-extract, PandaCSS,
styled-components, Emotion, shadcn/CVA, CSS Modules, Sass, Less, Vue, Svelte,
Astro, and template-heavy projects. Use `--list` to inspect the current
entries.

Useful focused runs:

```bash
npm run smoke:styling-corpus -- --project emotion --project styled-components
npm run smoke:styling-corpus -- --project shadcn-admin --project shadcn-vite
npm run smoke:styling-corpus -- --max-projects 2 --skip-clone
npm run smoke:styling-corpus -- --refresh --project pandacss
```

Spike comparison uses
`scripts/fixtures/styling-corpus-smoke-baseline.json`. A spike is a new or
increased issue-code count, or a new or increased high-confidence sub-kind count,
that is not allowlisted. The default command highlights spikes but does not fail;
add `--fail-on-spikes` when a release gate should stop on unreviewed spikes.
