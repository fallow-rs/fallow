//! Custom serde serializers for `PathBuf`, `Option<PathBuf>`, and `Vec<PathBuf>` that always
//! output forward slashes, regardless of platform. This ensures consistent
//! JSON/SARIF output on Windows.

use std::path::{Path, PathBuf};

use serde::Serializer;

/// Serialize a `Path` with forward slashes for cross-platform consistency.
///
/// # Errors
///
/// Returns any serializer error produced while writing the normalized path string.
pub fn serialize<S: Serializer>(path: &Path, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&path.to_string_lossy().replace('\\', "/"))
}

/// Serialize an `Option<PathBuf>` with forward slashes for cross-platform consistency.
///
/// # Errors
///
/// Returns any serializer error produced while writing the normalized optional path.
pub fn serialize_option<S: Serializer>(path: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    match path {
        Some(path) => s.serialize_some(&path.to_string_lossy().replace('\\', "/")),
        None => s.serialize_none(),
    }
}

/// Serialize a `Vec<PathBuf>` with forward slashes for cross-platform consistency.
///
/// # Errors
///
/// Returns any serializer error produced while writing the normalized path list.
pub fn serialize_vec<S: Serializer>(paths: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = s.serialize_seq(Some(paths.len()))?;
    for p in paths {
        seq.serialize_element(&p.to_string_lossy().replace('\\', "/"))?;
    }
    seq.end()
}

#[cfg(test)]
mod tests {
    /// Property tests that drive the real `serialize` / `serialize_option` /
    /// `serialize_vec` functions through `serde_json`. The forward-slash output
    /// is a load-bearing cross-platform invariant for JSON/SARIF, and the input
    /// space (arbitrary separators) is unbounded, so it is encoded as
    /// properties.
    mod proptests {
        use proptest::prelude::*;
        use serde::Serialize;
        use std::path::PathBuf;

        /// Wrapper that routes its field through the real scalar serializer.
        #[derive(Serialize)]
        struct ScalarPath {
            #[serde(serialize_with = "crate::serde_path::serialize")]
            path: PathBuf,
        }

        /// Wrapper that routes its field through the real option serializer.
        #[derive(Serialize)]
        struct OptionalPath {
            #[serde(serialize_with = "crate::serde_path::serialize_option")]
            path: Option<PathBuf>,
        }

        /// Wrapper that routes its field through the real vec serializer.
        #[derive(Serialize)]
        struct PathList {
            #[serde(serialize_with = "crate::serde_path::serialize_vec")]
            paths: Vec<PathBuf>,
        }

        /// Path-like strings over an alphabet that mixes both separators, so the
        /// backslash-to-forward-slash rewrite is actually exercised (arbitrary
        /// unicode would almost never hit the `\` branch).
        fn path_like() -> impl Strategy<Value = String> {
            prop::collection::vec(
                prop::sample::select(vec!['a', 'b', '1', '/', '\\', '.', '-', '_', ' ']),
                0..40,
            )
            .prop_map(|chars| chars.into_iter().collect())
        }

        /// Serialize one path through `ScalarPath` and return the emitted string.
        fn scalar_json(path: &str) -> String {
            let value = serde_json::to_value(ScalarPath {
                path: PathBuf::from(path),
            })
            .expect("scalar wrapper serializes");
            value["path"].as_str().expect("path is a string").to_owned()
        }

        /// Serialize one optional path through `OptionalPath`.
        fn option_json(path: Option<&str>) -> serde_json::Value {
            serde_json::to_value(OptionalPath {
                path: path.map(PathBuf::from),
            })
            .expect("option wrapper serializes")
        }

        proptest! {
            /// The serializer never emits a backslash and equals the input with
            /// every `\` rewritten to `/`. Exercises the real `serialize` fn.
            #[test]
            fn serialize_emits_only_forward_slashes(path in path_like()) {
                let out = scalar_json(&path);
                prop_assert!(!out.contains('\\'), "output {out:?} still contains a backslash");
                prop_assert_eq!(out, path.replace('\\', "/"));
            }

            /// Round-trip: a serialized path read back out of the JSON is its
            /// forward-slashed form. `PathBuf` has no custom deserializer, so the
            /// normalized string is the fixed point a second pass cannot change.
            #[test]
            fn serialize_then_read_back_is_normalized(path in path_like()) {
                let json = serde_json::to_string(&ScalarPath { path: PathBuf::from(&path) })
                    .expect("scalar wrapper serializes");
                let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
                let restored = parsed["path"].as_str().expect("path is a string");
                prop_assert_eq!(restored, path.replace('\\', "/"));
            }

            /// Idempotence: serializing the already-normalized output again is a
            /// no-op, so repeated passes never corrupt a path.
            #[test]
            fn serialize_is_idempotent(path in path_like()) {
                let once = scalar_json(&path);
                let twice = scalar_json(&once);
                prop_assert_eq!(once, twice);
            }

            /// The option serializer keeps `None` as null and normalizes `Some`.
            #[test]
            fn serialize_option_normalizes_some(path in path_like()) {
                let value = option_json(Some(&path));
                let out = value["path"].as_str().expect("path is a string");
                prop_assert!(!out.contains('\\'), "output {out:?} still contains a backslash");
                prop_assert_eq!(out, path.replace('\\', "/"));
            }

            /// None remains a JSON null rather than a string sentinel.
            #[test]
            fn serialize_option_none_is_null(_path in path_like()) {
                let value = option_json(None);
                prop_assert!(value["path"].is_null());
            }

            /// The vec serializer agrees element-for-element with the scalar
            /// serializer, so the two independent functions cannot drift apart.
            #[test]
            fn serialize_vec_matches_scalar(paths in prop::collection::vec(path_like(), 0..8)) {
                let value = serde_json::to_value(PathList {
                    paths: paths.iter().map(PathBuf::from).collect(),
                })
                .expect("vec wrapper serializes");
                let array = value["paths"].as_array().expect("paths is an array");
                prop_assert_eq!(array.len(), paths.len());
                for (element, original) in array.iter().zip(&paths) {
                    let serialized = element.as_str().expect("element is a string");
                    prop_assert_eq!(serialized.to_owned(), scalar_json(original));
                }
            }
        }
    }
}
