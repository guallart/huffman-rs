//! Core Huffman coding primitives: frequency analysis, tree construction,
//! code table generation, and bit-level encode/decode.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::fmt;

#[derive(Debug, Eq, PartialEq)]
pub enum Node {
    Internal(Box<HuffmanNode>, Box<HuffmanNode>),
    Leaf(u8),
}

#[derive(Debug, Eq, PartialEq)]
pub struct HuffmanNode {
    pub node: Node,
    pub frequency: usize,
}

// `BinaryHeap` is a max-heap, but Huffman's algorithm needs the *least*
// frequent nodes first, so the ordering is reversed here.
impl Ord for HuffmanNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.frequency.cmp(&self.frequency)
    }
}

impl PartialOrd for HuffmanNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BitCode {
    bits: u32,
    length: u32,
}

impl fmt::Display for BitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:0width$b}", self.bits, width = self.length as usize)
    }
}

/// Counts how many times each byte value appears in `message`.
/// Only byte values that actually occur are returned.
pub fn compute_frequencies(message: &[u8]) -> Vec<(u8, usize)> {
    let mut frequencies = [0usize; 256];

    for &ch in message {
        frequencies[ch as usize] += 1;
    }

    frequencies
        .into_iter()
        .enumerate()
        .filter(|&(_, count)| count > 0)
        .map(|(index, count)| (index as u8, count))
        .collect()
}

/// Builds a Huffman tree from byte frequencies, repeatedly merging the two
/// least-frequent nodes until a single root remains.
///
/// Returns `None` if `frequencies` is empty (i.e. the input had no bytes).
pub fn build_tree(frequencies: &[(u8, usize)]) -> Option<HuffmanNode> {
    if frequencies.is_empty() {
        return None;
    }

    let mut heap = BinaryHeap::with_capacity(frequencies.len());
    for &(ch, freq) in frequencies {
        heap.push(HuffmanNode {
            node: Node::Leaf(ch),
            frequency: freq,
        });
    }

    while heap.len() > 1 {
        let left = heap.pop().unwrap();
        let right = heap.pop().unwrap();

        heap.push(HuffmanNode {
            frequency: left.frequency + right.frequency,
            node: Node::Internal(Box::new(left), Box::new(right)),
        });
    }

    heap.pop()
}

/// Walks the tree and records the bit pattern that leads to each leaf,
/// producing a byte -> code lookup table for encoding.
pub fn build_table(node: &Node, current_bits: u32, current_length: u32, table: &mut HashMap<u8, BitCode>) {
    match node {
        Node::Leaf(ch) => {
            table.insert(*ch, BitCode { bits: current_bits, length: current_length });
        }
        Node::Internal(left, right) => {
            build_table(&left.node, (current_bits << 1) | 1, current_length + 1, table);
            build_table(&right.node, current_bits << 1, current_length + 1, table);
        }
    }
}

/// Encodes `data` using the given code table, returning the packed bytes
/// along with the exact number of bits written (needed since the last byte
/// is usually padded).
pub fn encode(data: &[u8], table: &HashMap<u8, BitCode>) -> (Vec<u8>, usize) {
    let mut encoded_bytes = Vec::with_capacity(data.len() / 2);

    let mut bit_accumulator: u64 = 0;
    let mut bits_in_accumulator: u32 = 0;
    let mut total_bits: usize = 0;

    for &byte in data {
        if let Some(code) = table.get(&byte) {
            bit_accumulator = (bit_accumulator << code.length) | (code.bits as u64);
            bits_in_accumulator += code.length;
            total_bits += code.length as usize;

            while bits_in_accumulator >= 8 {
                bits_in_accumulator -= 8;
                let byte_to_write = (bit_accumulator >> bits_in_accumulator) as u8;
                encoded_bytes.push(byte_to_write);
                bit_accumulator &= (1 << bits_in_accumulator) - 1;
            }
        }
    }

    if bits_in_accumulator > 0 {
        let byte_to_write = (bit_accumulator << (8 - bits_in_accumulator)) as u8;
        encoded_bytes.push(byte_to_write);
    }

    (encoded_bytes, total_bits)
}

