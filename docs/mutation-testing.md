# Mutation testing

`fallow mutation` is an opt-in workflow that measures how well the tests of a
project detect changes in the code. It makes small changes (mutants) in the
source, runs the project tests against each mutant, and reports which mutants
no test detects. Fallow stays an evidence engine: it reports facts and
read-only next steps. It never writes a test and never proposes a code change.

## Command surface

| Invocation | Runs project code | Output kind |
|---|---|---|
| `fallow mutation --plan` | no | `mutation-plan` |
| `fallow mutation` | yes | `mutation` |
| `fallow mutation inspect <mutant_id> [--report <file>]` | no | `mutation-inspect` |
| `fallow mutation inspect <mutant_id> --replay [--args <json>] [--export <name>]` | yes | `mutation-inspect` with `replay` |
| `fallow mutation tasks [--report <file>]` | no | `mutation-tasks` |
| `fallow mutation export [--report <file>]` | no | `mutation-testing-report-schema` document |
| `fallow mutation --recheck [--report <file>] [--mutant <id>]` | yes | `mutation` |
| `fallow mutation --compare <file>` | yes | `mutation` |
| `fallow health --mutation-report <file>` | no | `health` |
| `fallow report --from <results> --mutation-report <file>` | no | the format of the render |

- The default scope is the changed code against the detected base ref, the same
  way as `fallow audit`. `--changed-since <ref>` (alias `--base`) sets the
  base, and `--diff-file` gives the diff.
  Generated mutation diffs use Git's rename and copy detection (`-M -C`):
  unchanged copied lines are excluded, while edits in a detected copy remain
  in scope. Detection considers modified sources and keeps Git's configured
  rename limit. Copies from unchanged sources, sources outside a selected
  package root, and untracked files can still appear as new additions.
  Supplied `--diff-file` hunks stay literal. A plain addition patch selects
  all its added lines; generate a patch with `git diff -M -C` to retain copy
  ancestry, or add `--find-copies-harder` when unchanged sources must also be
  considered. Exhaustive source searches can cost more on large repositories.
  For changed scope, an explicit patch whose paths cannot be placed exits 2
  with `FALLOW_MUTATION_DIFF_NOT_APPLIED`, before a test runner starts. This
  applies to `--diff-file`, `--diff-stdin`, `--diff-file -`, and the API's
  `analysis.diff_file`. Empty supplied patches select no mutants. Inherited
  `FALLOW_DIFF_FILE` behavior is unchanged; this refusal applies to explicit
  inputs only.
- `--all` mutates each source file. Positional paths mutate only those files or
  directories. The three scopes are exclusive.
- `--workspace` and `--changed-workspaces` exit with code 2 in version 1,
  because the plan cannot narrow to a workspace yet. Pass the workspace
  directory as a path.
