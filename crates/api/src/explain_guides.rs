//! Worked examples and fix texts for the structure, styling, runtime coverage
//! and coverage intelligence rules of [`crate::explain`].

use crate::explain::RuleGuide;

const STRUCTURE_RULES: &[&str] = &[
    "fallow/re-export-cycle",
    "fallow/package-cycle",
    "fallow/route-collision",
    "fallow/dynamic-segment-name-conflict",
    "fallow/invalid-client-export",
    "fallow/mixed-client-server-barrel",
    "fallow/misplaced-directive",
];

/// Whether [`structure_rule_guide`] has a guide for `id`.
pub fn is_structure_rule(id: &str) -> bool {
    STRUCTURE_RULES.contains(&id)
}

/// The guide of a module-graph, package or route-structure rule.
pub fn structure_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/re-export-cycle" => RuleGuide {
            example: "src/ui/index.ts has `export * from './forms'`, and src/ui/forms/index.ts has `export * from '..'`, so the two barrels re-export from each other.",
            how_to_fix: "Remove one `export ... from` statement in the loop that points back into the cycle. For a self-loop, remove the statement with which the file re-exports itself.",
        },
        "fallow/package-cycle" => RuleGuide {
            example: "packages/api imports `@acme/db`, and packages/db imports a type from `@acme/api`.",
            how_to_fix: "Remove the imports on one hop. Move the code that both packages need into a third package that both packages import.",
        },
        "fallow/route-collision" => RuleGuide {
            example: "`app/(marketing)/about/page.tsx` and `app/(shop)/about/page.tsx` both resolve to the URL `/about`.",
            how_to_fix: "Move or merge one of the files, so that each URL has one page or one route handler.",
        },
        "fallow/dynamic-segment-name-conflict" => RuleGuide {
            example: "`app/blog/[id]/page.tsx` and `app/blog/[slug]/edit/page.tsx` use two names for the dynamic segment at the same position.",
            how_to_fix: "Rename the dynamic segments at that position to one name, for example `[slug]` in both paths.",
        },
        "fallow/invalid-client-export" => RuleGuide {
            example: "app/page.tsx starts with `\"use client\"` and exports `metadata`.",
            how_to_fix: "Move the server-only export to a module without `\"use client\"`, such as a server component or `layout.tsx`. Or remove the directive when the module does not need to be a client boundary.",
        },
        "fallow/mixed-client-server-barrel" => RuleGuide {
            example: "components/index.ts re-exports `Button` from a `\"use client\"` module and `getUser` from a module that imports `server-only`.",
            how_to_fix: "Split the barrel: re-export the client modules from one file and the server-only modules from another file.",
        },
        "fallow/misplaced-directive" => RuleGuide {
            example: "A file has `import { useState } from 'react';` on line 1 and `\"use client\";` on line 2.",
            how_to_fix: "Move the directive to the first line of the file, above every import.",
        },
        _ => return None,
    })
}

/// The guide of a styling, runtime coverage or coverage intelligence rule.
pub fn health_rule_guide(id: &str) -> Option<RuleGuide> {
    styling_rule_guide(id)
        .or_else(|| runtime_rule_guide(id))
        .or_else(|| coverage_intelligence_rule_guide(id))
}

fn styling_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/css-token-drift" => RuleGuide {
            example: "Markup uses `class=\"w-[13px] bg-[#1a2b3c]\"`, Tailwind arbitrary values, or a stylesheet sets `color: #1a2b3c` where the project has color tokens.",
            how_to_fix: "Replace the value with the nearest token of the scale. Keep the value when the one-off value is deliberate.",
        },
        "fallow/css-duplicate-block" => RuleGuide {
            example: "`.card` in src/card.css and `.panel` in src/panel.css declare the same four or more declarations.",
            how_to_fix: "Move the shared declarations to one class or a shared mixin, and use it from both places.",
        },
        "fallow/css-selector-complexity" => RuleGuide {
            example: "`.sidebar nav ul li a.active` sits four levels deep in nested SCSS, and its block uses `!important` on most declarations.",
            how_to_fix: "Flatten the nesting, put a class on the element instead of a long descendant chain, and remove `!important` where the order of the rules can do the work.",
        },
        "fallow/css-dead-surface" => RuleGuide {
            example: "Card.vue defines `.card__badge` in `<style scoped>`, but no part of the component uses that class.",
            how_to_fix: "Delete the unused class after you check that no code adds it at runtime, for example through a class name built from a variable.",
        },
        "fallow/css-broken-reference" => RuleGuide {
            example: "src/app.css sets `animation: fade-in 1s`, but no stylesheet defines `@keyframes fade-in`. Or markup uses `class=\"nav-item\"` where only `.nav-items` exists.",
            how_to_fix: "Correct the name or add the missing definition. Keep the reference when a CSS-in-JS library or an external stylesheet defines the name.",
        },
        _ => return None,
    })
}

fn runtime_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/runtime-safe-to-delete" => RuleGuide {
            example: "No module references `formatLegacyInvoice` in src/billing/legacy.ts, and the production capture shows no call to it.",
            how_to_fix: "Check that the capture window covers rare paths, such as a monthly job, then delete the function.",
        },
        "fallow/runtime-review-required" => RuleGuide {
            example: "src/reports/page.tsx imports `exportCsv`, but the production capture shows no call to it.",
            how_to_fix: "Ask the owner whether the path is seasonal, an error path or dead. Remove the call sites and the function when nobody needs them.",
        },
        "fallow/runtime-low-traffic" => RuleGuide {
            example: "`legacyLogin` ran 3 times in a capture of 2 million calls, below the `--low-traffic-threshold` fraction.",
            how_to_fix: "Find the remaining callers, move them to the current path, then remove the function. Keep it when the rare callers matter.",
        },
        "fallow/runtime-coverage-unavailable" => RuleGuide {
            example: "`renderChart` runs in a worker thread, so the capture of the main process has no coverage entry for it.",
            how_to_fix: "Capture coverage in the worker too, or check that the source maps map the bundle to the source file. Do not delete code because of this finding.",
        },
        "fallow/runtime-coverage" => RuleGuide {
            example: "The runtime coverage add-on gives `syncInventory` a verdict that this fallow version does not know, so the finding has the `unknown` verdict.",
            how_to_fix: "Update fallow, then run the analysis again. Until then, treat the finding as no evidence for or against the function.",
        },
        _ => return None,
    })
}

fn coverage_intelligence_rule_guide(id: &str) -> Option<RuleGuide> {
    Some(match id {
        "fallow/coverage-intelligence-risky-change" => RuleGuide {
            example: "The change touches `applyDiscount`, a hot path in production, which has less than 70% test coverage and a CRAP score above the limit.",
            how_to_fix: "Add tests for the changed function before you merge, or split the change so that the hot path changes less.",
        },
        "fallow/coverage-intelligence-delete" => RuleGuide {
            example: "No module references `oldExport`, the production capture shows no call to it, and no test reaches it.",
            how_to_fix: "Confirm with the owner of the file, then delete the code.",
        },
        "fallow/coverage-intelligence-review" => RuleGuide {
            example: "`syncLegacy` is reachable but never ran in production, no test covers it, and its file has no owner or an inactive owner.",
            how_to_fix: "Give the code an owner, and let that owner decide whether to change or delete it.",
        },
        "fallow/coverage-intelligence-refactor" => RuleGuide {
            example: "`calculateTax` runs often in production and has 70% or more test coverage, but its CRAP score is above the limit.",
            how_to_fix: "Refactor the function in small steps, and run the tests after each step.",
        },
        _ => return None,
    })
}
