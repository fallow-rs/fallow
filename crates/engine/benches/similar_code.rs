#![allow(
    clippy::expect_used,
    reason = "bench fixtures use expect to keep setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::hint::black_box;
use std::mem::size_of;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_engine::similar_code::{
    EXTRACTION_SEMANTICS_VERSION, FunctionLocation, FunctionVector, SimilarCodeLimits,
    SimilarCodeSourceDigest, SimilarCodeVectorCache, VectorCacheKey, evaluate_similar_code,
    validate_function_vectors,
};

const VALIDATION_FUNCTIONS: usize = 1_000;
const RETRIEVAL_FUNCTIONS: usize = 256;
const VECTOR_DIMENSIONS: usize = 256;

fn digest(seed: u64) -> SimilarCodeSourceDigest {
    let mut bytes = [0; 32];
    bytes[24..].copy_from_slice(&seed.to_be_bytes());
    SimilarCodeSourceDigest::new(bytes)
}

fn fixture_vectors(functions: usize, clustered: bool) -> Vec<FunctionVector> {
    (0..functions)
        .map(|index| {
            let mut values = vec![0.0; VECTOR_DIMENSIONS];
            if clustered {
                values[0] = 1.0;
                values[1] = (index % 13) as f32 / 1_000.0;
            } else {
                for (dimension, value) in values.iter_mut().enumerate() {
                    let seed = index
                        .wrapping_mul(1_103_515_245)
                        .wrapping_add(dimension.wrapping_mul(12_345));
                    *value = ((seed % 2_001) as f32 - 1_000.0) / 1_000.0;
                }
            }
            FunctionVector {
                location: FunctionLocation {
                    file: format!("src/module-{index:04}.ts"),
                    start_byte: 0,
                    end_byte: 200,
                    start_line: 1,
                    start_column_utf8: 0,
                    end_line: 20,
                    end_column_utf8: 1,
                },
                source_sha256: digest(u64::try_from(index).unwrap_or(u64::MAX)),
                extraction_semantics_version: EXTRACTION_SEMANTICS_VERSION,
                values,
            }
        })
        .collect()
}

fn limits(functions: usize) -> SimilarCodeLimits {
    SimilarCodeLimits {
        dimensions: VECTOR_DIMENSIONS,
        max_functions: functions,
        max_comparisons: functions.saturating_mul(functions.saturating_sub(1)) / 2,
        max_candidates: 512,
        max_neighbors_per_function: 20,
        max_vector_bytes: functions
            .saturating_mul(VECTOR_DIMENSIONS)
            .saturating_mul(size_of::<f32>()),
    }
}

fn bench_similar_code(c: &mut Criterion) {
    let validation = fixture_vectors(VALIDATION_FUNCTIONS, false);
    c.bench_function("similar_code/vector_validation_1000x256", |bencher| {
        bencher.iter(|| {
            validate_function_vectors(
                black_box(&validation),
                VECTOR_DIMENSIONS,
                EXTRACTION_SEMANTICS_VERSION,
            )
            .expect("valid benchmark vectors");
        });
    });

    let retrieval = fixture_vectors(RETRIEVAL_FUNCTIONS, false);
    c.bench_function("similar_code/retrieval_256x256", |bencher| {
        bencher.iter(|| {
            evaluate_similar_code(
                black_box(&retrieval),
                0.90,
                limits(RETRIEVAL_FUNCTIONS),
                EXTRACTION_SEMANTICS_VERSION,
            )
            .expect("valid benchmark evaluation")
        });
    });

    let ranking = fixture_vectors(RETRIEVAL_FUNCTIONS, true);
    c.bench_function("similar_code/ranking_256x256", |bencher| {
        bencher.iter(|| {
            evaluate_similar_code(
                black_box(&ranking),
                0.95,
                limits(RETRIEVAL_FUNCTIONS),
                EXTRACTION_SEMANTICS_VERSION,
            )
            .expect("valid benchmark evaluation")
        });
    });

    let cache_key = VectorCacheKey {
        function_source_sha256: digest(42),
        extraction_semantics_version: EXTRACTION_SEMANTICS_VERSION,
        model_id: "fixture-model".to_string(),
        model_revision: "fixture-model@immutable".to_string(),
        dimensions: VECTOR_DIMENSIONS,
        provider_parameter_digest: 7,
    };
    let mut hit_cache = SimilarCodeVectorCache::new(4 * 1024 * 1024);
    hit_cache.insert(cache_key.clone(), vec![1.0; VECTOR_DIMENSIONS]);
    c.bench_function("similar_code/vector_cache_hit", |bencher| {
        bencher.iter(|| black_box(hit_cache.get(black_box(&cache_key))));
    });

    c.bench_function("similar_code/vector_cache_miss_insert", |bencher| {
        let mut index = 0u64;
        bencher.iter_batched(
            || {
                let mut key = cache_key.clone();
                key.function_source_sha256 = digest(index);
                index = index.wrapping_add(1);
                (
                    SimilarCodeVectorCache::new(4 * 1024 * 1024),
                    key,
                    vec![1.0; VECTOR_DIMENSIONS],
                )
            },
            |(mut cache, key, vector)| {
                black_box(cache.insert(key, vector));
                cache
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_similar_code);
criterion_main!(benches);
