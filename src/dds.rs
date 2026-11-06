//! Reading and writing the `.dds` textures of car skins.
//!
//! Reads the formats skins really use: DXT1, DXT3, DXT5 and uncompressed 24 and 32 bit, top mip
//! level only. Writes uncompressed 32 bit BGRA with a full mip chain, which the game loads like
//! any other skin texture.

use std::fmt;

const MAGIC: &[u8; 4] = b"DDS ";
const HEADER_LEN: usize = 128;
const MAX_SIDE: u32 = 16384;

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdsError {
    NotDds,
    Truncated,
    Unsupported(String),
}

impl fmt::Display for DdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DdsError::NotDds => write!(f, "this is not a DDS file"),
            DdsError::Truncated => write!(f, "the DDS file is cut short"),
            DdsError::Unsupported(format) => write!(f, "DDS format {format} is not supported"),
        }
    }
}

impl std::error::Error for DdsError {}

/// A picture as RGBA bytes, four per pixel, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn rgb565(color: u16) -> [u8; 3] {
    let (r, g, b) = ((color >> 11) & 31, (color >> 5) & 63, color & 31);
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

/// The four colours of a DXT colour block. With `three_colour` the block may use the punch-through
/// mode of DXT1.
fn block_colours(block: &[u8], three_colour: bool) -> [[u8; 4]; 4] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |wa: u32, wb: u32, total: u32| {
        let channel = |i: usize| ((u32::from(a[i]) * wa + u32::from(b[i]) * wb) / total) as u8;
        [channel(0), channel(1), channel(2), 255]
    };
    if !three_colour || c0 > c1 {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(1, 1, 2), [0, 0, 0, 0]]
    }
}

/// The sixteen alphas of a DXT5 alpha block.
fn dxt5_alpha(block: &[u8]) -> [u8; 16] {
    let (a0, a1) = (u32::from(block[0]), u32::from(block[1]));
    let table: [u32; 8] = if a0 > a1 {
        [a0, a1, (6 * a0 + a1) / 7, (5 * a0 + 2 * a1) / 7, (4 * a0 + 3 * a1) / 7, (3 * a0 + 4 * a1) / 7, (2 * a0 + 5 * a1) / 7, (a0 + 6 * a1) / 7]
    } else {
        [a0, a1, (4 * a0 + a1) / 5, (3 * a0 + 2 * a1) / 5, (2 * a0 + 3 * a1) / 5, (a0 + 4 * a1) / 5, 0, 255]
    };
    let mut bits: u64 = 0;
    for (i, byte) in block[2..8].iter().enumerate() {
        bits |= u64::from(*byte) << (8 * i);
    }
    let mut out = [0u8; 16];
    for (i, alpha) in out.iter_mut().enumerate() {
        *alpha = table[((bits >> (3 * i)) & 7) as usize] as u8;
    }
    out
}

fn decode_blocks(data: &[u8], width: u32, height: u32, fourcc: &[u8]) -> Result<Vec<u8>, DdsError> {
    let block_size = if fourcc == b"DXT1" { 8 } else { 16 };
    let (across, down) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
    if data.len() < across * down * block_size {
        return Err(DdsError::Truncated);
    }
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for by in 0..down {
        for bx in 0..across {
            let block = &data[(by * across + bx) * block_size..][..block_size];
            let (colour_block, alphas): (&[u8], Option<[u8; 16]>) = match fourcc {
                b"DXT5" => (&block[8..], Some(dxt5_alpha(block))),
                b"DXT3" => {
                    let mut alphas = [0u8; 16];
                    for (i, alpha) in alphas.iter_mut().enumerate() {
                        let nibble = (block[i / 2] >> (4 * (i % 2))) & 15;
                        *alpha = nibble * 17;
                    }
                    (&block[8..], Some(alphas))
                }
                _ => (block, None),
            };
            let colours = block_colours(colour_block, fourcc == b"DXT1");
            let indices = u32_at(colour_block, 4);
            for py in 0..4usize {
                for px in 0..4usize {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x >= width as usize || y >= height as usize {
                        continue;
                    }
                    let n = py * 4 + px;
                    let mut colour = colours[((indices >> (2 * n)) & 3) as usize];
                    if let Some(alphas) = &alphas {
                        colour[3] = alphas[n];
                    }
                    let at = (y * width as usize + x) * 4;
                    pixels[at..at + 4].copy_from_slice(&colour);
                }
            }
        }
    }
    Ok(pixels)
}

