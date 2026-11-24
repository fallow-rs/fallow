use crate::common::{copy_fixture, parse_json, run_fallow, run_fallow_in_root};

#[test]
fn grouped_clone_issues_keep_the_owner_of_each_region_in_live_and_saved_reports() {
    let project = copy_fixture("duplicate-code");
    let root = project.path();
    std::fs::write(
        root.join("CODEOWNERS"),
        "src/copy1.ts @team-a\nsrc/original.ts @team-b\nsrc/partial.ts @team-b\n",
    )
    .expect("write owners");

    let options = ["--group-by", "owner", "--quiet"];
    let render = |command, args: &[&str]| {
        run_fallow_in_root(command, root, &[options.as_slice(), args].concat())
    };
    let saved = render("dupes", &["--format", "json"]);
    let envelope = root.join("dupes.json");
    std::fs::write(&envelope, saved.stdout).expect("save the duplication report");
    for output in [
        render("dupes", &["--format", "codeclimate"]),
        render(
            "report",
            &[
                "--format",
                "codeclimate",
                "--from",
                envelope.to_str().expect("path"),
            ],
        ),
    ] {
        let json = parse_json(&output);
        let mut regions: Vec<_> = json
            .as_array()
            .expect("issues")
            .iter()
            .filter(|issue| issue["location"]["path"] == "src/copy1.ts")
            .map(|issue| {
                (
                    issue["location"]["lines"]["end"]
                        .as_u64()
                        .expect("end line"),
                    issue["group"].as_str().expect("owner"),
                )
            })
            .collect();
        regions.sort_unstable();
        assert_eq!(regions, [(12, "@team-b"), (27, "@team-a")]);
    }
}

#[test]
fn a_second_read_of_one_flag_on_the_same_line_has_its_own_fingerprint() {
    let project = copy_fixture("feature-flag-suppression");
    let source = project.path().join("src/index.ts");
    let render = || {
        let output = run_fallow_in_root(
            "flags",
            project.path(),
            &["--format", "codeclimate", "--no-cache"],
        );
        parse_json(&output)
    };
    std::fs::write(&source, "export const x = process.env.FEATURE_X;\n").expect("single read");
    let single = render();
    assert_eq!(single[0]["fingerprint"], "bc904c480573910a");
    std::fs::write(
        &source,
        "export const x = process.env.FEATURE_X && process.env.FEATURE_X !== 'off';\n",
    )
    .expect("two reads");
    let both = render();
    let rows = both.as_array().expect("issues");
    assert_eq!(rows.len(), 2);
    let first = rows[0]["fingerprint"].as_str().expect("first fingerprint");
    assert_eq!(single[0]["fingerprint"], first);
    assert_ne!(
        first,
        rows[1]["fingerprint"].as_str().expect("second fingerprint")
    );
}

#[test]
fn the_badge_and_terminal_display_the_same_rounded_health_score() {
    let health = |format| {
        run_fallow(
            "health",
            "flags-retirement",
            &["--score", "--format", format, "--quiet"],
        )
    };
    let json = parse_json(&health("json"));
    let score = json["health_score"]["score"].as_f64().expect("score");
    let grade = json["health_score"]["grade"].as_str().expect("grade");
    let human = health("human").stdout;
    let badge = health("badge").stdout;
    assert!(
        human.contains(&format!("Health score: {score:.0} {grade}")),
        "{human}"
    );
    assert!(
        badge.contains(&format!("aria-label=\"fallow: {grade} ({score:.0})\"")),
        "{badge}"
    );
}
