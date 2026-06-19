use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::fmt;

#[derive(Debug, Eq, PartialEq)]
enum Node {
    Internal(Box<HuffmanNode>, Box<HuffmanNode>),
    Leaf(u8)
}

#[derive(Debug, Eq, PartialEq)]
struct HuffmanNode {
    node: Node,
    frequency: usize
}

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
struct BitCode {
    bits: u32,
    length: u32,
}

impl fmt::Display for BitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:0width$b}", self.bits, width = self.length as usize)
    }
}

fn build_tree(frequencies: &[(u8, usize)]) -> Option<HuffmanNode> {
    if frequencies.is_empty() { return None; }

    // Fill the tree with leaves
    let mut heap = BinaryHeap::with_capacity(frequencies.len());
    for &(ch, freq) in frequencies {
        heap.push(
            HuffmanNode {
                node: Node::Leaf(ch),
                frequency: freq
            }
        );
    }

    // Take the least frequent two nodes and create the parent node
    while heap.len() > 1 {
        let left = heap.pop().unwrap();
        let right = heap.pop().unwrap();

        heap.push(
            HuffmanNode {
                frequency: left.frequency + right.frequency,
                node: Node::Internal(Box::new(left), Box::new(right))
            }
        );
    }

    // Return the root node
    heap.pop()
}

fn build_table(node: &Node, current_bits: u32, current_length: u32, table: &mut HashMap<u8, BitCode>) {
    match node {
        Node::Leaf(ch) => {
            table.insert(
                *ch,
                BitCode{ bits: current_bits, length: current_length }
            );
        },
        Node::Internal(left, right) => {
            build_table(&left.node, (current_bits << 1) | 1, current_length + 1, table);
            build_table(&right.node, current_bits << 1, current_length + 1, table);
        }
    }
}

fn compute_frequencies(message: &[u8]) -> Vec<(u8, usize)> {
    let mut frequencies = [0; 256];

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

fn encode(data: &[u8], table: &HashMap<u8, BitCode>) -> (Vec<u8>, usize) {
    let mut encoded_bytes = Vec::with_capacity(data.len() / 2);

    let mut bit_accumulator: u64 = 0;
    let mut bits_in_accumulator: u32 = 0;
    let mut total_bits: usize = 0;

    for &byte in data {
        if let Some(code) = table.get(&byte) {
            bit_accumulator = (bit_accumulator << code.length) | (code.bits as u64);
            bits_in_accumulator += code.length;
            total_bits += code.length as usize;

            // Extract bytes from accumulator
            while bits_in_accumulator >= 8 {
                bits_in_accumulator -= 8;
                let byte_to_write = (bit_accumulator >> bits_in_accumulator) as u8;
                encoded_bytes.push(byte_to_write);

                // Clean written bits
                bit_accumulator &= (1 << bits_in_accumulator) - 1;
            }
        }
    }

    // Write leftover bits in accumulator
    if bits_in_accumulator > 0 {
        let byte_to_write = (bit_accumulator << (8 - bits_in_accumulator)) as u8;
        encoded_bytes.push(byte_to_write);
    }

    (encoded_bytes, total_bits)
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

        let root_opt = build_tree(&freqs);
        assert!(root_opt.is_some());

        let root = root_opt.unwrap();

        let mut table = HashMap::new();
        build_table(&root.node, 0, 0, &mut table);

        //for (&ch, &bit_code) in &table {
        //    println!("'{}' -> {}", ch as char, bit_code);
        //}

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
    fn test_encode() {
        let message = get_message();
        let freqs = compute_frequencies(&message);

        let root_opt = build_tree(&freqs);
        assert!(root_opt.is_some());

        let root = root_opt.unwrap();

        let mut table = HashMap::new();
        build_table(&root.node, 0, 0, &mut table);

        let (encoded_bytes, total_bits) = encode(&message, &table);

        let encoded_str = encoded_bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<String>>()
            .join(", ");

        println!("{}", encoded_str);
    }
}