- The run flags are `--max-mutants`, `--mutants-per-line`, `--function-scope`,
  `--time-budget` (`90s`, `10m`, `1h`),
  `--setup-timeout` (default `2m`), `--runner-config`, `--kill-matrix`,
  `--kill-matrix-for`, `--concurrency`, `--runtime`, `--allow-dirty`, `--allow-side-effects`,
  `--min-score`, `--operators`, `--verdicts`, `--typecheck`, `--resume` and
  `--recheck` (with `--report` and `--mutant`) and `--compare`.
  The global `--no-cache` flag also turns off the result cache (see "Result
  cache"). `--resume` continues an interrupted or partial run (see "Outcome
  journal and resume").
- Only `--format human` and `--format json` are valid. `tasks` also accepts
  `--format markdown`. `export` writes JSON with both values (see
  [Export](#export)). The command has no SARIF or LSP surface. `fallow
  report --mutation-report` renders a saved report into the CI formats,
  CodeClimate included, next to an analysis result. `report --from` refuses
  a `mutation` report as its primary input. The GitHub Action and the GitLab
  template run the command and render its result (see [CI](#ci)).
- Each run saves its full JSON report to
  `<cache dir>/mutation/last-report.json` (by default `.fallow/mutation/`).
  The cache directory has a `.gitignore` with `*`, so the saved report does
  not make the worktree dirty.
- `-o <path>` (`--output-file`) writes the report only after the run, also
  for a run that exits `2` because of baseline failures. The
  worktree check leaves that path out, so a report from an earlier run does
  not make the worktree dirty. A shell redirect such as `> mutation.json`
  creates the file before Fallow starts, so the dirty-worktree check refuses
  the run. Use `-o` or the saved report instead.

## Source files

- JavaScript and TypeScript files (`.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`,
  `.tsx`, `.mts`, `.cts`) are one program each.
- In a Vue or Svelte component and in an Astro component, only the script
  blocks get mutants: the Vue `<script>` and `<script setup>` blocks, the
  Svelte instance and module scripts, and the Astro frontmatter. A block with
  a `src` attribute has no inline code. The extract crate finds the blocks
  (`fallow_extract::sfc::extract_sfc_scripts`,
  `fallow_extract::astro::extract_astro_frontmatter`), so the plan uses the
  same blocks as the analysis.
- The instrumented copy of a component is the whole file with each script
  block replaced. The template and the styles stay the same, so the framework
  plugin of the project compiles the copy. The companion serves a component
  copy in the Vite `load` hook, so a framework plugin with its own `pre`
  `transform` hook also compiles the copy. A request with a query (a style or
  a template block) is not served: the framework plugin makes it from the
  component that it compiled.
- Spans, lines, columns and diff ranges use the coordinates of the whole
  file. A mutant id depends on the text of its function, not on the offset of
  the block, so an edit of the template keeps the ids.
- The prelude goes into each block with module scope (the Vue `<script>`, the
  Svelte module script). When a component has no such block, each block gets
  its own prelude.
- Each call of a prelude function in a switch has a `typeof` guard, as in
  `typeof __fallow_m_a === "function" && __fallow_m_a("ID")`. A test can
  serialize a source function with `toString` and run it in another realm:
  a `node:vm` context, or a browser page through Playwright `page.evaluate`
  or `addInitScript`. The prelude functions do not exist in that realm.
  Without the guard, the function throws
  `ReferenceError: __fallow_m_a is not defined`, and the test is a baseline
  failure. With the guard, the function runs the original code there. The
  mutants of that function then get no hit and no kill from that realm.
- The framework compiler moves some code out of the block scope, where the
  prelude is not visible or where a switch is not valid. These positions give
  no mutant:
  - a call of `defineProps`, `defineEmits`, `defineExpose`, `defineModel`,
    `defineOptions`, `defineSlots` or `withDefaults` in a Vue
    `<script setup>` block, its arguments, and the destructure defaults of a
    declaration such as `const { size = 1 } = defineProps()`. A
    `removed-call` switch around `defineExpose()` hides the macro from the
    compiler, and the component then throws `defineExpose is not defined`;
  - an expression that holds a Svelte rune call (`$state`, `$derived`,
    `$effect`, `$props`, `$bindable`, `$inspect`, `$host`). The rune
    arguments still get mutants;
  - an exported declaration in the Astro frontmatter.
- Svelte module files (`.svelte.js`, `.svelte.ts`) can use runes. They are one
  program each with the Svelte rune rule.
- The script blocks of a component parse as one program for the side-effect
  and framework-config rules. A sink in a component template does not run when
  the module loads, so it does not prune the top-level mutants.

## Operators

The operators are in two sets. A run that names no operator uses the default
set: the operators of the first table. The opt-in operators of the second
table run only when `--operators`, the `mutation.operators` config key or the
MCP `operators` parameter names them. A list that names an operator replaces
the default set, so `--operators regex-literal,regex-assertion` runs only
those two. To add an opt-in operator to the defaults, name each default
operator and the opt-in operator.

Default set:

| Operator | Change |
|---|---|
| `arithmetic-operator` | `+` and `-` swap, `*` and `/` swap, `%` becomes `*`; in a compound assignment, `+=` and `-=`, `*=` and `/=`, `<<=` and `>>=`, `&=` and `\|=` swap, and `%=` becomes `*=` |
| `relational-operator` | the non-redundant relational subset, and the constant result |
| `logical-operator` | `&&` and `\|\|` swap, `??` becomes `&&`; `&&=` and `\|\|=` swap, `??=` becomes `&&=` |
| `negate-condition` | a branch or loop test becomes `true` and `false`; a boolean operand of `&&` becomes `true`, of `\|\|` becomes `false` |
| `unary-operator` | `!x`, `-x` and `~x` become `x`; `-x` and `+x` swap |
| `update-operator` | `++` and `--` swap |
| `removed-call` | a call statement becomes a no-op |
| `return-value` | `return expr` and the concise body of `x => expr` become `undefined` |
| `boolean-literal` | `true` and `false` swap |
| `string-literal` | a string or template literal becomes empty; an empty one becomes `"fallow"` or `` `fallow` `` |
| `optional-chaining` | `a?.b` becomes `a.b` |
| `regex-literal` | one pattern token changes |
| `removed-chain-link` | `r.m(args)` becomes `r` in a fluent chain of a module-level `const` |

Opt-in set:

| Operator | Change |
|---|---|
| `block-statement` | the statements of a non-empty block or function body are removed |
| `array-literal` | `[x]` becomes `[]`, and `[]` becomes `["fallow"]` |
| `object-literal` | a non-empty object literal becomes `{}` |
| `method-expression` | a method call becomes its pair, or the call is removed |
| `switch-case` | the statements of a `case` or `default` clause are removed |
| `call-expression` | a call in an expression position becomes `void 0` |
| `throw-statement` | `throw new X(...)` becomes `{}` |
| `regex-constructor` | the `regex-literal` and `regex-assertion` changes of a `new RegExp("...")` pattern |
| `regex-assertion` | a lookaround is negated, or `\b` and `\B` swap |

The opt-in operators follow the StrykerJS mutator set. They make many more
mutants, and many of them are equivalent or killed by any test that runs the
code. The rules of each opt-in operator:

- `block-statement`: the directive prologue of a function body stays, so the
  body keeps its strictness. In a constructor, a body or block with a
  `super(...)` call keeps its statements, because each instance creation
  would throw. A class `static` block gives no mutant.
- `block-statement` and `switch-case`: the range gives no mutant when it
  holds a declaration at its own top level (`let`, `const`, `class`,
  `function`, `var` or a TypeScript declaration), or a `var` or function
  declaration at any depth outside a nested function. The instrumentation
  moves the statements into a nested block, so a top-level declaration
  would change its scope, and a hoisted `var` would stay declared while the
  reported mutant removes it.
- `array-literal` and `object-literal`: a destructuring pattern and an
  assignment target give no mutant.
- `method-expression`: `startsWith` and `endsWith`, `every` and `some`,
  `toLowerCase` and `toUpperCase`, `toLocaleLowerCase` and
  `toLocaleUpperCase`, `trimEnd` and `trimStart`, `min` and `max` swap. The
  date setters `setDate` and `setTime`, `setFullYear` and `setMonth`,
  `setHours` and `setMinutes`, `setSeconds` and `setMilliseconds`, and their
  UTC forms swap; `setUTCDate` becomes `setTime`. `charAt`, `filter`,
  `reverse`, `slice`, `sort`, `substr`, `substring` and `trim` are removed
  with their call: `s.trim()` becomes `s`. The callee must be a static
  member without parentheses. A call in an optional chain and a call on
  `super` give no mutant.
- `switch-case`: the clause falls through to the next clause.
- `call-expression`: a call statement, a returned call and a concise arrow
  call body give no mutant, because `removed-call` or `return-value` gives
  the same result. This is also true in a run that does not select those
  two operators: select them to cover such calls. A call in an optional chain, a `super(...)` call, a
  `require(...)` call and a logging call give no mutant. The replacement is
  always `(void 0)`, so it keeps its grouping before an operator: `f() + 1`
  becomes `(void 0) + 1`.
- `throw-statement`: only a `throw` of a `new` expression is removed.
- `regex-constructor`: the pattern must be a string literal, and the flags a
  string literal or absent. The callee is `RegExp`, with or without `new`.
- `regex-assertion`: `\b` in a character class is a backspace, so it gives
  no mutant.

The block, switch case and throw mutants change statements, not
expressions. The instrumentation wraps them in an `if` statement:
`{if (active) {mutated} else {original}}`. The wrapper adds no line, so line
numbers in the instrumented file do not change.

The rules of the operators in detail:

- `relational-operator`: `<`, `<=`, `>` and `>=` get two operator swaps and
  one constant (`a < b` to `<=`, `!=` and `false`). An equality gets its
  negation and both constants: `a === b` becomes `a !== b`, `false` and
  `true`. When `==` or `===` is the whole test of a `while`, `do` or `for`
  loop, it gets no `true` constant, because `true` gives an endless loop.
  `!=` and `!==` keep their `true` constant in a loop test.
- `negate-condition`: the test of an `if` statement, a `? :` expression and a
  loop becomes `true` and `false`. A loop test that is a comparison or a
  logical expression becomes `false` only, because `true` gives an endless
  loop. Only the Vitest adapter stops such a loop before the watchdog (see
  Timeouts). A plain loop test such as `while (x)` keeps both mutants.
- `negate-condition`: `for (init;; update)` gets the test `false`, so the body
  never runs. The mutant inserts `false`, so its `original` text is empty.
- `negate-condition`: an operand of `&&` that is a comparison or a `!x`
  becomes `true`, and such an operand of `||` becomes `false`. The other
  constant gives the same result as a constant for the whole expression, so
  it is not made. For `dir === "." && !path.includes("/")`, the mutants are
  `true && !path.includes("/")` and `dir === "." && true`.
- `negate-condition`: a constant that a test or an operand always has gives
  no mutant. The rules of the zero comparisons on an array length decide
  this: for `items.length >= 0`, the `true` mutant is equivalent.
- `arithmetic-operator`: an expression with a string or template operand
  gives no mutant, for each operator. A compound assignment other than `+=`
  and `-=` with a string or template value gives no mutant either.
- `unary-operator`: `-x` becomes `+x` only when `x` is not a number literal,
  because `+1` gives the same value as the removal mutant `1`.
- `return-value`: an arrow function with an expression body, such as
  `x => x + 1`, gets the body `undefined`. A body that is already `undefined`,
  a `void` expression or a boolean literal gives no mutant. A call body,
  such as `() => save()`, gets this mutant too.
- `string-literal`: a template literal follows the rules of a string. A
  tagged template gives no mutant, because the tag reads the raw parts, but
  its expressions still do. A template in a type position gives no mutant.

Two rules can give the same text at the same span. An example is the
relational constant `true` of `a === b` and the `true` operand mutant of
`a === b && c`. The site keeps one mutant: the first one that the selected
operators allow. With the default operators, the existing rule keeps the
mutant, so the id of an existing mutant does not change.

The `regex-literal` operator gives one mutant for each of these tokens of a
regular expression literal:

- a `^` at the start of an alternative is removed: at the start of the
  pattern, or right after `(`, `(?:` or `|`. A `^` in a lookbehind stays;
- a `$` at the end of an alternative is removed: at the end of the pattern,
  or right before `)` or `|`;
- a `+`, `*` or `?` quantifier is removed, together with its lazy `?`
  (`/\/+$/` to `/\/$/`). The `?` of a group opening such as `(?:` is not a
  quantifier. When the removal would join two tokens into another token,
  an empty group `(?:)` takes the place of the quantifier: `/(a)\1?2/`
  becomes `/(a)\1(?:)2/`, not `/(a)\12/`. The same rule covers an
  incomplete `\x`, `\u`, `\c` or `\k` escape and a literal `{` before
  digits;
- a `{n,m}` or `{n,}` quantifier becomes `{n}`. A `{n}` quantifier and a
  brace that is not a quantifier give no mutant;
- a character class is negated (`[a-z]` to `[^a-z]`, and the reverse). A
  pattern with the `v` flag gives no negation;
- a class escape gets the opposite case (`\d` to `\D`, and the same for `\w`
  and `\s`).

The flags stay the same, and an empty pattern becomes `(?:)`. A mutated
literal that does not parse is dropped, because an invalid literal is a
syntax error for the whole file, also while the mutant is not active. The corpus for
this operator is the JavaScript and TypeScript source files of six public
projects (date-fns, zod, vue core, SvelteKit, reka-ui, runed), without tests.
Each of its 7971 regex mutants compiled with `new RegExp`. Most literals give
one mutant. Locale data with many patterns gives many: date-fns has 6723 regex
mutants, 6555 of them in its locale files.

The `removed-chain-link` operator removes one link of a fluent call chain.
For `export const quantitySchema = z.number().int().positive();` it gives
two mutants: one without `.int()` and one without `.positive()`. A schema of
this type has no literal or operator, so the other operators give no mutant
for it. A link is a call `r.m(args)` that meets all of these conditions:

- the call is in the initializer of a `const` declarator, at any depth
  through arguments, object literals and array literals;
- no function encloses the declaration. A `const` in a function body or an
  arrow function gives no mutant;
- the callee is a static member `.m` without parentheses or a type wrapper;
- the receiver `r` is a call, also behind parentheses or a type wrapper. The
  root call of a chain, such as `z.number()`, is not a link;
- no link of the receiver chain is optional (`?.`), and the call itself is
  not optional. A wrapper around such a receiver ends the short circuit;
- the receiver is not a logging call or a `require` call.

The removed region starts at the dot of the link, so whitespace and comments
before the link stay. A `let` or `var` declaration, an assignment and a call
statement give no mutant. A call statement gets the `removed-call` mutant.

A plan on five public projects measured the volume. The operator adds 72
mutants to the 28150 of zod, 10 to the 26204 of vue core, 4 to the 36215 of
date-fns, 1 to the 13820 of vscode-languageserver-node, and none to the 672
of pathe. The side-effect rules prune 71 of these 87 mutants. Most of the
other mutants remove an array method such as `.filter()`, `.map()` or
`.join()`, or a schema method such as `.transform()`. A link that only adds
metadata, such as `.describe()` in zod, gives an equivalent mutant. Mark it
with an `equivalent` verdict.

A `case` label of a `switch` gives no `string-literal` or `boolean-literal`
mutant when the mutant is equivalent. The rule needs all of these conditions:

- the switch has a `default` clause;
- each label is a string, number, boolean or `null` literal;
- the clause body and the `default` body have the same source text;
- both bodies end in `return`, `throw`, or a `break` or `continue` without
  a label;
- no other label has the original value or the mutated value.

The original label and the mutated label then select the same body for each
input. Mutants in the clause body do not change.

The test of a counted `for` loop gives no `!=` mutant for `relational-operator`,
because that mutant is equivalent. The rule needs all of these conditions:

- the init declares one `let` counter with a non-negative integer literal start;
- the test is `i < N` or `i > N`, where `N` is a non-negative integer literal
  that the start does not pass;
- the update is `i++`, `++i` or `i += 1` for `<`, and `i--`, `--i` or
  `i -= 1` for `>`;
- the body has no write to a variable with the counter name, also in a nested
  function, and no `eval` call.

The counter then takes each integer from the start to the bound, so `i < N` and
`i != N` select the same iterations. The `<=` or `>=` mutant and the constant
mutant of the test do not change. A `var` counter, a bound that is not a
literal, and the reversed form `N > i` keep the `!=` mutant.

A zero comparison on the length of a known array gives no equivalent
`relational-operator` mutant. An array length is a non-negative integer `n`,
so these mutants are equivalent:

| Comparison | Equivalent mutant |
| --- | --- |
| `n > 0` or `0 < n` | the `!=` swap |
| `n <= 0` or `0 >= n` | the `==` swap |
| `n < 0` or `0 > n` | the constant `false` |
| `n >= 0` or `0 <= n` | the constant `true` |

The receiver of `.length` must be an array literal or a name that resolves
to a known array binding. Scope resolution finds the binding of each
reference. A binding is a known array when it is one of these:

- a `const` with an array literal, `Array.from(...)` or `Array.of(...)` value;
- a variable or parameter with the type `T[]`, `readonly T[]`, `Array<T>`,
  `ReadonlyArray<T>` or a tuple type;
- a rest parameter (`...items`) with no assignment to it.

A reference to a global, or to a different binding with the same name, keeps
each mutant. A binding with a redeclaration is not a known array. A `with`
statement, a direct `eval` call, or a local binding of `Array` or
`ReadonlyArray` stops each proof in the file. An object with a `length`
property, an optional chain (`items?.length`) and a computed access
(`items["length"]`) keep each mutant. A type annotation is a type-level
proof: a cast such as `as any` can pass another value.

A conditional lookup in an array before `.filter(Boolean)` gives no `true`
mutant for `negate-condition`. An example is
`["base", align ? alignment[align] : ""].filter(Boolean)`. For a truthy
`align`, the `true` mutant takes the same branch. For a falsy `align`, the
mutant reads `alignment[align]`, which is `undefined`, and `filter(Boolean)`
drops it as it drops `""`. The rule needs all of these conditions:

- the conditional is a direct element of an array literal, and the array
  literal is the receiver of a `.filter(Boolean)` call that is not optional,
  with the global `Boolean`;
- the test is a name with a local binding, and the consequent is `obj[test]`
  or `obj?.[test]` with the same binding as its key;
- the alternate is `""`, an empty template, `null`, the global `undefined`,
  `false`, `0` or `0n`;
- `obj` is a `const` that is not exported, has no redeclaration, and has an
  object literal value, optionally with `as` or `satisfies`;
- the declaration of `obj` ends before the conditional starts, so the
  mutant cannot read `obj` before its declaration runs;
- the object literal has only plain properties with name, string or number
  keys, and no spread, getter, setter, method, computed key or `__proto__`
  key;
- no key is `undefined`, `null`, `false`, `0`, `NaN` or the empty string,
  because a falsy key converts to one of these keys;
- each use of `obj` is a member read, not an assignment target, a `delete`
  operand or a callee.

The `false` mutant does not change. A template literal, a `.join` without
`.filter(Boolean)`, a `.filter((v) => v)` callback, a different key, a truthy
alternate and a local `Boolean` binding keep the `true` mutant. A `with`
statement or a direct `eval` call stops each proof in the file. A change to
`Object.prototype` or `Array.prototype.filter` at run time can break the
proof. A call from the value of `obj` to a hoisted function that holds the
conditional can read `obj` before its declaration runs. The rule accepts these
risks.

A `console.*` or `debug(...)` call gives no mutant, also for each expression
in its arguments. An `if` statement whose only effect is such a call gives no
mutant either: a changed test can only change which message is logged. The
rule needs all of these conditions:

- the test is pure: names, `this`, literals, templates, member reads, and
  unary, binary, logical and `? :` operators. A call, an optional call, `new`,
  an assignment, an update, `delete`, `await`, `yield`, a tagged template, a
  comma expression or a function keeps each mutant;
- the consequent is a logging call statement, or a non-empty block of them;
- the alternate is absent, a logging call statement, a non-empty block of
  them, or an `else if` that meets the same conditions;
- each argument of each logging call is pure, with no spread;
- the callee is the global `console` or `debug`. A computed key, as in
  `console[name]`, must be pure. A file that declares or imports a binding
  named `console` or `debug` keeps each condition that calls that name.

A branch with another statement, a `return` or a `throw`, an empty branch
and a nested `if` keep each mutant. The rule reads only `if` statements, so a
logging call in a `? :` expression or after `&&` keeps the mutants of its
test. A test can still have an effect through a getter, a `valueOf` method,
a Proxy trap that `in` reaches, or a `Symbol.hasInstance` method that
`instanceof` calls. The rule accepts this risk.

No absolute-value operator (`x` to `-x`) is planned. Without types, it applies
to each operand of an arithmetic or relational expression. The same corpus has
114119 mutants, and the operator adds 6473 mutants (5.7 %), 24 % of them on
numeric literals. Each one is on an expression that already has an arithmetic
or relational mutant, so it adds little test-strength signal for its run time.

## Safety boundary

- Only `fallow mutation` without `--plan` runs code. Project config cannot
  start a run. The MCP server exposes the plan and inspect only.
- The run refuses a dirty git worktree unless `--allow-dirty` is set. After the
  run, it compares the worktree with the start state and reports each changed
  path.
- The check covers tracked and untracked paths. For ignored files, it covers
  the files that `git status --ignored=matching` lists at the start, up to
  10000 files in path order: a write to one of them or a delete is a change.
  Git lists an ignored directory such as `node_modules/` or `coverage/` as one
  entry, so the check does not see the files below it. A new ignored file is
  not a change, because test runs make such files (coverage, logs).
- The check leaves out `<cache>/mutation` and the output files of the run. For
  ignored files it also leaves out the whole cache directory, because the
  analysis writes its own cache files there. The run refuses a cache directory
  that is the project root or a directory above it, or that holds a planned
  source or test file (`FALLOW_MUTATION_UNSAFE_CACHE_DIR`, exit code `2`). Such
  a cache directory would hide the writes of the tests from the check.
- Mutants in a function whose own body calls a file system, process, network or
  database sink are pruned (`side-effect-sink`). A mutant in top-level code is
  pruned when the module calls a sink at the top level. The pruning does not
  follow calls: a mutant in a function that calls a helper, which then calls a
  sink, still runs. The sinks come from the security catalogue, which matches
  injection shapes only. A database write call with safe arguments, such as
  `db.insert(table)`, `db.insertInto("person")` or `prisma.user.create(...)`, is
  a sink too. The call must be a member call or a tagged template with a write
  method name (`insert`, `insertOne`, `update`, `delete`, `del`, `create`,
  `upsert`, `save`, `$transaction`, `$executeRaw` and similar). A call whose
  first argument is a path, such as the route `app.delete("/items/:id",
  handler)`, does not count. A prepared statement run (`run`, `all`, `get`,
  `iterate`, `each`) counts when the receiver is a `.prepare(...)` call, such as
  `db.prepare(INSERT_SQL).run(value)`, or a name or a property that the same
  module sets to one. A declarator, an assignment, a class field (also a
  `static` or a `#private` field) and an object literal property each set one.
  Examples are `insertStmt.run(value)` after `const insertStmt =
  db.prepare("INSERT ...")`, `this.insert.run(value)` after the class field
  `insert = db.prepare("INSERT ...")`, and `statements.insert.run(value)` after
  `const statements = { insert: db.prepare("INSERT ...") }`. A property matches
  by its name only, whatever object holds it. A prepared statement whose static
  SQL text starts with `select` is a read and does not count. The check matches
  names in the same module only, so a statement that a function gets as a
  parameter, or that the module imports, does not count. In a module that loads a database client package
  itself, every `run` and `exec` call counts too, except `exec` on a regular
  expression, such as `/(\d+)/.exec(text)`. A computed member with a string key,
  such as the callee `db["insert"]`, counts too. A tagged template, or a call
  whose first argument is a string or a template, is also a sink when its first
  SQL text starts with a write verb (`insert`, `update`, `delete`, `merge`,
  `replace`, `upsert`, `truncate`, `create`, `drop`, `alter`): ``sql`insert into
  ...` ``, `pool.query("DELETE ...")`, `lite.prepare("INSERT ...").run(value)`.
  The module must load a database client package (`drizzle-orm`,
  `@prisma/client`, `kysely`, `knex`, `pg`, `mongodb` and similar), or load a
  project module that loads one. An ES import, a `require()` call and an
  `import()` call with a literal source each count. A type-only import counts
  too, because a function that takes the client as a parameter often imports
  only its type. The check follows one import only. A client that a module
  reaches through two or more project imports does not count, and a package that
  is not in the list does not count. In a module with database reach,
  `map.delete(key)` and `log.info("update done")` are also sinks: the plan does
  not know the type of the receiver or the purpose of the text.
  `--allow-side-effects` keeps the pruned mutants. This override is a flag only:
  the config rejects `allowSideEffects`, so a project config cannot widen what a
  run executes.
- Mutants in framework config are pruned (`framework-config`). Framework
  config is an export that an active plugin marks as used, for example
  `export const revalidate = 60` in a Next.js page. It is also the default
  export of a tool config file, which is a file that the `config_patterns`
  or the `always_used` globs of an active plugin match (`vite.config.ts`,
  `eslint.config.js`, `.eslintrc.js`, and the VitePress config file in
  `.vitepress`). Some plugins list their config file only as always used. Only a glob whose file name holds `config`, `.conf.` or an
  `rc` suffix counts, because some plugins also list application files as
  config, for example `codegen*.ts`, `src/module.ts` or `app/routes.ts`.
  The config and always-used globs of external plugins count too. In a tool config file, a
  top-level `module.exports = ...` statement counts as the default export. A mutant inside a function in such an export is not
  pruned, so an inline Vite plugin keeps its mutants. The rule lives in
  `framework_config_exports` (`crates/engine/src/mutation/mod.rs`) and
  `framework_regions` (`crates/engine/src/mutation/plan.rs`).
- Fallow does not write to project files. The instrumented files live in the
  Fallow cache. The report lists any path that the tests changed.
- The companions run in their own process group on Unix and in a Job Object on
  Windows. Fallow ends that group when a run ends, and when it receives
  SIGINT, SIGTERM, SIGHUP or SIGQUIT. When Fallow starts with SIGHUP ignored,
  as under `nohup`, it keeps SIGHUP ignored. The release binary aborts on a
  panic, and a panic hook ends the group first. A companion whose stdin closes
  without a `shutdown` request kills its own process group and exits, so it
  does not outlive a Fallow process that ended in another way.
- Each companion gets its own temp directory, named `fallow-mt-<id>` in the
  system temp directory. Fallow sets `TMPDIR`, `TMP` and `TEMP` of the
  companion to it, so the runner and the test processes write their temp
  files there. Fallow deletes the directory when the companion ends, also
  after a kill, because a killed test run cannot run its teardown. On SIGINT,
  SIGTERM, SIGHUP or SIGQUIT, Fallow ends the companions and then deletes
  their directories. A lock file next to the directory marks a live Fallow
  process. The next run deletes a directory whose lock is free, for example
  after a crash, a `SIGKILL`, or a file that Fallow could not delete. On Unix, only the owner can read the
  directory, and Fallow deletes only a directory whose lock file is a regular
  file of the same user.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The plan, the run or inspect finished. Survivors do not fail a run. |
| `1` | `--min-score` or `mutation.minScore` is set, and the score is lower. |
| `2` | Invalid input or an execution error: a dirty worktree without `--allow-dirty`, a cache directory that hides project files, a missing runtime or runner, a companion file with a wrong digest, an invalid verdict document, `--typecheck` without a type-aware companion, a run in which baseline failures leave each mutant without a test result, a run path that fails the self-check (see Run-path self-check), a `--resume` journal of other inputs (`FALLOW_MUTATION_RESUME_MISMATCH`), a `--kill-matrix-for` run in which no target selects a test of the dry run (`FALLOW_MUTATION_TARGET_UNMATCHED`), a `--kill-matrix-for` test ID target that names a duplicate test ID (`FALLOW_MUTATION_TARGET_DUPLICATE_TEST_ID`), a `--recheck` with a changed source file, an unknown mutant id or no open task (see Recheck), a `--compare` with a saved report or a run that is not complete (`FALLOW_MUTATION_COMPARE_INCOMPLETE`, see Compare), a replay of a stale mutant, a runner with no replay, or a replay companion that fails (see Replay). |
| `128 + signal` | SIGINT, SIGTERM, SIGHUP or SIGQUIT stopped the run (`130` for SIGINT). A run that a signal stops during the mutant runs prints the report of the finished mutants, with the completion `partial`. A run that a signal stops earlier prints no report and no error. |

The flag wins over the config key. The minimum applies to the score, in
percent. A run with a minimum writes the result into the report as
`gate_outcomes["mutation-min-score"]`, and the exit code `1` reads that
entry (see [Gate](#gate)). A run in which no mutant has a test result has no score. It exits
`0` when it has no baseline failure, also when each mutant is `no_coverage`.

A run in which baseline failures leave each mutant without a test result
exits `2` with `FALLOW_MUTATION_BASELINE_FAILED`, the same code as a dry run
that fails outside each test file. The failed tests cannot detect a mutant,
so the run measured nothing.

- A mutant has a test result when it is `killed`, `timeout`, `survived` or
  `flaky`. A run with only `runtime_error`, `compile_error` and
  `no_coverage` mutants and a baseline failure exits `2`.
- A `pending` mutant keeps the exit code at `0`. The time budget,
  `--max-mutants` or `--mutants-per-line` stopped that run, so the baseline
  failure is not the cause. The completion is `partial`.
- The error names the first baseline failure and the first line of its
  message.
- The run always saves the full report to
  `<cache dir>/mutation/last-report.json`, and to the `-o` file when `-o`
  is given. The `help` field of the error gives these paths.
- With `--format json`, stdout gets the error envelope. When the run cannot
  save `last-report.json` and has no `-o`, stdout gets the report, so the
  data stays available, and the error goes to stderr.
- The human output prints the report and then the error on stderr.
- This check uses the statuses before `--verdicts`. An `equivalent`
  verdict for a survivor does not change the exit code to `2`. The
  `--min-score` check uses the statuses after the verdicts.
- `fallow_api::mutation::baseline_blocked_run_error` gives the same error
  to a caller of the programmatic API. MCP does not run mutants.

## Mutant limit

`--max-mutants N` limits the run to the covered mutants. The plan applies
the limit in rank order before the dry run, so the plan cannot know the
coverage. The scheduler corrects this after the dry run:

- A planned mutant that a dry-run test covers keeps its place.
- A planned mutant that no dry-run test covers is `no_coverage` and does not
  use the limit.
- Each free place goes to the next covered `pending` mutant in rank order.
  That mutant runs and gets a normal status.
- `min(N, covered mutants)` mutants are inside the limit. Each of them gets
  a job or a result from the cache, so a run with the cache can execute
  fewer mutants. The covered mutants after the filled limit stay `pending`.
- A statically `no_coverage` mutant that the dry run hits (a static map
  miss) runs outside the limit.
- The completion has `mutant-limit` only when a covered mutant stays
  `pending`. When the limit leaves only `no_coverage` mutants, the limit cut
  nothing.

`--plan` runs no dry run, so its `planned` and `pending` dispositions are a
view before the coverage is known. A run can execute a mutant that the plan
shows as `pending`.

## Runner version check before a run

A run gets the runner version from the `ready` response of the companion.
It stops with `FALLOW_MUTATION_RUNNER_UNSUPPORTED` when the version is
outside the range of `crates/api/mutation-runner-protocol.json`. `--plan` and
`fallow doctor` start no companion. They read the `package.json` of the
installed Vitest (`crates/api/src/mutation/runner_version.rs`). The lookup
follows the Node resolution of the companion: the `node_modules` directory of
the project root, then the `node_modules` directory of each ancestor. It runs
no project code.

- The `mutation-plan` JSON has a `runner` object: `name`, `package`,
  `installed_version`, `supported_range`, `supported`, `message` and `help`.
  `supported` is false for a version outside the range and for a Vitest that
  is not installed. It is `null` for `node-test` and `bun-test`, because their
  range applies to the runtime that the run selects.
- The human plan prints a `Runner:` line and a `hint:` line only when the run
  starts a runner: the plan has a test file and a mutant.
- The plan exits 0 also when `supported` is false. A plan is not a run, and
  the exit code table promises 0 for a finished plan.
- `fallow doctor` has the advisory `mutation-runner` check, with the same
  message and help. It is skipped when `mutation.runner` does not select
  Vitest, the root `package.json` declares no Vitest, and the root
  `node_modules` holds no Vitest. The root install counts because a
  workspace package can declare a Vitest that the package manager installs
  at the root.

## Function scope

The changed-code scope mutates the added lines of the diff only. A change to
a parameter or a signature line holds no mutant, so the default scope can
select nothing for it. `--function-scope` (config key
`mutation.functionScope`) widens the scope:

- Each added line selects the whole function that holds it. A parameter line,
  a signature line and a body line all select the same function.
- The function is the innermost function that holds the first non-blank
  byte of the line. When no function holds it, the largest function that
  starts on the line counts, for example `export function f() {`. A line with
  no such function, for example a top-level `const`, selects its own mutants
  only.
- A mutant of a nested function is part of the outer function.
- The plan reads the function spans from the syntax tree. The `--operators`
  filter does not hide a function that holds no emitted mutant.
- The ranking still puts the mutants on added lines first, because the
  `in_diff` fact of a mutant is not widened.
- `--all` and positional paths do not use the option, and the flag is an
  error with them. `--recheck` does not accept the flag.

## Mutants per line

`--mutants-per-line N` (config key `mutation.mutantsPerLine`) keeps at most
`N` planned mutants on each source line. The flag wins over the config key.
`N` must be at least 1.

- The plan makes the choice, before the dry run and before `--max-mutants`.
  The result cache does not change it.
- A line keeps its mutants in this operator order: `relational-operator`,
  `negate-condition`, `arithmetic-operator`, `logical-operator`,
  `unary-operator`, `update-operator`, `return-value`, `removed-call`,
  `optional-chaining`, `boolean-literal`, `string-literal`, `regex-literal`
  and `removed-chain-link`, then the opt-in operators `method-expression`,
  `call-expression`, `throw-statement`, `switch-case`, `regex-assertion`,
  `regex-constructor`, `array-literal`, `object-literal` and
  `block-statement`.
- Mutants of the same operator on one line keep the order of a SHA-256 hash
  of the mutant id. A second run on the same code keeps the same mutants.
- Each mutant that the limit leaves out is `pending` with
  `pending_reason: "sampled-out"`. The run never executes it. `--plan` shows
  the same mutant as `pending` with the same `pending_reason`.
- A left-out mutant that no dry-run test covers is `no_coverage`, so the
  coverage does not depend on the limit. The coverage counts the other
  left-out mutants as covered.
- `summary.sampled_out` and `plan.counts.sampled_out` give the number of
  left-out mutants. They are part of the `pending` count. A left-out mutant
  that becomes `no_coverage` is in `plan.counts.sampled_out`, but not in
  `summary.sampled_out`. To count the `pending` mutants that the mutant limit
  stopped, subtract `sampled_out` from `pending`.
- The completion is `partial` with the reason `sampled-out` when a left-out
  mutant stays `pending`.
- `--recheck` does not accept the flag, and a run of selected mutant ids never
  samples, also through the API.

## Score and coverage

- `summary.score` covers the executed mutants only:
  `score = (killed + timeout) / (killed + timeout + survived)`.
- `summary.coverage` reports the unreached mutants apart:
  `coverage = covered / (covered + no_coverage)` over the whole plan.
  `covered` counts each executed mutant (`killed`, `timeout`, `survived`,
  `runtime_error`, `flaky`, `compile_error`) with a covering test, and each
  `pending` mutant
  that a passing dry-run test covers. The dry run loads each mutant of the
  plan, also a mutant after the `--max-mutants` limit. A test covers a
  mutant when it hits the mutant, or when its file loads the module of a
  mutant that the module load hits. A static import path alone is not a
  cover. When a module loads at collection time in one test file and inside
  a test of another file (for example through a dynamic `import()`), the
  mutant is static and runs the tests of both files. A `pending` mutant that no dry-run test covers is `no_coverage`.
  A run that `--max-mutants` caps therefore reports the coverage and the
  `no_coverage` mutants of the full run.
- `no_coverage` never enters the score. A capped run of 200
  mutants in a project with 20000 unreached mutants still gets a score that
  describes the 200 tested mutants.
- `pending`, `equivalent`, `flaky`, `runtime_error`, `pruned` and
  `compile_error` mutants never count in the score. `equivalent` and `pruned`
  mutants never count in the coverage.
- Both values are fractions from 0 to 1 in JSON. `--min-score` and
  `mutation.minScore` use percent.
- Only `completion.status: "complete"` makes an empty survivor list
  conclusive.
- The runner can leave out a static test file, for example a test that only
  a second Vitest config includes. The tests of that file never run. The
  `errors` entry `test-files-not-discovered` names each such file, and the
  completion is `partial` (`test-files-not-discovered`). A survivor or a
  `no_coverage` mutant that such a file reaches is then not proof of a
  test gap.
- Two tests of one file can have the same full name, for example
  `test.each` with a title that has no placeholder. The runner keeps the
  first test ID as it is and numbers the next ones in report order
  (`name (2)`). A change in the report order moves the numbers, so the kills
  and survivors of these IDs are not reliable. Only tests that ran count: a
  skipped or todo twin gives no evidence, so it makes no duplicate. A file
  that an unhandled error excludes reports no duplicate. The `errors` entry
  `duplicate-test-ids` names the IDs, and the human output shows it as a
  warning. The run does not stop, and the verdicts of the other tests stay
  valid. Give each test a unique title to remove the warning.
- A test file can leave a test out of the run, for example with `it.skip`.
  Such a test runs no assertion, so it cannot kill a mutant. The run warns
  about these tests in each runner test file that the diff of the
  changed-code scope changes. A file counts when the diff adds, changes or
  removes a line in it, also when it reaches no changed source. A deleted
  file has no test. A scan of the syntax tree finds the tests, so a change
  to the tests alone starts no runner and no companion. A run without a
  diff scope (`--all` or paths) gives no such warning, and `--plan` does not
  print it.
- The scan recognizes `it`, `test`, `describe` and `suite` with `.skip` or
  `.todo`, also after `.concurrent`, `.sequential` or `.shuffle`. It
  recognizes `xit`, `xtest` and `xdescribe`. It recognizes the `node:test`
  options `{ skip: true }` and `{ todo: true }`, and a non-empty string
  value of either key. The last write to a key wins, as in an object
  literal. A condition that the run decides, such as `it.skipIf(flag)`,
  `{ skip: process.env.CI }`, or a spread or a computed key after the last
  literal write, is not a skipped test. A spread before the last literal
  write does not matter.
  A call such as `t.skip()` inside a test is not recognized.
- The `errors` entry `skipped-tests` names up to ten of these tests with
  file, line and name. The name is present when the first argument is a
  string literal. The human output shows the entry as a warning. The run
  does not stop, and the completion and the exit code do not change. Run
  each test or remove it to remove the warning.
- The `node-test` and `bun-test` adapters find test files with the default
  patterns of the runner. These glob patterns do not go into a dot
  directory, such as `.github/`. Fallow therefore sends the static test files
  in `init.options.extra_test_files`. The adapter adds each file that exists,
  is not in `node_modules` or `.git`, and matches a default pattern when its
  directory names have no leading dot. A static test file that the adapter
  does not add stays a `test-files-not-discovered` error. The Vitest adapter
  does not get this list, because Vitest has its own `include` setting.
- A `partial` completion does not change the exit code. The run exits `0`,
  or `1` when the score is under `--min-score`, or `2` when baseline
  failures leave each mutant without a test result (see Exit codes).

## Redundant test candidates

`--kill-matrix` records which tests kill each mutant. It runs all covering tests with bail disabled, which can increase execution time.

```bash
fallow mutation --all --kill-matrix --format json --quiet
```

The report separates matrix evidence from mutation completion. Normal mode reports unavailable evidence with the reason `kill-matrix-disabled`.
With `--kill-matrix`, incomplete matrix evidence produces no redundant test candidates. With `--kill-matrix-for`, only the tests of complete targets can be candidates. Check the status and reasons before you review tests.

`kills` and `unique_kills` are arrays of mutant IDs.
A unique kill belongs to one test only. Individual subsumers kill every mutant that the candidate kills.
A candidate can also have collective coverage: other tests together kill all its mutants without one individual subsumer.

Review each candidate separately. Equal kill sets can make both tests candidates, so removing both can remove the remaining protection.
The evidence describes observed mutants only. A candidate does not prove that you can safely remove the test.

The report lists tests with runtime coverage and no observed kills separately. Zero kills do not prove that a test has no assertions.
Survivors and timeouts do not count as kills. Baseline-failing tests do not qualify as candidates or subsumers.

Sole-file notes describe observed runtime coverage of instrumented files. Public-export notes use static test-file reach, which does not prove per-test export coverage.

The `mutation` and `mutation-inspect` JSON envelopes use schema version `"2"`. The `mutation-plan` and `mutation-tasks` envelopes and the verdict document use schema version `"1"`.

The `kill_matrix` object has these fields:

| Field | Meaning |
|---|---|
| `status` | `complete`, `incomplete`, or `unavailable`, separate from mutation completion. |
| `reasons` | Evidence limitations, such as `kill-matrix-disabled`, `incomplete-mutant-results`, `baseline-failure`, `partial-mutation-run`, `missing-test-file-metadata`, or `unmatched-target`. |
| `tests` | Entries with `test_id`, `file`, `kills`, `unique_kills`, `subsuming_tests`, and `notes`. In target mode, also `target_only_kills`. |
| `candidates` | Test IDs whose observed kill sets other tests cover. |
| `coverage_without_kills` | Test IDs with runtime coverage and no observed kills. |
| `targets` | Only in target mode: each `--kill-matrix-for` target with the `test_ids` that it selects, and the `status` (`complete` or `incomplete`) and `reasons` of its own evidence. A target that selects no test is `incomplete` with the reason `unmatched-target`. |

Each mutant and attempt records `killing_tests` and `kill_matrix_complete`. The existing `killed_by` field keeps the first observed killer. Outside matrix mode, `killing_tests` is empty.

The result cache and outcome journal distinguish matrix mode from normal mode. Normal-mode results cannot supply complete matrix evidence.

Matrix mode does not add the other tests of the covering test files to the retry of a survivor. A survivor that normal mode kills in that retry can stay `survived` in matrix mode, so the two modes can give different scores.

### Named target tests

`--kill-matrix-for <target>` checks whether named tests are redundant. It does
not run the full kill matrix. Repeat the flag for each target. The flag and
`--kill-matrix` are exclusive.

```bash
fallow mutation src/math.ts --kill-matrix-for test/math.test.ts --format json --quiet
fallow mutation src/math.ts \
  --kill-matrix-for "test/math.test.ts::adds" \
  --kill-matrix-for "test/math.test.ts::adds again" --format json --quiet
```

- A target is a test file, which selects each test in the file, or a test ID
  (`file::name`), the same string as `kill_matrix.tests[].test_id`. Fallow
  removes a leading `./`, changes `\` to `/` in the file part, and makes an
  absolute path inside the root relative.
- The dry run runs the test files that reach the planned code, so the scope
  must include code that the target tests load. A target that selects no
  test of the dry run does not stop the run. Its entry in
  `kill_matrix.targets` has empty `test_ids`, the status `incomplete`, and
  the reason `unmatched-target`. One more reason tells why:

  | Reason | Meaning |
  |---|---|
  | `not-a-test-file` | The runner does not list the file of the target as a test file. |
  | `no-planned-code` | The runner lists the file, but the file reaches no planned code, so the dry run does not run it. A test that runs the code only in a child process (for example with `spawnSync`) is an example. |

  With only `unmatched-target`, the dry run ran the file, but no test of the
  file has the target ID, or each test of the file has a duplicate ID.
- When no target selects a test, the run stops with exit `2`
  (`FALLOW_MUTATION_TARGET_UNMATCHED`). A run with one target stops in the
  same way. The message names each target and its reason. When the dry run
  runs the file of no target, the run stops before the dry run. This is also
  true when no test file reaches the planned code.
- A test ID target that names a duplicate test ID stops the run with exit
  `2` (`FALLOW_MUTATION_TARGET_DUPLICATE_TEST_ID`). The message names each
  such ID. A test file target drops the duplicate IDs and keeps its other
  tests. The `duplicate-test-ids` error above names the dropped IDs. A file
  whose tests are all duplicates is unmatched, as a file with no test is.
- A mutant that no target test covers in the dry run does not run. It is
  `pruned` with `prune_reason: "no-target-coverage"`. It does not count in
  the score, the coverage, or the `--max-mutants` limit. The score therefore
  describes only the mutants that the targets cover.
- Fallow runs each other mutant against its covering target tests, with bail
  disabled. After a kill or a survivor, it runs the other covering tests
  with bail enabled. The first other test that fails stops that run. A
  timeout or an error of the target run ends the attempt.
- The mutant status has the same meaning as in normal mode: `killed` when any
  covering test kills the mutant.
- `killing_tests` of a mutant holds each target test that kills it, and the
  first other test that kills it. `kill_matrix_complete` is true when the
  target run gave an exact result for each target test and, after a target
  kill, the other run gave a final result. Each run holds only a part of the
  covering tests. A test can fail in such a run because it depends on the
  order of the tests. After a target kill, Fallow runs the target tests with
  no active mutant. When another test also kills the mutant, Fallow runs
  those other tests with no active mutant too. When a control run does not
  pass, the evidence of the mutant is incomplete.

Each entry of `kill_matrix.targets` has its own `status` and `reasons`. A
target is `incomplete` with the reason `incomplete-mutant-results` when one
of its tests covers a mutant with incomplete evidence. Only the tests of a
complete target can be candidates. One incomplete mutant therefore removes the
candidates of the targets that cover it, not of all targets. The top-level
`status` stays `incomplete` when any evidence is incomplete, so `candidates`
can be nonempty while `status` is `incomplete`.

A baseline failure (a test or a test file that fails with no active mutant)
gives a target the reason `baseline-failure` only when the failure can affect
the target:

- the failing test is a test of the target;
- the failing test, or a file error, is in the test file of a test of the
  target;
- the failing test failed a control run or the dry run of a restarted
  companion. It covers or kills a mutant that a test of the target covers or
  kills, or that mutant ran without it (`mutant_ids` of the failure).

A failure in the first dry run drops the coverage of the failing test. For
such a failure, only the test file can connect it to a target. Shared state
across test files, for example a global or a database, is outside this rule.
The human report names the failures of each target under its line.

When baseline failures are the only cause of a partial run, only the targets
with `baseline-failure` also get `partial-mutation-run`. Each other cause of
a partial run, for example the time budget, gives each target
`partial-mutation-run`. `missing-test-file-metadata` also applies to each
target.

A failing test gives no kill evidence. A passing test in the test file of a
baseline failure can share state with the failure. Neither test makes the kill
of a target test shared. This is also true for a test of another target. A
kill that only such tests also make stays in `unique_kills`, and the target
test is not a candidate. The `target_only_kills` of the target test do not
name such a test.

Each target test with coverage gets a row in `kill_matrix.tests`. Other tests
get no row.

| Field | Meaning in target mode |
|---|---|
| `kills` | The mutants that the test kills. |
| `unique_kills` | The kills that no other test kills, target or not. |
| `target_only_kills` | The kills that other target tests also kill and no other test kills. Each entry has `mutant_id` and the other `targets` that kill it. |
| `subsuming_tests` | Always empty. The note `subsumers-not-computed` says so, because bail hides the other killers. |

A target with kills and no `unique_kills` is a candidate. When the candidate
has `target_only_kills`, it stays redundant only while the suite keeps, for
each entry, one of the `targets` of that entry. A change that adds several
equal tests makes each of them a candidate. Keep one test of each shared
kill.

The evidence covers the planned mutants only. A test can have a unique kill
outside the scope of the run, for example outside the changed code. Use a
scope that holds each source file that the targets test.

The cache key and the journal header include the targets. A normal or full
matrix result never supplies target evidence, and `--resume` with other
targets is a mismatch. The order of the targets does not matter.

## Timeouts

The dry run executes the instrumented code, which is slower than the
original code. The adapters therefore multiply the test timeout of a dry run
by `DRY_RUN_TIMEOUT_FACTOR` (3, in `adapters/common.mjs`):

- The Vitest adapter multiplies the `testTimeout` and `hookTimeout` of each
  project. A value of 0 stays 0, because Vitest reads 0 as no limit.
- The `bun test` adapter passes `--timeout` with 3 times the Bun default of
  5 seconds. Bun reads no default timeout from `bunfig.toml`.
- The `node:test` adapter sets no run timeout in a dry run, so only the
  timeouts in the test files apply.

A timeout that a test file sets on one test stays as it is in each adapter.

Each mutant run has two time limits:

- `timeout_ms` is `1.5` times the dry-run time of the covering tests, plus
  5 seconds. The runner gets it as the test timeout. The watchdog of the
  companion waits `timeout_ms`, the setup time of the covering test files,
  and 2 seconds. When the watchdog fires, Fallow stops the companion, starts
  a new one, and dry-runs again.
- `loop_budget_ms` is `1.5` times the same dry-run time, plus 500 ms. It is
  part of the `run` request. Only the Vitest adapter checks it. The
  `node:test` and `bun test` adapters ignore it.

A synchronous endless loop blocks the test worker, so the test timeout of
the runner cannot stop it. In a mutant run, each instrumented site calls the
mutant switch on each evaluation. The switch counts these calls inside a
test. At every 128th call, it compares the CPU time of the tests of the file
so far with `loop_budget_ms`. The CPU time is the time that the test thread
works between `beforeEach` and `afterEach` of the tests of the run. When the
time is more, the switch throws an error, and the adapter reports the test
as a `timeout` with `loop_timeout: true` in the `run_result`. The companion
stays alive, so the next mutant needs no new companion and no new dry run.

- The switch reads the CPU time of the test thread
  (`process.threadCpuUsage`). A test that waits uses no CPU time, load from
  other processes on the host does not count, and fake timers in a test do
  not move the clock. A runtime without `process.threadCpuUsage` (Node
  before 22.19 and 23.9, or browser mode) uses the wall time. Its loop
  timeout has no `loop_timeout` flag, so it always gets the retry.
- A loop through code with no instrumented site, such as test code or a
  dependency, still waits for the watchdog. So does a loop outside a test:
  at module scope (each static mutant), or in a `beforeAll` or `afterAll`
  hook.
- A `catch` block of the project can catch the error. The loop then
  continues, the switch throws again after 128 more calls, and an
  endless loop that catches each error waits for the watchdog.
- The retry of a timeout doubles both limits. The retry of a survivor
  computes both limits from the time of its retry tests.
- A loop timeout on the thread CPU clock gets no retry when the covering
  tests take more than 0 ms and at most 200 ms in the dry run, so when
  `loop_budget_ms` is more than 500 ms and at most 800 ms. The budget is
  then at least four times the CPU time that the tests need, and host load
  does not use it. Each other timeout gets the retry.

A mutant that makes a test do much more work can also pass
`loop_budget_ms`. Its status is then `timeout` in place of `killed` or
`survived`. A `survived` mutant that becomes `timeout` raises the score.

## Retries

A `survived` or `timeout` result gets one retry in a new companion. Host
load can make a test time out, so a single timeout is not proof that the
tests detect the mutant. A loop timeout of short covering tests is the
exception (see [Timeouts](#timeouts)).

| First result | Retry result | Final status |
|---|---|---|
| `survived` | `survived` | `survived` |
| `survived` | `timeout` | `survived` |
| `timeout` | `timeout` | `timeout` |
| `timeout` | `killed` | `killed` |
| `survived` | `killed` by a test that the first run did not execute | `killed` |
| any | `runtime_error` | `runtime_error` |
| any | other result | `flaky` |

- A `survived` result with a `timeout` retry stays `survived`. The first
  run finished all tests with the mutant active. Host load alone can make
  the retry time out, so the timeout is no proof that the tests detect the
  mutant. A `killed` retry of a survivor gives `flaky` when a test of the
  first run kills it.
- The retry of a survivor runs each passing test of the files of its
  covering tests, not only the covering tests (issue #3670). The per-test
  hits of the dry run name only the tests that execute the mutated code. A
  test can read a value that an earlier test of its file computed and
  cached, so the dry run gives that test no hit. The first run stays
  narrow, because most mutants are killed. A static mutant and a runner
  without `test_filter` already run whole files, so their retry runs the
  same tests.
- A kill in the retry by a test that the first run did not execute is no
  disagreement with the first run, so the status is `killed`. A test can
  fail in a run of part of its file because it depends on the order of the
  tests. Thus the scheduler keeps this kill only when the retry tests pass
  in a control run with no active mutant (one control run for each test
  set, the same memo as for a kill with no executed test). When the control
  run fails, the first run decides: the mutant stays `survived`, and the
  failed tests are baseline failures.
- The retry of a survivor stops at the first failing test, and that test
  decides the status. The companion runs the tests of each file in file
  order, so a new test can fail before a test of the first run. The result
  is then `killed`, also when the test of the first run would fail too.
  Before, such a pair of runs was `flaky`. The control run still guards the
  kill, and the mutant fails a test in both cases.
- `covering_tests` of a retried survivor stays the dry-run hits of the
  first run. It names the tests that reached the mutant and need an
  assertion. A kill by a test of the retry can thus name a `killed_by` test
  that is not in `covering_tests`.
- The retry of a `timeout` waits twice as long, and it doubles
  `loop_budget_ms`. Host load often lasts into the retry, and a real
  infinite loop still times out.
- A `killed` result gets no retry. In repeated runs under host load, no
  kill became a survivor, and a retry of each kill doubles the run time.
- A retry starts only before the `--time-budget` deadline. A mutant that
  gets no retry keeps its first status, and the completion is `partial`
  (`time-budget`).
- With `--time-budget`, the first pass starts the mutants with the lowest
  estimated test time first. The estimate is the sum of the dry-run times of
  the tests that cover the mutant. Thus the budget settles more mutants.
  Static mutants stay last. Without `--time-budget`, the jobs keep the plan
  order.
- With `--time-budget`, the first pass keeps time free for the retries of
  the survivors and timeouts that it found. Before each new mutant run, it
  adds these parts to the current time: the estimated retry time of each
  suspect (the measured run time, divided by `--concurrency` or by the
  suspect count when that count is smaller), the
  setup and dry-run time of the retry companions, and
  twice the mean run time. When the sum reaches the deadline, the next
  mutants stay `pending`. Thus a budgeted run confirms its survivors, the
  result cache stores them, and a repeat run with the same budget continues
  with the next mutants. A survivor that gets no retry is not stored, so a
  repeat run does it again. The reserve uses measured run times, so the
  first run of each worker starts without a reserve. A budget that is
  shorter than one wave of runs can still end before any retry.
  For a survivor, the reserve adds to its measured run time the dry-run
  time that the other tests of its files add to its retry. A timeout
  reserves twice its run time. The control run that confirms a kill by a
  new test is not in the reserve. When no time is left for it, the retry
  gets no result and the mutant keeps its first status.
- A run that started before the deadline continues until its watchdog
  fires, so the command can end after the deadline. For a mutant run, the
  overrun is at most `timeout_ms` plus the setup time plus 2 seconds. For
  a timeout retry, it is at most twice `timeout_ms` and the setup time,
  plus 2 seconds.
- `retried: true` on a mutant result marks a status from a first run and
  a retry. A `survived` or `timeout` mutant without the field got no retry.
- A retried mutant result has an `attempts` array: the first run, then the
  retry. Each entry has the `status` and the `duration_ms` of that run
  alone. A `killed` entry has `killed_by` and a `failure_kind`
  (`assertion`, `error` or `timeout`). A `timeout` entry has the
  `failure_kind` `timeout`. A `runtime_error` entry has a `detail` with the
  first line of the reason. A mutant without a retry, and a `cached`
  mutant, have no `attempts`. No fingerprint or baseline comparison uses
  the field.
- A real infinite loop through instrumented code gets no retry when its
  covering tests take 200 ms or less. Else its retry costs twice
  `loop_budget_ms`. A retry of another infinite loop costs two more
  timeout windows.
- A `runtime_error` retry takes the mutant out of the score, also after a
  first `timeout`. The completion is then `partial` (`runtime-error`).
- A `killed` result after a retry reports the duration of the retry. Each
  other status keeps the duration of the first run.

### Measure the flaky rate

`scripts/mutation-flaky-rate.mjs` compares the saved reports of repeated runs
with the same scope:

```bash
fallow mutation --all --no-cache --root <project>
cp <project>/.fallow/mutation/last-report.json run-1.json
# Repeat for run-2.json and run-3.json, then:
node scripts/mutation-flaky-rate.mjs run-1.json run-2.json run-3.json
```

- `--no-cache` makes each run execute each mutant. A result from the result
  cache did not run: the script leaves it out of `compared`, and `cached`
  gives its count for each report.
- `compared` counts the mutants that each run executed.
- `unstable` counts the compared mutants with a different status between runs
  or a `flaky` status in one run.
- `score_flips` counts the unstable mutants that one run detected and another
  run did not. `timeout_flips` counts the score flips with a `timeout` in one
  run.
- Each count has a rate over `compared`.

## Test reach of no-coverage mutants

Each `no_coverage` mutant has a `test_reach` object in the run report and in
the plan. It gives the static reason why no test covers the mutant. The other
statuses do not have the field. It never changes the score or the coverage.

- `reach` uses the classes of the `audit` test reach: `mocked-only`, `none`,
  `none-file`, `transitive` and `direct`. Treat the set as open.
- A mutant in the top-level statement that declares a value export gets the
  class of that export, and `export_name` names the export.
- A mutant outside each value export declaration gets the class of its file
  and has no `export_name`. The class is `none-file` when no test loads the
  file, `mocked-only` when no test loads the file and a mock explains an
  export of the file, and `none` when a test loads the file.
- `tests` and `mocked_targets` are present only for `mocked-only`. They list
  the test files whose references a mock removed and the mocked modules.
- A project with no test files gives `none-file` to each mutant.
- `direct` or `transitive` on a `no_coverage` mutant is not a conflict. The
  graph has a covered reference to the export, but no test ran the code of
  the mutant.

The plan classifies the exports of each instrumented file once, with
`module_graph::module_exports` (the per-file part of
`changed_exports_for_changed_paths`) and
`module_graph::classify_export_test_reach` (the per-export part of the `audit`
test reach). `ModuleGraph::test_entry_points_reaching` stays a separate query:
it names each test entry point that loads a file, with the mask of that test,
and the runner selects the tests from that list. `export_test_reach` reads
the shared profiles of the reachability index. It names only the test files
that reference the export, so it cannot give that list.

## Setup timeout

`--setup-timeout` limits the start, the dry run and each reload of a runner
process. The default is 2 minutes, and units from `ms` to `h` are valid. A
slow or busy host can need more, because the dry run executes each test file.

- For the first runner process, the run stops with exit code 2 and
  `FALLOW_MUTATION_RUNNER_FAILED`, and the help names the flag.
- The dry run of a restarted runner process gets the larger of
  `--setup-timeout` and a limit that scales with the first dry run (see
  Baseline failures).
- For a restarted runner process, the mutant is `runtime_error`, and its error
  names the flag. The run continues.
- A reload that passes the limit restarts the runner process.
- The config has no key for this limit.

## Baseline failures

`baseline_failures` lists the tests and test files that fail with no active
mutant. Their results decide no mutant status.

- A test that fails in the first dry run never covers a mutant. Its message
  is the first error of the test, in the shape that the next item describes.
- A test that fails in a control run (see Run-path self-check) is also a
  baseline failure. Its message says that the test fails in a mutant run
  with no active mutant but passed in the dry run of its whole file. A test
  that depends on another test of its file gives this result.
- The message of a control-run failure keeps the first error of the test
  after that reason: the error name and message on the first line, then the
  stack frames of the project. The companion removes the frames of the
  runner, Node and dependencies, makes paths relative to the project, and
  removes color codes. The companion cuts the error at 4096 UTF-16 code
  units, and fallow keeps at most 8 KiB of it. The first line of the
  message, with the assertion, also goes into the
  `FALLOW_MUTATION_BASELINE_FAILED` error. The human footer shows the
  reason on the `Message:` line and the first line of the error on the
  `Error:` line. A companion that sends no error gives the reason only.
- Each restarted companion runs its own dry run. A covering test that does
  not pass there is a baseline failure of that companion, with the ids of the
  affected mutants in `mutant_ids`. The mutant runs with its other covering
  tests. Only a mutant with no covering test left is `runtime_error`, and its
  `errors[]` entry names the mutant. The message says that the test did not
  pass in the dry run of a restarted companion, then gives the first error
  of the test from the first restarted companion that sent one. The error
  has the same shape and bounds as the error of a control-run failure, and
  the human footer splits it in the same way. A test that passes in the
  first dry run and fails after a restart can depend on state outside the
  test, for example a file or the process id of the runner.
- Parallel companions use the same project root and can share files or a
  database. Vitest's `fileParallelism: false` serializes tests inside each
  companion, but several companions can still run at once. When a phase has
  more than one worker, restarted init and dry-run faults and baseline
  mismatches suggest `--concurrency 1` or `mutation.concurrency: 1`. Shared
  state is a possible cause; the report keeps the original error. Serial
  phases and the first companion's setup do not receive this hint.
- A static mutant needs a new companion before and after its run when the
  runner keeps modules loaded between runs (Vitest with `isolate: false`).
  After a reload of the worker gave `restart_required`, the companion that
  starts for such a mutant dry-runs only the test files of its covering
  tests. Each other companion dry-runs each test file of the run. A kept
  companion with a narrow dry run restarts when a later job needs a test file
  outside it.
- The dry run of a restarted companion shares the host with the other
  workers. Its timeout is the first dry-run time times 2 times the worker
  count, and at least `--setup-timeout` (2 minutes by default). On a slow
  or loaded host, a fixed timeout made each mutant of a restarted companion a
  `runtime_error`. The time budget does not stop a dry run in progress, so a
  companion that hangs in its dry run can extend the run past the budget.
- A baseline failure makes the completion `partial` (`baseline-failure`).
- A run in which baseline failures leave each mutant without a test
  result exits `2` (see Exit codes).

## Run-path self-check

The dry run selects whole test files. A mutant run selects the covering
tests by id, so it uses another path of the runner API. When a runner
release changes that path, each mutant run can fail with an error before a
test starts. An error outside a test kills a mutant, so each mutant looks
killed and the score is a false 100%. Vitest 4.0 had this fault.

A control run is a `run` request with `mutant_id: null`. The companion runs
the requested tests through the run path of a mutant run, with no active
mutant. A control run does not stop at the first failure, so it names each
covering test that fails. Its timeout is the timeout of the mutant job, and
at least `--setup-timeout`. The scheduler uses control runs in two places:

1. The self-check. After the dry run and before the first mutant job, the
   first companion runs the tests of the first job as a control run.
   - The control run passes when each requested test runs and passes, with
     no error.
   - When no test ran, the run path is broken. The run stops with exit
     code 2 and `FALLOW_MUTATION_RUNNER_FAILED`, and no mutant gets a
     status. The message says that the runner executes no tests in a mutant
     run. It names the runner and its version, and the help asks you to
     report it.
   - When tests ran and some of them failed, the run continues. Each failed
     test is a baseline failure, the same as a test that fails in the dry
     run: it covers no mutant. A mutant with no covering test left is
     `no_coverage`. When this leaves each mutant without a test result,
     the run exits `2` with `FALLOW_MUTATION_BASELINE_FAILED` (see Exit
     codes).
   - When tests ran and none failed, but the result is still not a pass
     (an error, or a requested test that did not run), the run stops with
     `FALLOW_MUTATION_RUNNER_FAILED`. The message says that the runner gives
     no trustworthy result in a mutant run.
   - When the control run times out, the run stops with
     `FALLOW_MUTATION_RUNNER_FAILED`. The message says that the self-check
     timed out, and the help names `--setup-timeout` and a less loaded host.
     A timeout is not reported as a broken run path.
   - Each case of a parameterized test (`it.each`, `describe.each`,
     `test.each`) and each `node:test` subtest is its own test id, so the
     self-check counts it once.
   - A lone UTF-16 surrogate in a test name becomes U+FFFD in the test id,
     because the protocol carries only well-formed text. Two names that then
     match get the ` (2)` suffix, as two equal names do.
   - The check needs no run when no job exists: no mutant is covered, or
     each covered mutant has a result from the cache. The check also needs
     no run when the time budget ended before it.
   - The check costs one run of the tests of one mutant. Its time is part of
     `timings.mutants_ms`.
   - The check runs only the tests of the first job. A test outside that set
     that depends on another test of its file can still kill a mutant. The
     check does not examine that kill again. A later control run can list
     such a test as a baseline failure after it killed a mutant earlier in
     the run.
   - With `--max-mutants`, a planned mutant whose covering tests the check
     drops becomes `no_coverage`. The jobs are built again after the check,
     so the next covered `pending` mutant takes the free place, the same as
     for a dry-run baseline failure. A mutant that becomes `no_coverage`
     during the mutant runs (see item 2) leaves its place free: the jobs
     already exist.
2. A kill with no executed test. A mutant run that executes no test and
   reports an error is `killed` only when the same tests pass in a control
   run.
   - A mutant that throws when its module loads gives this result. Its
     tests pass with no active mutant, so it stays `killed`, with no killer
     test.
   - When the control run does not pass, the mutant is `runtime_error`, and
     its `errors[]` entry gives the reason. The mutant gets no retry.
   - A test that ran and failed in such a control run is a baseline
     failure for the rest of the run. Each later mutant run leaves it out,
     and `mutant_ids` of the failure lists those mutants. A later mutant
     whose covering tests are all such failures is `no_coverage`, with no
     retry, the same as after a drop by the self-check.
   - The scheduler stores the control result of each test set for the run.
     Each distinct test set gets at most one control run, and the test set
     of the self-check gets none. A fault of the companion is not stored.
   - A control run that times out is stored for its test set. The mutant
     is `runtime_error` with no retry, and each later mutant of this kind
     with the same tests gets the same reason at once. A test set that
     hangs therefore costs one control run.
   - After the `--time-budget` deadline, a control run that has no stored
     result does not start, and the mutant is `pending`.

A control result is never a mutant result, so the result cache never stores
it.

## Runners

The companion has three runner adapters. The `mutation.runner` config key
selects one. The default `auto` reads the project in this order:

1. `vitest` in the root `package.json` dependencies selects Vitest.
2. Otherwise the test module that the most static test files import:
   `vitest`, `bun:test` or `node:test`. A tie goes to Vitest, then
   `bun test`.
3. Otherwise a `bunfig.toml`, `bun.lock` or `bun.lockb` at the root selects
   `bun test`.
4. Otherwise a `node --test` or `borp` command in a `package.json` script
   selects `node:test`.
5. Otherwise Vitest.

| Runner | Config value | Versions | Runtime of the companion |
|---|---|---|---|
| Vitest | `vitest` | 4.1 to 5 | `node`, then `bun` |
| `node:test` | `node-test` | Node.js 22.18 to 26 | `node` |
| `bun test` | `bun-test` | Bun 1.4 and later 1.x releases | `bun` |

The companion refuses a runner version outside this range. The run exits
with code 2 and `FALLOW_MUTATION_RUNNER_UNSUPPORTED`. The message names the
installed version and the supported range. The help gives the install step
for the range. When the minimum version is not a `.0.0` release, the help
also says that a release of that major version is enough. For Vitest, the
help is:

```text
install vitest 4.1.0 or a later version below 6.0.0; a vitest 4.x release from 4.1.0 is in the range
```

Vitest 4.0 is not supported. It reads the test-id filter of a test
specification as a list of test lines. Each mutant run then fails with an
error, and each mutant looks killed. Vitest 4.1 added the test-id filter.

Bun 1.3 is not supported. Bun 1.3.0 to 1.3.9 write no JUnit report when
`--bail` is set, so a run gets no test results. Bun 1.3 also gives each
failed test the JUnit type `AssertionError`, so a runtime error looks like
an assertion failure. Bun 1.4 fixes both.

`borp` is a test runner that runs `node:test`. The script parser reads the
test files of a `borp` command in the order of `borp` 1.0:

1. The positional arguments. A glob that starts with `!` excludes files.
2. Otherwise the glob of `-p` or `--pattern`.
3. Otherwise the default glob `**/*.test.{js,mjs,cjs,ts,mts,cts}`.

These files are test entry points, so the plan finds the static test files
of the project. The parser does not apply a `!` exclusion glob: a file that
the exclusion removes stays a test entry point when another glob matches it.
The parser does not read a `.borp.yaml` file. A project that sets its test
files in `.borp.yaml` and runs a bare `borp` command gets the default glob.

### Runner config

`--runner-config <path>` (or the `mutation.runnerConfig` config key) selects
the config file of the test runner. Use it for a project that keeps its
integration, UI or browser tests in a separate Vitest config, for example:

```bash
fallow mutation src/math.ts --runner-config vitest.integration.config.mjs
```

- The flag wins over the config key. `--config` is not this flag: it selects
  the Fallow config.
- A relative flag path resolves against the project root, then against the
  current directory. A relative config key path resolves against the project
  root.
- The file must exist inside the project root. Otherwise the run exits with
  code 2, `FALLOW_INVALID_MUTATION_OPTIONS` and the context
  `mutation.runnerConfig`.
- Only the Vitest adapter reads a config file. With the `node-test` or
  `bun-test` runner, a selected config exits with code 2.
- Without a selection, Vitest finds its config the usual way.
- The companion starts Vitest from the project root, as `vitest run` does
  there. The `root` and `test.root` of the config stay in effect. A relative
  root resolves against the project root. Test files, test ids and report
  paths stay relative to the project root, also when the Vitest root is a
  subdirectory.
- The selected config sets the test files that Vitest lists. A static test
  file of the plan that the selected config does not include stays visible as
  a `test-files-not-discovered` error, and the run is `partial`.

`--runtime` takes a name or a file path. A bare name such as `node` or
`bun`, without a path separator, is a `PATH` lookup, as in a shell. A path
with a separator, such as `./bin/node`, is a file path. A relative file path
resolves against the working directory. On Unix, the `PATH` lookup skips a
file without an execute bit, as a shell does.

`--runtime` must name a runtime that the runner accepts. A Bun runtime for
`node-test`, or a Node runtime for `bun-test`, exits with code 2 and
`FALLOW_MUTATION_RUNTIME_NOT_FOUND`.

### Test environment

Each adapter gives the tests the environment variables that the native
runner command sets. Code that reads `NODE_ENV` then behaves as in a normal
test run.

| Runner | Variables | Source |
|---|---|---|
| Vitest | `TEST=true`, `VITEST=true`, `NODE_ENV=test` when it is not set | The adapter sets them as the `vitest` command does |
| `node:test` | None | `node --test` sets no `NODE_ENV` |
| `bun test` | `NODE_ENV=test` when it is not set | `bun test` sets it in each test process |

- The Vitest adapter calls `createVitest` directly and not the `vitest`
  command. Without these variables, Vite sets `NODE_ENV=development`, and
  the test workers inherit it.
- A `NODE_ENV` value of the environment wins, the same as with the Vitest
  CLI. For example, `NODE_ENV=production fallow mutation` runs the tests
  under `NODE_ENV=production`.
- Internal protocol detail: the companion applies the `options.env` values
  of the init request after these variables, so they win. The API sends an
  empty `options.env`.

### Vitest module loading

- A Vite plugin of the adapter serves the instrumented files to each
  `import` that the Vite module runner handles. A script file gets its copy
  in the `transform` hook, and a component file in the `load` hook.
- A native `require`, for example through `createRequire(import.meta.url)`,
  does not go through Vite. The setup file of the adapter registers a
  synchronous module hook (`module.registerHooks`) in each worker, so that
  load also gets the instrumented file. The hook replaces only the files of
  the overlay, and it reports each served path in the test meta
  (`fallowLoaded`). The adapter adds these paths to the loaded set, so
  `unloaded_paths` stays correct.
- The adapter gives the overlay to each project with `project.provide`. A
  worker that runs more than one test file (`isolate: false`) registers the
  hook once.
- The hook needs Node 22.15 or later. On an older Node, and in browser mode,
  a native `require` loads the original file, and the path stays in
  `unloaded_paths`. A static `import` of the file is the workaround.
- The setup file also adds `--import` of
  `runtime/node-subprocess-preload.mjs` to `NODE_OPTIONS` of the worker,
  with the overlay path and the active mutant in the URL. So a Node process
  that a test starts (`spawn`, `exec`, `execFile` or `fork`) loads the
  instrumented files with the same mutant, as with `node:test`. The setup
  file sets the value again before each test and removes the value of an
  earlier run, so a worker with `isolate: false` gives each child the
  current mutant. The adapter clears the integrity log of the overlay
  before each run and reads it after the run, so a copy that matches more
  than one planned file reports `overlay-incomplete`. In a dry run, the
  hits of a child process count for the test that started it (see "Child
  process coverage" below). Without the hook API (Node older than 22.15)
  and in browser mode, the value stays as it is and a child loads the
  original files.

### Vitest shared workers

Vitest starts one worker for each test file of a run when module isolation
is on (the default). After the dry run, the adapter runs the same test files
once more with `isolate: false`, with no active mutant. When each test gives
the same status in both runs, and the shared run has no error outside a test
that the isolated run does not have, the companion runs its later mutant and
control runs with `isolate: false`.

- Vitest then gives the test files of the run that have the same project,
  environment and `sequence.groupOrder` to one worker. Each run still
  starts a new worker, and Vitest loads the source modules again for each
  test file, so state on the global object is the only state that crosses
  files.
- A mutant can change state on the global object that a later file of the
  worker reads. When a shared run detects a mutant in a test file that did
  not run first in the worker, the companion runs that file again alone,
  with the mutant. When that run does not detect the mutant, the companion
  runs the whole request again with each file in its own worker, and that
  result counts.
- A mutant can also stay `survived` when global state of an earlier file
  of the worker hides it. This error lowers the score and never raises it.
  The retry of a survivor shares a worker in the same way.
- A test that depends on a fresh worker for each file in the dry run, for
  example a global that another test file sets, makes the two dry runs
  differ. The companion then keeps isolated runs.
- When any project has `isolate: false`, or the dry run has one test
  specification, the companion does not share workers. A project in browser
  mode keeps its own setting.
- The check repeats the dry run, and `duration_ms` of the dry run includes
  both runs. The check is skipped when the isolated dry run takes more than
  45 seconds. A failed shared run keeps isolated runs.

### Vitest reused workers

When the shared-worker check passes, the companion also keeps one Vitest
worker process for many mutant runs. Vitest stops the worker of a run when
the run ends, and a new process costs more CPU than most mutant runs. A
custom pool of the adapter (`adapters/vitest-reuse.mjs`) wraps the `forks`
or `threads` worker of Vitest and answers its `start` and `stop` messages
itself, so the next mutant run gets the same process.

- The check runs the dry-run files two more times in one reused process,
  with no active mutant. The first round shares one worker for all files in
  a fresh process, so it also decides the shared workers above. When it
  differs from the isolated dry run, the plain shared check of the section
  above decides, and each run gets a new process. The second round loads
  each module a second time in the same process. When a test then gives
  another status, for example because a module fails when it loads a second
  time, the companion shares workers within a run but keeps a new process
  for each run.
- Only mutant runs with bail use the reused process. Dry runs, control runs
  and runs without bail (`--kill-matrix`) start a new worker.
- Vitest sends the active mutant with each run. At the start of each test
  file of a reused run, the setup file calls `vi.resetModules()` and removes
  the overlay files from the CommonJS cache, so the source modules load
  again with the current mutant. A static mutant is therefore active when
  its module loads. A file that a native ESM `import` loads outside Vite
  stays in the ESM cache of the process.
- The first test of a process records the state outside the modules, after
  each setup file of the project ran: the global object, the main built-in
  prototypes, `process.env`, the `process` listeners, the working directory,
  the exit code and the active timers. When a test changes this state, the
  companion ends the reused process after the run. A kill in that run, and a
  kill with no failing test, count only when the test file also detects the
  mutant in a new worker.
- The worker sets up its test environment only when it starts, so a run
  with another environment gets a new process. The worker also reads
  `testTimeout`, `hookTimeout` and `bail` only at start. A reused run rounds
  `timeout_ms` up to a whole second, and a run with other values gets a new
  process. A test can therefore run up to 1 second longer than `timeout_ms`
  before Vitest stops it, which stays inside the 2-second grace of the
  watchdog.
- A process ends after 200 runs. A process that ends by itself (for example
  because a mutant kills it), a process that Vitest stops without a stop
  message, and a process that sends a message between runs are not reused.

### `node:test` and `bun test`

- Each dry run and each mutant run starts one test process for each test
  file. The `node:test` adapter uses `run()` of `node:test` with process
  isolation. The `bun test` adapter starts `bun test` with a JUnit report.
- A preload file serves the instrumented files. `node:test` uses a
  synchronous module hook (`module.registerHooks`), so `import` and
  `require` both get the instrumented file. Node strips the types of a
  TypeScript file. `bun test` uses a `Bun.plugin` load hook.
- The `node:test` adapter gives each test process the module loading flags
  of the project test script. The API reads the `package.json` scripts at
  the root: the `test` script first, then the other scripts in name order.
  The first `node ... --test` command gives the flags
  (`crates/api/src/mutation/node_args.rs`). The init request carries them
  in `options.node_args`, each flag with a value as two entries.
- These flags pass: `--experimental-transform-types`,
  `--experimental-strip-types`, `--no-experimental-strip-types`,
  `--import`, `--require` (`-r`), `--experimental-loader` (`--loader`) and
  `--conditions` (`-C`). Other flags of the command are ignored. A
  relative `--import`, `--require` or loader value resolves against the
  project root. The preload of the adapter comes before these flags, so a
  loader of the project wraps the hook of the preload.
- The detection reads only a command whose program is `node`. A wrapper
  command, for example `cross-env X=1 node --test` or `tsx --test`, gives
  no flags. `npm run` indirection needs no special step, because the API
  reads every script.
- `NODE_OPTIONS` of the `fallow mutation` process also reaches each test
  process. Use it for a flag that the script detection does not read, for
  example when the baseline fails with `ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX`.
- A test file that does not load (for example a TypeScript `enum` without
  `--experimental-transform-types`) is a baseline failure. Its message
  starts with the error line of the test process stderr, for example
  `SyntaxError [ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX]: TypeScript enum is not
  supported in strip-only mode`. The last lines of the stderr follow.
- The test files are the files that the default patterns of the runner
  match, without `node_modules` and `.git`. A static test file that the
  patterns do not match is not run, and the report says so.
- A test id is `<file>::<suite> > <test>`, as for Vitest.
- `node:test` reports the hits of a test in a test diagnostic. Bun hooks get
  no test context, so the `bun test` adapter joins the hits to the JUnit
  results by order. When the order is not safe, each test that ran in the
  file gets the hits of the file: tests that overlap (`concurrency` in
  `node:test`, `test.concurrent` in Bun), a count that differs, or
  `randomize = true` in `bunfig.toml`. The scheduler then runs a mutant
  against each test of the file that can kill it.
- A run selects the requested tests with a name pattern. Each whitespace
  run in the pattern matches any whitespace, because a Node test process
  matches a name without its trailing whitespace.
- With `bail`, a run stops the test process at the first failure. Node 22
  applies the run timeout to the whole test file process. A requested test
  with no result after that timeout counts as a timeout.
- The `node:test` preload adds `--import` of
  `runtime/node-subprocess-preload.mjs` to `NODE_OPTIONS` of the test
  process. So a Node process that a test starts (`spawn`, `exec`,
  `execFile` or `fork`) also loads the instrumented files with the active
  mutant. The URL of that import carries the overlay path and the mutant
  id, so the test environment has no `FALLOW_MUTATION_*` variable. In a dry
  run, the hits of such a child process count for the test that started it
  (see "Child process coverage" below). A child that is not Node (for
  example `bun`), or a child that gets an environment without
  `NODE_OPTIONS`, loads the original files and reports no hits.
- A Node overlay also recognizes byte-identical copies of planned source
  files, including renamed temporary files. It serves the instrumented
  source only when the bytes identify one planned file. Original paths keep
  their own attribution even when other planned files have identical bytes.
  A copy that matches several planned files reports `overlay-incomplete`,
  including when the test ignores its child's output or exit status. The
  baseline then fails, or the mutant has a runtime error and the run is
  incomplete. Edited or transformed copies cannot be attributed by bytes.
- The in-process mode of `node:test` (`isolation: "none"`) is not
  supported.
- Neither runtime can load a Vue, Svelte or Astro component file. The
  `node:test` hook calls the default loader first, and that loader refuses
  the file type. The Bun hook serves the copy with a TypeScript loader, and
  a project plugin that serves the file first serves the original. So the
  plan leaves each component file out, with the skip reason
  `runner-cannot-load` in `skipped_files`. Its mutants are not in the plan
  and not in the coverage.
- The runner decision reads the static test files of the plan. When the
  first plan has a component mutant and the runner cannot load components,
  the plan is built again from the same analysis without the component
  files (`plan_mutations_for_session_with`). The run keeps the runner of
  the first decision. `--plan` uses the same rule, so the plan and the run
  agree.

### Child process coverage

A test can assert only on the output of a Node child process, for example a
CLI test. The Vitest and `node:test` adapters count the hits of such a child
for the test that started it, so the scheduler selects that test for the
mutants that the child reaches
(`tools/mutation-runner/src/runtime/child-coverage.mjs`).

- Before each test of a dry run, the test process sets a new token in the
  URL of the subprocess preload in `NODE_OPTIONS`. Tests that run at the
  same time share one token, as they share the hits of the test process.
- At exit, a child with a token writes its hits and its loaded paths to one
  file in `<overlay file>.hits/` of the private work directory. The child
  writes under a temporary name and renames the file, so the test process
  never reads a part of a file.
- After the test, the test process takes the files of its token. The hits
  join the hits of the test, and the loaded paths join `loaded`, so a path
  that only a child loads is not in `unloaded_paths`.
- All hits of a child count as test hits, also its module-scope hits,
  because the child runs for the test.
- A mutant run gives no token, so a child writes no file.
- A child that a signal ends, or that runs after the end of its test, gives
  no hits to the test. `bun test` has no subprocess preload, so its children
  give no hits.

### Type tests

A type test (`*.test-d.ts`, `*.spec-d.ts`, or a TypeScript file below a
`test-d` directory) is not a test file of the plan. A compiler checks it (`tsd`,
`expect-type`, the Vitest type check), and no runner executes it. Before this
rule, a `tsd` file was the only static test file of some projects. The dry run
then ran it as a runtime test, it failed as a baseline failure, and runner
detection read its imports. A project whose only test entry points are type
tests has `no_test_files: true`. A file that only type tests reach has no
coverage, and its mutants get the test reach `none-file`.

### Bench files and test helpers

Two other kinds of test entry points are not test files of the plan, because
`vitest run`, `node --test` and `bun test` do not run them:

- a Vitest bench file (`*.bench.*` or `*.benchmark.*`). `vitest bench` runs
  it. A bench file with a test marker, such as `perf.bench.test.ts`, stays a
  test file;
- a test helper: a module below a `__tests__` directory that another module
  imports, and whose name has no test marker of a supported runner (`.test.`,
  `.spec.`, `_test.`, `-test.`, `_spec.`, or a `test.` or `test-` prefix). An
  example is `src/__tests__/utils.ts`.

Without this rule the runner did not list these files, and each run was
`partial` with `test-files-not-discovered`. A test file that a custom
`include` selects is never imported, so it stays a test file. The rules of
type tests for `no_test_files` and for a file that only these entry points
reach apply the same way.

## Runner notes

`runner.notes` lists facts of the runner setup that limit the result. The key
is absent when the runner states no fact.

- `typecheck-not-run`: the project config enables the Vitest type-check pass
  (`typecheck.enabled`). The run does not execute it, so a mutant that only a
  type test kills is not counted as killed. The human output prints one line
  for this note.

`runner` is absent when no companion started, for example in a project with
no test files.

## Result cache

A run stores the result of each `killed` mutant and each `survived` mutant
that a retry confirmed. A budgeted run keeps time for these retries (see the
retry section), so a repeat run with the same `--time-budget` continues
after the stored results. The file is `<cache dir>/mutation/results.json`. A
later run uses a stored result when the key is equal, and does not run that
mutant. The dry run always runs, because it gives the covering tests.

The key hashes these parts:

- the key format and the Fallow version;
- the runner name, the runner version and the runtime version;
- the mutant id, and the content of the mutated source file;
- the covering test ids of the first run. The retry of a survivor adds
  only tests of the same files, so the file digests below also cover the
  tests of the retry;
- the content of the covering test files, and of the files that they reach
  through static imports in the module graph (`MutationPlan::test_file_reach`).
  The run computes one digest for each test file and its reach, and each
  mutant key combines the digests of its covering test files;
- the content of the runner setup files (Vitest `setupFiles`, the `preload`
  files of the root `bunfig.toml`, also when no `@types/bun` or `bun-types`
  package enables the Bun plugin. A Vitest run keys on them too, because a
  project can start Vitest through `bun run`, and Bun then runs a top-level
  preload first. A Vitest project with such a file gets more misses, never a
  stale hit), of the runner config modules at the root
  (`vitest.config.*`, `vite.config.*` and `vitest.workspace.*`), and of the
  files that they reach through static imports (`MutationPlan::support_files`).
  A change to a config fragment that the runner config imports changes the
  key. A runner config that `--runner-config` or `mutation.runnerConfig`
  selects is a support file too, so a change to it changes the key. The key
  does not hash the files that a selected config imports;
- the lockfiles of the first directory from the root upward that has one;
- the runner config files at the root: `vitest.config.*`, `vite.config.*`,
  `vitest.workspace.*`, `bunfig.toml`, `package.json` and `tsconfig.json`. An
  absent file is also a key fact, so a new config file changes the key;
- the configs of the `extends` chain of the root `tsconfig.json`, at most 32
  files. A relative `extends` resolves to the path, or to the path with
  `.json`. A package `extends` resolves in each `node_modules` directory from
  the config upward, to the path, the path with `.json`, or `tsconfig.json`
  in the path. The `exports` map of a package is not read. An `extends` that
  does not resolve adds only its text to the key.

The reach ignores `vi.mock`, so a replaced module is still in the key. This
gives more misses, never a stale hit.

Known limit: dynamic imports that the module graph does not resolve, and files
that a test or a runner config reads, are not in the key. Runner configs and
tsconfig files below the root, for example in a workspace package, are not in
the key. A change to them does not invalidate a stored result. Each cache entry records this limit in `not_keyed`, and the
report repeats it in `cache.not_keyed`.

- A `timeout`, `flaky`, `runtime_error` or `pending` result is never stored.
  A control run (see Run-path self-check) is never stored.
  A timeout depends on the load of the machine. A survivor that the time
  budget left without a retry is not stored. A survivor of a run without a
  dropped covering test (see Baseline failures) is not stored, because the
  key covers that test.
- A reused result has `cached: true` in `mutants[]`. It counts in the score
  and the coverage the same as a result of this run.
- `cache` in the report gives `hits`, `stored` and `not_keyed`. It is absent
  with `--no-cache` and when no companion started.
- The human output prints one `Cache:` line when a result came from the cache.
- The global `--no-cache` flag turns the result cache off. The run then reads
  and writes no entry.
- The file keeps one entry for each mutant id, and at most 20000 entries. It
  drops the entries that no recent run stored or used. A file that does not
  parse, or that has another format version, gives an empty cache.
- A failed write of the file adds a `cache-write-failed` error and keeps the
  run result.
- The scheduler talks to the cache through the `ResultLookup` trait in
  `crates/api/src/mutation/scheduler`. The key and the file live in
  `crates/api/src/mutation/result_cache`.

## Outcome journal and resume

The result cache saves the results only after the last retry. An interrupted
run, for example by Ctrl-C or a machine crash, then loses each result. The
outcome journal keeps them.

- Each run writes `<cache dir>/mutation/journal.jsonl`. The first line is a
  header with the fingerprint of the run. Each next line is one final result
  with its mutant id and the key of the result cache (see "Result cache").
- A result is final when the mutant is `killed`, or when it is `survived`
  and a retry confirmed it without a dropped covering test. The run writes
  the line of a kill after the first run of the mutant, and the line of a
  survivor after its retry. The run writes one line with one write call and
  flushes it to disk.
- A `timeout`, `flaky`, `runtime_error`, `no_coverage` or `pending` result
  is never in the journal. A survivor without a retry is not in it. These
  mutants run again in a resumed run.
- The journal is not a report. The saved report and the `-o` file stay the
  report of a run.
- The run writes the journal with and without `--no-cache`. `--no-cache`
  turns off only the result cache.

`fallow mutation --resume` continues a run:

- The run reads the journal before the mutant runs. The dry run always
  runs, because it gives the covering tests.
- The header must match the run. The fingerprint has the journal format,
  the Fallow version, the runner, the runtime name and version, the run
  directory hash (each planned path and its instrumented text), the runner
  config, the scope, the operators and `--allow-side-effects`. A header of
  other inputs exits `2` with `FALLOW_MUTATION_RESUME_MISMATCH`. The message
  names each input that differs, with both values.
- The fingerprint sorts the operators and removes duplicates. It writes the
  paths of a `paths` scope relative to the root, without `.` parts, sorted
  and without duplicates. `./src/a` and `src/a` give one fingerprint.
- `--max-mutants`, `--mutants-per-line`, `--time-budget` and `--concurrency`
  are not in the fingerprint. A run that `--max-mutants` or the time budget
  stopped can continue with other limits. A resumed result takes a place in
  the `--max-mutants` limit, the same as a cached result.
- `--function-scope` can add mutants. That changes the instrumented text and
  the run directory hash, so a resume that changes the option exits `2` in
  this case.
- A journal result whose key does not match the key of this run is
  rejected, for example after a change to a covering test file. The result
  is not reused from the journal. The mutant runs again, or its status comes
  from the result cache.
- With no journal, or a journal without a valid header, the run starts from
  the beginning. It does not exit. The `resume.note` field says why, and the
  human output prints it.
- The run checks the journal size before it reads the file. A journal over
  64 MiB, or a journal that does not read, is not read. The run moves it to
  `journal.jsonl.unread`, so no earlier result is erased, and starts from
  the beginning with a note. When the move fails, the file stays and the run
  writes no journal.
- The run parses each line as bytes. A last line that an interrupt cut, also
  inside a multibyte character, is one bad line. The other lines stay.
- A resumed result has `resumed: true` in `mutants[]`, not `cached: true`.
  It counts in the score and the coverage the same as a result of this run.
  The result cache stores it, because the interrupted run did not save its
  cache.
- A run with no mutant that needs a test run reads no journal. Its
  `resume.note` says so.

The report of a `--resume` run has a `resume` object: `journal`, `reused`,
`rejected`, and `note` when the run did not read a journal. `journal` is
relative to the root. When the cache directory is outside the root, it is
absolute, because the report has no other cache path to follow. The file
stays only when the run did not complete. The object is absent without
`--resume`. The human output prints one `Resumed:` line.

The lifecycle of the journal:

- A run without `--resume` replaces the journal when its runner starts.
- A `--resume` run keeps the valid lines of a matching journal and appends
  its new results. It drops a last line that an interrupt cut.
- A run with `completion.status: complete` removes the journal after it
  saves `last-report.json`. When the save fails, the journal stays. A run
  that started no test runner did not write or check the journal, so it
  never removes it.
- A `partial` run, for example after `--time-budget` or `--max-mutants`,
  keeps the journal, so a later `--resume` continues it. An interrupted run
  and a run that exits `2` also keep it.
- A failed journal write adds a `journal-write-failed` error and keeps the
  run result.
- A start writes the journal to `journal.jsonl.<pid>.tmp` and renames it. A
  failed start removes its temporary file. A start also removes each
  temporary journal file older than one hour, which a run that ended before
  its rename leaves.
- Two concurrent runs on one project write the same journal file, so the
  journal of one run can replace the journal of the other.
- The scheduler talks to the journal through the `OutcomeJournal` trait in
  `crates/api/src/mutation/scheduler`. The file lives in
  `crates/api/src/mutation/journal`. The CLI removes a finished journal
  through `fallow_api::mutation::remove_finished_mutation_journal`. MCP does
  not run mutants, so it has no resume option.

## Run directory

The run writes the instrumented files, `paths.json` and the runtime hooks to
`<cache dir>/mutation/runs/<content hash>/`. The hash covers each planned
path and its instrumented text, so two runs of the same plan share one
directory.

- The write goes to a `.staging-*` directory below `runs/`, which the run
  renames into place. A crash or a concurrent run never leaves a partial
  directory, and a run that reads the directory never sees a partial file.
- A directory whose files have the expected content is used again without a
  write. A directory with other content is replaced in one rename.
- Each run records its use in a `.last-used` marker file. At the start of a
  run, Fallow removes the run directories that are not among the 5 most
  recently used ones and that no run used in the last hour. The hour keeps
  the directory of a concurrent run. The run never removes its own
  directory. A `.staging-*` directory older than one hour, which a crash can
  leave, is also removed.

## Type check of survivors

`--typecheck` (or `mutation.typecheck: true`) sends each `survived` mutant to
the TypeScript-Go companion of `--type-aware` after the tests. Other mutants
are not checked, so the cost grows with the number of survivors only.

- The companion applies one mutant at a time to an in-memory copy of its file.
  It never writes a project file.
- It compares the error diagnostics of the mutated file with the diagnostics
  of the original file. The comparison uses the diagnostic code and message,
  because the positions move. An error that the original file already has
  does not count.
- A survivor with a new error gets the status `compile_error`, and
  `type_error_codes` lists the codes (at most 8). The mutant does not count in
  the score, and it counts as covered.
- The check reads only the mutated file. A mutant that breaks only another
  file, for example a caller of a function with an inferred return type,
  stays `survived`. The check also does not run Vitest type tests, so the
  `typecheck-not-run` note stays.
- A survivor that removes an optional-chaining link can get an advisory
  hint. The mutants `a?.b` to `a.b`, `a?.()` to `a()` and `a?.[k]` to `a[k]`
  change nothing when `a` is never `null` or `undefined` at run time. The
  companion reads the declared type of `a` in the original file. When the
  type excludes `null` and `undefined`, and the mutated file type-checks, the
  survivor gets `equivalence_hint: "type-non-nullish"`. The status stays
  `survived`, and the mutant stays in the score. `typecheck.hinted` counts the
  hinted survivors, and `typecheck.checked` counts them too. The human report
  and `fallow mutation tasks` show one line under a hinted survivor: if no
  runtime value can be nullish, the mutant is equivalent, and a verdict file
  (`--verdicts`) records it.
- The hint is advisory and can be wrong. A type does not prove a run-time
  value. Two examples: a method that a caller detaches
  (`const read = box.read; read()`) loses its `this`, so `this?.value` returns
  `undefined` and `this.value` throws; and a function that declares the
  return type `{ b: number }` but returns the result of `JSON.parse("null")`,
  so `parse(s)?.b` returns `undefined` and `parse(s).b` throws. The hint
  rules are conservative on purpose, but they do not cover each such case.
  Read the code before you write a verdict.
- A verdict for a hinted survivor sets the status `equivalent` and removes the
  hint. `typecheck.hinted` counts the hints that stay in the final report.
- The hint needs `strictNullChecks` (or `strict`) and
  `noUncheckedIndexedAccess` in the `tsconfig.json` of the file, because
  without the second option `xs[0]` has a non-nullable type and is `undefined`
  for an empty array. The file must be a TypeScript file.
- The hint reads the declared type of the receiver, not the type that control
  flow narrowing gives at the use. A call can invalidate a narrowing that the
  checker keeps, so `if (a) { reset(); a?.b }` gets no hint when `a` is
  declared as `T | undefined`. The receiver can be a parameter, a local
  binding with an initializer, a property, a call to a function with a
  declared return type, `this`, or a fresh value (`new`, a literal).
- The hint does not apply to `any`, `unknown`, a type parameter, or a union
  with `null` or `undefined`. It does not apply to an assertion (`as`,
  `<T>x`, `x!`) anywhere in the receiver, or in the initializer of a local
  binding that the receiver reads. It looks through parentheses and
  `satisfies`.
- The hint does not apply to a declaration that no code in the project
  checks: a `declare` variable, a variable in a `.d.ts` file, a binding
  without an initializer, a `for` or `catch` variable, a destructured
  binding, a class property with `!` or `declare`, and a class property
  without an initializer that the constructor does not assign in its body. An
  element access (`xs[0]`) and a call without a declared return type get no
  hint either. A `let`, `var` or parameter binding that any code assigns
  after its declaration gets no hint: the check counts `=`, compound
  assignment, `++`, `--`, destructuring targets, `for` loop targets and
  assignments in closures. A top-level `let` or `var` of a script file gets
  no hint.
- A hinted survivor still gets the compile check. A hinted mutant whose
  mutated file does not type-check is a `compile_error` without a hint. The
  report names a code, not the expression, because the report holds no
  project text. The `file`, `line` and `column` of the mutant locate the
  receiver. The hint runs after the tests and only for survivors, in the same
  companion request as the compile check.
- A survivor that the checker cannot check stays `survived`. The reasons are
  `no-project` (no `tsconfig.json` includes the file), `not-type-checked`
  (`// @ts-nocheck`, or a JavaScript file without `checkJs`),
  `source-changed`, `file-missing` and `unsupported-file` (a Vue, Svelte or
  Astro component). `typecheck.skipped` counts them.
- The run does not send a component survivor to the companion. The
  TypeScript checker cannot read a component file, so the run gives it the
  reason `unsupported-file` directly.
- The run looks for the companion before the tests start. When no trusted
  companion exists, the run exits with code 2 and
  `FALLOW_MUTATION_TYPECHECK_UNAVAILABLE`. Discovery is the same as for
  `--type-aware`: `FALLOW_TYPE_AWARE_BIN`, the npm wrapper, or a sibling of
  the Fallow executable.
- When the companion fails after the tests, the survivors keep the status
  `survived`. The report has `typecheck.completed: false`, an error with the
  code `typecheck-failed`, and the completion reason `typecheck-failed`.
  `typecheck.skipped` still counts the component survivors, because they
  never go to the companion.
- `typecheck` is absent from the report when the option is off. A run
  without the option never sets `compile_error`.
- `--plan --typecheck` exits with code 2, because a plan has no survivors.
  The config key has no effect on a plan.
- The protocol operation is `mutant-typecheck` in
  `crates/api/type-aware-protocol.json`. It is a one-shot request; a semantic
  session refuses it.

## Verdicts for equivalent mutants

A verdict document replaces an inline suppression comment:

```json
{
  "schema_version": "1",
  "verdicts": [
    { "review_key": "3fc446a8dd3213b8", "verdict": "equivalent", "note": "same result for each input" }
  ]
}
```

- `review_key` is the `mutant_id`. The id hashes the path, the text of the
  enclosing function, the span in that function, the operator and the
  replacement. It stays the same until the function changes, so a separate
  `review_key` field would carry no extra information.
- A verdict for a `survived`, `no_coverage` or `flaky` mutant sets the status
  `equivalent`. A verdict for a killed, timed-out or `compile_error` mutant is
  `contradicted`, and the status stays. A verdict for a `pending`, `runtime_error` or `pruned`
  mutant is `deferred`. A verdict that matches no mutant is `unmatched`; one
  document can serve many scoped runs.
- A malformed document, an empty `review_key` or a duplicate `review_key` stops
  the command with exit code 2 before any test runs. Unknown fields are
  refused. The check is `fallow_api::validate_mutation_verdicts`.
- The join is `fallow_api::apply_mutation_verdicts`. The report records the
  document digest and each list under `verdicts`. A run without `--verdicts`
  has no `verdicts` key.
- Version 1 still runs the mutants that a verdict marks. The join happens after
  the run.

## The change of a mutant

Each mutant of the `mutation` report and of the `mutation-plan` document has a
`change` object, for example `{"from": "??", "to": "&&"}`. The object tells
which edit the mutant makes, and it keeps the rule that a report holds no
source text.

- The collector sets the change when it makes the candidate. The type of each
  side is `&'static str` (`fallow_mutation::MutantChange`), so a side cannot
  hold text of the source file.
- A side is an operator token, a fixed literal (`true`, `false`, `undefined`,
  `void 0`, `""`, `''`, `"fallow"`, `'fallow'`), or a regular expression token
  (`^`, `$`, `[`, `[^`, a quantifier, or a class escape such as `\d`).
- A side that is project code is a placeholder. A `negate-condition` test is
  `<condition>`, also the missing test of `for (;;)`. A `removed-call`
  statement is `<call>`, and a `removed-chain-link` link is `<call>` with an
  empty `to`. A `return-value` argument or arrow body and a relational
  expression that becomes `true` or `false` are `<expression>`. A non-empty
  `string-literal` is `<string>`, and a non-empty template literal is
  `<template>`.
- The opt-in operators use `<statements>` for the statements of a block,
  function body or `case` clause, `<statement>` for a `throw` statement,
  `<array>` and `<object>` for a non-empty literal, and `<call>` for a
  replaced or removed call. A `method-expression` pair is the two method
  names, for example `{"from": "startsWith", "to": "endsWith"}`.
- A `regex-assertion` side is `(?=`, `(?!`, `(?<=`, `(?<!`, `\b` or `\B`.
- A `regex-literal` quantifier side is `+`, `*` or `?`, and a shortened
  range is `{"from": "{n,m}", "to": "{n}"}`.
- An empty side means that the edit removes the token, for example
  `{"from": "!", "to": ""}` for `unary-operator`.
- A report that an older version wrote has no `change`. `inspect` copies the
  saved value and does not add one.

## Inspect

`inspect` reads a saved `mutation` or `mutation-plan` document and the current
file. Without `--report`, it reads the report that the last run saved in the
Fallow cache. Pass the same `--root` and `--config` as the run, because the
config can move the cache directory. Without `--root` and `--config`, a
search from a subdirectory walks up to the git worktree root, so it finds the
report of a run at the project root and reads the files of that root. When no run saved a report, inspect exits
with code 2 and `FALLOW_MUTATION_NO_REPORT`. Each run replaces the saved
report, so an id from an earlier run exits with code 2 and
`FALLOW_MUTATION_UNKNOWN_MUTANT`. Pass `--report <file>` to read another saved
report. The report holds no source text,
so inspect instruments the current file in memory and finds the mutant by id
and span.

- `fresh`: the file has the SHA-256 digest of the report, and the instrumented
  file gives the mutant. The packet shows the original and mutated lines.
- `stale`: `file-missing`, `no-digest`, `source-changed` or
  `mutant-not-reproduced`. The packet shows no code, so a changed file never
  gives a wrong answer.

The run report lists a digest for each mutated file. The plan lists digests
only for the files that it instruments, so a plan mutant in a file that no test
reaches inspects as `no-digest`.

## Replay

`fallow mutation inspect <mutant_id> --replay` calls one export of the file of
the mutant with one input on the original code and on the mutant, and shows
the results side by side. Use it to find a ready test input for a survivor.
The replay runs project code: the module of the file and each module that it
imports. It runs no test.

- `--args <json>` gives the arguments as a JSON array, for example
  `--args '[2, 3]'`. The default is no arguments.
- `--export <name>` names the export to call. The default is the export
  whose declaration holds the function of the mutant: `export function f`,
  `export const f = () => {}`, `export default`, or a local function that an
  `export { f as g }` list exports. A mutant in a nested function calls the
  outer export. A class or an object literal is not a function, so a mutant
  in a method needs `--export`. A mutant in top-level code, or in a function
  that no callable export holds, needs `--export` too, for example the
  export that reads the value.
- `--call-timeout <duration>` sets the limit of one call (default `5s`, at
  most `10m`). Another value exits with code 2 and
  `FALLOW_INVALID_MUTATION_OPTIONS`.
- The replay needs a `fresh` packet. A stale mutant exits with code 2 and
  `FALLOW_MUTATION_REPLAY_STALE`. The replay finds the mutant by its id and
  its span. When the current file gives no such mutant, it exits with code 2
  and `FALLOW_MUTATION_UNKNOWN_MUTANT`. `--args` that is not a JSON array
  exits with code 2 and `FALLOW_INVALID_MUTATION_OPTIONS`.
- The Vitest and `node:test` runners replay. The `bun test` runner exits
  with code 2 and `FALLOW_MUTATION_REPLAY_UNSUPPORTED`. A companion failure
  exits with code 2 and `FALLOW_MUTATION_REPLAY_FAILED`.

### Verdicts

The replay calls each side three times, in turns: original, mutant,
original, mutant, original, mutant. A side is stable when each of its calls
gives the same outcome and the same value. The comparison ignores the key
order of an object and the order of a set and a map, because a deep
equality check of a test does not see it. An outcome is part of the value:
a `threw` call never equals a `returned` call.

| Verdict | Meaning |
|---|---|
| `witness` | Both sides are stable, and they differ. A test with this input can kill the mutant. |
| `no_witness_found` | Both sides are stable and equal. This input shows no difference. Another input can still show one. |
| `unstable` | The calls of one side differ, for example from a source of random values that the replay does not fix. A difference that does not repeat is never a witness. |
| `timeout` | A call did not end before the call limit, or the replay did not end before its wall limit. The module load, the original or the mutant can block. |
| `not_callable` | The export is not a function. |
| `not_replayable` | The replay did not run: no export holds the mutant, or the file is a component file. `reason` says why. |

A replay never gives the verdict `equivalent`. Equal results say only that
this input shows no difference. Use `--verdicts` to record an equivalent
mutant after a review.

### Replay contract

The runner companion owns the replay contract. Its version is
`replay.contract_version` in `crates/api/mutation-runner-protocol.json`, and
the output gives it as `replay.contract_version`. Version 1 has these rules:

- The API sends one `replay` request with the file, the export, the mutant
  id, the arguments, the number of calls of each side, the call limit, the
  fixed clock and the seed. The companion answers with `replay_result`: one
  record for each call, in call order, with the side, the 1-based index, the
  outcome (`returned`, `threw`, `timed_out` or `not_callable`), the value and
  a short text. The API decides the verdict.
- Each call runs in a new worker with new modules: Vitest runs the replay
  test file once for each call, and `node:test` starts a new Node process
  for each call. So module state, globals, the environment and the timers of
  one call never reach the next call or the other side. A switch in
  top-level code runs once at import, so the load counts as part of the
  call: the mutant is active when the module evaluates, and an error that
  the load throws is a `threw` outcome.
- Each call runs in an `AsyncLocalStorage` store with its own active mutant.
  A timer or a promise of one call reads the mutant of that call, also when
  it runs during a later call.
- A Node process that a call starts inherits the side through the
  subprocess preload in `NODE_OPTIONS`, for the original and for the mutant.
- In each call, the clock starts at 2026-01-01T00:00:00Z for `Date.now()`,
  `new Date()` and `Date()`, and at 0 for `performance.now()`. Each read
  moves the clock by 1 ms, so a busy wait on the clock ends. `Math.random`
  starts from seed 1. The runner can read the clock while it loads the
  module, so a value can be some milliseconds after the start, but it is
  the same on each call of a side. Values from the clock gave false
  witnesses before this rule. Other sources (for example
  `crypto.randomUUID()`, the network or the file system) are not fixed.
  They give `unstable`, never `witness`.
- A call that runs past the call limit is `timed_out`. The call timer uses
  the timers of `node:timers`, so fake timers of the project do not stop it.
  A synchronous endless loop of the mutant through instrumented code ends at
  the loop check. A loop that blocks the worker in other code ends at the
  wall limit of the request (the call limit and 20 seconds for each call,
  and 30 seconds more): the API stops the companion, and the verdict is
  `timeout`. A process that a `node:test` call starts and that outlives the
  call runs until the request ends, because the API stops the companion and
  its process tree after each replay request.
- Arguments use the replay encoding: JSON values, and objects with a
  `$replay` tag for `undefined` (`{"$replay": "undefined"}`), `NaN`,
  `Infinity` and `-0` (`{"$replay": "number", "value": "NaN"}`), bigints,
  dates, regular expressions, maps, sets and typed arrays (`{"$replay":
  "bytes", "type", "values"}`, with tagged special numbers). A plain object
  with an own `$replay` key is `{"$replay": "object", "entries"}`. A
  function, a symbol, a class instance (also a subclass of a built-in class,
  such as `Buffer`), a proxy, a stream, a DOM node, a cyclic value and an
  accessor property are not replayable as an argument. A hole of a sparse
  array becomes `undefined`, and the extra own properties of an array, a map
  or a date are not part of the value.
- Results use the same encoding, with more tags for values that cannot be
  arguments: `error` (`name` and `message`, no stack), `instance` (`class`
  and the own enumerable `fields`), `function` (`name`), `symbol`
  (`description`), `proxy`, `cycle`, `accessor`, `truncated` (more than 32
  levels or 10,000 values) and `unreadable`. A `timed_out` call has the value
  `null`, so read `outcome` before `value`. `text` is a short display text,
  and `duration_ms` is wall time, so these two fields can differ between two
  replays of the same input.

## Tasks

`tasks` reads a saved `mutation` report and lists its open tasks as one
batch. A task is a `survived` or `no_coverage` mutant: a small change to the
code that made no test fail. The batch is the input for the next tests that a
person or a coding agent writes. `tasks` runs no code.

- The report comes from `--report <file>`, or else from the last run, with the
  same search as `inspect`. A `mutation-plan` document has no test results,
  so `tasks` exits with code 2 and `FALLOW_MUTATION_REPORT_INVALID`.
- Each task has the `mutant_id`, `file`, `line`, `column`, `operator`,
  `function` and `status` (the status of the mutant in the report), the
  source lines before and after the change
  (`original_lines`, `mutated_lines`), and the `covering_test_files`. A task
  holds no other source text. `inspect` gives the test ids and the static
  test files of one mutant.
- The source lines come from the current file, with the freshness rule of
  `inspect`. A task whose file changed after the run has a `stale_reason` and
  no lines.
- The tasks are sorted by file, line, column and mutant id. `summary` counts
  the tasks, the `survived` and `no_coverage` tasks, and the stale tasks.
  `report_completion` is the completion status of the report: a `partial`
  report can miss tasks, because a `pending` mutant is not a task.
- The exit code is `0`, also when the batch has tasks. The gate of a run is
  `--min-score`. A task path outside the project root fails the command with
  code 2 and `FALLOW_MUTATION_REPORT_INVALID`, as for `inspect`.

The human and the markdown output start with a short header, then one item
for each task:

```text
1. src/math/inRange.ts:48, function inRange (survived)
   current: if (minimum >= maximum) {
   changed: if (minimum > maximum) {
   tests: src/math/inRange.spec.ts
```

The header says that one test can cover several items, and that an item that
no input can detect can be skipped. The markdown output puts the two lines in
a fenced block. The MCP tool `mutation_tasks` returns the JSON batch. It runs
no mutants.

## Export

`export` writes a saved `mutation` report in the open
`mutation-testing-report-schema` format, schema version 2. Mutation
dashboards and HTML report viewers read this format, so a team can keep the
dashboard that it uses. `export` runs no code.

```bash
fallow mutation src/utils.ts                     # run, and save the report
fallow mutation export -o mutation-report.json   # the report in the schema format
```

- The report comes from `--report <file>`, or else from the last run, with the
  same search as `inspect`. A `mutation-plan` document has no test results,
  so `export` exits with code 2 and `FALLOW_MUTATION_REPORT_INVALID`. The
  report paths are relative to the project root, so pass `--root` when you
  run `export` from another directory.
- The schema needs the full source of each mutated file. The source comes
  from the current file. Each mutated file must have the SHA-256 digest of
  the report. Otherwise `export` exits with code 2 and
  `FALLOW_MUTATION_REPORT_INVALID`, and the message names each changed or
  missing file, with the cause. Thus a location never points into changed
  code. The `source` does not hold a leading byte order mark, because the run
  instruments the file without it.
- The output is JSON with `--format human` and with `--format json`. `-o`
  writes it to a file.
- `mutatorName` is the operator, and `replacement` is the text of the change.
  `location` has 1-based lines and columns. A column counts UTF-16 code units,
  the unit of a source string in a browser. Lines break at `\r\n`, `\r`,
  `\n`, U+2028 and U+2029, the same as in a browser report viewer. The
  mutants of a file are in source order.
- `coveredBy` holds the covering tests. `killedBy` holds the killing tests of
  a kill matrix run, or else the first killing test. `testFiles` lists each
  test by its file. A test id with no test file is under the key `unknown`.
- `static` is the `static_mutant` field of the report. `duration` is the
  `duration_ms` field: the time of one run of the mutant, with the time of
  the runner. `export` writes `statusReason` from the status (see the
  table). `thresholds` is `high: 80`, `low: 60`. `framework.name` is
  `fallow`.
- `config.completion` holds the `completion` of the report: the status and
  the reasons of a partial run.

The schema has fewer statuses than Fallow. The export maps each status so
that a mutant counts as killed or survived only when the run showed it:

| Fallow status | Schema status | `statusReason` |
|---|---|---|
| `killed` | `Killed` | none |
| `timeout` | `Timeout` | none |
| `survived` | `Survived` | none, or the partial-run note |
| `no_coverage` | `NoCoverage` | none, or the partial-run note |
| `runtime_error` | `RuntimeError` | the runner failed outside the tests |
| `compile_error` | `CompileError` | the mutated file does not type-check, with the TypeScript codes |
| `flaky` | `Ignored` | `Flaky:` a second run changed the result |
| `pruned` | `Ignored` | `Pruned:` the static rule |
| `equivalent` | `Ignored` | `Equivalent:` a verdict marks the mutant |
| `pending` | `Pending` | the mutant did not run |

The schema score leaves out `Ignored`, `Pending`, `CompileError` and
`RuntimeError`, the same as the Fallow score. `NoCoverage` counts in the
schema score, but not in `summary.score` (see "Score and coverage"), so a
dashboard can show a lower score than Fallow.

A partial run (`completion.status: "partial"`) can miss kills. Each
`Survived` and `NoCoverage` mutant of a partial run gets a `statusReason`
that names the reasons and says that the result is not conclusive. The
status stays `Survived` or `NoCoverage`, because the run found no test that
detects the mutant, and `Ignored` would hide a real test gap. A dashboard
reads only the status, so the score of a partial run can be lower than the
score of a complete run. Read `config.completion` before you compare the
score of a partial run.

## Recheck

`--recheck` reruns mutants of a saved report against the current tests. It
closes the loop of `tasks`: write tests for the open tasks, recheck, and the
mutants that still survive are the next batch.

```bash
fallow mutation src/utils.ts       # run, and save the report
fallow mutation tasks              # the open tasks as one batch
# write or change tests
fallow mutation --recheck          # rerun the open tasks
fallow mutation tasks              # what is still open
```

- The mutants come from the report: the ids of `--mutant` (comma-separated
  or repeated, any status), else each `survived` and `no_coverage` mutant.
  The report is `--report <file>`, or else the report of the last run, with
  the same search as `inspect`.
- Each selected file must have the SHA-256 digest of the report. A changed
  file changes its mutants, so the recheck exits with code 2 and
  `FALLOW_MUTATION_SOURCE_CHANGED`. Run `fallow mutation` again for new
  results.
- The worktree can be dirty, but each dirty path must be a test file: a path
  that the test path rules match, or a test file of the plan. Another dirty
  path exits with code 2 and `FALLOW_MUTATION_NON_TEST_CHANGE`.
- These two checks cover the selected files and the uncommitted changes. The
  report stores no commit, so a recheck does not see a change that was
  committed after the report to another file, for example a module that a
  selected file imports or the runner config. Run `fallow mutation` again
  after such a commit.
- The run plans the files of the selected mutants. The other mutants of
  these files are `pruned` with `prune_reason` `not-selected`, because the
  instrumented file holds their switch too. They leave the score.
- An id that the report does not have exits with code 2 and
  `FALLOW_MUTATION_UNKNOWN_MUTANT`. A report without an open task exits with
  code 2 and `FALLOW_MUTATION_NOTHING_TO_RECHECK`.
- `--recheck` conflicts with `--all`, paths, `--changed-since`, `--plan`,
  `--resume`, `--allow-dirty`, `--max-mutants` and `--min-score`. The score
  covers only the rechecked mutants, so `mutation.minScore` does not apply.
  The other run flags apply.
- The output is a normal `mutation` report with the optional `recheck`
  object. A recheck without `--mutant` replaces the saved report, so `tasks`
  then lists the mutants that are still open. A recheck with `--mutant`
  keeps the saved report, so the other open tasks stay in it. Pass
  `-o <file>` to keep a copy of the output.

The `recheck` object has `report_sha256` (the digest of the earlier report),
a count for each outcome, and `mutants`: each selected mutant with its
`mutant_id`, `file`, `line`, `previous_status`, `status` and `outcome`.

| `outcome` | Meaning |
|---|---|
| `killed` | A test now fails on the mutant (`killed` or `timeout`). |
| `survived` | Each covering test still passes on the mutant. |
| `no_coverage` | No test reaches the mutant. |
| `tests_fail_on_original` | A test of the run fails on the original code (a baseline failure), so the run proves no survivor. Fix that test first. |
| `unsettled` | No final result, for example `runtime_error`, `flaky` or `pending`. `status` gives the reason. |

## Compare

`--compare <file>` compares the run with a saved report. It shows if a
change of the tests weakens the detection of mutants: a mutant that a test
killed before and that survives now.

```bash
fallow mutation src/utils.ts --format json > before.json   # save a report
# change, move or remove tests, then commit
fallow mutation src/utils.ts --compare before.json
```

- The saved report is a `fallow mutation --format json` report, for example
  the `-o` file of an earlier run. Keep it in a file of its own: each run
  replaces the report of the last run in the Fallow cache.
- The compare matches the mutants of the two runs by `mutant_id`. A mutant id
  stays the same until its enclosing function changes, so an edit of a test
  file keeps the ids. Run the two runs over the same files.
- The saved report and this run must have `completion.status: complete`.
  The mutants of a partial run have no final status. A saved report that is
  not complete exits with code 2 and `FALLOW_MUTATION_COMPARE_INCOMPLETE`
  before the run starts. A run that is not complete writes its report, then
  exits with code 2 and the same code, and the report has no `compare`
  object.
- A regression does not change the exit code. To fail a CI job on a
  regression, read `compare.regressed` of the JSON output.
- `--compare` conflicts with `--plan` and `--recheck`. The other run flags
  apply. A verdict document applies before the compare, so an `equivalent`
  mutant is not a regression.

The `compare` object has `report_sha256` (the digest of the saved report),
`compared` (the number of mutant ids in both runs), a count for each change,
and `mutants`: each mutant with a change, sorted by file, line and mutant id.
Each entry has `mutant_id`, `file`, `line`, `change`, `previous_status`
(absent for `added`), `status` (absent for `removed`) and
`previous_killing_tests`: the `killing_tests` of the saved report, else its
`killed_by`. A mutant in both runs without a change is not in the list.

| `change` | Meaning |
|---|---|
| `regressed` | The saved report detects the mutant (`killed` or `timeout`), and this run does not (`survived` or `no_coverage`). |
| `inconclusive` | The mutant is `flaky` in one of the two runs, so the compare of this mutant proves nothing. |
| `added` | Only this run has the mutant id. |
| `removed` | Only the saved report has the mutant id. |

The human output adds a compare section after the run report. Each
regressed mutant names the test that killed it in the saved report. When
no mutant id is in both runs, the section says so, because the compare
checked no mutant. The exit code stays 0.

## Health join

`fallow health --mutation-report <file>` reads a saved run report and adds
the mutation score of each complexity finding next to its CRAP score. It
reads files only. It runs no test.

- The file is the saved report (`<cache dir>/mutation/last-report.json`) or
  the `-o` file of a run. A plan has no results, so health refuses it with
  exit code 2 and `FALLOW_MUTATION_REPORT_INVALID`. A missing or invalid file
  gets the same exit code and code.
- A finding matches one entry of `functions[]` when the path, the first line
  and the name are equal. Two entries with the same key make the match
  ambiguous, and the finding gets no evidence.
- The finding gets `mutation.state`. `fresh` means the file has the SHA-256
  digest of the report. Then `mutation.counts` has the counts of the function
  and `mutation.score` has its score. The score is absent when no mutant of
  the function ran to a tested result.
- `stale` means the file changed (`source-changed`), is missing
  (`file-missing`) or has no digest in the report (`no-digest`). A stale
  finding has `stale_reason` and no score and no counts, so a changed
  function never shows an old value.
- The top-level `mutation` object has the completion status of the run, the
  number of function results, the number of fresh, stale and unmatched
  findings, and the stale files. `sections` then has `mutation`.
- The join runs after the health report is complete. The mutation score
  never changes `crap`, the health score, the vital signs or a gate.
- Only complexity findings get a score. A function under each threshold has
  no finding, so its score stays in the mutation report. The findings of
  each `--group-by` group get the same evidence.
- The human output prints one line under the CRAP line of each matched
  finding, and one note line with the matched and stale counts. The other
  output formats (SARIF, CodeClimate, markdown, compact) do not show it.
- The score uses the final statuses after the retries (see Retries).

## Progress

- A run with human output and no `--quiet` prints phase lines on stderr.
  Each line starts with `fallow: mutation:` and ends with the run time, for
  example `fallow: mutation: 8/20 mutants done (1m25s)`.
- The phases are the runner setup, the baseline run, the mutant runs, the
  retries of survivors and timeouts, and the final worktree check and
  report. A run with no covered mutant and no test file prints only the
  final phase.
- The start of each phase prints at once. A mutant count prints at most
  once in each interval and at the end of its phase. A phase that prints
  nothing for one interval prints a line that says it continues. The
  interval is 5 seconds on a terminal and 15 seconds in captured output.
- `--format json` and `--quiet` print no progress lines. Stdout never gets
  a progress line.
- `fallow_api::run_mutation_with_progress` sends the same phases as
  `fallow_api::MutationProgress` events to a caller of the programmatic
  API.

## Human output

- The output uses the section headers and colors of the other commands.
  `NO_COLOR`, a pipe and `-o` remove the colors.
- The header says `(ran the project tests)` only when a companion started or a
  mutant has a tested result. Otherwise it says `(no test ran)`.
- The score line: the score, killed, survived, timeout, and the completion
  status. The line adds `pending`, `equivalent`, `runtime error`, `flaky` and
  `compile error` only when the count is not zero. A run with
  `--mutants-per-line` shows the left-out part of the `pending` count as
  `pending 12 (10 sampled out)`.
- The scope line shows a full 40-character base SHA as its first 12
  characters, as `audit` does. No other line repeats the base ref.
- The coverage line: the coverage and the no-coverage count.
- One line when `runner.notes` has `typecheck-not-run`: type-level kills are
  not counted. When the survivor type check ran, the line says that only type
  errors in the mutated file are counted.
- One line for the survivor type check: the checked count, the count that
  does not type-check, and the skipped count by reason. A failed check says
  that the survivors keep the status `survived`.
- One `Cache:` line when a result came from the result cache. It names
  `--no-cache`.
- One `Resumed:` line for a `--resume` run: the count of results from the
  journal and the count of journal results that were not reused from the
  journal because their inputs changed. When the run read no journal, the line gives the reason and the
  journal path.
- The 10 best-ranked survivors grouped by file, files and mutants in rank
  order, and one `and N more survivors` line. Each survivor has the location,
  the operator, the original and mutated code, and one line with the count of
  covering tests and the first test name, cut to 60 characters. A long code
  line shows a window around the changed token, the text that differs
  between the original and the replacement. Both sides use the same window
  start. The JSON report lists all survivors and all covering tests.
- The `inspect` output shows the first 10 covering tests under a
  `Covering tests (N)` header, or the first 10 static test files under a
  `Test files that reach the file (N)` header, and one `and N more` line. The
  `--format json` packet lists all of them.
- The plan lists the first 20 planned mutants under a `Planned (N)` header,
  in run order, and one `and N more mutants` line.
- The minimum-score line uses the status prefixes of the other commands:
  `[OK]` when the score meets the minimum, `[X]` when it misses the minimum,
  and `[-]` when no score is measured.
- One line for the baseline failures. It names the failing test when there
  is one, and the count and the first test when there are more. A `Message`
  line under it shows the first line of the message of the first failure, at
  most 120 characters, for example `Invalid environment variables`.
- Each `Error` line names its mutant when the error belongs to one mutant.
- A separate no-coverage section with one line for each file and its count.
  The line adds the counts by test reach class, for example
  `test reach: 2 mocked-only, 1 none-file`, in the class order of the
  `audit` test reach. The section shows the 10 files with the most
  mutants and one `and N more files` line. The JSON report has the full list.
- The plan prints one `No coverage by test reach` line with the same counts
  when a mutant is `no_coverage` and the project has test files.
- One `Skipped files:` line with the count of each skip reason, in the run
  and in the plan. The JSON report lists each path. A `runner-cannot-load` skip adds
  one line that names `mutation.runner` and Vitest as the next step.
- The pruned counts by reason, and one line for "no tests" or "no mutation
  data", never one line for each function. A project with no test files gets
  the one "no tests" line with the mutant count, and no no-coverage section.
  An empty changed-code scope names `fallow mutation --plan --all`, which
  lists the mutants of each file and runs no code.
- The next step is read-only: `fallow mutation inspect <id>` for the first
  survivor. It reads the saved report of the run, so no step runs the tests
  again. The step repeats `--root` and `--config` when the run used them.

## Config

```json
{
  "mutation": {
    "runner": "auto",
    "operators": ["arithmetic-operator", "relational-operator"],
    "minScore": 80,
    "concurrency": 4,
    "mutantsPerLine": 2,
    "functionScope": true,
    "typecheck": true,
    "runnerConfig": "vitest.integration.config.mjs"
  }
}
```

- `runner` is `auto`, `vitest`, `node-test` or `bun-test`. See
  [Runners](#runners).
- `operators`, `minScore`, `concurrency` and `mutantsPerLine` apply to each run.
  A CLI flag wins.
- `mutantsPerLine` and `functionScope` are config keys of the shared API, so
  the CLI and the MCP tools select the same mutants for one project. The
  `mutation_plan` tool also takes `mutants_per_line` and `function_scope`.
  The score ignores a `pending` mutant, so `--min-score` judges only the
  mutants that the run executed. A run of
  selected mutant ids (`--recheck`) never samples and never widens, whatever
  the config says.
- `functionScope: true` widens the changed-code scope to whole functions for
  each run. `--function-scope` does the same for one run. No flag turns it
  off. See [Function scope](#function-scope).
- `runnerConfig` selects the runner config file for each run.
  `--runner-config` wins. See [Runner config](#runner-config).
- `typecheck: true` turns on the survivor type check for each run.
  `--typecheck` turns it on for one run. No flag turns it off.
- The section has no source globs and no timeout keys. `MutationRunOptions`
  has no field for them, and a public key must have an effect. The section
  rejects unknown keys, so a config with `include`, `exclude`, `timeoutFactor`
  or `timeoutMs` fails to load. Add a key only together with its run option.
- The section is not part of the detection digest of dead-code findings.

## CI

The GitHub Action and the GitLab template run `fallow mutation` before the
analysis. `fallow report --mutation-report` then renders the result into the
sticky comment, the inline review, the annotations, the job summary and the
Code Quality report. The shell scripts only run the step and pass the result
file to `fallow report`. Rust owns all the text. The step is off by default.

| Setting | Action input | GitLab variable | Default |
|---|---|---|---|
| Turn the step on | `mutation` | `FALLOW_MUTATION` | `false` |
| Scope | `mutation-scope` | `FALLOW_MUTATION_SCOPE` | `changed` |
| `--time-budget` | `mutation-time-budget` | `FALLOW_MUTATION_TIME_BUDGET` | `10m` |
| `--max-mutants` | `mutation-max-mutants` | `FALLOW_MUTATION_MAX_MUTANTS` | none |
| `--min-score` | `mutation-min-score` | `FALLOW_MUTATION_MIN_SCORE` | none |

The scope is `changed`, `all`, or a list of files and directories relative to
the root, separated by spaces or commas. The `changed` scope uses the diff of
the change-scope step (`--diff-file`). Without a diff, it uses the base ref of
that step (`--changed-since`). A job with neither, for example a push to the
default branch, skips the run. The section then says that the run did not
run, with the reason `no-change-scope`.

```yaml
# GitHub Actions
permissions:
  contents: read
  pull-requests: write
steps:
  - uses: actions/checkout@v6
  - uses: actions/setup-node@v6
    with:
      node-version: 24
  - run: npm ci
  - uses: fallow-rs/fallow@vX.Y.Z
    with:
      comment: true
      mutation: true
```

```yaml
# GitLab CI
fallow:
  extends: .fallow
  variables:
    FALLOW_COMMENT: "true"
    FALLOW_MUTATION: "true"
  script:
    - npm ci
    - bash /tmp/fallow-run.sh
```

### Step order

The run refuses a dirty worktree, and the CI step never passes
`--allow-dirty`. Put the steps of your job in this order:

1. Check out the code.
2. Install the dependencies. `node_modules/` is ignored, so it does not make
   the worktree dirty.
3. Run the code generation and the build steps that the tests need. Each file
   that they write must be committed or ignored by `.gitignore`. Otherwise the
   run stops with exit code 2 and `FALLOW_MUTATION_DIRTY_WORKTREE`. A package
   manager cache inside the project is such a file too, for example a pnpm
   store at `.pnpm-store/` (the usual GitLab cache setup). Ignore it in
   `.gitignore`, or put the store outside the project.
4. Run the Fallow step.

Inside the Fallow step, the parts run in this order:

1. **Change scope.** The Action step `Resolve change scope`
   (`action/scripts/scope.sh`) or the inline `Change scope` block of the
   GitLab template resolves the base ref and writes one diff. The analysis,
   the mutation run, the comment and the review all read this diff.
2. **Mutation run.** The run writes its report with `-o` to the runner temp
   directory. A shell redirect into the workspace would make the worktree
   dirty before the run starts (see [Command surface](#command-surface)).
3. **Analysis.** The analysis writes its result files into the workspace,
   so it runs after the mutation run.
4. **Posting.** The comment, the annotations, the job summary, the review and
   the Code Quality report render with `fallow report --from <results>` and
   the mutation flag.
5. **Result step.** The last step of the Action, or the end of the GitLab
   job, applies the mutation outcome to the job result (see
   [Exit codes in CI](#exit-codes-in-ci)).

### Change scope

On GitHub, the scope step resolves the auto changed-since, the merge base,
the changed files and the unified diff. It writes only to
`$RUNNER_TEMP/fallow-scope/` (`pr.diff`, `changed-files.json`) and to `.git`
(a fetch). It exports `FALLOW_DIFF_FILE`, `FALLOW_SCOPE_CHANGED_SINCE`,
`FALLOW_CHANGED_FILES_FILE`, `FALLOW_ARTIFACTS_DIR`, `HAS_NATIVE_REPORT` and
`HAS_MUTATION_REPORT` through `GITHUB_ENV`. The paths in the diff are
relative to the `root` input. The `gh pr diff` fallback is rebased to the
root, and the step drops each file section outside the root. The analysis
step copies the diff and the changed files to `fallow-pr.diff` and
`fallow-changed-files.json` in `artifacts-dir`.

A pull request with `auto-changed-since: false` and no `changed-since` has no
base ref, so the analysis covers the whole project. The step still writes the
PR diff (`gh pr diff`), so the comment and the review keep their line filter,
and the `changed` mutation scope uses the lines of the pull request.

On GitLab, the base is `FALLOW_CHANGED_SINCE`, or else
`CI_MERGE_REQUEST_DIFF_BASE_SHA`. The block exports it as
`FALLOW_SCOPE_CHANGED_SINCE`. When `FALLOW_CHANGED_SINCE` is set and
`FALLOW_DIFF_FILE` is empty, the block writes
`${TMPDIR:-/tmp}/fallow-scope/mr.diff` with
`git diff --unified=0 --relative` from `FALLOW_ROOT`, and exports
`FALLOW_DIFF_FILE`. After the mutation run, the job copies this diff to
`fallow-mr.diff` for the earlier file name. A `fix` job has no auto
changed-since, so its mutation run gets `--changed-since <merge request base>`
and no diff.

`FALLOW_DIFF_FILTER=diff_context` or `file` widens the finding filter of the
comment and the review. It does not widen the mutation scope: a changed-scope
run plans mutants only on the added lines.

### Result files

The step writes these files to `artifacts-dir` (Action) or to the job
workspace (GitLab) after the run:

| File | Content |
|---|---|
| `fallow-mutation.json` | The report of the run. When the run wrote no report, the JSON error envelope of the run. When the run wrote neither, a fixed-text error envelope with the code `FALLOW_MUTATION_NO_REPORT`, `FALLOW_MUTATION_INVALID_SCOPE` (a scope path that starts with `-`) or `FALLOW_MUTATION_UNAVAILABLE` (Action: the installed fallow has no `mutation` command) |
| `fallow-mutation-status.json` | `{"outcome", "exit_code", "not_run_reason"}`, with closed values only |
| `fallow-mutation-stderr.log` | The stderr of the run, which can hold test output |

Only the shell reads the status file. The values of `not_run_reason` are
`event-refused`, `fork-refused`, `no-change-scope`, `interrupted` and `null`.
The stderr log never reaches a rendered surface.

To inspect a survivor on your machine, download `fallow-mutation.json` and
run `fallow mutation inspect <mutant-id> --report fallow-mutation.json`. Or
run `fallow mutation` with the same scope on the same commit, then
`fallow mutation inspect <mutant-id>`. The GitLab job keeps the files as
artifacts. The Action writes them to `artifacts-dir`; add an upload step to
keep the report:

```yaml
- uses: actions/upload-artifact@v4
  if: always() && steps.fallow.outputs.mutation-results != ''
  with:
    name: fallow-mutation
    path: ${{ steps.fallow.outputs.mutation-results }}
```

### Render with `fallow report`

The posting steps pass one of two flags to `fallow report --from <results>`:

- `--mutation-report <file>`: the `fallow-mutation.json` of the job. The file
  is a `kind: "mutation"` report or the error envelope of a failed run.
- `--mutation-not-run <reason>`: `event-refused`, `fork-refused`,
  `no-change-scope` or `interrupted`. `interrupted` applies only when the run
  left no report and no error envelope.

The two flags conflict. They apply to the `pr-comment-github`,
`pr-comment-gitlab`, `review-github`, `review-gitlab`, `github-summary`,
`github-annotations` and `codeclimate` formats. With another format, or with
a saved `fallow fix` envelope, `report` exits 2. The paths in the report are
relative to the root of the mutation run, so the `--root` of the `report`
call must be the same root.

`report` exits 2 with `FALLOW_MUTATION_REPORT_INVALID` for these input
errors only:

- the file is missing, cannot be read, or is larger than 64 MiB;
- the JSON is valid, but it is not a `mutation` report and not an error
  envelope;
- the document is a `mutation-plan`, `mutation-tasks` or `mutation-inspect`
  document, or a `--lean` report;
- a mutant path is absolute or contains `..`.

Other faults do not stop the render. A file that is not valid JSON, or a
`mutation` report that does not parse (wrong types, a missing `summary`, or
a `gate_outcomes["mutation-min-score"]` entry that does not parse), gives the
unreadable state. `report` then prints
`warning: FALLOW_MUTATION_REPORT_INVALID: <reason>` on stderr, renders the
analysis output and the unreadable section, and exits 0. A failed mutation
gate never changes the exit code of `report`.

Without either flag, the output of `report` is byte-identical to the output
without mutation testing.

### The section

The sticky comment and the review summary get a `#### Mutation testing`
section. The job summary gets a `## Mutation testing` section. The section
goes after the findings, before the section of config patterns that matched
nothing, and above `Generated by fallow.`. The comment section has no elapsed
time and no cache count, so the same report gives the same bytes. A partial
run with a failed gate and no-coverage mutants gives this comment section:

```markdown
#### Mutation testing

**Partial run** (`time-budget`). The counts cover only the mutants that ran. The next run with the same budget continues from the result cache.

**Score below the minimum.** The score is 62.5%, and the minimum is 80%.

| Killed | Survived | No coverage | Not measured | Score |
| --- | --- | --- | --- | --- |
| 10 | 6 | 2 | 0 | 62.5% |

Scope: the changed lines.

Each survived mutant is a small code change that made no test fail. The **Tests** findings above show the survived mutants on the changed lines. Add a test that fails on the change.

<details>
<summary>No test reaches these lines (2)</summary>

- `src/age.ts:30`, function `formatAge`
- `src/age.ts:41`

</details>

To inspect a mutant, run `fallow mutation inspect <mutant-id> --report fallow-mutation.json` with the `fallow-mutation.json` file of this job.
```

The job summary adds `The run took 4m 12s. 3 results came from the cache.`
after the scope line. It leaves out the sentence about the **Tests**
findings, because the summary has no findings tables.

The headline is the first match of this table:

| State | Headline |
|---|---|
| Unreadable report | `**Fallow could not read the mutation report** (`FALLOW_MUTATION_REPORT_INVALID`).` and the reason in a text block |
| Gate failed | `**Score below the minimum.** The score is X%, and the minimum is Y%.` |
| No mutant in the scope | `The scope has no mutant.` |
| No score, gate set | `**No mutant was counted.** The score gate was skipped.` |
| No score, no gate | `**No mutant was counted.**` |
| Survivors | `**N mutants survived.** Score X%.`, with ` (minimum Y%)` when a gate is set |
| Clean | `The tests detected each mutant that ran.` |

Other rules of the section:

- The score shows with one decimal and is rounded down, so a score just below
  the minimum never shows as equal to it.
- Killed counts `killed` and `timeout`. Not measured counts `pending`,
  `runtime_error`, `flaky` and `compile_error`.
- The partial-run line comes before the headline. The result-cache sentence
  shows only for the `time-budget` reason.
- `N survived mutants are not shown because their file changed after the run.`
  shows when the digest check hides survivors (see
  [Survivor findings](#survivor-findings)).
- `N survived mutants are on lines that this change did not add, so they have
  no finding.` shows when the diff filter removes survivors, for example in a
  run with the `all` scope.
- The sentence that points at the **Tests** findings (or, in a review, at the
  inline comments) shows only when the render has at least one survivor
  finding.
- `N more survived mutants are not shown inline. The fallow-mutation.json file
  of this job lists them.` shows when the inline cap applies.
- The gate lines and the decision row show the mutation score with one decimal,
  rounded down. The `observed` value of the gate entry keeps the full number.
- The no-coverage list shows 10 lines at most, with `(10 of N)` in the
  summary line.
- The inspect command adds `--root <root>` when the `--root` of `report` is
  not `.`.
- A section over 6000 characters keeps the heading, the partial-run line,
  the headline and the table, then
  `The mutation section is shortened. The fallow-mutation.json file of this job has all results.`

When the run did not finish, one text replaces the body of the section:

| State | Text |
|---|---|
| `event-refused` | `**Mutation testing did not run.** This event can run the code of a fork with the secrets of the base repository. Mutation testing runs only for the pull_request, push, schedule, workflow_dispatch and merge_group events.` |
| `fork-refused` | `**Mutation testing did not run.** The merge request comes from a fork, and the pipeline runs in the target project.` |
| `no-change-scope` | `**Mutation testing did not run.** The changed scope needs a pull request diff or a base ref, and this job has neither. Run the job in a pull request, set a base ref, or use the all scope.` |
| `interrupted` | `**Mutation testing stopped.** A signal stopped the run, and the run wrote no report.` |
| Error envelope | `**Mutation testing failed** (exit code 2, `FALLOW_X`).`, then the message in a text block, then the help in a second text block when present |

The message is capped at 1000 characters and ends in ` ... (shortened)` when
cut. The text blocks use a fence of four backticks.

No source line and no mutated line reaches a CI body. `fallow mutation
inspect` shows the code on your machine.

### Survivor findings

Each `survived` mutant is a finding with the rule `fallow/mutation-survived`
in the category **Tests**. The finding goes through the same diff filter,
tables, decision annotations, review comments and Code Quality entries as
an analysis finding, so in a pull request only the survivors on the diff
show. The description is plain text, for example:

```text
Changing >= to > in isAdult made no test fail. Mutant 9f3c2a1b.
```

- `report` sorts the survivors by file, line, column and mutant id, applies
  the shared diff filter, and keeps the first 20 (`MUTATION_MAX_INLINE`). The
  section counts the rest.
- `report` compares the SHA-256 digest of each file in the report with the
  file under `--root`. A survivor in a file that changed, or that is missing,
  is hidden and counted in the section. So a render in a later job on
  another commit shows no stale survivor.
- The CodeClimate severity is `minor` and the category is `Bug Risk`. The
  fingerprint is a hash of the rule, the path, the operator and the mutant
  id, not of the line. A review thread therefore stays when the function
  moves.
- The inline review gets one comment for each survivor on an added line, in
  the layout of the other findings. The comment never has a `suggestion`
  block. With `FALLOW_REVIEW_GUIDANCE=1`, it adds the guidance of the rule:
  "Add or change a test so that it fails on this change. To see the code,
  run `fallow mutation inspect` with the mutant ID that the finding names."
  A survivor on the
  same line as an analysis finding merges into one comment.
- The review summary gets one line, for example
  `Mutation testing: score 62.5%, 6 survivors (4 inline).`, then the section.
  In the review summary, the section says that the inline comments show the
  survived mutants.
- `github-annotations` prints one
  `::warning file=<path>,line=<line>,title=fallow/mutation-survived::<description>`
  line for each survivor, after the analysis annotations and inside the
  annotation budget. The line has no column.
- `no_coverage` mutants are not findings. They stay in the capped list of the
  section.

`fallow explain fallow/mutation-survived` shows the rule. `fallow schema`
lists it with the command `mutation`.

### Check run and decision

The PR decision (`fallow-pr-decision/v1`) gets a `mutation` row (label
"Mutation testing") after the command row. Its scope is `changed lines`,
`all files`, `selected paths`, or `-` when the report has no scope.

| State | Status | Observed |
|---|---|---|
| Complete run, no survivor, no no-coverage mutant | `success` | `100.0% score` |
| No mutant in the scope | `success` | `no mutant` |
| Survivors | `neutral` | `62.5% score, 6 survived` |
| No-coverage mutants, no survivor | `neutral` | `100.0% score, 2 not covered` |
| Partial run | `neutral` | `partial: 40 of 120 mutants ran` |
| `event-refused`, `fork-refused`, `no-change-scope` | `skipped` | `not run` |
| Error envelope, `interrupted`, unreadable report | `failure` | `failed` |

When the report has a gate entry, the existing gate rows add a
`Mutation score` row with the observed score and the minimum, with the scope
`this run`. The survivor annotations of the decision come after the analysis
annotations.

The aggregate conclusion of the check run does not read the gate rows, so a
failed mutation gate does not change it. To block a merge on the score, post
the gates as separate checks with `--split-gates` and make
`Fallow / mutation-min-score` a required check. The job itself fails in the
result step.

`github-annotations` also prints one run-state line before the analysis
annotations: `::error title=Mutation testing::` for a failed run, an
interrupted run or an unreadable report, and
`::notice title=Mutation testing::Partial run (<reasons>). The annotations cover only the mutants that ran.`
for a partial run.

### Gate

`fallow mutation` writes the gate into its report as
`gate_outcomes["mutation-min-score"]` when `--min-score` or
`mutation.minScore` sets a minimum:

```json
"gate_outcomes": {
  "mutation-min-score": { "status": "fail", "enforced": true, "observed": 62.5, "threshold": 80.0 }
}
```

- `status` is `fail` when the score in percent is lower than the minimum,
  `pass` otherwise, and `skipped` when `summary.score` is null. A `skipped`
  entry has `observed: null` and keeps the `threshold`.
- `enforced` is always `true`. The exit code 1 of `fallow mutation` and a
  `fail` entry are one rule.
- A `--recheck` run drops the minimum, so its report has no entry.

`fallow report --mutation-report` merges this entry into the `gate_outcomes`
of the analysis envelope. Only the `mutation-min-score` key is merged. The
status note, the summary gate line and the annotation gate notice then name
it, for example `mutation-min-score (62.5 against 80)`. The Action outputs
`gates-failed`, `gates-warned`, `gates-skipped` and `gates-passed`, and the
GitLab `FALLOW_GATES_*` dotenv lists, name `mutation-min-score`.

A gate that is set is always enforced in CI. For an advisory run, leave
`mutation-min-score` (`FALLOW_MUTATION_MIN_SCORE`) and `mutation.minScore`
empty.

### Exit codes in CI

The mutation step itself always finishes, so the analysis and the posting
steps still run. The result step applies the outcome:

| Outcome | `fallow mutation` exit code | Job result |
|---|---|---|
| `ok` | `0` (complete or partial) | passes |
| `skipped` | not run (`no-change-scope`) | passes |
| `gate-failed` | `1` | fails with exit code 1 |
| `tool-failed` | `2`, or no usable result | fails with exit code 2 and the error code and message |
| `interrupted` | `128 + signal` | fails with exit code 2 |
| `refused` | not run (`event-refused` or `fork-refused`) | fails with exit code 2 |

An invalid mutation input is a `tool-failed` outcome: fallow validates the
input, exits 2 and prints its error envelope. A `fallow` without the
`mutation` command is also `tool-failed`, with the message
`fallow <version> has no mutation command.`

The Action has the outputs `mutation-outcome`, `mutation-completion`,
`mutation-score`, `mutation-results` and `mutation-report-rendered`. The
GitLab dotenv report has `FALLOW_MUTATION_OUTCOME`.

### Version compatibility

The scripts probe the capability, not the version. The Action scope step and
the GitLab change-scope block read `fallow report --help` and look for
`--mutation-report`. The result is `HAS_MUTATION_REPORT` (Action, also the
output `mutation-report-rendered`) or `FALLOW_HAS_MUTATION_REPORT` (GitLab).

| Scripts | CLI | Behavior |
|---|---|---|
| Current | Has `--mutation-report` | Rust renders all mutation output. |
| Current | Has `fallow mutation`, no `--mutation-report` | The run, the cache, the result files and the result step work. The gate fails closed: the result step still fails the job on exit code 1. The comment, the review, the annotations and the summary have no mutation content. The job summary says `Mutation results are not in this summary: fallow <version> cannot render them. Use fallow <version> or later.` The GitLab job log prints a warning with the same advice. |
| Current | No `fallow mutation` | The probe `fallow mutation --help` fails. The outcome is `tool-failed`, and the result step exits 2. The analysis still runs and posts. |
| Earlier | Current | The earlier scripts pass no flag and keep their own section. The `gate_outcomes` member of the report is additive. |

The lowest version with `--mutation-report` is `MUTATION_REPORT_VERSION` in
`ci/scripts/mutation_common.sh`. The GitLab template pins its scripts to the
installed CLI version. The change-scope block is inline in the template, so
a custom `FALLOW_SCRIPTS_REF` needs no new script.

### Fork safety

The run executes the tests of the pull request. A test can read each secret
that the job has. The event and fork checks below are the security boundary.
They run in the shell, before any test.

- GitHub: the step runs only for the `pull_request`, `push`, `schedule`,
  `workflow_dispatch` and `merge_group` events. A `pull_request` job from a
  fork gets no secrets and a read-only token. The other four events run code
  of the repository itself. Each other event, for example
  `pull_request_target`, `issue_comment` or `workflow_run`, runs with the
  secrets and a write token of the base repository, and the job can check out
  the code of a fork. The payload of `issue_comment` and `workflow_run` does
  not always name the pull request, so the step refuses these events
  (`event-refused`). Do not check out the head of a fork pull request in a
  `pull_request_target` job: each later step runs the code of the fork with
  your secrets.
- GitHub: a private repository can send secrets to the workflows of fork pull
  requests (the setting "Send secrets and variables to workflows from fork
  pull requests"). The step cannot see that setting. Do not turn on mutation
  testing in such a repository for fork pull requests.
- GitLab: a merge request pipeline from a fork that runs in the target project
  has the CI/CD variables of the target project. The job refuses it
  (`fork-refused`). A pipeline that runs in the fork project has only the
  variables of the fork, and the job runs.

The run also removes values from the environment of the tests. This is
defense in depth, not a boundary:

- `GH_TOKEN`, `GITHUB_TOKEN`, `GITLAB_TOKEN`, `FALLOW_MUTATION_GH_TOKEN`, the
  GitHub OIDC request token and the Actions runtime token;
- `GITHUB_ENV`, `GITHUB_OUTPUT`, `GITHUB_PATH`, `GITHUB_STEP_SUMMARY`,
  `GITHUB_STATE` and `BASH_ENV`, so a test cannot write to the files of
  the later steps through these paths.

The mutation step gets no token. Only the scope step, which runs no project
code, gets `GH_TOKEN` for `gh pr diff`. `CI_JOB_TOKEN` stays, because a test
can need it for a package registry. Other secrets that the workflow puts into
the environment of the job stay visible to the tests.

A test process can live until the job ends, and it can change files that
later steps read. `fallow report` therefore treats `fallow-mutation.json`,
the status file and the scope diff as untrusted input. It caps the size,
checks the kind and the paths, checks the file digests, escapes each value
for markdown and for workflow commands, and caps each list. The read-only
token of a fork pull request limits the impact.

### Result cache in CI

The result cache (`<root>/.fallow/mutation/results.json`) makes the next run
faster, and a budgeted run continues after the stored results. The render
reads only the report of the job, never the cache.

- GitHub: `actions/cache/restore` and `actions/cache/save` keep the file. The
  key has the runner OS, the root, a digest of the lockfiles and the runner
  config files, and the run id. A restore takes the newest entry of the same
  digest. The analysis cache leaves out `.fallow/mutation`. A fork pull
  request restores the cache but never saves it, so its tests cannot write
  results that a later trusted run reads.
- GitLab: the job cache keeps `.fallow/`. A GitLab cache key cannot hash
  files that the template does not know, so the job writes the same digest to
  `.fallow/mutation/ci-cache-key`. A restored cache with another digest is
  removed before the run. A fork pipeline that runs in the fork project
  writes only to the cache of the fork project.

The digest covers `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`,
`pnpm-lock.yaml`, `bun.lock` and `bun.lockb` at the root and at the top of the
git worktree, and `vitest.config.*`, `vite.config.*`, `vitest.workspace.*`,
`bunfig.toml`, `package.json` and `tsconfig.json` at the root. The keys of
the entries in the file stay the authority (see [Result cache](#result-cache)).
A config that moves the cache directory moves the file out of the CI cache.

### Base ref on GitLab

The change-scope block fetches the base commit with `--depth=1` when the
clone does not have it. The diff uses three dots (from the merge base)
first, and two dots when the shallow clone has no merge base. When both
fail, the job prints a `[shallow-clone]` warning, and the run uses
`--changed-since`, which can stop with `FALLOW_CHANGED_FILES_FAILED` (a
missing commit or merge base) or `FALLOW_MUTATION_BASE_NOT_FOUND` (a ref
that does not resolve). Set `GIT_DEPTH: "0"` for that job.

## Ownership

| Part | Path |
|---|---|
| Mutant sites and instrumentation | `crates/mutation` |
| Component script blocks | `crates/mutation/src/blocks.rs`, `crates/engine/src/mutation/layout.rs` |
| Selection, test map, prune and rank | `crates/engine/src/mutation` |
| Runner protocol, companion, scheduler, report, verdict join, survivor type check, outcome journal | `crates/api/src/mutation` |
| Type check request and response | `crates/api/src/type_aware/mutant_typecheck.rs` |
| Type check in the companion | `tools/type-aware-sidecar/src/mutant-typecheck.mjs` |
| Envelopes | `crates/output/src/mutation.rs` |
| Config section | `crates/config/src/config/mutation.rs` |
| CLI adapter, human output, inspect | `crates/cli/src/mutation_cli.rs`, `mutation_human.rs`, `mutation_inspect.rs` |
| Health join | `crates/cli/src/health/mutation_report.rs`, `crates/output/src/health_mutation.rs` |
| MCP plan and inspect tools | `crates/mcp/src/tools/mutation.rs` |
| Node companion and runner adapters | `tools/mutation-runner` |
| Runner selection | `crates/api/src/mutation/runner_select.rs` |
| Score gate entry | `crates/cli/src/gates.rs` (`mutation_gate_outcomes`) |
| Rule `fallow/mutation-survived` | `crates/api/src/explain.rs` (`MUTATION_RULES`) |
| CI section, decision row and survivor text | `crates/output/src/mutation_ci.rs` |
| Report reader for `report --mutation-report` | `crates/cli/src/report/mutation_evidence.rs` |
| Join of the mutation evidence to the render | `crates/cli/src/cli_report.rs` |
| CI step, status file, render flags and cache digest | `ci/scripts/mutation_common.sh` |
| GitHub Action change scope | `action/scripts/scope.sh` |
| GitHub Action step and render flags | `action/scripts/mutation.sh`, `action/scripts/render-common.sh`, `action.yml` |
| GitLab template step and change scope | `ci/scripts/mutation.sh`, `ci/gitlab-ci.yml` |

## Tests

- `cargo test -p fallow-cli mutation` runs the CLI unit tests and the
  integration tests in `crates/cli/tests/integration/mutation_tests.rs`.
- The integration tests use a project with no test files for the exit policy,
  the verdict join and inspect, so they need no JavaScript runtime.
- The Vitest end-to-end test needs `node` on `PATH` and `npm ci` in
  `tools/mutation-runner`. Without them it prints a skip message and passes.
  The type check end-to-end test also needs `npm ci` in
  `tools/type-aware-sidecar`.
- `node --test test/mutant-typecheck.test.mjs` in `tools/type-aware-sidecar`
  tests the companion side of the type check.
- The `node:test` end-to-end test needs a supported `node` on `PATH`, and
  the `bun test` end-to-end test needs a supported `bun`. Without it, the
  test prints a skip message and passes.
- `cargo test -p fallow-output mutation_ci` runs the snapshot tests of the
  CI section, one for each state.
- `crates/cli/tests/integration/report_mutation_tests.rs` tests
  `fallow report --mutation-report` and `--mutation-not-run` on each format,
  with git projects that the tests build in temp directories.
- `bash ci/tests/run.sh` and `bash action/tests/run.sh` test the CI step,
  the render flags, the change scope, the exit codes and the fork refusal
  with a fake `fallow` binary (`ci/tests/mutation.sh`,
  `action/tests/mutation.sh`, `action/tests/scope.sh`).
- `npm test` in `tools/mutation-runner` runs the companion suites. The
  `bun test` suite is skipped when `bun` is not on `PATH`.
