use super::common::{create_config, fixture_path};

// ── Enum/class members integration ─────────────────────────────

#[test]
fn enum_class_members_detects_unused_members() {
    let root = fixture_path("enum-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // Only Status.Active is used; Inactive and Pending should be unused
    assert!(
        unused_enum_member_names.contains(&"Inactive"),
        "Inactive should be detected as unused enum member, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"Pending"),
        "Pending should be detected as unused enum member, found: {unused_enum_member_names:?}"
    );

    let unused_class_member_names: Vec<&str> = results
        .unused_class_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // unusedMethod is never called
    assert!(
        unused_class_member_names.contains(&"unusedMethod"),
        "unusedMethod should be detected as unused class member, found: {unused_class_member_names:?}"
    );

    // greet() is called via instance: `const svc = new MyService(); svc.greet()`
    assert!(
        !unused_class_member_names.contains(&"greet"),
        "greet should NOT be unused (called via instance), found: {unused_class_member_names:?}"
    );

    // name property is never accessed (not via svc.name or this.name)
    assert!(
        unused_class_member_names.contains(&"name"),
        "name should be detected as unused class property, found: {unused_class_member_names:?}"
    );
}

#[test]
fn exported_instance_class_members_are_credited_to_class() {
    let root = fixture_path("exported-instance-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.parent_name, m.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"Box.bump".to_string()),
        "Box.bump should be credited through exported instance usage, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&"Box.current".to_string()),
        "Box.current getter/setter should be credited through exported instance usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"Box.unused".to_string()),
        "Box.unused should still be reported, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_instance_bindings() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

func main() {
    svc := shared.Service{}
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct {
    Name string
    hidden string
}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through svc.Run(), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Name".to_string())),
        "Service.Name should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_constructor_bindings() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

func main() {
    svc := shared.NewService()
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through shared.NewService().Run(), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_alias_bindings() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

func main() {
    svc := shared.NewService()
    alias := svc
    alias.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through alias.Run(), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_typed_var_constructor_bindings() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func main() {
    var svc Runner = shared.NewService()
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through typed var Runner = shared.NewService(), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_generic_typed_var_helper_results() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func main() {
    var svc Runner = shared.NewBox[int]()
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through typed var Runner = shared.NewBox[int](), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_generic_local_helper_interface_chains() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

type Runner interface {
    Run()
}

func buildRunner() Runner {
    return wrapRunner(NewBox[int]())
}

func wrapRunner(r Runner) Runner {
    return r
}

func main() {
    buildRunner().Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("box.go"),
        r#"package main

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a generic local helper interface chain, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_generic_helper_interface_chains() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner() Runner {
    return wrapRunner(shared.NewBox[int]())
}

func wrapRunner(r Runner) Runner {
    return r
}

func main() {
    buildRunner().Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through an imported generic helper interface chain, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_go_work_imported_generic_helper_interface_chains() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner() Runner {
    return wrapRunner(shared.NewBox[int]())
}

func wrapRunner(r Runner) Runner {
    return r
}

func main() {
    buildRunner().Run()
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a go.work imported generic helper interface chain, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_go_work_imported_generic_method_expressions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

var _ = shared.Box[int].Run

func main() {
    _ = shared.Box[int]{}
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a go.work imported generic method expression, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_go_work_imported_generic_type_assertions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(v any) {
    box := v.(shared.Box[int])
    box.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a go.work imported generic type assertion, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_go_work_imported_generic_type_switches() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(v any) {
    switch box := v.(type) {
    case shared.Box[int]:
        box.Run()
    }
}

func main() {
    use(shared.Box[int]{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a go.work imported generic type switch, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_generic_interface_usage() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through imported generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_go_work_imported_generic_interface_usage() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through go.work imported generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_generic_interface_usage_is_narrowed_by_unexported_direct_calls() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a narrowed unexported generic interface call, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should remain unused when the unexported call site is concrete, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_work_generic_interface_usage_is_narrowed_by_unexported_direct_calls() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    use(shared.Box[int]{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a narrowed go.work generic interface call, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should remain unused when the go.work unexported call site is concrete, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_generic_interface_usage_is_narrowed_by_unexported_helper_call_args() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    use(buildRunner())
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a narrowed helper-call interface arg, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should remain unused when the helper arg resolves concretely, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_work_generic_interface_usage_is_narrowed_by_unexported_helper_call_args() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    use(buildRunner())
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a narrowed go.work helper-call interface arg, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should remain unused when the go.work helper arg resolves concretely, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_generic_interface_usage_is_narrowed_by_bound_helper_call_args() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    svc := buildRunner()
    use(svc)
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a narrowed bound helper arg, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should remain unused when the bound helper arg resolves concretely, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_work_generic_interface_usage_is_narrowed_by_bound_helper_call_args() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func buildRunner() shared.Runner[int] {
    return shared.NewBox[int]()
}

func main() {
    svc := buildRunner()
    use(svc)
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a narrowed go.work bound helper arg, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should remain unused when the go.work bound helper arg resolves concretely, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_generic_interface_usage_is_narrowed_through_consistent_if_bindings() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    flag := true
    var svc shared.Runner[int]
    if flag {
        svc = shared.NewBox[int]()
    } else {
        svc = shared.Box[int]{}
    }
    use(svc)
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through consistent if-merged bindings, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should remain unused when the if merge stays concrete, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_generic_interface_usage_stays_conservative_with_multiple_implementers() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(shared.Box[int]{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should also be credited conservatively for ambiguous generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_work_generic_interface_usage_stays_conservative_with_multiple_implementers() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(shared.Box[int]{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/box.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write lib box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through go.work generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should also be credited conservatively for go.work generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_when_local_generic_type_implements_imported_generic_interface()
{
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(Box[int]{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("box.go"),
        r#"package main

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");
    fs::write(
        root.join("pkg/shared/runner.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}
"#,
    )
    .expect("write runner.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a local generic type implementing an imported generic interface, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_when_local_generic_type_implements_go_work_imported_generic_interface()
 {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(Box[int]{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("app/box.go"),
        r#"package main

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write app/box.go");
    fs::write(
        root.join("lib/pkg/shared/runner.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}
"#,
    )
    .expect("write lib runner.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a local generic type implementing a go.work imported generic interface, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_local_generic_implementers_of_imported_generic_interface_stay_conservative() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(Box[int]{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("box.go"),
        r#"package main

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write box.go");
    fs::write(
        root.join("pkg/shared/runner.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}
"#,
    )
    .expect("write runner.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through imported generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should also be credited conservatively for imported generic interface implementers, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_work_local_generic_implementers_of_imported_generic_interface_stay_conservative() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func Use(r shared.Runner[int]) {
    r.Run()
}

func main() {
    Use(Box[int]{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("app/box.go"),
        r#"package main

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}

type Crate[T any] struct{}

func (c Crate[T]) Run() {}
func (c Crate[T]) Stop() {}
"#,
    )
    .expect("write app/box.go");
    fs::write(
        root.join("lib/pkg/shared/runner.go"),
        r#"package shared

type Runner[T any] interface {
    Run()
}
"#,
    )
    .expect("write lib runner.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through go.work imported generic interface usage, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&("Crate".to_string(), "Run".to_string())),
        "Crate.Run should also be credited conservatively for go.work imported generic interface implementers, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Crate".to_string(), "Stop".to_string())),
        "Crate.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_local_helper_returns() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner() Runner {
    return shared.NewService()
}

func main() {
    svc := buildRunner()
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through buildRunner(), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_consistent_multi_return_helpers() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner(flag bool) Runner {
    if flag {
        return shared.NewService()
    }
    return shared.Service{}
}

func main() {
    svc := buildRunner(true)
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through consistent multi-return helper flow, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_forward_helper_chains() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner() Runner {
    return makeRunner()
}

func main() {
    svc := buildRunner()
    svc.Run()
}

func makeRunner() Runner {
    return shared.NewService()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through forward helper chain, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_passthrough_helpers() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func wrapRunner(runner Runner) Runner {
    return runner
}

func main() {
    svc := wrapRunner(shared.NewService())
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through passthrough helper, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_consistent_if_branch_bindings() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func main() {
    var svc Runner
    if true {
        svc = shared.NewService()
    } else {
        svc = shared.Service{}
    }
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through consistent if-branch bindings, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_consistent_switch_bindings() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func main() {
    var svc Runner
    switch 1 {
    case 1:
        svc = shared.NewService()
    default:
        svc = shared.Service{}
    }
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through consistent switch bindings, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_local_helper_binding_returns() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func buildRunner() Runner {
    svc := shared.NewService()
    return svc
}

func main() {
    svc := buildRunner()
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through helper local-binding returns, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_multi_param_helper_routing() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/shared"

type Runner interface {
    Run()
}

func chooseRunner(label string, runner Runner) Runner {
    alias := runner
    return alias
}

func main() {
    svc := chooseRunner("primary", shared.NewService())
    svc.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func NewService() Service { return Service{} }
func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through multi-param helper routing, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_type_assertions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/app")).expect("create pkg/app");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/app"

func main() {
    app.UseRunner(app.Service{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/app/service.go"),
        r#"package app

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}

func UseRunner(r Runner) {
    svc := r.(Service)
    svc.Run()
}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through a type assertion, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_type_assertions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(v any) {
    svc := v.(shared.Service)
    svc.Run()
}

func main() {
    use(shared.Service{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through an imported type assertion, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_go_work_imported_type_assertions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib/pkg/shared");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(v any) {
    svc := v.(shared.Service)
    svc.Run()
}

func main() {
    use(shared.Service{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write lib/pkg/shared/service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through a go.work imported type assertion, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_type_switches() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(v any) {
    switch svc := v.(type) {
    case shared.Service:
        svc.Run()
    }
}

func main() {
    use(shared.Service{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through an imported type switch, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_method_expressions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

var _ = shared.Service.Run

func main() {
    _ = shared.Service{}
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through an imported method expression, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_generic_method_expressions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

var _ = shared.Box[int].Run

func main() {
    _ = shared.Box[int]{}
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through an imported generic method expression, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_same_package_generic_method_expressions() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

var _ = Box[int].Run

func main() {
    _ = Box[int]{}
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("box.go"),
        r#"package main

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a same-package generic method expression, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_same_package_generic_helper_call_results() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

func main() {
    NewBox[int]().Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("box.go"),
        r#"package main

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a same-package generic helper call result, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_across_same_package_files() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("service.go"),
        r#"package main

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");
    fs::write(
        root.join("use.go"),
        r#"package main

func use(v Runner) {
    svc := v.(Service)
    svc.Run()
}

func main() {
    use(Service{})
}
"#,
    )
    .expect("write use.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited across same-package files, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_interface_usage() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

func main() {
    use(Service{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("service.go"),
        r#"package main

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}

func use(r Runner) {
    r.Run()
}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through interface usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_direct_helper_call_results_prefer_concrete_receiver_methods() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    _ = shared.KeepOther()
    shared.BuildRunner().Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}

type OtherService struct{}

func (s OtherService) Run() {}
func (s OtherService) Stop() {}

func BuildRunner() Runner {
    return Service{}
}

func KeepOther() OtherService {
    return OtherService{}
}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through the direct helper call result, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("OtherService".to_string(), "Run".to_string())),
        "OtherService.Run should remain unused when the helper returns Service, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("OtherService".to_string(), "Stop".to_string())),
        "OtherService.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_interface_usage() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner) {
    r.Run()
}

func main() {
    use(shared.Service{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through imported interface usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_composite_literal_calls() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.Service{}.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through an imported composite literal call, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_parenthesized_addressed_imported_composite_literal_calls()
 {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    (&shared.Service{}).Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/service.go"),
        r#"package shared

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through a parenthesized addressed imported composite literal call, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_generic_composite_literal_calls() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.Box[int]{}.Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through an imported generic composite literal call, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_imported_generic_helper_call_results() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    shared.NewBox[int]().Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through an imported generic helper call result, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_parenthesized_addressed_imported_generic_helper_call_results()
 {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func main() {
    (&shared.NewBox[int]()).Run()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/shared/box.go"),
        r#"package shared

type Box[T any] struct{}

func NewBox[T any]() Box[T] { return Box[T]{} }
func (b Box[T]) Run() {}
func (b Box[T]) Stop() {}
"#,
    )
    .expect("write box.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Box".to_string(), "Run".to_string())),
        "Box.Run should be credited through a parenthesized addressed imported generic helper call result, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Box".to_string(), "Stop".to_string())),
        "Box.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_when_local_type_implements_imported_interface() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/shared")).expect("create pkg/shared");
    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import shared "github.com/acme/example/pkg/shared"

func use(r shared.Runner) {
    r.Run()
}

func main() {
    use(Service{})
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("service.go"),
        r#"package main

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write service.go");
    fs::write(
        root.join("pkg/shared/runner.go"),
        r#"package shared

type Runner interface {
    Run()
}
"#,
    )
    .expect("write runner.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through an imported interface implemented locally, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_through_go_work_imported_interface_usage() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(r shared.Runner) {
    r.Run()
}

func main() {
    use(shared.Service{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/pkg/shared/service.go"),
        r#"package shared

type Runner interface {
    Run()
}

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write lib service");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through go.work imported interface usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

#[test]
fn go_receiver_methods_are_credited_when_local_type_implements_go_work_imported_interface() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib");
    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import shared "github.com/acme/lib/pkg/shared"

func use(r shared.Runner) {
    r.Run()
}

func main() {
    use(Service{})
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("app/service.go"),
        r#"package main

type Service struct{}

func (s Service) Run() {}
func (s Service) Stop() {}
"#,
    )
    .expect("write app/service.go");
    fs::write(
        root.join("lib/pkg/shared/runner.go"),
        r#"package shared

type Runner interface {
    Run()
}
"#,
    )
    .expect("write lib runner");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.clone(), m.member_name.clone()))
        .collect();

    assert!(
        !unused_class_members.contains(&("Service".to_string(), "Run".to_string())),
        "Service.Run should be credited through a local type implementing a go.work imported interface, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("Service".to_string(), "Stop".to_string())),
        "Service.Stop should remain unused, found: {unused_class_members:?}"
    );
}

// ── Cross-package enum/class member access (issue #178) ────────

#[test]
fn cross_package_enum_class_members_credit_re_exported_origin() {
    let root = fixture_path("cross-package-enum-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // StatusCode.Active/Inactive/Pending are referenced cross-package via
    // `import { StatusCode } from '@repro/lib-a'` then `StatusCode.Active`,
    // where the `@repro/lib-a` import resolves to the barrel `index.ts`.
    // Without re-export chain propagation in `find_unused_members`, all
    // four members would be flagged. After the fix, only the genuinely
    // unused `Archived` should be reported.
    assert!(
        !unused_enum_member_names.contains(&"Active"),
        "StatusCode.Active should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "StatusCode.Inactive should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Pending"),
        "StatusCode.Pending should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"Archived"),
        "StatusCode.Archived is genuinely unused and should still be flagged, found: {unused_enum_member_names:?}"
    );

    // Direction: only East and West are referenced cross-package.
    assert!(
        !unused_enum_member_names.contains(&"East"),
        "Direction.East should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"West"),
        "Direction.West should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"North"),
        "Direction.North is genuinely unused, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"South"),
        "Direction.South is genuinely unused, found: {unused_enum_member_names:?}"
    );

    // Class static method case from the issue comment: StringUtils.toUpper
    // is called cross-package; the other two static methods are not.
    let unused_class_member_names: Vec<&str> = results
        .unused_class_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    assert!(
        !unused_class_member_names.contains(&"toUpper"),
        "StringUtils.toUpper should be credited via cross-package access, found: {unused_class_member_names:?}"
    );
    assert!(
        unused_class_member_names.contains(&"toLower"),
        "StringUtils.toLower is genuinely unused, found: {unused_class_member_names:?}"
    );
    assert!(
        unused_class_member_names.contains(&"reverse"),
        "StringUtils.reverse is genuinely unused, found: {unused_class_member_names:?}"
    );
}

#[test]
fn injected_dependency_object_credits_class_member_usage() {
    let root = fixture_path("injected-dependency-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(&str, &str)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.as_str(), m.member_name.as_str()))
        .collect();

    assert!(
        !unused_class_members.contains(&("FooClass", "foo")),
        "FooClass.foo should be credited through this.deps.foo.foo(), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("FooClass", "unused")),
        "the fixture should still report genuinely unused members, found: {unused_class_members:?}"
    );
}

// ── Whole-object enum member heuristics ────────────────────────

#[test]
fn enum_whole_object_uses_no_false_positives() {
    let root = fixture_path("enum-whole-object");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // Status used via Object.values — no members should be unused
    assert!(
        !unused_enum_member_names.contains(&"Active"),
        "Active should not be unused (Object.values), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "Inactive should not be unused (Object.values), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Pending"),
        "Pending should not be unused (Object.values), found: {unused_enum_member_names:?}"
    );

    // Direction used via Object.keys — no members should be unused
    assert!(
        !unused_enum_member_names.contains(&"Up"),
        "Up should not be unused (Object.keys), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Down"),
        "Down should not be unused (Object.keys), found: {unused_enum_member_names:?}"
    );

    // Color used via for..in — no members should be unused
    assert!(
        !unused_enum_member_names.contains(&"Red"),
        "Red should not be unused (for..in), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Green"),
        "Green should not be unused (for..in), found: {unused_enum_member_names:?}"
    );

    // Priority — only High accessed via computed literal, Low and Medium should be unused
    assert!(
        unused_enum_member_names.contains(&"Low"),
        "Low should be unused (only High accessed via computed), found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"Medium"),
        "Medium should be unused (only High accessed via computed), found: {unused_enum_member_names:?}"
    );
}

// ── Type-level enum member usage ──────────────────────────────

#[test]
fn enum_type_level_usage_no_false_positives() {
    let root = fixture_path("enum-type-level");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // BreakpointString used as mapped type constraint — all members should be used
    assert!(
        !unused_enum_member_names.contains(&"xs"),
        "xs should not be unused (mapped type constraint), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"xxl"),
        "xxl should not be unused (mapped type constraint), found: {unused_enum_member_names:?}"
    );

    // Status.Active used via qualified type name, Status.Inactive via runtime access
    assert!(
        !unused_enum_member_names.contains(&"Active"),
        "Active should not be unused (type qualified name), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "Inactive should not be unused (runtime access), found: {unused_enum_member_names:?}"
    );

    // Status.Pending is not used in any way — should be unused
    assert!(
        unused_enum_member_names.contains(&"Pending"),
        "Pending should be unused (no type-level or runtime access), found: {unused_enum_member_names:?}"
    );

    // Color used via Record<Color, string> — all members should be used
    assert!(
        !unused_enum_member_names.contains(&"Red"),
        "Red should not be unused (Record<Color, T>), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Blue"),
        "Blue should not be unused (Record<Color, T>), found: {unused_enum_member_names:?}"
    );

    // Direction used via { [K in keyof typeof Direction]: ... } — all members should be used
    assert!(
        !unused_enum_member_names.contains(&"Up"),
        "Up should not be unused (keyof typeof in mapped type), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Right"),
        "Right should not be unused (keyof typeof in mapped type), found: {unused_enum_member_names:?}"
    );
}
