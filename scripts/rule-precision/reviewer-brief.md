You review a random sample of findings from the static analyzer `fallow` on public JavaScript/TypeScript projects. Your verdicts measure the precision of one rule. Read-only work: do not edit any file in the project checkouts.

Rule: `{RULE}` (reported by `fallow {COMMAND}` in its default mode).
Input: `{REVIEW_JSON}`. It is a JSON array. Each item has `project`, `id` (the finding ID), `repo_root` (the checkout), `anchor` (file and line), `finding` (the finding as fallow reports it) and `snippet` (a code excerpt). The checkouts are complete: open any file in them.

Contract of this command: in the default mode, test files, stories and config files count as users.

The claim of this rule, as `fallow explain {RULE}` states it (also in `{CONTRACT}`):

```
{CONTRACT_TEXT}
```

Judge each finding against this claim. `real` means the claim is true for this item. A fix that also needs edits at other sites (for example the call sites of a prop, or the type that declares it) is still `real` when the explain text says that those edits are part of the fix. `noise` means the claim is false for this item, or the suggested fix would change behavior in a way that the explain text does not accept.

`{ALL}` lists every finding of this rule in each project, not only the sample. A file or item whose only users are themselves in that list is still unused: follow the chain until you reach a live root (an entry point, a route, a script, a config) or a dead end. A chain whose root is a real entry point that fallow missed is `noise`.

For EACH item, open the real code. Search the whole checkout for uses (`git grep` the name, check re-exports, barrels, package.json `exports`/`bin`/`scripts`, framework conventions such as Next.js, SvelteKit, NestJS or Vite file roles, config files, dynamic imports and string references). Then give one verdict:

- `real`: the claim is true for this item.
- `noise`: the claim is false for this item, or the fix changes behavior. Give `class`: a short cause name such as "framework convention export", "dynamic import by string", "used by a script outside the source tree", "re-exported by a package entry", "used in a template". Reuse the same class name for the same cause.
- `unsure`: the code does not decide it (for example a use only a deployment config outside the repository can show). Use it rarely.

Be strict and skeptical in both directions. Do not trust fallow, and do not assume that it is wrong.

Write the result to `{VERDICTS_JSON}` in exactly this shape, with one entry for every item of the input and no other entries:

{"reviewed_on": "{DATE}", "verdicts": [{"project": "...", "id": "...", "verdict": "real|noise|unsure", "class": "only for noise", "reason": "one short sentence that names the code fact"}]}

Then reply with: the path of the file, and the noise classes you used with the IDs in each. Write short sentences in active voice.
