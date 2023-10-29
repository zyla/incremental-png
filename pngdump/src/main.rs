use std::io::Read;
use std::{fs::File, path::PathBuf};

use clap::Parser;
use incremental_png::{dechunker::Dechunker, inflater::Inflater, stream_decoder::StreamDecoder};

#[derive(Parser, Debug)]
struct Args {
    #[arg()]
    input_file: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut file = File::open(args.input_file)?;

    let mut dechunker = Dechunker::new();
    let mut sd = StreamDecoder::new();
    let mut inflater = Inflater::<1024>::new();

    let mut buf = [0u8; 32];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let mut input = &buf[..n];

        while !input.is_empty() {
            let (consumed, mut dc_event) = dechunker.update(&input).unwrap();

            while let Some(e) = dc_event {
                println!("dc: {:?}", e);

                let (leftover, mut sd_event) = sd.update(e).unwrap();

                while let Some(e) = sd_event {
                    println!("sd: {:?}", e);
                    let (leftover, i_event) = inflater.update(e).unwrap();

                    println!("i: {:?}", i_event);

                    sd_event = leftover;
                }

                dc_event = leftover;
            }

            input = &input[consumed..];
        }
    }

    Ok(())
}
