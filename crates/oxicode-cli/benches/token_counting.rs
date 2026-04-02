//! Token counting and message serialization throughput benchmarks.

use criterion::{criterion_group, criterion_main, Criterion};

fn bench_message_creation(c: &mut Criterion) {
    c.bench_function("message_user_create", |b| {
        b.iter(|| {
            let _msg = oxicode_common::Message::user("Hello, how are you doing today?");
        });
    });
}

fn bench_message_text_extraction(c: &mut Criterion) {
    let mut msg = oxicode_common::Message::assistant();
    for i in 0..10 {
        msg.content.push(oxicode_common::ContentBlock::Text {
            text: format!("Block {i} with some content here. "),
        });
    }

    c.bench_function("message_text_extraction_10_blocks", |b| {
        b.iter(|| {
            let _text = msg.text();
        });
    });
}

fn bench_content_block_serde(c: &mut Criterion) {
    let block = oxicode_common::ContentBlock::ToolUse {
        id: "tu_123".to_string(),
        name: "bash".to_string(),
        input: serde_json::json!({"command": "ls -la /tmp"}),
    };

    c.bench_function("content_block_serialize", |b| {
        b.iter(|| {
            let _json = serde_json::to_string(&block).unwrap();
        });
    });

    let json = serde_json::to_string(&block).unwrap();
    c.bench_function("content_block_deserialize", |b| {
        b.iter(|| {
            let _block: oxicode_common::ContentBlock = serde_json::from_str(&json).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_message_creation,
    bench_message_text_extraction,
    bench_content_block_serde
);
criterion_main!(benches);
