extern crate idx;
use idx::*;
use idx::util::*;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lazy_static::lazy_static;
use rand::Rng;
use std::sync::{Arc, Mutex};


lazy_static! {
    pub static ref CACHE: Arc<Mutex<Cache>> = CacheBuilder::new().with_path("test_cache").build();
}

fn fetch_file_idx19_u32(id: u32) {
    let mut data_provider = FileProvider::from(&*CACHE);

    data_provider.index(19);
    data_provider.archive(&(id >> 8));

    let _ = data_provider.request(&(id & 0xff));
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("file_fetch_idx19_u32", |b| b.iter(|| fetch_file_idx19_u32(black_box(rand::thread_rng().gen_range(0..=15000)))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);