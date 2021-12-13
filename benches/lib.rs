extern crate idx;
use idx::*;
use idx::util::*;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};


lazy_static! {
    pub static ref CACHE: Arc<Mutex<Cache>> = Arc::from(Mutex::from(Cache::from_path("test_cache").unwrap()));
}

fn fetch_file_idx19_u32(id: u32) {
    let mut data_provider = FileProvider::from(&*CACHE);

    data_provider.index(0);
    data_provider.archive(&id);

    let _ = data_provider.request(&0);
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("file_fetch_idx19_u32", |b| b.iter(|| fetch_file_idx19_u32(black_box(1366))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);