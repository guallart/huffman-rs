//! Reads files from disk, drives the Huffman encode/decode pipeline, and
//! handles the `.huff` archive format: a small header (original file name,
//! the serialized Huffman tree, and bit counts) followed by the packed data.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use crate::huffman::{build_table, build_tree, compute_frequencies, decode, encode};
use crate::huffman::{HuffmanNode, Node};

const SIGNATURE: &[u8; 7] = b"HUFFMAN";

/// All length/count fields in the header are stored as fixed-width u64s,
/// regardless of the host's native `usize` width, so that an archive
/// produced on one platform (e.g. 64-bit) can still be decoded on another
/// (e.g. 32-bit).
const U64_SIZE: usize = std::mem::size_of::<u64>();

struct FileData {
    name: String,
    extension: String,
    data: Vec<u8>,
}

fn read_file(file_path: &str) -> io::Result<FileData> {
    let data = fs::read(file_path)
        .map_err(|e| io::Error::new(e.kind(), format!("could not open '{}': {}", file_path, e)))?;

    let path = Path::new(file_path);

    let name = path
        .file_stem()
        .and_then(|os_str| os_str.to_str())
        .ok_or_else(|| invalid_data(format!("could not determine a file name for '{}'", file_path)))?
        .to_string();

    let extension = path
        .extension()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    Ok(FileData { name, extension, data })
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

struct Header {
    original_name: String,
    tree_bit_count: u64,
    tree: Vec<u8>,
    data_bit_count: u64,
    original_byte_count: u64,
}

impl Header {
    fn serialize(self) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();

        let name_bytes = self.original_name.as_bytes();
        bytes.extend_from_slice(&(name_bytes.len() as u64).to_le_bytes());
        bytes.extend_from_slice(name_bytes);

        bytes.extend_from_slice(&self.tree_bit_count.to_le_bytes());
        bytes.extend_from_slice(&self.tree);

        bytes.extend_from_slice(&self.data_bit_count.to_le_bytes());
        bytes.extend_from_slice(&self.original_byte_count.to_le_bytes());

        bytes
    }

    fn deserialize(bytes: &[u8]) -> Result<Header, String> {
        let mut cursor = bytes;

        if cursor.len() < SIGNATURE.len() {
            return Err("file is too short to contain a valid signature".to_string());
        }
        let (signature_bytes, rest) = cursor.split_at(SIGNATURE.len());
        if signature_bytes != SIGNATURE {
            return Err("invalid signature — this doesn't look like a .huff archive".to_string());
        }
        cursor = rest;

        let original_name_length = read_u64(&mut cursor, "original_name_length")? as usize;
        if cursor.len() < original_name_length {
            return Err("file is too short to read the original file name".to_string());
        }
        let (name_bytes, rest) = cursor.split_at(original_name_length);
        let original_name = String::from_utf8(name_bytes.to_vec())
            .map_err(|e| format!("invalid UTF-8 in stored file name: {}", e))?;
        cursor = rest;

        let tree_bit_count = read_u64(&mut cursor, "tree_bit_count")?;
        let tree_bytes_len = (tree_bit_count as usize).div_ceil(8);
        if cursor.len() < tree_bytes_len {
            return Err("file is too short to read the Huffman tree".to_string());
        }
        let (tree_bytes, rest) = cursor.split_at(tree_bytes_len);
        let tree = tree_bytes.to_vec();
        cursor = rest;

        let data_bit_count = read_u64(&mut cursor, "data_bit_count")?;
        let original_byte_count = read_u64(&mut cursor, "original_byte_count")?;

        Ok(Header {
            original_name,
            tree_bit_count,
            tree,
            data_bit_count,
            original_byte_count,
        })
    }

    /// Total size in bytes of the serialized header (signature + all
    /// fixed-width fields + variable-length name and tree), used to find
    /// where the encoded payload begins.
    fn byte_len(&self) -> usize {
        SIGNATURE.len()
            + U64_SIZE
            + self.original_name.len()
            + U64_SIZE
            + self.tree.len()
            + U64_SIZE
            + U64_SIZE
    }
}

/// Reads a little-endian `u64` off the front of `cursor`, advancing it past
/// the bytes consumed. Centralizing this avoids repeating the same
/// bounds-check-and-parse logic for every header field.
fn read_u64(cursor: &mut &[u8], field_name: &str) -> Result<u64, String> {
    if cursor.len() < U64_SIZE {
        return Err(format!("file is too short to read '{}'", field_name));
    }
    let (value_bytes, rest) = cursor.split_at(U64_SIZE);
    let value = u64::from_le_bytes(
        value_bytes
            .try_into()
            .map_err(|_| format!("could not parse '{}'", field_name))?,
    );
    *cursor = rest;
    Ok(value)
}

struct BitWriter {
    bytes: Vec<u8>,
    current_byte: u8,
    bit_count: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self { bytes: Vec::new(), current_byte: 0, bit_count: 0 }
    }

    fn write_bit(&mut self, bit: u8) {
        self.current_byte = (self.current_byte << 1) | (bit & 1);
        self.bit_count += 1;

        if self.bit_count == 8 {
            self.bytes.push(self.current_byte);
            self.current_byte = 0;
            self.bit_count = 0;
        }
    }

    fn write_byte(&mut self, byte: u8) {
        for i in (0..8).rev() {
            self.write_bit((byte >> i) & 1);
        }
    }

    /// Flushes any partial trailing byte (padded with zero bits) and
    /// returns the packed bytes along with the exact bit count written.
    fn flush(mut self) -> (Vec<u8>, usize) {
        let mut total_bits = self.bytes.len() * 8;

        if self.bit_count > 0 {
            total_bits += self.bit_count as usize;
            self.current_byte <<= 8 - self.bit_count;
            self.bytes.push(self.current_byte);
        }

        (self.bytes, total_bits)
    }
}

