use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::collections::HashMap;
use crate::huffman::{compute_frequencies, build_tree, build_table, encode, decode};
use crate::huffman::{Node, HuffmanNode};

const SIGNATURE: &[u8; 7] = b"HUFFMAN";

struct FileData {
    pub name: String,
    pub extension: String,
    pub data: Vec<u8>,
}

fn read_file(file_path: &str) -> FileData {
    let data = fs::read(file_path)
        .expect(&format!("Could not open the file {}", file_path));

    let path = Path::new(file_path);

    let name = path.file_stem()
        .and_then(|os_str| os_str.to_str())
        .expect(&format!("Invalid file name {}", file_path))
        .to_string();

    let extension = path.extension()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    FileData {
        name,
        extension,
        data,
    }
}

struct Header {
    original_name_length: usize,
    original_name: String,
    tree_bit_count: usize,
    tree: Vec<u8>,
    data_bit_count: usize,
    original_byte_count: usize,
}

impl Header {
    fn serialize(self) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();

        bytes.extend_from_slice(&self.original_name_length.to_le_bytes());
        bytes.extend_from_slice(self.original_name.as_bytes());

        bytes.extend_from_slice(&self.tree_bit_count.to_le_bytes());
        bytes.extend_from_slice(&self.tree);

        bytes.extend_from_slice(&self.data_bit_count.to_le_bytes());
        bytes.extend_from_slice(&self.original_byte_count.to_le_bytes());

        bytes
    }

    fn deserialize(bytes: &[u8]) -> Result<Header, String> {
        let usize_size = std::mem::size_of::<usize>();
        let mut cursor = bytes;

        // Signature
        if cursor.len() < SIGNATURE.len() {
            return Err("File is not big enough to contain the signature.".to_string());
        }
        let (signature_bytes, rest) = cursor.split_at(SIGNATURE.len());
        if signature_bytes != SIGNATURE {
            return Err("Invalid file signature.".to_string());
        }
        cursor = rest;

        // Original name length
        if cursor.len() < usize_size {
            return Err("File is not big enough to read original_name_length.".to_string());
        }
        let (len_bytes, rest) = cursor.split_at(usize_size);
        let original_name_length = usize::from_le_bytes(
            len_bytes.try_into().map_err(|_| "Error parsing original_name_length.")?
        );
        cursor = rest;

        // Original name
        if cursor.len() < original_name_length {
            return Err("File is not big enough to read original_name.".to_string());
        }
        let (name_bytes, rest) = cursor.split_at(original_name_length);
        let original_name = String::from_utf8(name_bytes.to_vec())
            .map_err(|e| format!("Invalid UTF-8 in file name: {}", e))?;
        cursor = rest;

        // Tree bit count
        if cursor.len() < usize_size {
            return Err("File is not big enough to read tree_bit_count.".to_string());
        }
        let (tree_bits_bytes, rest) = cursor.split_at(usize_size);
        let tree_bit_count = usize::from_le_bytes(
            tree_bits_bytes.try_into().map_err(|_| "Error parsing tree_bit_count.")?
        );
        cursor = rest;

        // Tree bytes
        let tree_bytes_len = (tree_bit_count + 7) / 8;
        if cursor.len() < tree_bytes_len {
            return Err("File is not big enough to read tree bytes.".to_string());
        }
        let (tree_bytes, rest) = cursor.split_at(tree_bytes_len);
        let tree = tree_bytes.to_vec();
        cursor = rest;

        // Data bit count
        if cursor.len() < usize_size {
            return Err("File is not big enough to read data_bit_count.".to_string());
        }
        let (data_bits_bytes, rest) = cursor.split_at(usize_size);
        let data_bit_count = usize::from_le_bytes(
            data_bits_bytes.try_into().map_err(|_| "Error parsing data_bit_count.")?
        );
        cursor = rest;

        // Original byte count
        if cursor.len() < usize_size {
            return Err("File is not big enough to read original_byte_count.".to_string());
        }
        let (orig_bytes, _rest) = cursor.split_at(usize_size);
        let original_byte_count = usize::from_le_bytes(
            orig_bytes.try_into().map_err(|_| "Error parsing original_byte_count.")?
        );

        Ok(Header {
            original_name_length,
            original_name,
            tree_bit_count,
            tree,
            data_bit_count,
            original_byte_count,
        })
    }
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
    let file = read_file(input_path);

    let frequencies = compute_frequencies(&file.data);
    let root = build_tree(&frequencies)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty file. Nothing to encode."))?;

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
        original_name_length: original_name.len(),
        original_name,
        tree_bit_count,
        tree: tree_bytes,
        data_bit_count,
        original_byte_count: file.data.len(),
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

    println!("Encoded: {} -> {} ({} bytes -> {} bytes, {:.1} %)",
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
        .map_err(|e| io::Error::new(e.kind(), format!("Cannot open {}: {}", input_path, e)))?;

    let header = Header::deserialize(&raw)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut tree_reader = BitReader::new(header.tree.clone());
    let root_node = deserialize_node(&mut tree_reader)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Corrupt Huffman tree in header."))?;

    let root = HuffmanNode { node: root_node, frequency: header.original_byte_count };

    let usize_size = std::mem::size_of::<usize>();
    let tree_bytes_len = (header.tree_bit_count + 7) / 8;
    let header_size = SIGNATURE.len()
        + usize_size                  // original_name_length
        + header.original_name_length // name bytes
        + usize_size                  // tree_bit_count
        + tree_bytes_len              // tree bytes
        + usize_size                  // data_bit_count
        + usize_size;                 // original_byte_count

    if raw.len() < header_size {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "File too short — missing payload."));
    }
    let payload = &raw[header_size..];

    let decoded = decode(payload, header.data_bit_count, &root);

    if decoded.len() != header.original_byte_count {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Decoded size mismatch: expected {} bytes, got {}.",
                header.original_byte_count,
                decoded.len()
            ),
        ));
    }

    let input_dir = Path::new(input_path)
        .parent()
        .unwrap_or(Path::new("."));
    let output_path = input_dir.join(&header.original_name);

    let mut out_file = File::create(&output_path)?;
    out_file.write_all(&decoded)?;

    println!("Decoded: {} -> {} ({} bytes)",
        input_path,
        output_path.display(),
        decoded.len(),
    );

    Ok(())
}