/// Decodes `bytes` back into the original data by walking the tree one bit
/// at a time, stopping after exactly `bit_count` bits.
///
/// Special case: if the tree is a single leaf (the input contained only one
/// distinct byte value), there's no branch to encode, so `bit_count` is 0
/// and there's nothing to walk. `root.frequency` is repurposed by the
/// caller to carry the original byte count so the repeated byte can still
/// be reconstructed.
pub fn decode(bytes: &[u8], bit_count: usize, root: &HuffmanNode) -> Vec<u8> {
    // A single-leaf tree means the input had only one distinct byte value.
    // Its code has length 0 (there was no branch to encode), so `bit_count`
    // is legitimately 0 here — this check must come before the bit_count
    // guard below, or the repeated byte is silently lost.
    if let Node::Leaf(ch) = &root.node {
        return vec![*ch; root.frequency];
    }

    let mut data = Vec::new();

    if bit_count == 0 {
        return data;
    }

    let mut current_node = &root.node;
    let mut bits_processed = 0;

    for &byte in bytes {
        for i in (0..8).rev() {
            if bits_processed >= bit_count {
                break;
            }

            let bit = (byte >> i) & 1;
            bits_processed += 1;

            if let Node::Internal(left, right) = current_node {
                current_node = if bit == 1 { &left.node } else { &right.node };
            }

            if let Node::Leaf(ch) = current_node {
                data.push(*ch);
                current_node = &root.node;
            }
        }
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_message() -> Vec<u8> {
        let a = "a".repeat(100);
        let b = "b".repeat(50);
        let c = "c".repeat(10);
        let d = "d".repeat(5);
        let e = "e".repeat(1);
        let message_str = format!("{}{}{}{}{}", a, b, c, d, e);
        message_str.into_bytes()
    }

    #[test]
    fn test_compute_frequencies() {
        let message = get_message();
        let mut freqs = compute_frequencies(&message);

        freqs.sort_by_key(|&(ch, _)| ch);

        let expected = vec![
            (b'a', 100),
            (b'b', 50),
            (b'c', 10),
            (b'd', 5),
            (b'e', 1),
        ];

        assert_eq!(freqs, expected);
    }

    #[test]
    fn test_build_tree_table() {
        let message = get_message();
        let freqs = compute_frequencies(&message);

        let root = build_tree(&freqs).expect("non-empty input should yield a tree");

        let mut table = HashMap::new();
        build_table(&root.node, 0, 0, &mut table);

        assert_eq!(table.len(), 5);

        assert_eq!(table.get(&b'a').unwrap().length, 1);
        assert_eq!(table.get(&b'b').unwrap().length, 2);
        assert_eq!(table.get(&b'c').unwrap().length, 3);
        assert_eq!(table.get(&b'd').unwrap().length, 4);
        assert_eq!(table.get(&b'e').unwrap().length, 4);

        assert_eq!(format!("{}", table.get(&b'a').unwrap()), "0");
        assert_eq!(format!("{}", table.get(&b'b').unwrap()), "10");
        assert_eq!(format!("{}", table.get(&b'c').unwrap()), "110");
        assert_eq!(format!("{}", table.get(&b'd').unwrap()), "1110");
        assert_eq!(format!("{}", table.get(&b'e').unwrap()), "1111");
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let message = get_message();
        let freqs = compute_frequencies(&message);
        let root = build_tree(&freqs).expect("non-empty input should yield a tree");

        let mut table = HashMap::new();
        build_table(&root.node, 0, 0, &mut table);

        let (encoded_bytes, total_bits) = encode(&message, &table);
        let decoded_message = decode(&encoded_bytes, total_bits, &root);

        assert_eq!(message, decoded_message);
    }

    #[test]
    fn test_single_unique_byte_roundtrip() {
        // Edge case: input with only one distinct byte value produces a
        // single-leaf tree, which `decode` must special-case.
        let message = vec![b'x'; 42];
        let freqs = compute_frequencies(&message);
        let root = build_tree(&freqs).expect("non-empty input should yield a tree");

        let mut table = HashMap::new();
        build_table(&root.node, 0, 0, &mut table);

        let (encoded_bytes, total_bits) = encode(&message, &table);
        let decoded = decode(&encoded_bytes, total_bits, &root);

        assert_eq!(decoded, message);
    }

    #[test]
    fn test_empty_input_has_no_tree() {
        let freqs = compute_frequencies(&[]);
        assert!(build_tree(&freqs).is_none());
    }
}
