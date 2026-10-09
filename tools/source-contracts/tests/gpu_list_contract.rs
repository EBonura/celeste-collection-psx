//! Keep linked-list primitives within Sony's documented 16-word total size.
//!
//! Fixture is one completed Classic frame from the unchanged 0.2.3 executable.
//! No assets, executable code or RAM outside its GPU command list are included.

mod common;

use serde_json::Value;

/// 16 total words includes the linked-list tag.
const SAFE_PAYLOAD_WORDS: usize = 15;

type Packet = Vec<u32>;

/// Split a node's words into whole GPU packets by opcode.
fn packets(words: &[u32]) -> Result<Vec<Packet>, String> {
    let mut at = 0;
    let mut out = Vec::new();
    while at < words.len() {
        let opcode = words[at] >> 24;
        let size = match opcode {
            0xA0 => {
                let wh = i64::from(
                    *words
                        .get(at + 2)
                        .ok_or("GPU packet crosses node boundary")?,
                );
                let width = ((wh - 1) & 1023) + 1;
                let height = (((wh >> 16) - 1) & 511) + 1;
                3 + ((width * height + 1) / 2) as usize
            }
            0x00 | 0x01 | 0x1F | 0xE1..=0xE6 => 1,
            0x02 => 3,
            0x20..=0x3F => {
                let vertices = if opcode & 8 != 0 { 4 } else { 3 };
                let mut size = 1 + vertices + if opcode & 4 != 0 { vertices } else { 0 };
                size += if opcode & 16 != 0 { vertices - 1 } else { 0 };
                size as usize
            }
            0x60..=0x7F => (2 + usize::from(opcode & 4 != 0) + usize::from(opcode & 0x18 == 0)),
            _ => return Err(format!("Unsupported fixture opcode {opcode:02x}")),
        };
        if at + size > words.len() {
            return Err("GPU packet crosses node boundary".into());
        }
        out.push(words[at..at + size].to_vec());
        at += size;
    }
    Ok(out)
}

fn regroup(commands: &[Packet], limit: usize) -> Result<Vec<Vec<u32>>, String> {
    let (mut result, mut current): (Vec<Vec<u32>>, Vec<u32>) = (Vec::new(), Vec::new());
    for command in commands {
        if command.len() > limit {
            return Err("A complete GPU packet does not fit".into());
        }
        if current.len() + command.len() > limit {
            result.push(std::mem::take(&mut current));
        }
        current.extend(command);
    }
    if !current.is_empty() {
        result.push(current);
    }
    Ok(result)
}

struct Fixture {
    counts: Vec<u64>,
    node_words: Vec<Vec<u32>>,
    commands: Vec<Packet>,
}

fn fixture() -> Fixture {
    let text = common::read("tools/fixtures/gpu-list-classic-frame.json");
    let nodes: Value = serde_json::from_str(&text).expect("fixture JSON");
    let nodes = nodes.as_array().expect("array of nodes");
    let counts = nodes
        .iter()
        .map(|n| n["count"].as_u64().expect("count"))
        .collect();
    let node_words: Vec<Vec<u32>> = nodes
        .iter()
        .map(|n| {
            n["words"]
                .as_array()
                .expect("words")
                .iter()
                .map(|w| w.as_u64().expect("word") as u32)
                .collect()
        })
        .collect();
    let commands = node_words
        .iter()
        .flat_map(|words| packets(words).expect("fixture packets"))
        .collect();
    Fixture {
        counts,
        node_words,
        commands,
    }
}

#[test]
fn backend_uses_sdk_ordered_stream() {
    let source = common::read("shared/src/backend.rs");
    assert!(source.contains("gpu::ordered::OrderedCommandStream"));
    assert!(!source.contains("const NODE_MAX"));
    assert!(!source.contains("fn kick_pending"));
}

#[test]
fn shipped_fixture_demonstrates_violation() {
    let f = fixture();
    assert_eq!(f.counts, [254, 253, 252, 236]);
    assert!(f
        .counts
        .iter()
        .all(|&count| count > SAFE_PAYLOAD_WORDS as u64));
    assert_eq!(f.commands.len(), 264);
}

#[test]
fn regroup_preserves_every_word_and_packet() {
    let f = fixture();
    let small = regroup(&f.commands, SAFE_PAYLOAD_WORDS).unwrap();
    assert!(small.iter().all(|node| node.len() <= SAFE_PAYLOAD_WORDS));
    let flat = |nodes: &[Vec<u32>]| nodes.iter().flatten().copied().collect::<Vec<u32>>();
    assert_eq!(flat(&small), flat(&f.node_words));
    let regrouped: Vec<Packet> = small
        .iter()
        .flat_map(|node| packets(node).unwrap())
        .collect();
    assert_eq!(regrouped, f.commands);
}

#[test]
fn exact_limit_and_oversize_packet() {
    let lengths: Vec<usize> = regroup(&[vec![0; 11], vec![0; 4], vec![0; 1]], 15)
        .unwrap()
        .iter()
        .map(Vec::len)
        .collect();
    assert_eq!(lengths, [15, 1]);
    assert!(regroup(&[vec![0; 16]], 15).is_err());
}

#[test]
fn upload_cannot_be_split() {
    let f = fixture();
    let upload = f
        .commands
        .iter()
        .find(|p| p[0] >> 24 == 0xA0)
        .unwrap()
        .clone();
    assert_eq!(upload.len(), 11);
    assert_eq!(
        regroup(&[vec![0; 5], upload.clone()], 15).unwrap(),
        [vec![0; 5], upload.clone()]
    );
    assert!(packets(&upload[..upload.len() - 1]).is_err());
}
