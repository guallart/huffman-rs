use clap::{Parser, Subcommand};

mod huffman;
mod io;

#[derive(Parser)]
#[command(name = "huff", about = "A file compressor based on Huffman coding", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compress a file into a .huff archive
    Encode {
        /// Path to the file to compress
        file: String,
    },
    /// Decompress a .huff archive back to its original file
    Decode {
        /// Path to the .huff file to decompress
        file: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Encode { file } => io::encode_file(&file),
        Command::Decode { file } => io::decode_file(&file),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
