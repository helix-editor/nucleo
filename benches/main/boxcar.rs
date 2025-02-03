use std::{sync::Arc, thread::available_parallelism};

use criterion::{BenchmarkId, Criterion};
use nucleo::boxcar;
use rayon::prelude::*;

const TINY_LINE_COUNT: u32 = 100;
const SMALL_LINE_COUNT: u32 = 1_000;
const MEDIUM_LINE_COUNT: u32 = 50_000;
const LARGE_LINE_COUNT: u32 = 500_000;
const XLARGE_LINE_COUNT: u32 = 5_000_000;
const XXLARGE_LINE_COUNT: u32 = 20_000_000;

fn grow_boxcar(c: &mut Criterion) {
    let mut group = c.benchmark_group("grow_boxcar");
    for line_count in [
        TINY_LINE_COUNT,
        SMALL_LINE_COUNT,
        MEDIUM_LINE_COUNT,
        LARGE_LINE_COUNT,
        XLARGE_LINE_COUNT,
        XXLARGE_LINE_COUNT,
    ] {
        // generate random strings
        let lines = random_lines(line_count);

        group.bench_with_input(BenchmarkId::new("push", line_count), &lines, |b, lines| {
            b.iter(move || {
                let v = Arc::new(boxcar::Vec::with_capacity(2 * 1024, 1));
                for line in lines {
                    v.push(line, |_, _cols| {});
                }
            });
        });

        group.bench_with_input(
            BenchmarkId::new("extend", line_count),
            &lines,
            |b, lines| {
                b.iter(move || {
                    let v = Arc::new(boxcar::Vec::with_capacity(2 * 1024, 1));
                    v.extend(lines.iter(), |_, _cols| {});
                });
            },
        );
    }
}

fn grow_boxcar_threaded(c: &mut Criterion) {
    let mut group = c.benchmark_group("grow_boxcar_push_threaded");
    for line_count in [
        TINY_LINE_COUNT,
        SMALL_LINE_COUNT,
        MEDIUM_LINE_COUNT,
        LARGE_LINE_COUNT,
        XLARGE_LINE_COUNT,
        XXLARGE_LINE_COUNT,
    ] {
        // generate random strings
        let lines = random_lines(line_count);
        let available_parallelism = available_parallelism().unwrap();
        let batch_size = lines.len() / usize::from(available_parallelism);

        group.bench_with_input(BenchmarkId::new("push", line_count), &lines, |b, lines| {
            b.iter(|| {
                let v = Arc::new(boxcar::Vec::with_capacity(2 * 1024, 1));
                lines
                    .chunks(batch_size)
                    .par_bridge()
                    .into_par_iter()
                    .for_each(|batch| {
                        for line in batch {
                            v.push(line, |_, _cols| {});
                        }
                    });
            });
        });

        group.bench_with_input(
            BenchmarkId::new("extend", line_count),
            &lines,
            |b, lines| {
                b.iter(|| {
                    let v = Arc::new(boxcar::Vec::with_capacity(2 * 1024, 1));
                    lines
                        .chunks(batch_size)
                        .par_bridge()
                        .into_par_iter()
                        .for_each(|batch| {
                            v.extend(batch.iter(), |_, _cols| {});
                        });
                });
            },
        );
    }
}

fn random_lines(count: u32) -> Vec<String> {
    let count = i64::from(count);
    let word_count = 1;
    (0..count)
        .map(|_| fakeit::words::sentence(word_count))
        .collect()
}

criterion::criterion_group!(benches, grow_boxcar, grow_boxcar_threaded);
criterion::criterion_main!(benches);
