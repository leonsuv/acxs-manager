//! Minimal PNG codec for 8-bit pixels.
//!
//! The encoder writes RGBA pixels. Its file is laid out as: signature, IHDR,
//! one IDAT holding a zlib stream, and IEND. The zlib payload uses stored
//! (uncompressed) deflate blocks, so no entropy coding is involved. Only the
//! framing, the Adler-32 checksum and the per-chunk CRC-32 values have to be
//! computed.
//!
//! The decoder reads greyscale, grey with alpha, RGB and RGBA files. It inflates
//! any valid deflate stream (stored, fixed and dynamic Huffman blocks), so it
//! accepts files written by other encoders, not only by [`encode_rgba`].

use std::fmt;

/// The fixed eight-byte header that starts every PNG file.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Largest payload a chunk may declare, since the length field is 31 bits.
const MAX_CHUNK_LEN: usize = 0x7FFF_FFFF;

/// Largest payload one stored deflate block can carry, since LEN is a u16.
const MAX_STORED_BLOCK: usize = 0xFFFF;

/// Largest width or height that [`decode_rgba`] accepts.
const MAX_DECODE_DIMENSION: u32 = 16_384;

/// Largest pixel count that [`decode_rgba`] accepts. This caps the RGBA output at 128 MiB.
const MAX_DECODE_PIXELS: u64 = 32 * 1024 * 1024;

/// Adler-32 modulus: the largest prime below 2^16.
const ADLER_MOD: u64 = 65_521;

/// Bytes summed between modulo reductions. Accumulators are u64, so this only
/// keeps the reduction frequency sensible.
const ADLER_CHUNK: usize = 5_552;

/// Errors returned by [`encode_rgba`] and [`decode_rgba`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngError {
    /// The width or the height was zero.
    ZeroDimension,
    /// The image is larger than the encoder or decoder supports.
    TooLarge,
    /// The pixel buffer length did not equal `width * height * 4`.
    BufferLength { expected: usize, actual: usize },
    /// The input does not start with the PNG signature.
    InvalidSignature,
    /// The input ends before a complete chunk or stream has been read.
    Truncated,
    /// A chunk CRC-32 or the zlib Adler-32 value does not match the data.
    Checksum,
    /// The file breaks a structural rule of the PNG format.
    Malformed(&'static str),
    /// The compressed data is not a valid deflate stream for this image.
    Deflate(&'static str),
    /// The file is valid PNG but uses a feature this decoder does not implement.
    Unsupported(&'static str),
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PngError::ZeroDimension => write!(f, "image width and height must be non-zero"),
            PngError::TooLarge => write!(f, "image is too large for the supported size limits"),
            PngError::BufferLength { expected, actual } => {
                write!(f, "expected {expected} RGBA bytes, got {actual}")
            }
            PngError::InvalidSignature => write!(f, "input is not a PNG file"),
            PngError::Truncated => write!(f, "PNG data ends unexpectedly"),
            PngError::Checksum => write!(f, "PNG checksum does not match the data"),
            PngError::Malformed(what) => write!(f, "malformed PNG: {what}"),
            PngError::Deflate(what) => write!(f, "invalid compressed data: {what}"),
            PngError::Unsupported(what) => write!(f, "unsupported PNG: {what}"),
        }
    }
}

impl std::error::Error for PngError {}

