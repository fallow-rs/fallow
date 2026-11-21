//! The example projects of `crates/api/rule-examples/`, embedded.
//!
//! Generated from that directory. Do not edit by hand. To write the file again, run
//! `FALLOW_UPDATE_RULE_EXAMPLES=1 cargo test -p fallow-api --lib explain_examples`.

use super::{ExampleFile, RuleExample};

macro_rules! file {
    ($rule:literal, $part:literal, $path:literal, $hidden:literal) => {
        ExampleFile {
            path: $path,
            code: include_str!(concat!("../rule-examples/", $rule, "/", $part, "/", $path)),
            hidden: $hidden,
        }
    };
}

#[rustfmt::skip]
pub(super) const RULE_EXAMPLES: &[RuleExample] = &[
    RuleExample {
        rule: "absent-component-prop",
        args: &[],
        base: &[],
        files: &[
            file!("absent-component-prop", "files", ".fallowrc.json", false),
            file!("absent-component-prop", "files", "package.json", true),
            file!("absent-component-prop", "files", "src/Card.tsx", false),
            file!("absent-component-prop", "files", "src/main.tsx", false),
        ],
    },
    RuleExample {
        rule: "added-dependency",
        args: &[],
        base: &[
            file!("added-dependency", "base", ".fallowrc.json", false),
            file!("added-dependency", "base", "package.json", false),
            file!("added-dependency", "base", "src/order.ts", false),
        ],
        files: &[
            file!("added-dependency", "files", ".fallowrc.json", false),
            file!("added-dependency", "files", "package.json", false),
            file!("added-dependency", "files", "src/order.ts", false),
        ],
    },
    RuleExample {
        rule: "added-suppression",
        args: &[],
        base: &[
            file!("added-suppression", "base", "package.json", true),
            file!("added-suppression", "base", "src/cart.ts", false),
        ],
        files: &[
            file!("added-suppression", "files", "package.json", true),
            file!("added-suppression", "files", "src/cart.ts", false),
        ],
    },
    RuleExample {
        rule: "angular-trusted-html",
        args: &[],
        base: &[],
        files: &[
            file!("angular-trusted-html", "files", "package.json", true),
            file!("angular-trusted-html", "files", "src/comment-html.ts", false),
        ],
    },
    RuleExample {
        rule: "boundary-call-violation",
        args: &[],
        base: &[],
        files: &[
            file!("boundary-call-violation", "files", ".fallowrc.json", false),
            file!("boundary-call-violation", "files", "package.json", true),
            file!("boundary-call-violation", "files", "src/domain/policy.ts", false),
        ],
    },
    RuleExample {
        rule: "boundary-coverage",
        args: &[],
        base: &[],
        files: &[
            file!("boundary-coverage", "files", ".fallowrc.json", false),
            file!("boundary-coverage", "files", "package.json", true),
            file!("boundary-coverage", "files", "src/generated/client.ts", false),
            file!("boundary-coverage", "files", "src/index.ts", false),
        ],
    },
    RuleExample {
        rule: "boundary-violation",
        args: &[],
        base: &[],
        files: &[
            file!("boundary-violation", "files", ".fallowrc.json", false),
            file!("boundary-violation", "files", "package.json", true),
            file!("boundary-violation", "files", "src/app/admin.ts", false),
            file!("boundary-violation", "files", "src/features/billing/invoice.ts", false),
        ],
    },
    RuleExample {
        rule: "circular-dependency",
        args: &[],
        base: &[],
        files: &[
            file!("circular-dependency", "files", "package.json", true),
            file!("circular-dependency", "files", "src/cart.ts", false),
            file!("circular-dependency", "files", "src/order.ts", false),
        ],
    },
    RuleExample {
        rule: "cleartext-transport",
        args: &[],
        base: &[],
        files: &[
            file!("cleartext-transport", "files", "package.json", true),
            file!("cleartext-transport", "files", "src/orders-client.ts", false),
        ],
    },
    RuleExample {
        rule: "client-server-leak",
        args: &[],
        base: &[],
        files: &[
            file!("client-server-leak", "files", "package.json", true),
            file!("client-server-leak", "files", "src/CheckoutButton.tsx", false),
            file!("client-server-leak", "files", "src/main.tsx", true),
            file!("client-server-leak", "files", "src/payments.ts", false),
        ],
    },
    RuleExample {
        rule: "code-duplication",
        args: &[],
        base: &[],
        files: &[
            file!("code-duplication", "files", "package.json", true),
            file!("code-duplication", "files", "src/index.ts", true),
            file!("code-duplication", "files", "src/invoice.ts", false),
            file!("code-duplication", "files", "src/order.ts", false),
        ],
    },
    RuleExample {
        rule: "code-injection",
        args: &[],
        base: &[],
        files: &[
            file!("code-injection", "files", "package.json", true),
            file!("code-injection", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "command-injection",
        args: &[],
        base: &[],
        files: &[
            file!("command-injection", "files", "package.json", true),
            file!("command-injection", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "constant-parameter",
        args: &[],
        base: &[],
        files: &[
            file!("constant-parameter", "files", ".fallowrc.json", false),
            file!("constant-parameter", "files", "package.json", true),
            file!("constant-parameter", "files", "src/format.ts", false),
            file!("constant-parameter", "files", "src/index.ts", false),
        ],
    },
    RuleExample {
        rule: "css-broken-reference",
        args: &["--css"],
        base: &[],
        files: &[
            file!("css-broken-reference", "files", "package.json", true),
            file!("css-broken-reference", "files", "src/app.css", false),
            file!("css-broken-reference", "files", "src/index.ts", false),
        ],
    },
    RuleExample {
        rule: "css-dead-surface",
        args: &["--css"],
        base: &[],
        files: &[
            file!("css-dead-surface", "files", "package.json", true),
            file!("css-dead-surface", "files", "src/Card.vue", false),
            file!("css-dead-surface", "files", "src/main.ts", true),
        ],
    },
    RuleExample {
        rule: "css-duplicate-block",
        args: &["--css"],
        base: &[],
        files: &[
            file!("css-duplicate-block", "files", "package.json", true),
            file!("css-duplicate-block", "files", "src/card.css", false),
            file!("css-duplicate-block", "files", "src/index.ts", true),
            file!("css-duplicate-block", "files", "src/panel.css", false),
        ],
    },
    RuleExample {
        rule: "css-selector-complexity",
        args: &["--css"],
        base: &[],
        files: &[
            file!("css-selector-complexity", "files", "package.json", true),
            file!("css-selector-complexity", "files", "src/index.ts", true),
            file!("css-selector-complexity", "files", "src/sidebar.scss", false),
        ],
    },
    RuleExample {
        rule: "css-token-drift",
        args: &["--css"],
        base: &[],
        files: &[
            file!("css-token-drift", "files", "package.json", true),
            file!("css-token-drift", "files", "src/button.css", false),
            file!("css-token-drift", "files", "src/index.ts", true),
            file!("css-token-drift", "files", "src/theme.css", false),
        ],
    },
    RuleExample {
        rule: "dangerous-html",
        args: &[],
        base: &[],
        files: &[
            file!("dangerous-html", "files", "package.json", true),
            file!("dangerous-html", "files", "src/comments.ts", false),
        ],
    },
    RuleExample {
        rule: "deprecated-cipher",
        args: &[],
        base: &[],
        files: &[
            file!("deprecated-cipher", "files", "package.json", true),
            file!("deprecated-cipher", "files", "src/vault.ts", false),
        ],
    },
    RuleExample {
        rule: "deprecated-export-in-use",
        args: &[],
        base: &[],
        files: &[
            file!("deprecated-export-in-use", "files", ".fallowrc.json", false),
            file!("deprecated-export-in-use", "files", "package.json", true),
            file!("deprecated-export-in-use", "files", "src/cart.ts", false),
            file!("deprecated-export-in-use", "files", "src/checkout.ts", false),
            file!("deprecated-export-in-use", "files", "src/index.ts", true),
            file!("deprecated-export-in-use", "files", "src/money.ts", false),
        ],
    },
    RuleExample {
        rule: "dev-dependency-in-production",
        args: &[],
        base: &[],
        files: &[
            file!("dev-dependency-in-production", "files", "package.json", false),
            file!("dev-dependency-in-production", "files", "src/config.ts", false),
        ],
    },
    RuleExample {
        rule: "diverged-clone-sibling",
        args: &[],
        base: &[
            file!("diverged-clone-sibling", "base", ".fallowrc.json", false),
            file!("diverged-clone-sibling", "base", "package.json", true),
            file!("diverged-clone-sibling", "base", "src/cart/total.ts", false),
            file!("diverged-clone-sibling", "base", "src/index.ts", true),
            file!("diverged-clone-sibling", "base", "src/invoice/total.ts", false),
        ],
        files: &[
            file!("diverged-clone-sibling", "files", ".fallowrc.json", false),
            file!("diverged-clone-sibling", "files", "package.json", true),
            file!("diverged-clone-sibling", "files", "src/cart/total.ts", false),
            file!("diverged-clone-sibling", "files", "src/index.ts", true),
            file!("diverged-clone-sibling", "files", "src/invoice/total.ts", false),
        ],
    },
    RuleExample {
        rule: "dom-document-write",
        args: &[],
        base: &[],
        files: &[
            file!("dom-document-write", "files", "package.json", true),
            file!("dom-document-write", "files", "src/main.ts", false),
        ],
    },
    RuleExample {
        rule: "duplicate-export",
        args: &[],
        base: &[],
        files: &[
            file!("duplicate-export", "files", "package.json", true),
            file!("duplicate-export", "files", "src/components/button.ts", false),
            file!("duplicate-export", "files", "src/index.ts", false),
            file!("duplicate-export", "files", "src/ui/button.ts", false),
        ],
    },
    RuleExample {
        rule: "duplicate-export-alias",
        args: &[],
        base: &[],
        files: &[
            file!("duplicate-export-alias", "files", "package.json", true),
            file!("duplicate-export-alias", "files", "src/index.ts", false),
            file!("duplicate-export-alias", "files", "src/worker.ts", false),
        ],
    },
    RuleExample {
        rule: "duplicate-prop-shape",
        args: &[],
        base: &[],
        files: &[
            file!("duplicate-prop-shape", "files", ".fallowrc.json", false),
            file!("duplicate-prop-shape", "files", "package.json", true),
            file!("duplicate-prop-shape", "files", "src/CheckoutForm.tsx", true),
            file!("duplicate-prop-shape", "files", "src/FieldNumber.tsx", false),
            file!("duplicate-prop-shape", "files", "src/FieldSelect.tsx", false),
            file!("duplicate-prop-shape", "files", "src/FieldText.tsx", false),
            file!("duplicate-prop-shape", "files", "src/main.tsx", true),
        ],
    },
    RuleExample {
        rule: "dynamic-module-load",
        args: &[],
        base: &[],
        files: &[
            file!("dynamic-module-load", "files", "package.json", true),
            file!("dynamic-module-load", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "dynamic-regex",
        args: &[],
        base: &[],
        files: &[
            file!("dynamic-regex", "files", "package.json", true),
            file!("dynamic-regex", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "dynamic-segment-name-conflict",
        args: &[],
        base: &[],
        files: &[
            file!("dynamic-segment-name-conflict", "files", "app/blog/[id]/page.tsx", false),
            file!("dynamic-segment-name-conflict", "files", "app/blog/[slug]/edit/page.tsx", false),
            file!("dynamic-segment-name-conflict", "files", "app/layout.tsx", true),
            file!("dynamic-segment-name-conflict", "files", "package.json", true),
        ],
    },
    RuleExample {
        rule: "electron-unsafe-webpreferences",
        args: &[],
        base: &[],
        files: &[
            file!("electron-unsafe-webpreferences", "files", "package.json", true),
            file!("electron-unsafe-webpreferences", "files", "src/main.ts", false),
        ],
    },
    RuleExample {
        rule: "empty-catalog-group",
        args: &[],
        base: &[],
        files: &[
            file!("empty-catalog-group", "files", "package.json", true),
            file!("empty-catalog-group", "files", "packages/app/package.json", true),
            file!("empty-catalog-group", "files", "packages/app/src/index.ts", true),
            file!("empty-catalog-group", "files", "pnpm-workspace.yaml", false),
        ],
    },
    RuleExample {
        rule: "feature-flag",
        args: &[],
        base: &[],
        files: &[
            file!("feature-flag", "files", "package.json", true),
            file!("feature-flag", "files", "src/checkout.ts", false),
        ],
    },
    RuleExample {
        rule: "flag-retirement-candidate",
        args: &["--retirement"],
        base: &[],
        files: &[
            file!("flag-retirement-candidate", "files", "package.json", true),
            file!("flag-retirement-candidate", "files", "src/checkout.ts", false),
        ],
    },
    RuleExample {
        rule: "hardcoded-secret",
        args: &[],
        base: &[],
        files: &[
            file!("hardcoded-secret", "files", ".fallowrc.json", false),
            file!("hardcoded-secret", "files", "package.json", true),
            file!("hardcoded-secret", "files", "src/billing.ts", false),
        ],
    },
    RuleExample {
        rule: "header-injection",
        args: &[],
        base: &[],
        files: &[
            file!("header-injection", "files", "package.json", true),
            file!("header-injection", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "high-cognitive-complexity",
        args: &[],
        base: &[],
        files: &[
            file!("high-cognitive-complexity", "files", ".fallowrc.json", false),
            file!("high-cognitive-complexity", "files", "package.json", true),
            file!("high-cognitive-complexity", "files", "src/discount.ts", false),
            file!("high-cognitive-complexity", "files", "test/discount.test.ts", false),
        ],
    },
    RuleExample {
        rule: "high-complexity",
        args: &[],
        base: &[],
        files: &[
            file!("high-complexity", "files", ".fallowrc.json", false),
            file!("high-complexity", "files", "package.json", true),
            file!("high-complexity", "files", "src/import.ts", false),
            file!("high-complexity", "files", "test/import.test.ts", false),
        ],
    },
    RuleExample {
        rule: "high-crap-score",
        args: &[],
        base: &[],
        files: &[
            file!("high-crap-score", "files", "package.json", true),
            file!("high-crap-score", "files", "src/shipping.ts", false),
        ],
    },
    RuleExample {
        rule: "high-cyclomatic-complexity",
        args: &[],
        base: &[],
        files: &[
            file!("high-cyclomatic-complexity", "files", ".fallowrc.json", false),
            file!("high-cyclomatic-complexity", "files", "package.json", true),
            file!("high-cyclomatic-complexity", "files", "src/router.ts", false),
            file!("high-cyclomatic-complexity", "files", "test/router.test.ts", false),
        ],
    },
    RuleExample {
        rule: "insecure-cookie",
        args: &[],
        base: &[],
        files: &[
            file!("insecure-cookie", "files", "package.json", true),
            file!("insecure-cookie", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "insecure-randomness",
        args: &[],
        base: &[],
        files: &[
            file!("insecure-randomness", "files", "package.json", true),
            file!("insecure-randomness", "files", "src/reset.ts", false),
        ],
    },
    RuleExample {
        rule: "insecure-temp-file",
        args: &[],
        base: &[],
        files: &[
            file!("insecure-temp-file", "files", "package.json", true),
            file!("insecure-temp-file", "files", "src/report.ts", false),
        ],
    },
    RuleExample {
        rule: "invalid-client-export",
        args: &[],
        base: &[],
        files: &[
            file!("invalid-client-export", "files", "app/page.tsx", false),
            file!("invalid-client-export", "files", "package.json", true),
        ],
    },
    RuleExample {
        rule: "jquery-html",
        args: &[],
        base: &[],
        files: &[
            file!("jquery-html", "files", "package.json", true),
            file!("jquery-html", "files", "src/preview.ts", false),
        ],
    },
    RuleExample {
        rule: "jwt-alg-none",
        args: &[],
        base: &[],
        files: &[
            file!("jwt-alg-none", "files", "package.json", true),
            file!("jwt-alg-none", "files", "src/token.ts", false),
        ],
    },
    RuleExample {
        rule: "jwt-verify-missing-algorithms",
        args: &[],
        base: &[],
        files: &[
            file!("jwt-verify-missing-algorithms", "files", "package.json", true),
            file!("jwt-verify-missing-algorithms", "files", "src/auth.ts", false),
        ],
    },
    RuleExample {
        rule: "llm-call-injection",
        args: &[],
        base: &[],
        files: &[
            file!("llm-call-injection", "files", "package.json", true),
            file!("llm-call-injection", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "long-file",
        args: &[],
        base: &[],
        files: &[
            file!("long-file", "files", ".fallowrc.json", false),
            file!("long-file", "files", "package.json", true),
            file!("long-file", "files", "src/cart.ts", false),
        ],
    },
    RuleExample {
        rule: "mass-assignment",
        args: &[],
        base: &[],
        files: &[
            file!("mass-assignment", "files", "package.json", true),
            file!("mass-assignment", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "misconfigured-dependency-override",
        args: &[],
        base: &[],
        files: &[
            file!("misconfigured-dependency-override", "files", "package.json", false),
            file!("misconfigured-dependency-override", "files", "src/index.ts", true),
        ],
    },
    RuleExample {
        rule: "misplaced-directive",
        args: &[],
        base: &[],
        files: &[
            file!("misplaced-directive", "files", "app/cart-button.tsx", false),
            file!("misplaced-directive", "files", "app/page.tsx", true),
            file!("misplaced-directive", "files", "package.json", true),
        ],
    },
    RuleExample {
        rule: "missing-suppression-reason",
        args: &[],
        base: &[],
        files: &[
            file!("missing-suppression-reason", "files", ".fallowrc.json", false),
            file!("missing-suppression-reason", "files", "package.json", true),
            file!("missing-suppression-reason", "files", "src/index.ts", false),
            file!("missing-suppression-reason", "files", "src/money.ts", false),
        ],
    },
    RuleExample {
        rule: "mixed-client-server-barrel",
        args: &[],
        base: &[],
        files: &[
            file!("mixed-client-server-barrel", "files", "app/layout.tsx", true),
            file!("mixed-client-server-barrel", "files", "app/page.tsx", true),
            file!("mixed-client-server-barrel", "files", "components/button.tsx", false),
            file!("mixed-client-server-barrel", "files", "components/get-user.ts", false),
            file!("mixed-client-server-barrel", "files", "components/index.ts", false),
            file!("mixed-client-server-barrel", "files", "package.json", true),
        ],
    },
    RuleExample {
        rule: "mysql-multiple-statements",
        args: &[],
        base: &[],
        files: &[
            file!("mysql-multiple-statements", "files", "package.json", true),
            file!("mysql-multiple-statements", "files", "src/db.ts", false),
        ],
    },
    RuleExample {
        rule: "nextjs-open-redirect",
        args: &[],
        base: &[],
        files: &[
            file!("nextjs-open-redirect", "files", "app/login/route.ts", false),
            file!("nextjs-open-redirect", "files", "package.json", true),
        ],
    },
    RuleExample {
        rule: "nosql-injection",
        args: &[],
        base: &[],
        files: &[
            file!("nosql-injection", "files", "package.json", true),
            file!("nosql-injection", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "open-redirect",
        args: &[],
        base: &[],
        files: &[
            file!("open-redirect", "files", "package.json", true),
            file!("open-redirect", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "package-cycle",
        args: &[],
        base: &[],
        files: &[
            file!("package-cycle", "files", "package.json", true),
            file!("package-cycle", "files", "packages/api/package.json", false),
            file!("package-cycle", "files", "packages/api/src/index.ts", false),
            file!("package-cycle", "files", "packages/db/package.json", false),
            file!("package-cycle", "files", "packages/db/src/index.ts", false),
            file!("package-cycle", "files", "pnpm-workspace.yaml", true),
        ],
    },
    RuleExample {
        rule: "path-traversal",
        args: &[],
        base: &[],
        files: &[
            file!("path-traversal", "files", "package.json", true),
            file!("path-traversal", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "permissive-cors",
        args: &[],
        base: &[],
        files: &[
            file!("permissive-cors", "files", "package.json", true),
            file!("permissive-cors", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "policy-violation",
        args: &[],
        base: &[],
        files: &[
            file!("policy-violation", "files", ".fallowrc.json", false),
            file!("policy-violation", "files", "package.json", true),
            file!("policy-violation", "files", "packs/team.json", false),
            file!("policy-violation", "files", "src/app.ts", false),
        ],
    },
    RuleExample {
        rule: "postmessage-wildcard-origin",
        args: &[],
        base: &[],
        files: &[
            file!("postmessage-wildcard-origin", "files", "package.json", true),
            file!("postmessage-wildcard-origin", "files", "src/checkout-frame.ts", false),
        ],
    },
    RuleExample {
        rule: "private-type-leak",
        args: &[],
        base: &[],
        files: &[
            file!("private-type-leak", "files", ".fallowrc.json", false),
            file!("private-type-leak", "files", "package.json", true),
            file!("private-type-leak", "files", "src/index.ts", false),
            file!("private-type-leak", "files", "src/user.ts", false),
        ],
    },
    RuleExample {
        rule: "prop-drilling",
        args: &[],
        base: &[],
        files: &[
            file!("prop-drilling", "files", ".fallowrc.json", false),
            file!("prop-drilling", "files", "package.json", true),
            file!("prop-drilling", "files", "src/Layout.tsx", false),
            file!("prop-drilling", "files", "src/Page.tsx", false),
            file!("prop-drilling", "files", "src/Profile.tsx", false),
            file!("prop-drilling", "files", "src/Sidebar.tsx", false),
            file!("prop-drilling", "files", "src/main.tsx", true),
        ],
    },
    RuleExample {
        rule: "prototype-pollution",
        args: &[],
        base: &[],
        files: &[
            file!("prototype-pollution", "files", "package.json", true),
            file!("prototype-pollution", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "re-export-cycle",
        args: &[],
        base: &[],
        files: &[
            file!("re-export-cycle", "files", "package.json", true),
            file!("re-export-cycle", "files", "src/index.ts", true),
            file!("re-export-cycle", "files", "src/ui/forms/index.ts", false),
            file!("re-export-cycle", "files", "src/ui/forms/text-field.ts", true),
            file!("re-export-cycle", "files", "src/ui/index.ts", false),
        ],
    },
    RuleExample {
        rule: "redos-regex",
        args: &[],
        base: &[],
        files: &[
            file!("redos-regex", "files", "package.json", true),
            file!("redos-regex", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "refactoring-target",
        args: &[],
        base: &[],
        files: &[
            file!("refactoring-target", "files", "package.json", true),
            file!("refactoring-target", "files", "src/cart.ts", false),
            file!("refactoring-target", "files", "src/index.ts", false),
        ],
    },
    RuleExample {
        rule: "resource-amplification",
        args: &[],
        base: &[],
        files: &[
            file!("resource-amplification", "files", "package.json", true),
            file!("resource-amplification", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "route-collision",
        args: &[],
        base: &[],
        files: &[
            file!("route-collision", "files", "app/(marketing)/about/page.tsx", false),
            file!("route-collision", "files", "app/(shop)/about/page.tsx", false),
            file!("route-collision", "files", "app/layout.tsx", true),
            file!("route-collision", "files", "package.json", true),
        ],
    },
    RuleExample {
        rule: "route-send-file",
        args: &[],
        base: &[],
        files: &[
            file!("route-send-file", "files", "package.json", true),
            file!("route-send-file", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "secret-pii-log",
        args: &[],
        base: &[],
        files: &[
            file!("secret-pii-log", "files", "package.json", true),
            file!("secret-pii-log", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "secret-to-network",
        args: &[],
        base: &[],
        files: &[
            file!("secret-to-network", "files", ".fallowrc.json", false),
            file!("secret-to-network", "files", "package.json", true),
            file!("secret-to-network", "files", "src/report.ts", false),
        ],
    },
    RuleExample {
        rule: "sql-injection",
        args: &[],
        base: &[],
        files: &[
            file!("sql-injection", "files", "package.json", true),
            file!("sql-injection", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "ssrf",
        args: &[],
        base: &[],
        files: &[
            file!("ssrf", "files", "package.json", true),
            file!("ssrf", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "ssti",
        args: &[],
        base: &[],
        files: &[
            file!("ssti", "files", "package.json", true),
            file!("ssti", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "stale-suppression",
        args: &[],
        base: &[],
        files: &[
            file!("stale-suppression", "files", "package.json", true),
            file!("stale-suppression", "files", "src/index.ts", false),
            file!("stale-suppression", "files", "src/money.ts", false),
        ],
    },
    RuleExample {
        rule: "template-escape-bypass",
        args: &[],
        base: &[],
        files: &[
            file!("template-escape-bypass", "files", "package.json", true),
            file!("template-escape-bypass", "files", "src/profile.ts", false),
        ],
    },
    RuleExample {
        rule: "test-only-branch",
        args: &[],
        base: &[
            file!("test-only-branch", "base", "package.json", true),
            file!("test-only-branch", "base", "src/pricing.ts", false),
        ],
        files: &[
            file!("test-only-branch", "files", "package.json", true),
            file!("test-only-branch", "files", "src/pricing.ts", false),
        ],
    },
    RuleExample {
        rule: "test-only-dependency",
        args: &[],
        base: &[],
        files: &[
            file!("test-only-dependency", "files", "package.json", false),
            file!("test-only-dependency", "files", "src/cart.test.ts", false),
            file!("test-only-dependency", "files", "src/cart.ts", true),
        ],
    },
    RuleExample {
        rule: "thin-wrapper",
        args: &[],
        base: &[],
        files: &[
            file!("thin-wrapper", "files", ".fallowrc.json", false),
            file!("thin-wrapper", "files", "package.json", true),
            file!("thin-wrapper", "files", "src/Button.tsx", false),
            file!("thin-wrapper", "files", "src/CheckoutForm.tsx", false),
            file!("thin-wrapper", "files", "src/main.tsx", true),
        ],
    },
    RuleExample {
        rule: "tls-validation-disabled",
        args: &[],
        base: &[],
        files: &[
            file!("tls-validation-disabled", "files", "package.json", true),
            file!("tls-validation-disabled", "files", "src/orders-client.ts", false),
        ],
    },
    RuleExample {
        rule: "type-only-dependency",
        args: &[],
        base: &[],
        files: &[
            file!("type-only-dependency", "files", ".fallowrc.json", false),
            file!("type-only-dependency", "files", "package.json", false),
            file!("type-only-dependency", "files", "src/index.ts", false),
        ],
    },
    RuleExample {
        rule: "unlisted-dependency",
        args: &[],
        base: &[],
        files: &[
            file!("unlisted-dependency", "files", "package.json", false),
            file!("unlisted-dependency", "files", "packages/shared/package.json", true),
            file!("unlisted-dependency", "files", "packages/shared/src/index.ts", true),
            file!("unlisted-dependency", "files", "src/app.ts", false),
        ],
    },
    RuleExample {
        rule: "unproduced-catch-tag",
        args: &[],
        base: &[],
        files: &[
            file!("unproduced-catch-tag", "files", "package.json", true),
            file!("unproduced-catch-tag", "files", "src/errors.ts", false),
            file!("unproduced-catch-tag", "files", "src/main.ts", false),
        ],
    },
    RuleExample {
        rule: "unprovided-inject",
        args: &[],
        base: &[],
        files: &[
            file!("unprovided-inject", "files", "package.json", true),
            file!("unprovided-inject", "files", "src/App.vue", false),
            file!("unprovided-inject", "files", "src/keys.ts", false),
            file!("unprovided-inject", "files", "src/main.ts", true),
        ],
    },
    RuleExample {
        rule: "unrendered-component",
        args: &[],
        base: &[],
        files: &[
            file!("unrendered-component", "files", "package.json", true),
            file!("unrendered-component", "files", "src/App.vue", false),
            file!("unrendered-component", "files", "src/components/CartItem.vue", true),
            file!("unrendered-component", "files", "src/components/Orphan.vue", false),
            file!("unrendered-component", "files", "src/components/index.ts", false),
            file!("unrendered-component", "files", "src/main.ts", true),
        ],
    },
    RuleExample {
        rule: "unresolved-catalog-reference",
        args: &[],
        base: &[],
        files: &[
            file!("unresolved-catalog-reference", "files", "package.json", true),
            file!("unresolved-catalog-reference", "files", "packages/app/package.json", false),
            file!("unresolved-catalog-reference", "files", "packages/app/src/index.ts", true),
            file!("unresolved-catalog-reference", "files", "pnpm-workspace.yaml", false),
        ],
    },
    RuleExample {
        rule: "unresolved-import",
        args: &[],
        base: &[],
        files: &[
            file!("unresolved-import", "files", "package.json", true),
            file!("unresolved-import", "files", "src/app.ts", false),
            file!("unresolved-import", "files", "src/routes/shop.ts", false),
        ],
    },
    RuleExample {
        rule: "unsafe-buffer-alloc",
        args: &[],
        base: &[],
        files: &[
            file!("unsafe-buffer-alloc", "files", "package.json", true),
            file!("unsafe-buffer-alloc", "files", "src/receipt.ts", false),
        ],
    },
    RuleExample {
        rule: "unsafe-deserialization",
        args: &[],
        base: &[],
        files: &[
            file!("unsafe-deserialization", "files", "package.json", true),
            file!("unsafe-deserialization", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "untested-export",
        args: &[],
        base: &[],
        files: &[
            file!("untested-export", "files", ".fallowrc.json", false),
            file!("untested-export", "files", "package.json", true),
            file!("untested-export", "files", "src/cart.ts", false),
            file!("untested-export", "files", "src/money.ts", false),
            file!("untested-export", "files", "test/money.test.ts", false),
        ],
    },
    RuleExample {
        rule: "untested-file",
        args: &[],
        base: &[],
        files: &[
            file!("untested-file", "files", ".fallowrc.json", false),
            file!("untested-file", "files", "package.json", true),
            file!("untested-file", "files", "src/billing/invoice.ts", false),
            file!("untested-file", "files", "src/routes/checkout.ts", false),
            file!("untested-file", "files", "test/invoice.test.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-catalog-entry",
        args: &[],
        base: &[],
        files: &[
            file!("unused-catalog-entry", "files", "package.json", true),
            file!("unused-catalog-entry", "files", "packages/app/package.json", false),
            file!("unused-catalog-entry", "files", "packages/app/src/index.ts", true),
            file!("unused-catalog-entry", "files", "pnpm-workspace.yaml", false),
        ],
    },
    RuleExample {
        rule: "unused-class-member",
        args: &[],
        base: &[],
        files: &[
            file!("unused-class-member", "files", "package.json", true),
            file!("unused-class-member", "files", "src/index.ts", false),
            file!("unused-class-member", "files", "src/parser.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-component-emit",
        args: &[],
        base: &[],
        files: &[
            file!("unused-component-emit", "files", "package.json", true),
            file!("unused-component-emit", "files", "src/App.vue", false),
            file!("unused-component-emit", "files", "src/Widget.vue", false),
            file!("unused-component-emit", "files", "src/main.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-component-input",
        args: &[],
        base: &[],
        files: &[
            file!("unused-component-input", "files", "package.json", true),
            file!("unused-component-input", "files", "src/app.component.ts", false),
            file!("unused-component-input", "files", "src/main.ts", true),
            file!("unused-component-input", "files", "src/user-card.component.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-component-output",
        args: &[],
        base: &[],
        files: &[
            file!("unused-component-output", "files", "package.json", true),
            file!("unused-component-output", "files", "src/app.component.ts", false),
            file!("unused-component-output", "files", "src/main.ts", true),
            file!("unused-component-output", "files", "src/user-card.component.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-component-prop",
        args: &[],
        base: &[],
        files: &[
            file!("unused-component-prop", "files", "package.json", true),
            file!("unused-component-prop", "files", "src/App.vue", false),
            file!("unused-component-prop", "files", "src/Widget.vue", false),
            file!("unused-component-prop", "files", "src/main.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-dependency",
        args: &[],
        base: &[],
        files: &[
            file!("unused-dependency", "files", "package.json", false),
            file!("unused-dependency", "files", "src/index.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-dependency-override",
        args: &[],
        base: &[],
        files: &[
            file!("unused-dependency-override", "files", "package.json", false),
            file!("unused-dependency-override", "files", "src/index.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-dev-dependency",
        args: &[],
        base: &[],
        files: &[
            file!("unused-dev-dependency", "files", "package.json", false),
            file!("unused-dev-dependency", "files", "src/index.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-effect-layer",
        args: &[],
        base: &[],
        files: &[
            file!("unused-effect-layer", "files", "package.json", true),
            file!("unused-effect-layer", "files", "src/main.ts", false),
            file!("unused-effect-layer", "files", "src/services.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-enum-member",
        args: &[],
        base: &[],
        files: &[
            file!("unused-enum-member", "files", "package.json", true),
            file!("unused-enum-member", "files", "src/index.ts", false),
            file!("unused-enum-member", "files", "src/status.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-export",
        args: &[],
        base: &[],
        files: &[
            file!("unused-export", "files", "package.json", true),
            file!("unused-export", "files", "src/index.ts", false),
            file!("unused-export", "files", "src/money.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-file",
        args: &[],
        base: &[],
        files: &[
            file!("unused-file", "files", "package.json", true),
            file!("unused-file", "files", "src/index.ts", false),
            file!("unused-file", "files", "src/old-widget.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-load-data-key",
        args: &[],
        base: &[],
        files: &[
            file!("unused-load-data-key", "files", "package.json", true),
            file!("unused-load-data-key", "files", "src/routes/blog/+page.svelte", false),
            file!("unused-load-data-key", "files", "src/routes/blog/+page.ts", false),
            file!("unused-load-data-key", "files", "vite.config.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-namespace-member",
        args: &[],
        base: &[],
        files: &[
            file!("unused-namespace-member", "files", "package.json", true),
            file!("unused-namespace-member", "files", "src/index.ts", false),
            file!("unused-namespace-member", "files", "src/lib/legacy.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-optional-dependency",
        args: &[],
        base: &[],
        files: &[
            file!("unused-optional-dependency", "files", "package.json", false),
            file!("unused-optional-dependency", "files", "src/index.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-protected-class-member",
        args: &[],
        base: &[],
        files: &[
            file!("unused-protected-class-member", "files", ".fallowrc.json", false),
            file!("unused-protected-class-member", "files", "package.json", true),
            file!("unused-protected-class-member", "files", "src/index.ts", false),
            file!("unused-protected-class-member", "files", "src/shape.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-server-action",
        args: &[],
        base: &[],
        files: &[
            file!("unused-server-action", "files", "app/actions.ts", false),
            file!("unused-server-action", "files", "app/layout.tsx", true),
            file!("unused-server-action", "files", "app/page.tsx", false),
            file!("unused-server-action", "files", "package.json", true),
        ],
    },
    RuleExample {
        rule: "unused-store-member",
        args: &[],
        base: &[],
        files: &[
            file!("unused-store-member", "files", "package.json", false),
            file!("unused-store-member", "files", "src/index.ts", false),
            file!("unused-store-member", "files", "src/stores/cart.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-svelte-event",
        args: &[],
        base: &[],
        files: &[
            file!("unused-svelte-event", "files", "package.json", true),
            file!("unused-svelte-event", "files", "src/App.svelte", false),
            file!("unused-svelte-event", "files", "src/Child.svelte", false),
            file!("unused-svelte-event", "files", "src/main.ts", true),
        ],
    },
    RuleExample {
        rule: "unused-tagged-error",
        args: &[],
        base: &[],
        files: &[
            file!("unused-tagged-error", "files", "package.json", true),
            file!("unused-tagged-error", "files", "src/errors.ts", false),
            file!("unused-tagged-error", "files", "src/main.ts", false),
        ],
    },
    RuleExample {
        rule: "unused-type",
        args: &[],
        base: &[],
        files: &[
            file!("unused-type", "files", "package.json", true),
            file!("unused-type", "files", "src/button.ts", false),
            file!("unused-type", "files", "src/index.ts", false),
        ],
    },
    RuleExample {
        rule: "weak-crypto",
        args: &[],
        base: &[],
        files: &[
            file!("weak-crypto", "files", "package.json", true),
            file!("weak-crypto", "files", "src/checksum.ts", false),
        ],
    },
    RuleExample {
        rule: "weakened-tests",
        args: &[],
        base: &[
            file!("weakened-tests", "base", "package.json", true),
            file!("weakened-tests", "base", "src/tax.ts", true),
            file!("weakened-tests", "base", "test/tax.test.ts", false),
        ],
        files: &[
            file!("weakened-tests", "files", "package.json", true),
            file!("weakened-tests", "files", "src/tax.ts", true),
            file!("weakened-tests", "files", "test/tax.test.ts", false),
        ],
    },
    RuleExample {
        rule: "webview-injection",
        args: &[],
        base: &[],
        files: &[
            file!("webview-injection", "files", "package.json", true),
            file!("webview-injection", "files", "src/CheckoutView.tsx", false),
        ],
    },
    RuleExample {
        rule: "world-writable-permission",
        args: &[],
        base: &[],
        files: &[
            file!("world-writable-permission", "files", "package.json", true),
            file!("world-writable-permission", "files", "src/install.ts", false),
        ],
    },
    RuleExample {
        rule: "xpath-injection",
        args: &[],
        base: &[],
        files: &[
            file!("xpath-injection", "files", "package.json", true),
            file!("xpath-injection", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "xxe",
        args: &[],
        base: &[],
        files: &[
            file!("xxe", "files", "package.json", true),
            file!("xxe", "files", "src/server.ts", false),
        ],
    },
    RuleExample {
        rule: "zip-slip",
        args: &[],
        base: &[],
        files: &[
            file!("zip-slip", "files", "package.json", true),
            file!("zip-slip", "files", "src/import.ts", false),
        ],
    },
];
