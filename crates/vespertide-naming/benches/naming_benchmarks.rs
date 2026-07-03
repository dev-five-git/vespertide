use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use vespertide_naming::{build_foreign_key_name, build_index_name, build_unique_constraint_name};

fn columns(n: usize, sorted: bool) -> Vec<String> {
    let mut columns = (0..n).map(|i| format!("column_{i:02}")).collect::<Vec<_>>();
    if !sorted {
        columns.reverse();
    }
    columns
}

fn bench_constraint_names(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraint_name_builders");
    for n_columns in [1, 5, 20] {
        for sorted in [true, false] {
            let columns = columns(n_columns, sorted);
            let suffix = if sorted { "sorted" } else { "unsorted" };

            group.bench_with_input(
                BenchmarkId::new("index", format!("{n_columns}_{suffix}")),
                &columns,
                |b, columns| {
                    b.iter(|| build_index_name(black_box("orders"), black_box(columns), None));
                },
            );
            group.bench_with_input(
                BenchmarkId::new("unique", format!("{n_columns}_{suffix}")),
                &columns,
                |b, columns| {
                    b.iter(|| {
                        build_unique_constraint_name(black_box("orders"), black_box(columns), None);
                    });
                },
            );
            group.bench_with_input(
                BenchmarkId::new("foreign_key", format!("{n_columns}_{suffix}")),
                &columns,
                |b, columns| {
                    b.iter(|| {
                        build_foreign_key_name(black_box("orders"), black_box(columns), None)
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, bench_constraint_names);
criterion_main!(benches);
