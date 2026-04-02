//! Startup time benchmark — measures cold-start initialization cost.

use criterion::{criterion_group, criterion_main, Criterion};

fn bench_config_load(c: &mut Criterion) {
    c.bench_function("config_load", |b| {
        b.iter(|| {
            let _settings = oxicode_config::load_settings(None);
        });
    });
}

fn bench_system_prompt_assembly(c: &mut Criterion) {
    c.bench_function("system_prompt_assembly", |b| {
        b.iter(|| {
            let _prompt = oxicode_core::system_prompt::assemble_system_prompt(
                Some("# Global instructions"),
                Some("# Project instructions"),
                None,
            );
        });
    });
}

fn bench_tool_registry(c: &mut Criterion) {
    c.bench_function("tool_registry_init", |b| {
        b.iter(|| {
            let _reg = oxicode_tools::default_registry();
        });
    });
}

criterion_group!(
    benches,
    bench_config_load,
    bench_system_prompt_assembly,
    bench_tool_registry
);
criterion_main!(benches);
