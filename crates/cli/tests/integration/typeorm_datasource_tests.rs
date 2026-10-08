#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};

fn unused_files(fixture: &str) -> Vec<String> {
    let output = run_fallow(
        "dead-code",
        fixture,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    json["unused_files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file["path"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn datasource_migration_globs_credit_migrations_and_their_helpers() {
    let unused = unused_files("typeorm-datasource-globs");
    for credited in [
        "src/database/legacy/common/1700000000000-init.ts",
        "src/database/legacy/billing/1700000000001-billing.ts",
        "src/database/utils/add-column.util.ts",
    ] {
        assert!(
            !unused.iter().any(|path| path.ends_with(credited)),
            "{credited} is loaded through the DataSource migrations, got unused: {unused:?}"
        );
    }
    // The orphan has no glob. The legacy schema has only a glob with an unknown
    // interpolation. Entity and subscriber globs do not make entry points.
    for still_unused in [
        "src/database/orphan.ts",
        "src/legacy-schema/old.ts",
        "src/engine/user.entity.ts",
        "src/events/audit.subscriber.ts",
    ] {
        assert!(
            unused.iter().any(|path| path.ends_with(still_unused)),
            "{still_unused} is not credited and stays unused, got: {unused:?}"
        );
    }
}
