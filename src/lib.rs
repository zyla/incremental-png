#![no_std]

use heapless::Vec;

#[derive(Debug)]
pub enum Error {
    UnfinishedChunk,
}

pub mod dechunker {
    use super::*;

    pub struct Dechunker {
        state: State,
    }

    const CHUNK_HEADER_SIZE: usize = 8;
    const CRC_SIZE: usize = 4;

    #[derive(Clone, PartialEq, Eq, Debug)]
    enum State {
        ChunkHeader(Vec<u8, CHUNK_HEADER_SIZE>),
        InChunk { remaining: usize },
        CRC(Vec<u8, CRC_SIZE>),
    }

    impl State {
        fn initial() -> Self {
            Self::ChunkHeader(Vec::new())
        }
    }

    #[derive(Eq, PartialEq, Debug)]
    pub struct ChunkHeader {
        pub len: u32,
        pub type_: ChunkType,
    }

    pub type ChunkType = [u8; 4];

    #[derive(Eq, PartialEq, Debug)]
    pub enum Event<'a> {
        BeginChunk(ChunkHeader),
        Data(&'a [u8]),
        EndChunk,
    }

    impl Dechunker {
        pub fn new() -> Self {
            Self {
                state: State::initial(),
            }
        }

        pub fn eof(&self) -> Result<(), Error> {
            match &self.state {
                State::ChunkHeader(header) if header.is_empty() => Ok(()),
                _ => Err(Error::UnfinishedChunk),
            }
        }

        pub fn update<'a>(&mut self, input: &'a [u8]) -> Result<(usize, Option<Event<'a>>), Error> {
            match &mut self.state {
                State::ChunkHeader(buf) => {
                    let n = core::cmp::min(input.len(), buf.capacity() - buf.len());
                    buf.extend_from_slice(&input[..n]).unwrap();
                    if buf.is_full() {
                        let header = ChunkHeader {
                            len: u32::from_be_bytes(buf[0..4].try_into().unwrap()),
                            type_: buf[4..8].try_into().unwrap(),
                        };
                        self.state = State::InChunk {
                            remaining: header.len as usize,
                        };
                        Ok((CHUNK_HEADER_SIZE, Some(Event::BeginChunk(header))))
                    } else {
                        Ok((n, None))
                    }
                }
                State::InChunk { remaining } => {
                    let n = core::cmp::min(input.len(), *remaining);
                    self.state = if *remaining == n {
                        State::CRC(Vec::new())
                    } else {
                        State::InChunk {
                            remaining: *remaining - n,
                        }
                    };
                    Ok((n, Some(Event::Data(&input[..n]))))
                }
                State::CRC(buf) => {
                    let n = core::cmp::min(input.len(), buf.capacity() - buf.len());
                    buf.extend_from_slice(&input[..n]).unwrap();
                    if buf.is_full() {
                        // Ignoring CRC for now
                        self.state = State::initial();
                        Ok((n, Some(Event::EndChunk)))
                    } else {
                        Ok((n, None))
                    }
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn decode_simple_chunk() {
            let mut d = Dechunker::new();
            let mut data: &[u8] = &[
                0, 0, 0, 5, // len
                b'I', b'D', b'A', b'T', // type
                b'h', b'e', b'l', b'l', b'o', // data
                0, 0, 0, 0, // crc (ignored)
            ];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(
                event,
                Some(Event::BeginChunk(ChunkHeader {
                    len: 5,
                    type_: *b"IDAT"
                }))
            );
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, Some(Event::Data(b"hello")));
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, Some(Event::EndChunk));
            data = &data[n..];

            assert_eq!(data, b"");
            d.eof().unwrap();
        }
    }
}