fn serialize_node(node: &Node, writer: &mut BitWriter) {
    match node {
        Node::Internal(left, right) => {
            writer.write_bit(0);
            serialize_node(&left.node, writer);
            serialize_node(&right.node, writer);
        }
        Node::Leaf(val) => {
            writer.write_bit(1);
            writer.write_byte(*val);
        }
    }
}

struct BitReader {
    bytes: Vec<u8>,
    byte_index: usize,
    bit_index: u8,
}

impl BitReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, byte_index: 0, bit_index: 0 }
    }

    fn read_bit(&mut self) -> Option<u8> {
        if self.byte_index >= self.bytes.len() {
            return None;
        }
        let byte = self.bytes[self.byte_index];
        let bit = (byte >> (7 - self.bit_index)) & 1;

        self.bit_index += 1;
        if self.bit_index == 8 {
            self.bit_index = 0;
            self.byte_index += 1;
        }
        Some(bit)
    }

    fn read_byte(&mut self) -> Option<u8> {
        let mut byte = 0u8;
        for _ in 0..8 {
            let bit = self.read_bit()?;
            byte = (byte << 1) | bit;
        }
        Some(byte)
    }
}

/// Rebuilds a `Node` tree from its serialized bit stream. The `frequency`
/// fields on intermediate `HuffmanNode`s are irrelevant here (the tree
/// shape carries everything needed for decoding) so they're left at 0.
fn deserialize_node(reader: &mut BitReader) -> Option<Node> {
    let bit = reader.read_bit()?;

    if bit == 0 {
        let left = deserialize_node(reader)?;
        let right = deserialize_node(reader)?;

        Some(Node::Internal(
            Box::new(HuffmanNode { node: left, frequency: 0 }),
            Box::new(HuffmanNode { node: right, frequency: 0 }),
        ))
    } else {
        let val = reader.read_byte()?;
        Some(Node::Leaf(val))
    }
}

pub fn encode_file(input_path: &str) -> io::Result<()> {
    let file = read_file(input_path)?;

    let frequencies = compute_frequencies(&file.data);
    let root = build_tree(&frequencies)
        .ok_or_else(|| invalid_data("the input file is empty — nothing to encode"))?;

    let mut table = HashMap::new();
    build_table(&root.node, 0, 0, &mut table);

    let (encoded_data, data_bit_count) = encode(&file.data, &table);

    let mut tree_writer = BitWriter::new();
    serialize_node(&root.node, &mut tree_writer);
    let (tree_bytes, tree_bit_count) = tree_writer.flush();

    let original_name = if file.extension.is_empty() {
        file.name.clone()
    } else {
        format!("{}.{}", file.name, file.extension)
    };

    let header = Header {
        original_name,
        tree_bit_count: tree_bit_count as u64,
        tree: tree_bytes,
        data_bit_count: data_bit_count as u64,
        original_byte_count: file.data.len() as u64,
    };

    let mut output = header.serialize();
    output.extend_from_slice(&encoded_data);

    let output_path = Path::new(input_path)
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{}.huff", file.name));

    let mut out_file = File::create(&output_path)?;
    out_file.write_all(&output)?;

    let compression = (output.len() as f32) / (file.data.len() as f32) * 100.0;
    println!(
        "Encoded: {} -> {} ({} bytes -> {} bytes, {:.1}%)",
        input_path,
        output_path.display(),
        file.data.len(),
        output.len(),
        compression
    );

    Ok(())
}

pub fn decode_file(input_path: &str) -> io::Result<()> {
    let raw = fs::read(input_path)
        .map_err(|e| io::Error::new(e.kind(), format!("could not open '{}': {}", input_path, e)))?;

    let header = Header::deserialize(&raw).map_err(invalid_data)?;

    let mut tree_reader = BitReader::new(header.tree.clone());
    let root_node = deserialize_node(&mut tree_reader)
        .ok_or_else(|| invalid_data("corrupt Huffman tree in header"))?;

    // For the single-distinct-byte edge case, `decode` reads the repeat
    // count straight off `frequency` instead of walking a (nonexistent)
    // path through internal nodes — see huffman::decode.
    let root = HuffmanNode { node: root_node, frequency: header.original_byte_count as usize };

    let header_size = header.byte_len();
    if raw.len() < header_size {
        return Err(invalid_data("file is too short — missing payload"));
    }
    let payload = &raw[header_size..];

    let decoded = decode(payload, header.data_bit_count as usize, &root);

    if decoded.len() as u64 != header.original_byte_count {
        return Err(invalid_data(format!(
            "decoded size mismatch: expected {} bytes, got {} — the archive may be corrupt",
            header.original_byte_count,
            decoded.len()
        )));
    }

    let input_dir = Path::new(input_path).parent().unwrap_or(Path::new("."));

    // Only take the file name component of whatever was stored in the
    // header, so a crafted archive can't write outside `input_dir` via a
    // path like "../../etc/passwd".
    let safe_name = Path::new(&header.original_name)
        .file_name()
        .ok_or_else(|| invalid_data("archive header contains an invalid output file name"))?;
    let output_path = input_dir.join(safe_name);

    let mut out_file = File::create(&output_path)?;
    out_file.write_all(&decoded)?;

    println!("Decoded: {} -> {} ({} bytes)", input_path, output_path.display(), decoded.len());

    Ok(())
}
