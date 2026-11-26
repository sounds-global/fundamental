//! Scaling benchmark for rename detection in `SchemaDiffer::diff`, against a no-rename baseline.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use fundamental_db::migration::{
    DatabaseSchema, DiffConfig, SchemaDiffer, TableColumn, TableSchema,
};

/// Two orders of magnitude, so the per-10x ratio shows linear (~10x) or quadratic (~100x).
const SIZES: [usize; 3] = [10, 100, 1_000];

fn column(name: String) -> TableColumn {
    TableColumn {
        name,
        data_type: "TEXT".to_string(),
        nullable: false,
        default: None,
        primary_key: false,
        unique: false,
    }
}

fn schema(prefix: &str, n: usize) -> DatabaseSchema {
    let mut db = DatabaseSchema::new();
    let mut table = TableSchema::new("bench_table");
    for i in 0..n {
        table.add_column(column(format!("{prefix}_{i}")));
    }
    db.add_table(table);
    db
}

fn bench_rename_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_diff/rename_detection");
    let differ = SchemaDiffer::with_config(DiffConfig {
        detect_renames: true,
        ..Default::default()
    });
    for &n in &SIZES {
        let expected = schema("new", n);
        let actual = schema("old", n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(differ.diff(black_box(&expected), black_box(&actual))));
        });
    }
    group.finish();
}

fn bench_no_rename(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_diff/no_rename");
    let differ = SchemaDiffer::new();
    for &n in &SIZES {
        let expected = schema("new", n);
        let actual = schema("old", n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(differ.diff(black_box(&expected), black_box(&actual))));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_rename_detection, bench_no_rename);
criterion_main!(benches);