/// Encodes `rgba` as a PNG file.
///
/// `rgba` holds `width * height` pixels, four bytes each (red, green, blue,
/// alpha), row by row with the top row first.
pub fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, PngError> {
    if width == 0 || height == 0 {
        return Err(PngError::ZeroDimension);
    }
    let row_len = (width as usize).checked_mul(4).ok_or(PngError::TooLarge)?;
    let rows = height as usize;
    let expected = row_len.checked_mul(rows).ok_or(PngError::TooLarge)?;
    if rgba.len() != expected {
        return Err(PngError::BufferLength {
            expected,
            actual: rgba.len(),
        });
    }

    // Every scanline is preceded by a filter-type byte; 0 means "no filter".
    let raw_len = (row_len + 1).checked_mul(rows).ok_or(PngError::TooLarge)?;
    if zlib_stored_len(raw_len) > MAX_CHUNK_LEN {
        return Err(PngError::TooLarge);
    }

    let mut raw = Vec::with_capacity(raw_len);
    for row in rgba.chunks_exact(row_len) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let zlib = zlib_stored(&raw);

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth per channel
    ihdr[9] = 6; // colour type 6: truecolour with alpha
    // Bytes 10..13 hold compression method 0, filter method 0 and interlace 0.

    let mut out = Vec::with_capacity(SIGNATURE.len() + 12 + 13 + 12 + zlib.len() + 12);
    out.extend_from_slice(&SIGNATURE);
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &zlib);
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Decodes a PNG file into RGBA pixels, returning `(width, height, rgba)`.
///
/// Accepts 8-bit greyscale, grey with alpha, RGB and RGBA images that are not
/// interlaced. Greyscale and RGB pixels come back with full opacity. All five
/// scanline filter types are supported. Ancillary chunks such as `tRNS` are
/// ignored. The CRC-32 of every chunk and the Adler-32 of the zlib stream are
/// verified. Image size and decompressed size are capped, so a hostile file
/// cannot force an unbounded allocation.
pub fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), PngError> {
    if bytes.len() < SIGNATURE.len() {
        return Err(PngError::Truncated);
    }
    if bytes[..SIGNATURE.len()] != SIGNATURE {
        return Err(PngError::InvalidSignature);
    }

    let mut header = None;
    let mut idat = Vec::new();
    let mut idat_started = false;
    let mut idat_ended = false;
    let mut saw_iend = false;
    let mut pos = SIGNATURE.len();

    while pos < bytes.len() && !saw_iend {
        let head = bytes.get(pos..pos + 8).ok_or(PngError::Truncated)?;
        let len = be_u32(&head[0..4]) as usize;
        let kind = [head[4], head[5], head[6], head[7]];
        if len > MAX_CHUNK_LEN {
            return Err(PngError::Malformed("chunk length exceeds 31 bits"));
        }
        let body_end = (pos + 8).checked_add(len).ok_or(PngError::Truncated)?;
        let crc_end = body_end.checked_add(4).ok_or(PngError::Truncated)?;
        let body = bytes.get(pos + 8..body_end).ok_or(PngError::Truncated)?;
        let stored_crc = be_u32(bytes.get(body_end..crc_end).ok_or(PngError::Truncated)?);
        if chunk_crc(&kind, body) != stored_crc {
            return Err(PngError::Checksum);
        }
        pos = crc_end;

        if header.is_none() && &kind != b"IHDR" {
            return Err(PngError::Malformed("IHDR must be the first chunk"));
        }
        if idat_started && &kind != b"IDAT" {
            idat_ended = true;
        }
        match &kind {
            b"IHDR" => {
                if header.is_some() {
                    return Err(PngError::Malformed("duplicate IHDR chunk"));
                }
                header = Some(parse_ihdr(body)?);
            }
            b"IDAT" => {
                if idat_ended {
                    return Err(PngError::Malformed("IDAT chunks must be contiguous"));
                }
                idat_started = true;
                idat.extend_from_slice(body);
            }
            b"IEND" => saw_iend = true,
            // A palette does not change how greyscale or truecolour samples are read.
            b"PLTE" => {}
            // An uppercase first letter marks a critical chunk, which must be understood.
            _ if kind[0] & 0x20 == 0 => {
                return Err(PngError::Unsupported("unknown critical chunk"));
            }
            _ => {}
        }
    }

    if !saw_iend {
        return Err(PngError::Truncated);
    }
    let header = header.ok_or(PngError::Malformed("missing IHDR chunk"))?;
    if idat.is_empty() {
        return Err(PngError::Malformed("missing IDAT chunk"));
    }

    let row_len = header.width as usize * header.channels;
    let raw_len = (row_len + 1) * header.height as usize;
    let raw = inflate(&idat, raw_len)?;
    let rgba = unfilter(&raw, &header)?;
    Ok((header.width, header.height, rgba))
}

