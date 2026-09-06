/// Angular lifecycle hooks and framework-invoked methods.
///
/// These should never be flagged as unused class members because they are
/// called by the Angular framework, not user code.
pub(in crate::analyze) fn is_angular_lifecycle_method(name: &str) -> bool {
    matches!(
        name,
        "ngOnInit"
            | "ngOnDestroy"
            | "ngOnChanges"
            | "ngDoCheck"
            | "ngAfterContentInit"
            | "ngAfterContentChecked"
            | "ngAfterViewInit"
            | "ngAfterViewChecked"
            | "ngAcceptInputType"
            | "canActivate"
            | "canDeactivate"
            | "canActivateChild"
            | "canMatch"
            | "resolve"
            | "intercept"
            | "transform"
            | "validate"
            | "registerOnChange"
            | "registerOnTouched"
            | "writeValue"
            | "setDisabledState"
    )
}

pub(in crate::analyze) fn is_react_lifecycle_method(name: &str) -> bool {
    matches!(
        name,
        "render"
            | "componentDidMount"
            | "componentDidUpdate"
            | "componentWillUnmount"
            | "shouldComponentUpdate"
            | "getSnapshotBeforeUpdate"
            | "getDerivedStateFromProps"
            | "getDerivedStateFromError"
            | "componentDidCatch"
            | "componentWillMount"
            | "componentWillReceiveProps"
            | "componentWillUpdate"
            | "UNSAFE_componentWillMount"
            | "UNSAFE_componentWillReceiveProps"
            | "UNSAFE_componentWillUpdate"
            | "getChildContext"
            | "contextType"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn react_lifecycle_standard_methods() {
        assert!(is_react_lifecycle_method("render"));
        assert!(is_react_lifecycle_method("componentDidMount"));
        assert!(is_react_lifecycle_method("componentDidUpdate"));
        assert!(is_react_lifecycle_method("componentWillUnmount"));
        assert!(is_react_lifecycle_method("shouldComponentUpdate"));
        assert!(is_react_lifecycle_method("getSnapshotBeforeUpdate"));
    }

    #[test]
    fn react_lifecycle_static_methods() {
        assert!(is_react_lifecycle_method("getDerivedStateFromProps"));
        assert!(is_react_lifecycle_method("getDerivedStateFromError"));
    }

    #[test]
    fn react_lifecycle_error_boundary() {
        assert!(is_react_lifecycle_method("componentDidCatch"));
    }

    #[test]
    fn react_lifecycle_deprecated_and_unsafe() {
        assert!(is_react_lifecycle_method("componentWillMount"));
        assert!(is_react_lifecycle_method("componentWillReceiveProps"));
        assert!(is_react_lifecycle_method("componentWillUpdate"));
        assert!(is_react_lifecycle_method("UNSAFE_componentWillMount"));
        assert!(is_react_lifecycle_method(
            "UNSAFE_componentWillReceiveProps"
        ));
        assert!(is_react_lifecycle_method("UNSAFE_componentWillUpdate"));
    }

    #[test]
    fn react_lifecycle_context_methods() {
        assert!(is_react_lifecycle_method("getChildContext"));
        assert!(is_react_lifecycle_method("contextType"));
    }

    #[test]
    fn not_react_lifecycle_method() {
        assert!(!is_react_lifecycle_method("handleClick"));
        assert!(!is_react_lifecycle_method("fetchData"));
        assert!(!is_react_lifecycle_method("constructor"));
        assert!(!is_react_lifecycle_method("setState"));
        assert!(!is_react_lifecycle_method("forceUpdate"));
        assert!(!is_react_lifecycle_method("customMethod"));
    }

    #[test]
    fn angular_lifecycle_core_hooks() {
        assert!(is_angular_lifecycle_method("ngOnInit"));
        assert!(is_angular_lifecycle_method("ngOnDestroy"));
        assert!(is_angular_lifecycle_method("ngOnChanges"));
        assert!(is_angular_lifecycle_method("ngAfterViewInit"));
    }

    #[test]
    fn angular_lifecycle_check_hooks() {
        assert!(is_angular_lifecycle_method("ngDoCheck"));
        assert!(is_angular_lifecycle_method("ngAfterContentChecked"));
        assert!(is_angular_lifecycle_method("ngAfterViewChecked"));
    }

    #[test]
    fn angular_lifecycle_content_hooks() {
        assert!(is_angular_lifecycle_method("ngAfterContentInit"));
        assert!(is_angular_lifecycle_method("ngAcceptInputType"));
    }

    #[test]
    fn angular_lifecycle_guard_resolver_methods() {
        assert!(is_angular_lifecycle_method("canActivate"));
        assert!(is_angular_lifecycle_method("canDeactivate"));
        assert!(is_angular_lifecycle_method("canActivateChild"));
        assert!(is_angular_lifecycle_method("canMatch"));
        assert!(is_angular_lifecycle_method("resolve"));
        assert!(is_angular_lifecycle_method("intercept"));
        assert!(is_angular_lifecycle_method("transform"));
    }

    #[test]
    fn angular_lifecycle_form_methods() {
        assert!(is_angular_lifecycle_method("validate"));
        assert!(is_angular_lifecycle_method("registerOnChange"));
        assert!(is_angular_lifecycle_method("registerOnTouched"));
        assert!(is_angular_lifecycle_method("writeValue"));
        assert!(is_angular_lifecycle_method("setDisabledState"));
    }

    #[test]
    fn not_angular_lifecycle_method() {
        assert!(!is_angular_lifecycle_method("onClick"));
        assert!(!is_angular_lifecycle_method("handleSubmit"));
        assert!(!is_angular_lifecycle_method("render"));
    }

    /// Verify every Angular lifecycle hook is recognized.
    #[test]
    fn angular_lifecycle_all_hooks_exhaustive() {
        let all_hooks = [
            "ngOnInit",
            "ngOnDestroy",
            "ngOnChanges",
            "ngDoCheck",
            "ngAfterContentInit",
            "ngAfterContentChecked",
            "ngAfterViewInit",
            "ngAfterViewChecked",
            "ngAcceptInputType",
        ];
        for hook in &all_hooks {
            assert!(
                is_angular_lifecycle_method(hook),
                "{hook} should be recognized as Angular lifecycle"
            );
        }
    }

    /// Verify every Angular guard/resolver/interceptor method is recognized.
    #[test]
    fn angular_lifecycle_all_guards_exhaustive() {
        let all_guards = [
            "canActivate",
            "canDeactivate",
            "canActivateChild",
            "canMatch",
            "resolve",
            "intercept",
            "transform",
        ];
        for guard in &all_guards {
            assert!(
                is_angular_lifecycle_method(guard),
                "{guard} should be recognized as Angular lifecycle"
            );
        }
    }

    /// Verify every Angular form method is recognized.
    #[test]
    fn angular_lifecycle_all_form_methods_exhaustive() {
        let all_form = [
            "validate",
            "registerOnChange",
            "registerOnTouched",
            "writeValue",
            "setDisabledState",
        ];
        for method in &all_form {
            assert!(
                is_angular_lifecycle_method(method),
                "{method} should be recognized as Angular lifecycle"
            );
        }
    }

    /// Methods that look similar to Angular lifecycle hooks but are NOT.
    #[test]
    fn not_angular_lifecycle_similar_names() {
        assert!(!is_angular_lifecycle_method("ngOnInit2"));
        assert!(!is_angular_lifecycle_method("onInit"));
        assert!(!is_angular_lifecycle_method("ngInit"));
        assert!(!is_angular_lifecycle_method("onDestroy"));
        assert!(!is_angular_lifecycle_method("afterViewInit"));
        assert!(!is_angular_lifecycle_method("doCheck"));
        assert!(!is_angular_lifecycle_method("ngOnInitialize"));
        assert!(!is_angular_lifecycle_method("ngonInit")); // wrong case
    }

    /// Angular methods should be case-sensitive.
    #[test]
    fn angular_lifecycle_case_sensitivity() {
        assert!(!is_angular_lifecycle_method("ngoninit"));
        assert!(!is_angular_lifecycle_method("NGONINIT"));
        assert!(!is_angular_lifecycle_method("NgOnInit"));
        assert!(!is_angular_lifecycle_method("canactivate"));
        assert!(!is_angular_lifecycle_method("CANACTIVATE"));
    }

    /// Verify every React lifecycle method is recognized (complete list).
    #[test]
    fn react_lifecycle_all_methods_exhaustive() {
        let all_methods = [
            "render",
            "componentDidMount",
            "componentDidUpdate",
            "componentWillUnmount",
            "shouldComponentUpdate",
            "getSnapshotBeforeUpdate",
            "getDerivedStateFromProps",
            "getDerivedStateFromError",
            "componentDidCatch",
            "componentWillMount",
            "componentWillReceiveProps",
            "componentWillUpdate",
            "UNSAFE_componentWillMount",
            "UNSAFE_componentWillReceiveProps",
            "UNSAFE_componentWillUpdate",
            "getChildContext",
            "contextType",
        ];
        for method in &all_methods {
            assert!(
                is_react_lifecycle_method(method),
                "{method} should be recognized as React lifecycle"
            );
        }
    }

    /// Methods that look similar to React lifecycle methods but are NOT.
    #[test]
    fn not_react_lifecycle_similar_names() {
        assert!(!is_react_lifecycle_method("componentDidMounted"));
        assert!(!is_react_lifecycle_method("onComponentDidMount"));
        assert!(!is_react_lifecycle_method("didMount"));
        assert!(!is_react_lifecycle_method("willUnmount"));
        assert!(!is_react_lifecycle_method("shouldUpdate"));
        assert!(!is_react_lifecycle_method("getDerivedState"));
        assert!(!is_react_lifecycle_method("UNSAFE_render"));
        assert!(!is_react_lifecycle_method("unsafe_componentWillMount"));
    }

    /// React lifecycle methods should be case-sensitive.
    #[test]
    fn react_lifecycle_case_sensitivity() {
        assert!(!is_react_lifecycle_method("Render"));
        assert!(!is_react_lifecycle_method("RENDER"));
        assert!(!is_react_lifecycle_method("componentdidmount"));
        assert!(!is_react_lifecycle_method("COMPONENTDIDMOUNT"));
        assert!(!is_react_lifecycle_method("ComponentDidMount"));
    }

    /// Common class methods that should never match lifecycle detection.
    #[test]
    fn not_lifecycle_common_class_methods() {
        let common_methods = [
            "constructor",
            "setState",
            "forceUpdate",
            "handleClick",
            "handleSubmit",
            "fetchData",
            "toString",
            "valueOf",
            "toJSON",
            "init",
            "destroy",
            "update",
            "mount",
            "unmount",
        ];
        for method in &common_methods {
            assert!(
                !is_react_lifecycle_method(method),
                "{method} should NOT be a React lifecycle method"
            );
            assert!(
                !is_angular_lifecycle_method(method),
                "{method} should NOT be an Angular lifecycle method"
            );
        }
    }

    #[test]
    fn angular_lifecycle_non_angular_methods() {
        assert!(!is_angular_lifecycle_method("myCustomMethod"));
        assert!(!is_angular_lifecycle_method("constructor"));
        assert!(!is_angular_lifecycle_method("ngOnSomethingCustom"));
    }

    #[test]
    fn react_unsafe_lifecycle_methods() {
        assert!(is_react_lifecycle_method("UNSAFE_componentWillMount"));
        assert!(is_react_lifecycle_method(
            "UNSAFE_componentWillReceiveProps"
        ));
        assert!(is_react_lifecycle_method("UNSAFE_componentWillUpdate"));
    }

    #[test]
    fn react_non_lifecycle_methods() {
        assert!(!is_react_lifecycle_method("handleClick"));
        assert!(!is_react_lifecycle_method("constructor"));
        assert!(!is_react_lifecycle_method("setState"));
    }
}
