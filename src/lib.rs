#![no_std]

use heapless::Vec;

#[derive(Eq, PartialEq, Debug)]
pub enum Error {
    InvalidPngSignature,
    UnfinishedChunk,
    InvalidImageHeaderLength,
    NoImageHeader,
    InvalidDeflateStream,
    ChecksumMismatch,
    InvalidEndChunkSize,
    InvalidPaletteChunkSize,
}

pub struct Palette {
    data: Vec<u8, { 256 * 3 }>,
}

impl Palette {
    pub fn color_at(&self, index: u8) -> [u8; 3] {
        if (index as usize + 1) * 3 > self.data.len() {
            [0; 3]
        } else {
            self.data[index as usize * 3..(index as usize + 1) * 3]
                .try_into()
                .unwrap()
        }
    }
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
        PngSignature { pos: usize },
        ChunkHeader(Vec<u8, CHUNK_HEADER_SIZE>),
        InChunk { remaining: usize },
        CRC(Vec<u8, CRC_SIZE>),
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

    /// <https://www.w3.org/TR/png-3/#5PNG-file-signature>
    const PNG_SIGNATURE: &[u8; 8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    impl Dechunker {
        pub fn new() -> Self {
            Self {
                state: State::PngSignature { pos: 0 },
            }
        }

        #[cfg(test)]
        fn new_without_png_signature() -> Self {
            Self {
                state: State::ChunkHeader(Vec::new()),
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
                State::PngSignature { pos } => {
                    let n = core::cmp::min(input.len(), PNG_SIGNATURE.len() - *pos);
                    if &input[..n] != &PNG_SIGNATURE[*pos..*pos + n] {
                        return Err(Error::InvalidPngSignature);
                    }
                    *pos += n;
                    if *pos == PNG_SIGNATURE.len() {
                        self.state = State::ChunkHeader(Vec::new());
                    }
                    Ok((n, None))
                }
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
                    Ok((
                        n,
                        if n == 0 {
                            None
                        } else {
                            Some(Event::Data(&input[..n]))
                        },
                    ))
                }
                State::CRC(buf) => {
                    let n = core::cmp::min(input.len(), buf.capacity() - buf.len());
                    buf.extend_from_slice(&input[..n]).unwrap();
                    if buf.is_full() {
                        // Ignoring CRC for now
                        self.state = State::ChunkHeader(Vec::new());
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
        fn png_signature_and_chunk_header() {
            let mut d = Dechunker::new();
            let mut data: &[u8] = &[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
                0, 0, 0, 13, // len
                b'I', b'H', b'D', b'R', // type
            ];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, None);
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(
                event,
                Some(Event::BeginChunk(ChunkHeader {
                    len: 13,
                    type_: *b"IHDR"
                }))
            );
            data = &data[n..];

            assert_eq!(data, b"");
        }

        #[test]
        fn partial_png_signature_and_chunk_header() {
            let mut d = Dechunker::new();
            let mut data: &[u8] = &[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
                0, 0, 0, 13, // len
                b'I', b'H', b'D', b'R', // type
            ];

            let (n, event) = d.update(&data[..3]).unwrap();
            assert_eq!(event, None);
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, None);
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(
                event,
                Some(Event::BeginChunk(ChunkHeader {
                    len: 13,
                    type_: *b"IHDR"
                }))
            );
            data = &data[n..];

            assert_eq!(data, b"");
        }

        #[test]
        fn invalid_png_signature() {
            let mut d = Dechunker::new();
            let data: &[u8] = b"thesignatureisinvalid";

            assert_eq!(d.update(&data[..3]), Err(Error::InvalidPngSignature));
        }

        #[test]
        fn decode_simple_chunk() {
            let mut d = Dechunker::new_without_png_signature();
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
        fn decode_empty_chunk() {
            let mut d = Dechunker::new_without_png_signature();
            let mut data: &[u8] = &[
                0, 0, 0, 0, // len
                b'I', b'E', b'N', b'D', // type
                0, 0, 0, 0, // crc (ignored)
            ];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(
                event,
                Some(Event::BeginChunk(ChunkHeader {
                    len: 0,
                    type_: *b"IEND"
                }))
            );
            data = &data[n..];

            // Note: no data event
            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, None);
            data = &data[n..];

            let (n, event) = d.update(data).unwrap();
            assert_eq!(event, Some(Event::EndChunk));
            data = &data[n..];

            assert_eq!(data, b"");
            d.eof().unwrap();
        }

        #[test]
        fn partial_chunk_header() {
            let mut d = Dechunker::new_without_png_signature();
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
            let mut d = Dechunker::new_without_png_signature();
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
        palette: Palette,
    }

    #[derive(Clone, PartialEq, Eq, Debug)]
    enum State {
        BeforeChunk,
        IHDR(Vec<u8, { ImageHeader::SIZE }>),
        PLTE,
        IDAT,
        IgnoredChunk,
        IEND,
    }

    impl State {
        fn initial() -> Self {
            Self::BeforeChunk
        }
    }

    /// <https://www.w3.org/TR/png-3/#11IHDR>
    #[derive(Eq, PartialEq, Debug)]
    pub struct ImageHeader {
        pub width: u32,
        pub height: u32,
        pub bit_depth: u8,
        pub colour_type: u8,
        pub compression_method: u8,
        pub filter_method: u8,
        pub interlace_method: u8,
    }

    impl ImageHeader {
        const SIZE: usize = 13;
    }

    const IHDR: ChunkType = *b"IHDR";
    const PLTE: ChunkType = *b"PLTE";
    const IDAT: ChunkType = *b"IDAT";
    const IEND: ChunkType = *b"IEND";

    #[derive(Eq, PartialEq, Debug)]
    pub enum Event<'a> {
        ImageHeader(ImageHeader),
        ImageData(&'a [u8]),
        End,
    }

    impl StreamDecoder {
        pub fn new() -> Self {
            Self {
                state: State::initial(),
                palette: Palette {
                    data: Default::default(),
                },
            }
        }

        pub fn palette(&self) -> &Palette {
            &self.palette
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
                    dechunker::Event::BeginChunk(ChunkHeader { type_: IEND, len }) => {
                        if len != 0 {
                            return Err(Error::InvalidEndChunkSize);
                        }
                        self.state = State::IEND;
                        Ok((None, None))
                    }
                    dechunker::Event::BeginChunk(ChunkHeader { type_: PLTE, len }) => {
                        if len % 3 != 0 || len > 256 * 3 {
                            return Err(Error::InvalidPaletteChunkSize);
                        }
                        self.state = State::PLTE;
                        Ok((None, None))
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
                        self.state = State::BeforeChunk;
                        Ok((None, Some(Event::ImageHeader(header))))
                    }
                    dechunker::Event::BeginChunk(_) => {
                        panic!("Illegal BeginChunk inside of existing chunk")
                    }
                },

                State::PLTE => match input {
                    dechunker::Event::Data(input) => {
                        if self.palette.data.extend_from_slice(input).is_err() {
                            panic!("Too much data in PLTE chunk");
                        }
                        Ok((None, None))
                    }
                    dechunker::Event::EndChunk => {
                        self.state = State::initial();
                        Ok((None, None))
                    }
                    _ => panic!("Illegal event inside PLTE chunk"),
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

                State::IEND => match input {
                    dechunker::Event::Data(_) => panic!("Data in IEND chunk"),
                    dechunker::Event::EndChunk => {
                        self.state = State::initial();
                        Ok((None, Some(Event::End)))
                    }
                    _ => panic!("Illegal event inside IEND chunk"),
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
        fn decode_simple_ihdr_and_next_chunk() {
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

            assert_eq!(
                d.update(dechunker::Event::BeginChunk(ChunkHeader {
                    len: 0,
                    type_: *b"IDAT"
                }))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::Data(&[])).unwrap(),
                (None, Some(Event::ImageData(&[])))
            );

            assert_eq!(d.update(dechunker::Event::EndChunk).unwrap(), (None, None));
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

        #[test]
        fn decode_iend() {
            let mut d = StreamDecoder::new();

            assert_eq!(
                d.update(dechunker::Event::BeginChunk(ChunkHeader {
                    len: 0,
                    type_: *b"IEND"
                }))
                .unwrap(),
                (None, None)
            );

            assert_eq!(
                d.update(dechunker::Event::EndChunk).unwrap(),
                (None, Some(Event::End))
            );

            // Hmmm, should we assert that? Which layer checks if we had IEND?
            d.eof().unwrap();
        }

        #[test]
        fn invalid_iend() {
            let mut d = StreamDecoder::new();

            assert_eq!(
                d.update(dechunker::Event::BeginChunk(ChunkHeader {
                    len: 42,
                    type_: *b"IEND"
                })),
                Err(Error::InvalidEndChunkSize)
            );
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
        /// Passthrough
        ImageHeader(ImageHeader),
        ImageData(&'a [u8]),
        /// Passthrough
        End,
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
                            miniz_oxide::MZError::Buf => {
                                if !input.is_empty() {
                                    panic!("buffer error, input len={}", input.len())
                                }
                                // Otherwise okay, it just wants more input
                            }
                            miniz_oxide::MZError::Version => panic!("shouldn't happen"),
                            miniz_oxide::MZError::Param => panic!("shouldn't happen"),
                        },
                    }

                    let leftover_input = if result.bytes_consumed < input.len() {
                        Some(sd::Event::ImageData(&input[result.bytes_consumed..]))
                    } else if result.bytes_written == self.output_buf.len() {
                        // If we filled the output buffer, we might possibly need more calls
                        Some(sd::Event::ImageData(&[]))
                    } else {
                        None
                    };

                    Ok((
                        leftover_input,
                        Some(Event::ImageData(&self.output_buf[..result.bytes_written])),
                    ))
                }
                sd::Event::End => Ok((None, Some(Event::End))),
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
        fn very_incremental() {
            let mut d = Inflater::<1024>::new();

            const INPUT: &[u8] = b"hello world";
            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(INPUT, 5);

            let mut output = Vec::<u8, { INPUT.len() }>::new();

            let mut current_input = compressed.as_slice();
            while !current_input.is_empty() {
                let n = core::cmp::min(2, current_input.len());
                let mut event = Some(sd::Event::ImageData(&current_input[..n]));
                current_input = &current_input[n..];
                while let Some(e) = event {
                    let (leftover, output_event) = d.update(e).unwrap();
                    match output_event {
                        Some(Event::ImageData(data)) => output.extend_from_slice(data).unwrap(),
                        None => {}
                        _ => panic!("expected only ImageData output"),
                    }
                    event = leftover;
                }
            }

            assert_eq!(&INPUT, &output);
        }

        #[test]
        fn decode_inflated_output() {
            const N: usize = 65536;

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
