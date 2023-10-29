#![no_std]

use heapless::Vec;

#[derive(Eq, PartialEq, Debug)]
pub enum Error {
    UnfinishedChunk,
    InvalidImageHeaderLength,
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
                        Ok((n, Some(Event::BeginChunk(header))))
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

        #[test]
        fn partial_chunk_header() {
            let mut d = Dechunker::new();
            let mut data: &[u8] = &[
                0, 0, 0, 5, // len
                b'I', b'D', b'A', b'T', // type
            ];

            let (n, event) = d.update(&data[..5]).unwrap();
            assert_eq!(event, None);
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(
                event,
                Some(Event::BeginChunk(ChunkHeader {
                    len: 5,
                    type_: *b"IDAT"
                }))
            );
            data = &data[n..];

            assert_eq!(data, b"");
        }

        #[test]
        fn partial_data() {
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

            let (n, event) = d.update(&data[..3]).unwrap();
            assert_eq!(event, Some(Event::Data(b"hel")));
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, Some(Event::Data(b"lo")));
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, Some(Event::EndChunk));
            data = &data[n..];

            assert_eq!(data, b"");
            d.eof().unwrap();
        }

        #[test]
        #[ignore = "test not implemented"]
        fn test_unfinished_chunk() {
            todo!()
        }
    }
}

pub mod stream_decoder {
    use crate::dechunker::ChunkHeader;
    use crate::dechunker::ChunkType;

    use super::*;

    pub struct StreamDecoder {
        state: State,
    }

    #[derive(Clone, PartialEq, Eq, Debug)]
    enum State {
        BeforeChunk,
        IHDR(Vec<u8, { ImageHeader::SIZE }>),
    }

    impl State {
        fn initial() -> Self {
            Self::BeforeChunk
        }
    }

    /// <https://www.w3.org/TR/png-3/#11IHDR>
    #[derive(Eq, PartialEq, Debug)]
    pub struct ImageHeader {
        width: u32,
        height: u32,
        bit_depth: u8,
        colour_type: u8,
        compression_method: u8,
        filter_method: u8,
        interlace_method: u8,
    }

    impl ImageHeader {
        const TYPE: ChunkType = *b"IHDR";
        const SIZE: usize = 13;
    }

    #[derive(Eq, PartialEq, Debug)]
    pub enum Event<'a> {
        ImageHeader(ImageHeader),
        ImageData(&'a [u8]),
    }

    impl StreamDecoder {
        pub fn new() -> Self {
            Self {
                state: State::initial(),
            }
        }

        pub fn eof(&self) -> Result<(), Error> {
            // TODO: we should check if we got IEND
            Ok(())
        }

        pub fn update<'a>(
            &mut self,
            input: dechunker::Event<'a>,
        ) -> Result<(Option<dechunker::Event<'a>>, Option<Event<'a>>), Error> {
            match &mut self.state {
                State::BeforeChunk => match input {
                    dechunker::Event::BeginChunk(ChunkHeader {
                        len,
                        type_: ImageHeader::TYPE,
                    }) => {
                        if len as usize != ImageHeader::SIZE {
                            return Err(Error::InvalidImageHeaderLength);
                        }
                        self.state = State::IHDR(Vec::new());
                        Ok((None, None))
                    }
                    _ => panic!("Illegal event in BeforeChunk state"),
                },

                State::IHDR(buf) => match input {
                    dechunker::Event::Data(input) => {
                        if buf.extend_from_slice(input).is_err() {
                            panic!("Too much data in IHDR chunk");
                        }
                        Ok((None, None))
                    }

                    dechunker::Event::EndChunk => {
                        assert!(
                            buf.is_full(),
                            "Got IHDR EndChunk, but buffer is not filled!"
                        );
                        let header = ImageHeader {
                            width: u32::from_be_bytes(buf[0..4].try_into().unwrap()),
                            height: u32::from_be_bytes(buf[4..8].try_into().unwrap()),
                            bit_depth: buf[8],
                            colour_type: buf[9],
                            compression_method: buf[10],
                            filter_method: buf[11],
                            interlace_method: buf[12],
                        };
                        Ok((None, Some(Event::ImageHeader(header))))
                    }
                    dechunker::Event::BeginChunk(_) => {
                        panic!("Illegal BeginChunk inside of existing chunk")
                    }
                },
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn decode_simple_ihdr() {
            let mut d = StreamDecoder::new();

            assert_eq!(
                d.update(dechunker::Event::BeginChunk(ChunkHeader {
                    len: 13,
                    type_: *b"IHDR"
                }))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::Data(&[
                    0, 0, 0, 1, // width
                    0, 0, 0, 2, // height
                    3, 4, 5, 6, 7
                ]))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::EndChunk).unwrap(),
                (
                    None,
                    Some(Event::ImageHeader(ImageHeader {
                        width: 1,
                        height: 2,
                        bit_depth: 3,
                        colour_type: 4,
                        compression_method: 5,
                        filter_method: 6,
                        interlace_method: 7,
                    }))
                )
            );

            // Hmmm, should we assert that? Which layer checks if we had IEND?
            d.eof().unwrap();
        }

        #[test]
        fn decode_partial_ihdr() {
            let mut d = StreamDecoder::new();

            assert_eq!(
                d.update(dechunker::Event::BeginChunk(ChunkHeader {
                    len: 13,
                    type_: *b"IHDR"
                }))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::Data(&[
                    0, 0, 0, 1, // width
                    0, 0, 0,
                ]))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::Data(&[
                    2, // height
                    3, 4, 5, 6, 7
                ]))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::EndChunk).unwrap(),
                (
                    None,
                    Some(Event::ImageHeader(ImageHeader {
                        width: 1,
                        height: 2,
                        bit_depth: 3,
                        colour_type: 4,
                        compression_method: 5,
                        filter_method: 6,
                        interlace_method: 7,
                    }))
                )
            );

            // Hmmm, should we assert that? Which layer checks if we had IEND?
            d.eof().unwrap();
        }
    }
}
