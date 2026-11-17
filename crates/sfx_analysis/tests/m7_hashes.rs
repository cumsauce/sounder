#![allow(clippy::unwrap_used)]
mod common;
use common::*;
use serde_json::json;

fn fnv(x: &[f32]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for v in x {
        for b in v.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

#[test]
fn print_hashes() {
    for mat in ["stone","wood","metal","glass","ice","earth","granular","plant","soft","snow"] {
        for size in [0.1, 0.25, 0.5, 0.8, 1.4, 4.0] {
            for pos in [0.3, 0.5] {
                let nodes = vec![
                    json!({"type":"impulse","name":"hit","at":[0.0]}),
                    json!({"type":"modal","name":"m","excite":"hit","material":mat,"size":size,"strike_position":pos,"seed":2}),
                ];
                let d = std::panic::catch_unwind(|| node_audio(nodes, "m", 1.0));
                match d { Ok(x)=>println!("HASH {mat} {size} {pos} {:016x}", fnv(&x)), Err(_)=>println!("HASH {mat} {size} {pos} ERR") }
            }
        }
    }
}