/// Decodes the top mip level.
pub fn decode(bytes: &[u8]) -> Result<Rgba, DdsError> {
    if bytes.len() < 4 || &bytes[..4] != MAGIC {
        return Err(DdsError::NotDds);
    }
    if bytes.len() < HEADER_LEN {
        return Err(DdsError::Truncated);
    }
    let (height, width) = (u32_at(bytes, 12), u32_at(bytes, 16));
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(DdsError::Unsupported(format!("size {width} by {height}")));
    }
    let fourcc = &bytes[84..88];
    let data = &bytes[HEADER_LEN..];
    let pixels = if matches!(fourcc, b"DXT1" | b"DXT3" | b"DXT5") {
        decode_blocks(data, width, height, fourcc)?
    } else if fourcc == [0, 0, 0, 0] {
        let bits = u32_at(bytes, 88);
        let step = (bits / 8) as usize;
        if bits != 32 && bits != 24 {
            return Err(DdsError::Unsupported(format!("{bits} bit")));
        }
        let has_alpha = u32_at(bytes, 80) & 1 != 0;
        let count = width as usize * height as usize;
        if data.len() < count * step {
            return Err(DdsError::Truncated);
        }
        let (r_shift, b_shift) = (u32_at(bytes, 92).trailing_zeros() / 8, u32_at(bytes, 100).trailing_zeros() / 8);
        let mut pixels = Vec::with_capacity(count * 4);
        for texel in data[..count * step].chunks_exact(step) {
            let alpha = if step == 4 && has_alpha { texel[3] } else { 255 };
            pixels.extend_from_slice(&[texel[(r_shift as usize).min(2)], texel[1], texel[(b_shift as usize).min(2)], alpha]);
        }
        pixels
    } else {
        return Err(DdsError::Unsupported(String::from_utf8_lossy(fourcc).into_owned()));
    };
    Ok(Rgba { width, height, pixels })
}

/// Halves a picture by averaging, for the next mip level. An odd side keeps its last row or column.
fn half(image: &Rgba) -> Rgba {
    let (w, h) = (image.width as usize, image.height as usize);
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut pixels = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        for x in 0..nw {
            let (x0, y0) = (x * w / nw, y * h / nh);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            for channel in 0..4 {
                let at = |px: usize, py: usize| u32::from(image.pixels[(py * w + px) * 4 + channel]);
                pixels.push(((at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1)) / 4) as u8);
            }
        }
    }
    Rgba { width: nw as u32, height: nh as u32, pixels }
}

/// Encodes as uncompressed 32 bit BGRA with all mip levels.
pub fn encode(image: &Rgba) -> Vec<u8> {
    let mut levels = vec![image.clone()];
    while levels.last().is_some_and(|l| l.width > 1 || l.height > 1) {
        if let Some(last) = levels.last() {
            let next = half(last);
            levels.push(next);
        }
    }
    let mut out = Vec::with_capacity(HEADER_LEN + image.pixels.len() * 4 / 3 + 64);
    let put = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
    out.extend_from_slice(MAGIC);
    put(&mut out, 124);
    // Caps, height, width, pixel format, pitch, mipmap count.
    put(&mut out, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000 | 0x20000);
    put(&mut out, image.height);
    put(&mut out, image.width);
    put(&mut out, image.width * 4);
    put(&mut out, 0);
    put(&mut out, levels.len() as u32);
    out.extend_from_slice(&[0u8; 44]);
    // DDS_PIXELFORMAT: RGB with alpha, 32 bit, BGRA masks.
    for value in [32, 0x41, 0, 32, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000] {
        put(&mut out, value);
    }
    // Texture, mipmapped, complex.
    put(&mut out, 0x1000 | 0x40_0000 | 0x8);
    out.extend_from_slice(&[0u8; 16]);
    for level in &levels {
        for pixel in level.pixels.chunks_exact(4) {
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Rgba {
        let mut pixels = Vec::new();
        for y in 0..8u8 {
            for x in 0..8u8 {
                pixels.extend_from_slice(&[x * 30, y * 30, 200, 255 - x]);
            }
        }
        Rgba { width: 8, height: 8, pixels }
    }

    fn header(width: u32, height: u32, fourcc: &[u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes[..4].copy_from_slice(MAGIC);
        bytes[12..16].copy_from_slice(&height.to_le_bytes());
        bytes[16..20].copy_from_slice(&width.to_le_bytes());
        bytes[84..88].copy_from_slice(fourcc);
        bytes
    }

    #[test]
    fn what_we_write_reads_back_the_same() {
        let picture = image();
        let bytes = encode(&picture);
        assert_eq!(decode(&bytes).unwrap(), picture);
        // Eight, four, two and one pixel levels.
        assert_eq!(bytes.len(), HEADER_LEN + (64 + 16 + 4 + 1) * 4);
        assert_eq!(u32_at(&bytes, 28), 4);
    }

    #[test]
    fn dxt1_blocks_decode_to_their_end_colours() {
        let mut bytes = header(4, 4, b"DXT1");
        // Pure red and pure blue, every pixel index 1 (blue).
        bytes.extend_from_slice(&[0x00, 0xF8, 0x1F, 0x00, 0x55, 0x55, 0x55, 0x55]);
        let picture = decode(&bytes).unwrap();
        assert_eq!(&picture.pixels[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn dxt5_keeps_its_alpha() {
        let mut bytes = header(4, 4, b"DXT5");
        bytes.extend_from_slice(&[128, 128, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[0x00, 0xF8, 0x1F, 0x00, 0, 0, 0, 0]);
        assert_eq!(decode(&bytes).unwrap().pixels[..4], [255, 0, 0, 128]);
    }

    #[test]
    fn bad_files_are_refused_not_panicked_on() {
        assert_eq!(decode(b"nope"), Err(DdsError::NotDds));
        assert_eq!(decode(b"DDS "), Err(DdsError::Truncated));
        assert_eq!(decode(&header(4, 4, b"DXT1")), Err(DdsError::Truncated));
        assert!(matches!(decode(&header(4, 4, b"ATI2")), Err(DdsError::Unsupported(_))));
        assert!(matches!(decode(&header(0, 4, b"DXT1")), Err(DdsError::Unsupported(_))));
    }
}
