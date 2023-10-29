use std::io::Read;
use std::{fs::File, path::PathBuf};

use clap::Parser;
use incremental_png::{dechunker::Dechunker, inflater::Inflater, stream_decoder::StreamDecoder};

#[derive(Parser, Debug)]
struct Args {
    #[arg()]
    input_file: PathBuf,

    #[arg(long, default_value_t = 1024)]
    input_buffer_size: usize,

    #[arg(long)]
    print_sizes: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut file = File::open(args.input_file)?;

    let mut dechunker = Dechunker::new();
    let mut sd = StreamDecoder::new();
    let mut inflater = Inflater::<1024>::new();

    if args.print_sizes {
        println!("Memory usage:");
        println!("  Dechunker: {}", std::mem::size_of::<Dechunker>());
        println!("  StreamDecoder: {}", std::mem::size_of::<StreamDecoder>());
        println!(
            "  Inflater (output buffer=1024): {}",
            std::mem::size_of::<Inflater<1024>>()
        );
    }

    let mut buf = vec![0u8; args.input_buffer_size];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let mut input = &buf[..n];

        while !input.is_empty() {
            let (consumed, mut dc_event) = dechunker.update(&input).unwrap();

            while let Some(e) = dc_event {
                println!("c: {:?}", e);

                let (leftover, mut sd_event) = sd.update(e).unwrap();

                while let Some(e) = sd_event {
                    println!(" s: {:?}", e);
                    let (leftover, i_event) = inflater.update(e).unwrap();

                    println!("  i: {:?}", i_event);

                    sd_event = leftover;
                }

                dc_event = leftover;
            }

            input = &input[consumed..];
        }
    }

    Ok(())
}