/// The fields of an IHDR chunk that the decoder needs.
struct Header {
    width: u32,
    height: u32,
    /// Samples per pixel in the stream: 1 grey, 2 grey and alpha, 3 RGB, 4 RGBA.
    channels: usize,
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn parse_ihdr(body: &[u8]) -> Result<Header, PngError> {
    if body.len() != 13 {
        return Err(PngError::Malformed("IHDR must be 13 bytes"));
    }
    let width = be_u32(&body[0..4]);
    let height = be_u32(&body[4..8]);
    if width == 0 || height == 0 {
        return Err(PngError::ZeroDimension);
    }
    if width > MAX_DECODE_DIMENSION
        || height > MAX_DECODE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_DECODE_PIXELS
    {
        return Err(PngError::TooLarge);
    }
    if body[8] != 8 {
        return Err(PngError::Unsupported("only 8-bit samples are supported"));
    }
    let channels = match body[9] {
        0 => 1,
        4 => 2,
        2 => 3,
        6 => 4,
        _ => {
            return Err(PngError::Unsupported(
                "colour type is not greyscale, grey with alpha, RGB or RGBA",
            ));
        }
    };
    if body[10] != 0 || body[11] != 0 {
        return Err(PngError::Malformed("unknown compression or filter method"));
    }
    if body[12] == 1 {
        return Err(PngError::Unsupported("interlaced images are not supported"));
    }
    if body[12] != 0 {
        return Err(PngError::Malformed("unknown interlace method"));
    }
    Ok(Header {
        width,
        height,
        channels,
    })
}

/// Reverses the scanline filters and expands every pixel to RGBA.
fn unfilter(raw: &[u8], header: &Header) -> Result<Vec<u8>, PngError> {
    let bpp = header.channels;
    let row_len = header.width as usize * bpp;
    let mut rgba = Vec::with_capacity(header.width as usize * header.height as usize * 4);
    let mut prev = vec![0u8; row_len];
    let mut cur = vec![0u8; row_len];
    for line in raw.chunks_exact(row_len + 1) {
        let filter = line[0];
        let data = &line[1..];
        for i in 0..row_len {
            let left = if i >= bpp { cur[i - bpp] } else { 0 };
            let up = prev[i];
            let up_left = if i >= bpp { prev[i - bpp] } else { 0 };
            let x = data[i];
            cur[i] = match filter {
                0 => x,
                1 => x.wrapping_add(left),
                2 => x.wrapping_add(up),
                3 => x.wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8),
                4 => x.wrapping_add(paeth(left, up, up_left)),
                _ => return Err(PngError::Malformed("unknown filter type")),
            };
        }
        for px in cur.chunks_exact(bpp) {
            match bpp {
                1 => rgba.extend_from_slice(&[px[0], px[0], px[0], 255]),
                2 => rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
                3 => rgba.extend_from_slice(&[px[0], px[1], px[2], 255]),
                _ => rgba.extend_from_slice(px),
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Ok(rgba)
}

/// The Paeth predictor from the PNG specification.
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let (a, b, c) = (i16::from(left), i16::from(up), i16::from(up_left));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// Base match lengths for length codes 257..=285 (RFC 1951 section 3.2.5).
const LENGTH_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Base distances for distance codes 0..=29.
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Order in which the code-length code lengths appear in a dynamic block header.
const CODE_LENGTH_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Reads deflate bit fields, least significant bit first.
struct BitReader<'a> {
    src: &'a [u8],
    pos: usize,
    buf: u64,
    count: u32,
}

impl<'a> BitReader<'a> {
    fn new(src: &'a [u8]) -> Self {
        BitReader {
            src,
            pos: 0,
            buf: 0,
            count: 0,
        }
    }

    /// Loads input bytes until the buffer holds more than 56 bits or the input ends.
    fn fill(&mut self) {
        while self.count <= 56 {
            let Some(&byte) = self.src.get(self.pos) else {
                break;
            };
            self.buf |= u64::from(byte) << self.count;
            self.pos += 1;
            self.count += 8;
        }
    }

    /// Reads an `n`-bit field, where `n` is at most 16.
    fn bits(&mut self, n: u32) -> Result<u32, PngError> {
        if self.count < n {
            self.fill();
            if self.count < n {
                return Err(PngError::Truncated);
            }
        }
        let value = (self.buf & ((1u64 << n) - 1)) as u32;
        self.buf >>= n;
        self.count -= n;
        Ok(value)
    }

    /// Discards bits up to the next byte boundary.
    fn align(&mut self) {
        let spare = self.count % 8;
        self.buf >>= spare;
        self.count -= spare;
    }

    /// Returns the next `len` bytes. The reader must be byte-aligned.
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], PngError> {
        // Whole bytes still held in the bit buffer were read ahead of the true
        // position, so step back over them before slicing.
        self.pos -= (self.count / 8) as usize;
        self.buf = 0;
        self.count = 0;
        let src = self.src;
        let end = self.pos.checked_add(len).ok_or(PngError::Truncated)?;
        let out = src.get(self.pos..end).ok_or(PngError::Truncated)?;
        self.pos = end;
        Ok(out)
    }
}

/// Canonical Huffman decoding table with one entry per `bits`-bit prefix.
/// Each entry holds `symbol << 4 | length`, and a length of zero marks an
/// unused code.
struct Huffman {
    table: Vec<u32>,
    bits: u32,
}

impl Huffman {
    /// Builds the table from per-symbol code lengths (RFC 1951 section 3.2.2).
    fn new(lengths: &[u8]) -> Result<Self, PngError> {
        let mut count = [0i32; 16];
        for &len in lengths {
            count[usize::from(len)] += 1;
        }
        count[0] = 0;

        // Reject over-subscribed codes. Incomplete codes are accepted, and any
        // unused code fails when it is decoded.
        let mut left = 1i32;
        for len in 1..16 {
            left = (left << 1) - count[len];
            if left < 0 {
                return Err(PngError::Deflate("over-subscribed Huffman code"));
            }
        }

        let bits = (1..16).rev().find(|&len| count[len] > 0).unwrap_or(1) as u32;
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for len in 1..16 {
            code = (code + count[len - 1] as u32) << 1;
            next[len] = code;
        }

        let mut table = vec![0u32; 1 << bits];
        for (symbol, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let len = u32::from(len);
            let code = next[len as usize];
            next[len as usize] += 1;
            let entry = ((symbol as u32) << 4) | len;
            // Deflate stores codes most significant bit first in an LSB-first stream.
            let mut index = reverse_bits(code, len) as usize;
            while index < table.len() {
                table[index] = entry;
                index += 1usize << len;
            }
        }
        Ok(Huffman { table, bits })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<usize, PngError> {
        reader.fill();
        let entry = self.table[(reader.buf & ((1u64 << self.bits) - 1)) as usize];
        let len = entry & 0xF;
        if len == 0 {
            return Err(PngError::Deflate("invalid Huffman code"));
        }
        reader.bits(len)?;
        Ok((entry >> 4) as usize)
    }
}

fn reverse_bits(code: u32, len: u32) -> u32 {
    (0..len).fold(0, |acc, i| (acc << 1) | ((code >> i) & 1))
}

fn fixed_tables() -> Result<(Huffman, Huffman), PngError> {
    let mut lengths = [0u8; 288];
    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..].fill(8);
    Ok((Huffman::new(&lengths)?, Huffman::new(&[5u8; 32])?))
}

/// Reads the code tables of a dynamic Huffman block header.
fn read_dynamic(reader: &mut BitReader<'_>) -> Result<(Huffman, Huffman), PngError> {
    let hlit = reader.bits(5)? as usize + 257;
    let hdist = reader.bits(5)? as usize + 1;
    let hclen = reader.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err(PngError::Deflate("too many length or distance codes"));
    }

    let mut cl_lengths = [0u8; 19];
    for &slot in &CODE_LENGTH_ORDER[..hclen] {
        cl_lengths[slot] = reader.bits(3)? as u8;
    }
    let cl = Huffman::new(&cl_lengths)?;

    let total = hlit + hdist;
    let mut lengths: Vec<u8> = Vec::with_capacity(total);
    while lengths.len() < total {
        let (value, repeat) = match cl.decode(reader)? {
            sym @ 0..=15 => (sym as u8, 1),
            16 => {
                let prev = *lengths
                    .last()
                    .ok_or(PngError::Deflate("length repeat with no previous length"))?;
                (prev, 3 + reader.bits(2)? as usize)
            }
            17 => (0, 3 + reader.bits(3)? as usize),
            _ => (0, 11 + reader.bits(7)? as usize),
        };
        if lengths.len() + repeat > total {
            return Err(PngError::Deflate("code lengths overflow the block header"));
        }
        lengths.extend(std::iter::repeat_n(value, repeat));
    }
    if lengths[256] == 0 {
        return Err(PngError::Deflate("missing end-of-block code"));
    }
    let (lit, dist) = lengths.split_at(hlit);
    Ok((Huffman::new(lit)?, Huffman::new(dist)?))
}

