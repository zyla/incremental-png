#![no_std]

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

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum State {
        ChunkHeader,
        InChunk { remaining: usize },
        CRC,
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

    pub struct AdvanceToken(State);

    impl Dechunker {
        pub fn new() -> Self {
            Self {
                state: State::ChunkHeader,
            }
        }

        pub fn eof(&self) -> Result<(), Error> {
            if self.state != State::ChunkHeader {
                return Err(Error::UnfinishedChunk);
            }
            Ok(())
        }

        pub fn update<'a>(
            &self,
            input: &'a [u8],
        ) -> Result<(usize, AdvanceToken, Option<Event<'a>>), Error> {
            match self.state {
                State::ChunkHeader => {
                    if input.len() < CHUNK_HEADER_SIZE {
                        return Ok((0, AdvanceToken(self.state), None));
                    }
                    let header = ChunkHeader {
                        len: u32::from_be_bytes(input[0..4].try_into().unwrap()),
                        type_: input[4..8].try_into().unwrap(),
                    };
                    Ok((
                        CHUNK_HEADER_SIZE,
                        AdvanceToken(State::InChunk {
                            remaining: header.len as usize,
                        }),
                        Some(Event::BeginChunk(header)),
                    ))
                }
                State::InChunk { remaining } => {
                    let n = core::cmp::min(input.len(), remaining);
                    let next = if remaining == n {
                        State::CRC
                    } else {
                        State::InChunk {
                            remaining: remaining - n,
                        }
                    };
                    Ok((n, AdvanceToken(next), Some(Event::Data(&input[..n]))))
                }
                State::CRC => {
                    if input.len() < CRC_SIZE {
                        return Ok((0, AdvanceToken(self.state), None));
                    }
                    // Ignoring CRC for now
                    Ok((
                        CRC_SIZE,
                        AdvanceToken(State::ChunkHeader),
                        Some(Event::EndChunk),
                    ))
                }
            }
        }

        pub fn advance(&mut self, token: AdvanceToken) {
            self.state = token.0;
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

            let (n, token, event) = d.update(data).unwrap();
            assert_eq!(
                event,
                Some(Event::BeginChunk(ChunkHeader {
                    len: 5,
                    type_: *b"IDAT"
                }))
            );
            data = &data[n..];
            d.advance(token);

            let (n, token, event) = d.update(data).unwrap();
            assert_eq!(event, Some(Event::Data(b"hello")));
            data = &data[n..];
            d.advance(token);

            let (n, token, event) = d.update(data).unwrap();
            assert_eq!(event, Some(Event::EndChunk));
            data = &data[n..];
            d.advance(token);

            assert_eq!(data, b"");
            d.eof().unwrap();
        }
    }
}
