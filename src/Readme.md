# huff

A small command-line file compressor built from scratch in Rust, implementing canonical [Huffman coding](https://en.wikipedia.org/wiki/Huffman_coding).

```
$ huff encode report.txt
Encoded: report.txt -> report.huff (184320 bytes -> 98214 bytes, 53.3%)

$ huff decode report.huff
Decoded: report.huff -> report.txt (184320 bytes)
```

## How it works

1. **Frequency analysis** — count how often each byte (0–255) appears in the input.
2. **Tree construction** — repeatedly merge the two least-frequent nodes (via a binary min-heap) until a single Huffman tree remains, so frequent bytes end up with shorter codes.
3. **Code table** — walk the tree to assign each byte a variable-length bit code.
4. **Bit packing** — replace every byte in the input with its code and pack the bits tightly into bytes.
5. **Archive format** — write a small header (original file name, the serialized tree, and bit counts) followed by the packed data, so the file can be fully reconstructed on decode.

The encoder and decoder are split into two layers:

- [`src/huffman.rs`](src/huffman.rs) — the algorithm itself: frequency counting, tree building, code table generation, and bit-level encode/decode. Pure logic, no file I/O.
- [`src/io.rs`](src/io.rs) — reads files from disk, drives the algorithm, and defines the `.huff` archive format (header serialization, bit-level reader/writer, tree (de)serialization).

## Archive format

```
┌─────────────┬──────────────────┬───────────────────┬──────────────┬───────────────────┬──────────────────────┬──────────────┐
│ "HUFFMAN"   │ name length (u64)│ original file name │ tree bit len │ serialized tree    │ data bit len / count  │ packed data  │
│  7 bytes    │   8 bytes        │   variable          │  (u64)       │   variable          │   (2 × u64)           │   variable   │
└─────────────┴──────────────────┴───────────────────┴──────────────┴───────────────────┴──────────────────────┴──────────────┘
```

All length and count fields are stored as fixed-width, little-endian `u64`s (rather than the host's native `usize`), so an archive produced on one machine decodes correctly on another regardless of platform word size.

## Usage

```
huff encode <FILE>     # compress FILE into FILE_STEM.huff
huff decode <FILE.huff> # restore the original file alongside the archive
```

Run `huff --help` for the full command reference.

## Building

Requires a recent stable [Rust toolchain](https://www.rust-lang.org/tools/install).

```
cargo build --release
./target/release/huff encode some-file.txt
```

## Testing

```
cargo test
```

Covers frequency counting, tree/table construction, and encode/decode round-trips — including the edge case where a file contains only one distinct byte value (a single-leaf tree, which needs special handling since there's no bit pattern to encode).

## Design notes & known limitations

- Best suited for text and other byte-skewed data; already-compressed or random binary data will often grow slightly, since Huffman coding can't beat the Shannon entropy of a near-uniform byte distribution, and the header adds a small fixed overhead.
- The Huffman tree is rebuilt and stored per file rather than using a shared/adaptive model, which keeps the format simple at the cost of some overhead on very small files.
- Tree deserialization recurses without an explicit depth bound. This is safe for trees produced by this tool (depth is capped by the 256 possible byte values), but a deliberately corrupted archive could in principle trigger deep recursion — not a concern for normal use, but worth knowing if you extend this into something that parses untrusted archives.

## License

MIT — see [LICENSE](LICENSE).