/// Inflates a zlib stream whose decompressed size must be exactly `expected`.
fn inflate(zlib: &[u8], expected: usize) -> Result<Vec<u8>, PngError> {
    if zlib.len() < 2 {
        return Err(PngError::Truncated);
    }
    let (cmf, flg) = (zlib[0], zlib[1]);
    if (cmf & 0x0F) != 8 || (cmf >> 4) > 7 {
        return Err(PngError::Deflate("zlib stream is not deflate with a 32 KiB window"));
    }
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 {
        return Err(PngError::Deflate("bad zlib header check"));
    }
    if flg & 0x20 != 0 {
        return Err(PngError::Deflate("preset dictionaries are not supported"));
    }

    let mut reader = BitReader::new(&zlib[2..]);
    let mut out = Vec::with_capacity(expected);
    loop {
        let last = reader.bits(1)? == 1;
        match reader.bits(2)? {
            0 => inflate_stored(&mut reader, &mut out, expected)?,
            1 => {
                let (lit, dist) = fixed_tables()?;
                inflate_codes(&mut reader, &lit, &dist, &mut out, expected)?;
            }
            2 => {
                let (lit, dist) = read_dynamic(&mut reader)?;
                inflate_codes(&mut reader, &lit, &dist, &mut out, expected)?;
            }
            _ => return Err(PngError::Deflate("reserved block type")),
        }
        if last {
            break;
        }
    }
    if out.len() != expected {
        return Err(PngError::Deflate("decompressed size does not match the image"));
    }
    reader.align();
    let trailer = be_u32(reader.bytes(4)?);
    if adler32(&out) != trailer {
        return Err(PngError::Checksum);
    }
    Ok(out)
}

fn inflate_stored(
    reader: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), PngError> {
    reader.align();
    let len = reader.bits(16)? as usize;
    let nlen = reader.bits(16)? as usize;
    if (len ^ 0xFFFF) != nlen {
        return Err(PngError::Deflate("stored block LEN and NLEN disagree"));
    }
    let data = reader.bytes(len)?;
    if len > limit.saturating_sub(out.len()) {
        return Err(PngError::Deflate("output exceeds the image size"));
    }
    out.extend_from_slice(data);
    Ok(())
}

/// Decodes literal and match symbols until the end-of-block code.
fn inflate_codes(
    reader: &mut BitReader<'_>,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), PngError> {
    loop {
        let symbol = lit.decode(reader)?;
        if symbol < 256 {
            if out.len() >= limit {
                return Err(PngError::Deflate("output exceeds the image size"));
            }
            out.push(symbol as u8);
        } else if symbol == 256 {
            return Ok(());
        } else {
            let index = symbol - 257;
            if index >= LENGTH_BASE.len() {
                return Err(PngError::Deflate("invalid length code"));
            }
            let length = (LENGTH_BASE[index] + reader.bits(LENGTH_EXTRA[index])?) as usize;

            let dist_symbol = dist.decode(reader)?;
            if dist_symbol >= DIST_BASE.len() {
                return Err(PngError::Deflate("invalid distance code"));
            }
            let distance = (DIST_BASE[dist_symbol] + reader.bits(DIST_EXTRA[dist_symbol])?) as usize;

            if distance > out.len() {
                return Err(PngError::Deflate("distance reaches before the start of the output"));
            }
            if length > limit.saturating_sub(out.len()) {
                return Err(PngError::Deflate("output exceeds the image size"));
            }
            // Copy byte by byte, because the source may overlap the destination.
            let start = out.len() - distance;
            for i in 0..length {
                let byte = out[start + i];
                out.push(byte);
            }
        }
    }
}

/// Size of the zlib stream that [`zlib_stored`] produces for `raw_len` bytes.
fn zlib_stored_len(raw_len: usize) -> usize {
    let blocks = raw_len.div_ceil(MAX_STORED_BLOCK).max(1);
    2usize
        .saturating_add(raw_len)
        .saturating_add(5 * blocks)
        .saturating_add(4)
}

