#![no_std]

use heapless::Vec;

#[derive(Eq, PartialEq, Debug)]
pub enum Error {
    UnfinishedChunk,
    InvalidImageHeaderLength,
    NoImageHeader,
    InvalidDeflateStream,
    ChecksumMismatch,
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
        IDAT,
        IgnoredChunk,
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
        const SIZE: usize = 13;
    }

    const IHDR: ChunkType = *b"IHDR";
    const IDAT: ChunkType = *b"IDAT";
    const IEND: ChunkType = *b"IEND";

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
                    dechunker::Event::BeginChunk(ChunkHeader { len, type_: IHDR }) => {
                        if len as usize != ImageHeader::SIZE {
                            return Err(Error::InvalidImageHeaderLength);
                        }
                        self.state = State::IHDR(Vec::new());
                        Ok((None, None))
                    }
                    dechunker::Event::BeginChunk(ChunkHeader { type_: IDAT, .. }) => {
                        // TODO: check if we got header already?
                        self.state = State::IDAT;
                        Ok((None, None))
                    }
                    dechunker::Event::BeginChunk(ChunkHeader { type_: IEND, .. }) => {
                        todo!()
                    }
                    dechunker::Event::BeginChunk(ChunkHeader { .. }) => {
                        self.state = State::IgnoredChunk;
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

                State::IDAT => match input {
                    dechunker::Event::Data(input) => Ok((None, Some(Event::ImageData(input)))),
                    dechunker::Event::EndChunk => {
                        self.state = State::initial();
                        Ok((None, None))
                    }
                    _ => panic!("Illegal event inside IDAT chunk"),
                },

                State::IgnoredChunk => match input {
                    dechunker::Event::Data(_) => Ok((None, None)),
                    dechunker::Event::EndChunk => {
                        self.state = State::initial();
                        Ok((None, None))
                    }
                    _ => panic!("Illegal event inside ignored chunk"),
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

        #[test]
        fn decode_simple_idat() {
            let mut d = StreamDecoder::new();

            assert_eq!(
                d.update(dechunker::Event::BeginChunk(ChunkHeader {
                    len: 5,
                    type_: *b"IDAT"
                }))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::Data(b"hello")).unwrap(),
                (None, Some(Event::ImageData(b"hello")))
            );

            assert_eq!(d.update(dechunker::Event::EndChunk).unwrap(), (None, None,));

            // Hmmm, should we assert that? Which layer checks if we had IEND?
            d.eof().unwrap();
        }

        #[test]
        fn ignored_chunk() {
            let mut d = StreamDecoder::new();

            assert_eq!(
                d.update(dechunker::Event::BeginChunk(ChunkHeader {
                    len: 5,
                    type_: *b"tEXt"
                }))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::Data(b"hello")).unwrap(),
                (None, None)
            );

            assert_eq!(d.update(dechunker::Event::EndChunk).unwrap(), (None, None,));

            // Hmmm, should we assert that? Which layer checks if we had IEND?
            d.eof().unwrap();
        }
    }
}

pub mod inflater {
    use super::stream_decoder as sd;
    use super::*;
    use crate::stream_decoder::ImageHeader;
    use miniz_oxide::inflate::stream::InflateState;

    pub struct Inflater<const BUFFER_SIZE: usize = 1024> {
        decompressor: InflateState,
        output_buf: [u8; BUFFER_SIZE],
    }

    #[derive(Eq, PartialEq, Debug)]
    pub enum Event<'a> {
        /// Passthrough ImageHeader
        ImageHeader(ImageHeader),
        ImageData(&'a [u8]),
    }

    impl<const BUFFER_SIZE: usize> Inflater<BUFFER_SIZE> {
        pub fn new() -> Self {
            Self {
                decompressor: InflateState::new(miniz_oxide::DataFormat::Zlib),
                output_buf: [0; BUFFER_SIZE],
            }
        }
        pub fn update<'this, 'a>(
            &'this mut self,
            input: sd::Event<'a>,
        ) -> Result<(Option<sd::Event<'a>>, Option<Event<'this>>), Error> {
            match input {
                sd::Event::ImageHeader(header) => Ok((None, Some(Event::ImageHeader(header)))),
                sd::Event::ImageData(input) => {
                    let result = miniz_oxide::inflate::stream::inflate(
                        &mut self.decompressor,
                        input,
                        &mut self.output_buf,
                        miniz_oxide::MZFlush::None,
                    );

                    match result.status {
                        Ok(_) => {}
                        Err(e) => match e {
                            miniz_oxide::MZError::ErrNo => panic!("shouldn't happen"),
                            miniz_oxide::MZError::Stream => {
                                return Err(Error::InvalidDeflateStream)
                            }
                            miniz_oxide::MZError::Data => return Err(Error::InvalidDeflateStream),
                            miniz_oxide::MZError::Mem => panic!("shouldn't happen"),
                            miniz_oxide::MZError::Buf => panic!("buffer error"),
                            miniz_oxide::MZError::Version => panic!("shouldn't happen"),
                            miniz_oxide::MZError::Param => panic!("shouldn't happen"),
                        },
                    }

                    let leftover_input = if result.bytes_consumed < input.len() {
                        Some(sd::Event::ImageData(&input[result.bytes_consumed..]))
                    } else {
                        None
                    };

                    Ok((
                        leftover_input,
                        Some(Event::ImageData(&self.output_buf[..result.bytes_written])),
                    ))
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::sd;
        use super::*;
        use crate::inflater::Inflater;

        #[test]
        fn decode_simple_compressed_stream() {
            let mut d = Inflater::<1024>::new();

            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(b"hello", 5);

            assert_eq!(
                d.update(sd::Event::ImageData(&compressed)).unwrap(),
                (None, Some(Event::ImageData(b"hello")))
            );

            // TODO: check if we are at the end?
        }

        #[test]
        fn decode_inflated_output() {
            const N: usize = 2048;

            let mut d = Inflater::<1024>::new();

            let input = [b'A'; N];
            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&input, 5);

            let mut output = Vec::<u8, N>::new();

            let mut event = Some(sd::Event::ImageData(&compressed));
            while let Some(e) = event {
                let (leftover, output_event) = d.update(e).unwrap();
                match output_event {
                    Some(Event::ImageData(data)) => output.extend_from_slice(data).unwrap(),
                    None => {}
                    _ => panic!("expected only ImageData output"),
                }
                event = leftover;
            }

            assert_eq!(input.len(), output.len());
            for c in output {
                assert_eq!(c, b'A');
            }
        }
    }
}
