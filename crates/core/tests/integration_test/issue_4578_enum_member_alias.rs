use crate::common::{create_config, fixture_path};

/// Issue #4578: an enum member read through an alias of the enum is used.
///
/// The fixture has one enum for each alias form. Each enum has one member
/// that the alias reads and one member that no code reads:
/// - a `const` alias (`const Alias = Color; Alias.Red`),
/// - a class field with the name of the enum, read by an external Angular
///   template (`Tone = Tone`, `{{ Tone.Cool }}`),
/// - a renamed class field, read by an external Angular template
///   (`levels = Level`, `{{ levels.High }}`),
/// - a class field, read through `this` in TypeScript (`this.Kind.Two`),
/// - a renamed class field, read by an inline Angular template
///   (`moods = Mood`, `{{ moods.Happy }}`),
/// - a `const` alias in a function body (`const local = Hue; local.Dark`).
///
/// The alias credits only the members that it reads, so the other member of
/// each enum stays unused.
///
/// An enum that leaves whole (`takesEnum(Shape)`, `{ ref: Size }`, or
/// `registered.push(Dir)` in the module that declares it) can have any
/// member read, so each of its members is used.
///
/// A parameter or a nested `const` with the name of a module-scope alias
/// holds another value, so its reads do not credit the enum
/// (`Grade.Two`, `Step.Two`).
#[test]
fn enum_member_read_through_alias_is_used() {
    let root = fixture_path("issue-4578-enum-member-alias");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<String> = results
        .unused_enum_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    unused.sort_unstable();

    assert_eq!(
        unused,
        [
            "Color.Green",
            "Grade.Two",
            "Hue.Light",
            "Kind.One",
            "Level.Low",
            "Mood.Sad",
            "Step.Two",
            "Tone.Warm"
        ],
        "only the members that no alias reads and no escape exposes are unused"
    );
}