/// Wraps `data` in a zlib stream made of stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(zlib_stored_len(data.len()));
    // CMF 0x78: deflate with a 32 KiB window. FLG 0x01 makes the two-byte
    // header a multiple of 31, with no preset dictionary and the fastest level.
    out.extend_from_slice(&[0x78, 0x01]);

    let mut rest = data;
    loop {
        let take = rest.len().min(MAX_STORED_BLOCK);
        let (block, tail) = rest.split_at(take);
        let is_final = tail.is_empty();
        // Header byte: BFINAL in bit 0, BTYPE = 00 (stored) in bits 1-2, and
        // the remaining bits are padding up to the byte boundary.
        out.push(u8::from(is_final));
        let len = take as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
        if is_final {
            break;
        }
        rest = tail;
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Adler-32 checksum as used by zlib.
fn adler32(data: &[u8]) -> u32 {
    let mut a: u64 = 1;
    let mut b: u64 = 0;
    for chunk in data.chunks(ADLER_CHUNK) {
        for &byte in chunk {
            a += u64::from(byte);
            b += a;
        }
        a %= ADLER_MOD;
        b %= ADLER_MOD;
    }
    // Both halves are below 2^16 here, so the packing cannot overflow.
    ((b << 16) | a) as u32
}

/// Lookup table for the reflected CRC-32 polynomial 0xEDB88320.
const CRC_TABLE: [u32; 256] = build_crc_table();

const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

/// CRC-32 over a chunk's type code followed by its payload.
fn chunk_crc(kind: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in kind.iter().chain(data) {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Appends one chunk: length, type, payload and CRC.
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&chunk_crc(kind, data).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits a PNG file into `(type, payload)` pairs, checking each CRC.
    fn parse_chunks(file: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        assert_eq!(&file[..8], &SIGNATURE, "bad signature");
        let mut pos = 8;
        let mut chunks = Vec::new();
        while pos < file.len() {
            let len = u32::from_be_bytes(file[pos..pos + 4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = file[pos + 4..pos + 8].try_into().unwrap();
            let data = &file[pos + 8..pos + 8 + len];
            let stored =
                u32::from_be_bytes(file[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            assert_eq!(stored, chunk_crc(&kind, data), "bad CRC on {:?}", kind);
            chunks.push((kind, data.to_vec()));
            pos += 12 + len;
        }
        assert_eq!(pos, file.len(), "trailing bytes after IEND");
        chunks
    }

    /// Decodes a zlib stream of stored blocks, checking framing and Adler-32.
    fn inflate_stored(zlib: &[u8]) -> Vec<u8> {
        assert_eq!(zlib[0], 0x78, "compression method must be deflate");
        let header = (u16::from(zlib[0]) << 8) | u16::from(zlib[1]);
        assert_eq!(header % 31, 0, "zlib header checksum");
        let mut pos = 2;
        let mut out = Vec::new();
        loop {
            let block_header = zlib[pos];
            assert_eq!((block_header >> 1) & 0b11, 0, "block must be stored");
            let is_final = block_header & 1 == 1;
            let len = u16::from_le_bytes([zlib[pos + 1], zlib[pos + 2]]);
            let nlen = u16::from_le_bytes([zlib[pos + 3], zlib[pos + 4]]);
            assert_eq!(nlen, !len, "NLEN must be the complement of LEN");
            out.extend_from_slice(&zlib[pos + 5..pos + 5 + len as usize]);
            pos += 5 + len as usize;
            if is_final {
                break;
            }
        }
        let stored = u32::from_be_bytes(zlib[pos..pos + 4].try_into().unwrap());
        assert_eq!(pos + 4, zlib.len(), "bytes after the Adler-32 trailer");
        assert_eq!(stored, adler32(&out), "Adler-32 mismatch");
        out
    }

    fn idat_payload(file: &[u8]) -> Vec<u8> {
        let chunks = parse_chunks(file);
        let idat: Vec<_> = chunks.iter().filter(|c| &c.0 == b"IDAT").collect();
        assert_eq!(idat.len(), 1, "exactly one IDAT chunk expected");
        idat[0].1.clone()
    }

    /// Removes the filter byte from each scanline of raw image data.
    fn unfilter_rows(raw: &[u8], width: usize, height: usize) -> Vec<u8> {
        let row = width * 4 + 1;
        assert_eq!(raw.len(), row * height, "raw scanline size");
        let mut pixels = Vec::with_capacity(width * height * 4);
        for line in raw.chunks_exact(row) {
            assert_eq!(line[0], 0, "filter type must be None");
            pixels.extend_from_slice(&line[1..]);
        }
        pixels
    }

    fn pattern(width: usize, height: usize) -> Vec<u8> {
        (0..width * height * 4)
            .map(|i| ((i * 37 + i / 7) % 256) as u8)
            .collect()
    }

    #[test]
    fn chunk_order_and_signature() {
        let file = encode_rgba(2, 3, &pattern(2, 3)).unwrap();
        assert_eq!(
            &file[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        let kinds: Vec<[u8; 4]> = parse_chunks(&file).into_iter().map(|c| c.0).collect();
        assert_eq!(kinds, vec![*b"IHDR", *b"IDAT", *b"IEND"]);
    }

    #[test]
    fn ihdr_fields_match_input() {
        let file = encode_rgba(5, 7, &pattern(5, 7)).unwrap();
        let chunks = parse_chunks(&file);
        let ihdr = &chunks[0].1;
        assert_eq!(ihdr.len(), 13);
        assert_eq!(
            u32::from_be_bytes(ihdr[0..4].try_into().unwrap()),
            5,
            "width"
        );
        assert_eq!(
            u32::from_be_bytes(ihdr[4..8].try_into().unwrap()),
            7,
            "height"
        );
        assert_eq!(ihdr[8], 8, "bit depth");
        assert_eq!(ihdr[9], 6, "colour type RGBA");
        assert_eq!(ihdr[10], 0, "compression method");
        assert_eq!(ihdr[11], 0, "filter method");
        assert_eq!(ihdr[12], 0, "interlace method");
    }

    #[test]
    fn iend_is_the_standard_trailer() {
        let file = encode_rgba(1, 1, &[1, 2, 3, 4]).unwrap();
        let trailer = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];
        assert_eq!(&file[file.len() - 12..], &trailer);
    }

    #[test]
    fn crc_and_adler_known_vectors() {
        // CRC-32 check value for "123456789", split across kind and data.
        assert_eq!(chunk_crc(b"1234", b"56789"), 0xCBF4_3926);
        // Adler-32 check value for "Wikipedia".
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(&[]), 1);
    }

    #[test]
    fn pixels_round_trip_through_idat() {
        let (w, h) = (3, 2);
        let rgba = pattern(w, h);
        let file = encode_rgba(w as u32, h as u32, &rgba).unwrap();
        let raw = inflate_stored(&idat_payload(&file));
        assert_eq!(unfilter_rows(&raw, w, h), rgba);
    }

    #[test]
    fn multi_block_stream_round_trips() {
        // 200 x 100 gives 80_100 raw bytes, which needs two stored blocks.
        let (w, h) = (200, 100);
        let rgba = pattern(w, h);
        let file = encode_rgba(w as u32, h as u32, &rgba).unwrap();
        let zlib = idat_payload(&file);
        assert_eq!(zlib[2] & 1, 0, "first block must not be final");
        assert_eq!(
            u16::from_le_bytes([zlib[3], zlib[4]]) as usize,
            MAX_STORED_BLOCK
        );
        let raw = inflate_stored(&zlib);
        assert_eq!(unfilter_rows(&raw, w, h), rgba);
    }

    #[test]
    fn single_pixel_is_valid() {
        let file = encode_rgba(1, 1, &[10, 20, 30, 40]).unwrap();
        let raw = inflate_stored(&idat_payload(&file));
        assert_eq!(raw, vec![0, 10, 20, 30, 40]);
    }

    #[test]
    fn zero_dimension_is_rejected() {
        assert_eq!(encode_rgba(0, 4, &[]), Err(PngError::ZeroDimension));
        assert_eq!(encode_rgba(4, 0, &[]), Err(PngError::ZeroDimension));
    }

    #[test]
    fn wrong_buffer_length_is_rejected() {
        assert_eq!(
            encode_rgba(2, 2, &[0; 15]),
            Err(PngError::BufferLength {
                expected: 16,
                actual: 15
            })
        );
    }

    /// LSB-first bit writer for hand-built deflate streams.
    #[derive(Default)]
    struct BitWriter {
        out: Vec<u8>,
        pending: u32,
        used: u32,
    }

    impl BitWriter {
        fn bit(&mut self, value: u32) {
            self.pending |= (value & 1) << self.used;
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.pending as u8);
                self.pending = 0;
                self.used = 0;
            }
        }

        /// Writes `count` bits of `value`, least significant bit first.
        fn bits(&mut self, value: u32, count: u32) {
            for i in 0..count {
                self.bit(value >> i);
            }
        }

        /// Writes a Huffman code, most significant bit first.
        fn code(&mut self, code: u32, len: u32) {
            for i in (0..len).rev() {
                self.bit(code >> i);
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.used > 0 {
                self.out.push(self.pending as u8);
            }
            self.out
        }
    }

    fn write_fixed_literal(w: &mut BitWriter, sym: u32) {
        match sym {
            0..=143 => w.code(0x30 + sym, 8),
            144..=255 => w.code(0x190 + sym - 144, 9),
            256..=279 => w.code(sym - 256, 7),
            _ => w.code(0xC0 + sym - 280, 8),
        }
    }

    /// Writes a back-reference with the fixed codes.
    fn write_fixed_match(w: &mut BitWriter, len: u32, dist: u32) {
        let idx = LENGTH_BASE.iter().rposition(|&b| b <= len).unwrap();
        write_fixed_literal(w, 257 + idx as u32);
        w.bits(len - LENGTH_BASE[idx], LENGTH_EXTRA[idx]);
        let didx = DIST_BASE.iter().rposition(|&b| b <= dist).unwrap();
        w.code(didx as u32, 5);
        w.bits(dist - DIST_BASE[didx], DIST_EXTRA[didx]);
    }

    /// Canonical Huffman codes for the given lengths (RFC 1951 section 3.2.2).
    fn canonical_codes(lengths: &[u8]) -> Vec<u32> {
        let max = *lengths.iter().max().unwrap() as usize;
        let mut count = vec![0u32; max + 1];
        for &l in lengths {
            if l > 0 {
                count[l as usize] += 1;
            }
        }
        let mut next = vec![0u32; max + 1];
        let mut code = 0;
        for len in 1..=max {
            code = (code + count[len - 1]) << 1;
            next[len] = code;
        }
        lengths
            .iter()
            .map(|&l| {
                if l == 0 {
                    0
                } else {
                    let c = next[l as usize];
                    next[l as usize] += 1;
                    c
                }
            })
            .collect()
    }

    /// Wraps a deflate body in a zlib header and an Adler-32 trailer.
    fn zlib_wrap(body: &[u8], uncompressed: &[u8]) -> Vec<u8> {
        let mut stream = vec![0x78, 0x01];
        stream.extend_from_slice(body);
        stream.extend_from_slice(&adler32(uncompressed).to_be_bytes());
        stream
    }

    /// Builds an IHDR payload with the given fields.
    fn ihdr(width: u32, height: u32, depth: u8, colour: u8, interlace: u8) -> [u8; 13] {
        let mut out = [0u8; 13];
        out[0..4].copy_from_slice(&width.to_be_bytes());
        out[4..8].copy_from_slice(&height.to_be_bytes());
        out[8] = depth;
        out[9] = colour;
        out[12] = interlace;
        out
    }

    /// Assembles a PNG file from an IHDR payload and a zlib stream.
    fn assemble_with(ihdr_body: &[u8; 13], zlib: &[u8]) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        write_chunk(&mut out, b"IHDR", ihdr_body);
        write_chunk(&mut out, b"IDAT", zlib);
        write_chunk(&mut out, b"IEND", &[]);
        out
    }

    /// Assembles a PNG file of the given size and colour type.
    fn assemble(width: u32, height: u32, colour: u8, zlib: &[u8]) -> Vec<u8> {
        assemble_with(&ihdr(width, height, 8, colour, 0), zlib)
    }

    /// Applies forward filter `kind` to one scanline.
    fn filter_row(kind: u8, bpp: usize, cur: &[u8], prev: &[u8]) -> Vec<u8> {
        (0..cur.len())
            .map(|i| {
                let left = if i >= bpp { cur[i - bpp] } else { 0 };
                let up = prev[i];
                let up_left = if i >= bpp { prev[i - bpp] } else { 0 };
                let predictor = match kind {
                    0 => 0,
                    1 => left,
                    2 => up,
                    3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                    _ => paeth(left, up, up_left),
                };
                cur[i].wrapping_sub(predictor)
            })
            .collect()
    }

    /// Filters all scanlines, choosing the filter type by row so that all five occur.
    fn filtered_raw(width: usize, height: usize, bpp: usize, pixels: &[u8]) -> Vec<u8> {
        let row = width * bpp;
        let zero = vec![0u8; row];
        let mut raw = Vec::new();
        for y in 0..height {
            let cur = &pixels[y * row..(y + 1) * row];
            let prev = if y == 0 {
                &zero[..]
            } else {
                &pixels[(y - 1) * row..y * row]
            };
            let kind = (y % 5) as u8;
            raw.push(kind);
            raw.extend(filter_row(kind, bpp, cur, prev));
        }
        raw
    }

    /// Scanlines with filter type 0, for tests that need no prediction.
    fn unfiltered_raw(row_len: usize, pixels: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        for row in pixels.chunks_exact(row_len) {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        raw
    }

    #[test]
    fn decode_inverts_encode() {
        for &(w, h) in &[(1usize, 1usize), (3, 2), (200, 100)] {
            let rgba = pattern(w, h);
            let file = encode_rgba(w as u32, h as u32, &rgba).unwrap();
            assert_eq!(decode_rgba(&file).unwrap(), (w as u32, h as u32, rgba));
        }
    }

    #[test]
    fn every_filter_type_is_reversed_for_rgb_and_rgba() {
        let (w, h) = (9usize, 12usize);
        for &(colour, bpp) in &[(2u8, 3usize), (6, 4)] {
            let samples: Vec<u8> = (0..w * h * bpp)
                .map(|i| ((i * 53 + i / 5) % 251) as u8)
                .collect();
            let raw = filtered_raw(w, h, bpp, &samples);
            let file = assemble(w as u32, h as u32, colour, &zlib_stored(&raw));

            let (dw, dh, rgba) = decode_rgba(&file).unwrap();
            assert_eq!((dw, dh), (w as u32, h as u32));
            let expected: Vec<u8> = samples
                .chunks_exact(bpp)
                .flat_map(|p| [p[0], p[1], p[2], if bpp == 4 { p[3] } else { 255 }])
                .collect();
            assert_eq!(rgba, expected, "colour type {colour}");
        }
    }

    #[test]
    fn greyscale_and_grey_alpha_expand_to_rgba() {
        let grey = [0u8, 10, 20, 30, 40, 50];
        let file = assemble(3, 2, 0, &zlib_stored(&unfiltered_raw(3, &grey)));
        let (_, _, rgba) = decode_rgba(&file).unwrap();
        assert_eq!(
            rgba,
            vec![0, 0, 0, 255, 10, 10, 10, 255, 20, 20, 20, 255, 30, 30, 30, 255, 40, 40, 40, 255, 50, 50, 50, 255]
        );

        let grey_alpha = [7u8, 0, 8, 128, 9, 255];
        let file = assemble(3, 1, 4, &zlib_stored(&unfiltered_raw(6, &grey_alpha)));
        let (_, _, rgba) = decode_rgba(&file).unwrap();
        assert_eq!(rgba, vec![7, 7, 7, 0, 8, 8, 8, 128, 9, 9, 9, 255]);
    }

    #[test]
    fn fixed_huffman_block_with_matches() {
        let mut w = BitWriter::default();
        w.bits(1, 1); // BFINAL
        w.bits(1, 2); // BTYPE 01: fixed codes
        for &b in b"abc" {
            write_fixed_literal(&mut w, u32::from(b));
        }
        write_fixed_match(&mut w, 10, 3);
        write_fixed_match(&mut w, 258, 1);
        write_fixed_literal(&mut w, 256);

        let mut expected = b"abcabcabcabca".to_vec();
        expected.extend(std::iter::repeat_n(b'a', 258));
        let stream = zlib_wrap(&w.finish(), &expected);
        assert_eq!(inflate(&stream, expected.len()).unwrap(), expected);
    }

    #[test]
    fn dynamic_huffman_block() {
        // Bytes 0..=255 get 9-bit codes. End-of-block and symbol 257 (length 3)
        // get 2-bit codes. The single distance code is 1 bit long (distance 1).
        let mut lit_lengths = vec![9u8; 256];
        lit_lengths.push(2);
        lit_lengths.push(2);
        let dist_lengths = [1u8];

        // Code-length alphabet: symbols 1, 2 and 9 are used.
        let mut cl_lengths = [0u8; 19];
        cl_lengths[9] = 2;
        cl_lengths[2] = 2;
        cl_lengths[1] = 1;
        let cl_codes = canonical_codes(&cl_lengths);
        let lit_codes = canonical_codes(&lit_lengths);
        let dist_codes = canonical_codes(&dist_lengths);

        let mut w = BitWriter::default();
        w.bits(1, 1); // BFINAL
        w.bits(2, 2); // BTYPE 10: dynamic codes
        w.bits(1, 5); // HLIT: 258 literal/length codes
        w.bits(0, 5); // HDIST: 1 distance code
        let hclen = CODE_LENGTH_ORDER
            .iter()
            .rposition(|&s| cl_lengths[s] > 0)
            .unwrap()
            + 1;
        w.bits((hclen - 4) as u32, 4);
        for &slot in &CODE_LENGTH_ORDER[..hclen] {
            w.bits(u32::from(cl_lengths[slot]), 3);
        }
        for &len in lit_lengths.iter().chain(dist_lengths.iter()) {
            w.code(cl_codes[len as usize], u32::from(cl_lengths[len as usize]));
        }

        w.code(lit_codes[65], 9); // 'A'
        w.code(lit_codes[257], 2); // length 3
        w.code(dist_codes[0], 1); // distance 1
        w.code(lit_codes[257], 2);
        w.code(dist_codes[0], 1);
        w.code(lit_codes[256], 2); // end of block

        let expected = vec![b'A'; 7];
        let stream = zlib_wrap(&w.finish(), &expected);
        assert_eq!(inflate(&stream, expected.len()).unwrap(), expected);
    }

    #[test]
    fn split_idat_chunks_are_joined() {
        let raw = unfiltered_raw(4, &[1, 2, 3, 4]);
        let zlib = zlib_stored(&raw);
        let mut file = SIGNATURE.to_vec();
        write_chunk(&mut file, b"IHDR", &ihdr(1, 1, 8, 6, 0));
        write_chunk(&mut file, b"IDAT", &zlib[..5]);
        write_chunk(&mut file, b"IDAT", &zlib[5..]);
        write_chunk(&mut file, b"IEND", &[]);
        assert_eq!(decode_rgba(&file).unwrap(), (1, 1, vec![1, 2, 3, 4]));
    }

    #[test]
    fn paeth_predictor_known_values() {
        assert_eq!(paeth(0, 0, 0), 0);
        assert_eq!(paeth(10, 20, 10), 20);
        assert_eq!(paeth(20, 10, 10), 20);
        assert_eq!(paeth(10, 10, 20), 10);
    }

    #[test]
    fn rejects_bad_signature_and_truncation() {
        assert_eq!(decode_rgba(&[0u8; 4]), Err(PngError::Truncated));
        assert_eq!(
            decode_rgba(b"not a png at all"),
            Err(PngError::InvalidSignature)
        );
        let file = encode_rgba(2, 2, &pattern(2, 2)).unwrap();
        assert_eq!(decode_rgba(&file[..file.len() - 1]), Err(PngError::Truncated));
        assert_eq!(decode_rgba(&file[..20]), Err(PngError::Truncated));
    }

    #[test]
    fn rejects_corrupt_chunk() {
        let mut file = encode_rgba(2, 2, &pattern(2, 2)).unwrap();
        file[19] ^= 1; // low byte of the IHDR width
        assert_eq!(decode_rgba(&file), Err(PngError::Checksum));
    }

    #[test]
    fn rejects_bad_adler_checksum() {
        let mut zlib = zlib_stored(&[0, 1, 2, 3, 4]);
        let last = zlib.len() - 1;
        zlib[last] ^= 0xFF;
        let file = assemble(1, 1, 6, &zlib);
        assert_eq!(decode_rgba(&file), Err(PngError::Checksum));
    }

    #[test]
    fn rejects_unsupported_and_invalid_headers() {
        let zlib = zlib_stored(&[0, 0, 0, 0, 0]);
        let unsupported = [
            ihdr(1, 1, 16, 6, 0),
            ihdr(1, 1, 8, 3, 0),
            ihdr(1, 1, 8, 6, 1),
            ihdr(1, 1, 8, 6, 7),
        ];
        for body in &unsupported {
            let file = assemble_with(body, &zlib);
            assert!(
                matches!(decode_rgba(&file), Err(PngError::Unsupported(_) | PngError::Malformed(_))),
                "{body:?}"
            );
        }
        let oversized = assemble_with(&ihdr(20_000, 1, 8, 6, 0), &zlib);
        assert_eq!(decode_rgba(&oversized), Err(PngError::TooLarge));
        let zero = assemble_with(&ihdr(0, 1, 8, 6, 0), &zlib);
        assert_eq!(decode_rgba(&zero), Err(PngError::ZeroDimension));
    }

    #[test]
    fn rejects_unknown_filter_type() {
        let zlib = zlib_stored(&[5, 1, 2, 3, 4]);
        assert!(matches!(
            decode_rgba(&assemble(1, 1, 6, &zlib)),
            Err(PngError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_wrong_decompressed_size() {
        let longer = zlib_stored(&[0, 1, 2, 3, 4, 5]);
        assert!(matches!(
            decode_rgba(&assemble(1, 1, 6, &longer)),
            Err(PngError::Deflate(_))
        ));
        let shorter = zlib_stored(&[0, 1, 2]);
        assert!(matches!(
            decode_rgba(&assemble(1, 1, 6, &shorter)),
            Err(PngError::Deflate(_))
        ));
    }

    #[test]
    fn rejects_distance_before_stream_start() {
        let mut w = BitWriter::default();
        w.bits(1, 1);
        w.bits(1, 2);
        write_fixed_match(&mut w, 3, 1); // nothing has been output yet
        write_fixed_literal(&mut w, 256);
        let stream = zlib_wrap(&w.finish(), &[0, 0, 0]);
        assert!(matches!(
            inflate(&stream, 3),
            Err(PngError::Deflate(_))
        ));
    }

    #[test]
    fn rejects_reserved_block_type() {
        let mut w = BitWriter::default();
        w.bits(1, 1);
        w.bits(3, 2);
        let stream = zlib_wrap(&w.finish(), &[]);
        assert!(matches!(inflate(&stream, 0), Err(PngError::Deflate(_))));
    }
}
