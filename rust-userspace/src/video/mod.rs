use std::{
    default,
    ops::{Index, IndexMut},
};

mod dct;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unalign, Unaligned};

#[derive(FromBytes, Immutable, KnownLayout, Unaligned, IntoBytes)]
#[repr(C)]
/// Represents two horizontal pixels in a YUVV422 image.
pub struct YUYV422Sample {
    /// Luminance of first pixel
    y0: u8,
    /// Cb
    u: u8,
    /// Luminance of second pixel
    y1: u8,
    /// Cr
    v: u8,
}

pub struct YUVFrame<'a> {
    /// Width of the frame in pixels.
    width: usize,
    /// Height of the frame in pixels.
    height: usize,
    /// Data of the frame. Number of YUV422 samples will be (width / 2) * height,
    /// since each sample is two pixels.
    data: &'a [YUYV422Sample],
}

pub struct MutableYUVFrame<'a> {
    /// Width of the frame in pixels.
    width: usize,
    /// Height of the frame in pixels.
    height: usize,
    /// Data of the frame. Number of YUV422 samples will be (width / 2) * height,
    /// since each sample is two pixels.
    data: &'a mut [YUYV422Sample],
}

impl<'a> YUVFrame<'a> {
    pub fn new(width: usize, height: usize, data: &'a [u8]) -> Self {
        let data = <[YUYV422Sample]>::ref_from_bytes(data).unwrap();
        assert_eq!(data.len(), width * height / 2);
        Self {
            width,
            height,
            data,
        }
    }

    /// Get the luminance of a pixel at (x, y).
    fn get_luma(&self, x: usize, y: usize) -> u8 {
        let pixel = &self.data[y * self.width / 2 + x / 2];
        if x % 2 == 0 {
            pixel.y0
        } else {
            pixel.y1
        }
    }

    /// Get the chrominance of a pixel at (x, y).
    /// Returns (Cb, Cr).
    fn get_chroma(&self, x: usize, y: usize) -> (u8, u8) {
        let pixel = &self.data[y * self.width / 2 + x / 2];
        (pixel.u, pixel.v)
    }
}

impl<'a> MutableYUVFrame<'a> {
    pub fn new(width: usize, height: usize, data: &'a mut [u8]) -> Self {
        let data = <[YUYV422Sample]>::mut_from_bytes(data).unwrap();
        assert_eq!(data.len(), width * height / 2);
        Self {
            width,
            height,
            data,
        }
    }

    /// Set the luminance of a pixel at (x, y).
    fn set_luma(&mut self, x: usize, y: usize, value: u8) {
        let pixel = &mut self.data[y * self.width / 2 + x / 2];
        if x % 2 == 0 {
            pixel.y0 = value;
        } else {
            pixel.y1 = value;
        }
    }

    /// Set the chrominance of a pixel at (x, y).
    fn set_chroma(&mut self, x: usize, y: usize, value: (u8, u8)) {
        let pixel = &mut self.data[y * self.width / 2 + x / 2];
        pixel.u = value.0;
        pixel.v = value.1;
    }

    /// Get the luminance of a pixel at (x, y).
    fn get_luma(&self, x: usize, y: usize) -> u8 {
        let pixel = &self.data[y * self.width / 2 + x / 2];
        if x % 2 == 0 {
            pixel.y0
        } else {
            pixel.y1
        }
    }

    /// Get the chrominance of a pixel at (x, y).
    /// Returns (Cb, Cr).
    fn get_chroma(&self, x: usize, y: usize) -> (u8, u8) {
        let pixel = &self.data[y * self.width / 2 + x / 2];
        (pixel.u, pixel.v)
    }
}

/// A macroblock. Spans a 16x16 block of pixels,
/// with 4 8x8 blocks for Y and 1 8x8 block for U and V each.
#[derive(Default, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Macroblock {
    y0: [[u8; 8]; 8],
    y1: [[u8; 8]; 8],
    y2: [[u8; 8]; 8],
    y3: [[u8; 8]; 8],
    u: [[u8; 8]; 8],
    v: [[u8; 8]; 8],
}

impl Macroblock {
    /// Copy macroblock into a YUV422 buffer at given x and y coordinates.
    pub fn copy_to_yuv422_frame<'a>(&self, mut frame: MutableYUVFrame<'a>, x: usize, y: usize) {
        for (y_block, x_start, x_end, y_start, y_end) in [
            (&self.y0, 0, 8, 0, 8),
            (&self.y1, 8, 16, 0, 8),
            (&self.y2, 0, 8, 8, 16),
            (&self.y3, 8, 16, 8, 16),
        ] {
            for y_offset in y_start..y_end {
                for x_offset in x_start..x_end {
                    frame.set_luma(x + x_offset, y + y_offset, y_block[x_offset - x_start][y_offset - y_start]);
                }
            }
        }

        for y_offset in (0..16).step_by(2) {
            for x_offset in (0..16).step_by(2) {
                // 4:2:0 chroma subsampling -> 4:2:2 chroma subsampling
                frame.set_chroma(x + x_offset, y + y_offset, (self.u[x_offset / 2][y_offset / 2], self.v[x_offset / 2][y_offset / 2]));
                frame.set_chroma(x + x_offset, y + y_offset + 1, (self.u[x_offset / 2][y_offset / 2], self.v[x_offset / 2][y_offset / 2]));
            }
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct MacroblockWithPosition {
    pub block: Macroblock,
    pub x: usize,
    pub y: usize,
}

pub struct YUVFrameMacroblockIterator<'a> {
    frame: &'a YUVFrame<'a>,
    x_start: usize,
    y_start: usize,
    x: usize,
    y: usize,
    x_end: usize,
    y_end: usize,
}

impl<'a> YUVFrameMacroblockIterator<'a> {
    pub fn new(frame: &'a YUVFrame<'a>) -> Self {
        Self { frame, x_start: 0, y_start: 0, x: 0, y: 0, x_end: frame.width, y_end: frame.height }
    }

    /// Iterate over a subset of the frame defined by the rectangle (x, y) to (x_end, y_end).
    pub fn new_with_bounds(frame: &'a YUVFrame<'a>, x_start: usize, y_start: usize, x_end: usize, y_end: usize) -> Self {
        Self { frame, x_start, y_start, x: x_start, y: y_start, x_end, y_end }
    }
}

impl<'a> Iterator for YUVFrameMacroblockIterator<'a> {
    type Item = MacroblockWithPosition;

    fn next(&mut self) -> Option<Self::Item> {
        if self.y >= self.y_end {
            return None;
        }

        let mut block = Macroblock::default();

        for (y_block, x_start, x_end, y_start, y_end) in [
            (&mut block.y0, 0, 8, 0, 8),
            (&mut block.y1, 8, 16, 0, 8),
            (&mut block.y2, 0, 8, 8, 16),
            (&mut block.y3, 8, 16, 8, 16),
        ] {
            for y in y_start..y_end {
                for x in x_start..x_end {
                    y_block[x - x_start][y - y_start] = self.frame.get_luma(self.x + x, self.y + y);
                }
            }
        }

        for y in (0..16).step_by(2) {
            for x in (0..16).step_by(2) {
                // note that this ignores the chroma of the x, y + 1 pixel, i.e. making this 4:2:0
                block.u[x / 2][y / 2] = self.frame.get_chroma(self.x + x, self.y + y).0;
                block.v[x / 2][y / 2] = self.frame.get_chroma(self.x + x, self.y + y).1;
            }
        }

        let (x, y) = (self.x, self.y);

        self.x += 16;
        if self.x >= self.x_end {
            self.x = self.x_start;
            self.y += 16;
        }

        Some(MacroblockWithPosition { x, y, block })
    }
}

// ref: https://github.com/autergame/JpegView-Rust/blob/main/src/jpeg.rs
/// Standard JPEG luminance quantization table
#[rustfmt::skip]
const LUMINANCE_QUANTIZATION_TABLE: [[f64; 8]; 8] = [
	[16.0f64, 11.0f64, 10.0f64, 16.0f64,  24.0f64,  40.0f64,  51.0f64,  61.0f64],
	[12.0f64, 12.0f64, 14.0f64, 19.0f64,  26.0f64,  58.0f64,  60.0f64,  55.0f64],
	[14.0f64, 13.0f64, 16.0f64, 24.0f64,  40.0f64,  57.0f64,  69.0f64,  56.0f64],
	[14.0f64, 17.0f64, 22.0f64, 29.0f64,  51.0f64,  87.0f64,  80.0f64,  62.0f64],
	[18.0f64, 22.0f64, 37.0f64, 56.0f64,  68.0f64, 109.0f64, 103.0f64,  77.0f64],
	[24.0f64, 35.0f64, 55.0f64, 64.0f64,  81.0f64, 104.0f64, 113.0f64,  92.0f64],
	[49.0f64, 64.0f64, 78.0f64, 87.0f64, 103.0f64, 121.0f64, 120.0f64, 101.0f64],
	[72.0f64, 92.0f64, 95.0f64, 98.0f64, 112.0f64, 100.0f64, 103.0f64,  99.0f64]
];

/// Standard JPEG chrominance quantization table
#[rustfmt::skip]
const CHROMINANCE_QUANTIZATION_TABLE: [[f64; 8]; 8] = [
	[17.0f64, 18.0f64, 24.0f64, 47.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64],
	[18.0f64, 21.0f64, 26.0f64, 66.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64],
	[24.0f64, 26.0f64, 56.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64],
	[47.0f64, 66.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64],
	[99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64],
	[99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64],
	[99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64],
	[99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64, 99.0f64]
];


/// Quantizes DCT block with flexible quantization. Returns a signed value.
fn quantize_block(dct_block: &[[f64; 8]; 8], quantization_table: &[[f64; 8]; 8]) -> [[i8; 8]; 8] {
    let mut result = [[0; 8]; 8];
    for i in 0..8 {
        for j in 0..8 {
           result[i][j] = (dct_block[i][j] / quantization_table[i][j]).round() as i8;
        }
    }
    result
}

/// Entry-for-entry product of quantized block and quantization table.
fn dequantize_block(
    quantized_block: &[[i8; 8]; 8],
    quantization_table: &[[f64; 8]; 8],
) -> [[f64; 8]; 8] {
    let mut result = [[0.0; 8]; 8];
    for i in 0..8 {
        for j in 0..8 {
            result[i][j] = quantized_block[i][j] as f64 * quantization_table[i][j];
        }
    }
    result
}

/// Range quality from 0.1 to 0.03. (Lower is better)
fn quality_scaled_q_matrix(q_matrix: &[[f64; 8]; 8], quality: f64) -> [[f64; 8]; 8] {
    let factor = 0.03;
    q_matrix.map(|row| row.map(|x| x * factor))
}

const QUALITY_LEVEL: f64 = 90.0;

/// Process an entire YUV block for DCT and quantization
pub fn quantize_macroblock(block: &Macroblock) -> QuantizedMacroblock {
    let quality_scaled_luminance_q_matrix =
        quality_scaled_q_matrix(&LUMINANCE_QUANTIZATION_TABLE, QUALITY_LEVEL);
    let quality_scaled_chrominance_q_matrix =
        quality_scaled_q_matrix(&CHROMINANCE_QUANTIZATION_TABLE, QUALITY_LEVEL);

    QuantizedMacroblock {
        y0: quantize_block(&dct::dct2d(&block.y0), &quality_scaled_luminance_q_matrix),
        y1: quantize_block(&dct::dct2d(&block.y1), &quality_scaled_luminance_q_matrix),
        y2: quantize_block(&dct::dct2d(&block.y2), &quality_scaled_luminance_q_matrix),
        y3: quantize_block(&dct::dct2d(&block.y3), &quality_scaled_luminance_q_matrix),
        u: quantize_block(&dct::dct2d(&block.u), &quality_scaled_chrominance_q_matrix),
        v: quantize_block(&dct::dct2d(&block.v), &quality_scaled_chrominance_q_matrix),
    }
}

pub fn dequantize_macroblock(block: &QuantizedMacroblock) -> Macroblock {
    let quality_scaled_luminance_q_matrix =
    quality_scaled_q_matrix(&LUMINANCE_QUANTIZATION_TABLE, QUALITY_LEVEL);
    let quality_scaled_chrominance_q_matrix =
        quality_scaled_q_matrix(&CHROMINANCE_QUANTIZATION_TABLE, QUALITY_LEVEL);

    Macroblock {
        y0: dct::inverse_dct2d(&dequantize_block(&block.y0, &quality_scaled_luminance_q_matrix)),
        y1: dct::inverse_dct2d(&dequantize_block(&block.y1, &quality_scaled_luminance_q_matrix)),
        y2: dct::inverse_dct2d(&dequantize_block(&block.y2, &quality_scaled_luminance_q_matrix)),
        y3: dct::inverse_dct2d(&dequantize_block(&block.y3, &quality_scaled_luminance_q_matrix)),
        u: dct::inverse_dct2d(&dequantize_block(&block.u, &quality_scaled_chrominance_q_matrix)),
        v: dct::inverse_dct2d(&dequantize_block(&block.v, &quality_scaled_chrominance_q_matrix)),
    }
}

/// A quantized macroblock. Spans a 16x16 block of pixels,
/// with 4 8x8 blocks for Y and 1 8x8 block for U and V each.
#[derive(Default, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct QuantizedMacroblock {
    y0: [[i8; 8]; 8],
    y1: [[i8; 8]; 8],
    y2: [[i8; 8]; 8],
    y3: [[i8; 8]; 8],
    u: [[i8; 8]; 8],
    v: [[i8; 8]; 8],
}

#[derive(FromBytes, KnownLayout, IntoBytes, Immutable, Unaligned)]
#[repr(transparent)]
struct QuantizedZigZagBlock {
    data: [[i8; 8]; 8],
}

impl QuantizedZigZagBlock {
    // Implementation note: I'm quite happy with how the zero-copy cast works here.
    // Allows having nice zero-copy wrapper types that are generic over the mutability of the underlying data.
    // Otherwise, this would be split into QuantizedZigZagBlock<'a>(&'a ...) and QuantizedZigZagBlockMut<'a>(&'a mut ...).

    fn new_ref(data: &'_ [[i8; 8]; 8]) -> &'_ Self {
        Self::ref_from_bytes(data.as_bytes()).unwrap()
    }

    fn new_ref_mut(data: &'_ mut [[i8; 8]; 8]) -> &'_ mut Self {
        Self::mut_from_bytes(data.as_mut_bytes()).unwrap()
    }

    fn len(&self) -> usize {
        64
    }
}

// A miraculous zig-zag scan implementation by AI
#[rustfmt::skip]
const ZIGZAG_ORDER: [(usize, usize); 64] = [
    (0, 0), (0, 1), (1, 0), (2, 0), (1, 1), (0, 2), (0, 3), (1, 2),
    (2, 1), (3, 0), (4, 0), (3, 1), (2, 2), (1, 3), (0, 4), (0, 5),
    (1, 4), (2, 3), (3, 2), (4, 1), (5, 0), (6, 0), (5, 1), (4, 2),
    (3, 3), (2, 4), (1, 5), (0, 6), (0, 7), (1, 6), (2, 5), (3, 4),
    (4, 3), (5, 2), (6, 1), (7, 0), (7, 1), (6, 2), (5, 3), (4, 4),
    (3, 5), (2, 6), (1, 7), (2, 7), (3, 6), (4, 5), (5, 4), (6, 3),
    (7, 2), (7, 3), (6, 4), (5, 5), (4, 6), (3, 7), (4, 7), (5, 6),
    (6, 5), (7, 4), (7, 5), (6, 6), (5, 7), (6, 7), (7, 6), (7, 7),
];

impl Index<usize> for QuantizedZigZagBlock {
    type Output = i8;

    fn index(&self, index: usize) -> &Self::Output {
        let (i, j) = ZIGZAG_ORDER[index];
        &self.data[i][j]
    }
}

impl IndexMut<usize> for QuantizedZigZagBlock {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let (i, j) = ZIGZAG_ORDER[index];
        &mut self.data[i][j]
    }
}

/// Currently performs RLE encoding.
fn encode_quantized_block(block: &[[i8; 8]; 8], buf: &mut Vec<u8>) {
    let zig_zag_block = QuantizedZigZagBlock::new_ref(block);

    let mut index = 0;

    while index < zig_zag_block.len() {
        let current_element = zig_zag_block[index];
        let mut run_length = 1u8;
        index += 1;

        while index < zig_zag_block.len() {
            if zig_zag_block[index] == current_element && run_length < u8::MAX {
                run_length += 1;
                index += 1;
            } else {
                break;
            }
        }

        buf.push(current_element as u8);
        buf.push(run_length);
    }
}

pub fn encode_quantized_macroblock(quantized_macroblock: &QuantizedMacroblock, buf: &mut Vec<u8>) {
    for plane in &[
        quantized_macroblock.y0,
        quantized_macroblock.y1,
        quantized_macroblock.y2,
        quantized_macroblock.y3,
        quantized_macroblock.u,
        quantized_macroblock.v,
    ] {
        encode_quantized_block(plane, buf);
    }
}

/// Decodes a quantized block from the stream, returning the block and a pointer to the remaining data.
fn decode_quantized_block(data: &[u8]) -> ([[i8; 8]; 8], &[u8]) {
    log::trace!("Decode quantized block: given data slice of length {}", data.len());
    let mut block = [[0; 8]; 8];
    let quantized_block = QuantizedZigZagBlock::new_ref_mut(&mut block);

    let mut encoded_data_index = 0;
    let mut zig_zag_index = 0;

    // let mut zig_zag_out = Vec::new();
    // let mut s = String::new();

    while zig_zag_index < quantized_block.len() {
        let value = data[encoded_data_index];
        let run_length = data[encoded_data_index + 1];

        // s.push_str(&format!("{:02x}x{} ", value, run_length));

        encoded_data_index += 2;

        for _ in 0..run_length {
            quantized_block[zig_zag_index] = value as i8;
            // zig_zag_out.push(value as i8);
            zig_zag_index += 1;
        }
    }

    // log::trace!("{} -> {zig_zag_out:?}", s);

    (block, &data[encoded_data_index..])
}

/// Decodes a quantized macroblock from the stream, returning a pointer to the remaining data.
pub fn decode_quantized_macroblock(data: &[u8]) -> (QuantizedMacroblock, &[u8]) {
    let mut block = QuantizedMacroblock::default();
    let mut remaining = data;

    for plane in [
        &mut block.y0,
        &mut block.y1,
        &mut block.y2,
        &mut block.y3,
        &mut block.u,
        &mut block.v,
    ] {
        (*plane, remaining) = decode_quantized_block(remaining);
    }

    (block, remaining)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_quantization() {
        let block = Macroblock {
            y0: [[128; 8]; 8],
            y1: [[128; 8]; 8],
            y2: [[128; 8]; 8],
            y3: [[128; 8]; 8],
            u: [[128; 8]; 8],
            v: [[128; 8]; 8],
        };

        let quantized_block = quantize_macroblock(&block);
        let dequantized_block = dequantize_macroblock(&quantized_block);

        assert_eq!(block.y0, dequantized_block.y0);
        assert_eq!(block.y1, dequantized_block.y1);
        assert_eq!(block.y2, dequantized_block.y2);
        assert_eq!(block.y3, dequantized_block.y3);
        assert_eq!(block.u, dequantized_block.u);
        assert_eq!(block.v, dequantized_block.v);
    }

    #[test]
    fn test_macroblock_compression() {
        simplelog::SimpleLogger::init(simplelog::LevelFilter::Trace, simplelog::Config::default())
            .unwrap();

        let macroblock = Macroblock {
            y0: [
                [157, 157, 157, 157, 157, 156, 157, 156],
                [156, 156, 156, 155, 153, 154, 154, 155],
                [157, 158, 158, 157, 156, 156, 156, 155],
                [159, 159, 159, 159, 158, 158, 157, 156],
                [159, 158, 158, 159, 159, 159, 157, 158],
                [158, 157, 157, 159, 159, 158, 158, 157],
                [158, 158, 159, 159, 158, 158, 158, 157],
                [159, 159, 159, 159, 158, 158, 158, 157],
            ],
            y1: [
                [159, 160, 159, 159, 158, 158, 158, 158],
                [159, 159, 159, 158, 158, 158, 158, 158],
                [158, 158, 159, 158, 159, 158, 158, 158],
                [158, 157, 158, 158, 158, 158, 158, 158],
                [158, 158, 157, 157, 158, 158, 157, 157],
                [158, 157, 157, 158, 158, 158, 157, 157],
                [157, 157, 157, 157, 158, 158, 157, 157],
                [157, 157, 156, 156, 157, 157, 157, 157],
            ],
            y2: [
                [156, 157, 157, 156, 156, 156, 156, 155],
                [155, 155, 155, 154, 154, 154, 154, 154],
                [155, 156, 155, 156, 155, 155, 155, 155],
                [156, 157, 157, 157, 157, 157, 156, 156],
                [157, 158, 158, 158, 157, 157, 156, 156],
                [157, 158, 158, 157, 157, 156, 157, 156],
                [157, 157, 157, 157, 157, 157, 157, 156],
                [157, 158, 157, 157, 157, 157, 157, 156],
            ],
            y3: [
                [159, 157, 157, 157, 157, 158, 158, 157],
                [158, 158, 157, 157, 157, 157, 157, 156],
                [158, 158, 158, 158, 157, 157, 157, 156],
                [157, 157, 158, 158, 158, 157, 157, 156],
                [157, 157, 158, 158, 157, 157, 157, 156],
                [157, 157, 158, 157, 156, 157, 157, 156],
                [157, 157, 157, 156, 156, 156, 156, 156],
                [157, 156, 156, 156, 156, 156, 156, 156],
            ],
            u: [
                [131, 131, 131, 131, 132, 131, 132, 131],
                [128, 128, 129, 129, 129, 128, 128, 129],
                [128, 130, 128, 128, 128, 128, 129, 129],
                [128, 129, 128, 128, 128, 128, 129, 128],
                [129, 128, 128, 129, 129, 128, 128, 128],
                [129, 128, 128, 128, 128, 128, 128, 128],
                [128, 129, 128, 129, 129, 128, 128, 128],
                [128, 128, 128, 128, 129, 128, 129, 128],
            ],
            v: [
                [130, 129, 129, 129, 129, 129, 129, 129],
                [131, 130, 131, 131, 131, 131, 131, 130],
                [130, 130, 130, 131, 131, 130, 130, 130],
                [130, 131, 130, 130, 131, 131, 131, 131],
                [130, 130, 130, 130, 130, 130, 131, 130],
                [131, 130, 129, 129, 130, 131, 130, 130],
                [131, 131, 130, 131, 131, 130, 130, 131],
                [131, 131, 130, 130, 130, 130, 131, 130],
            ],
        };
        let quantized_macroblock = quantize_macroblock(&macroblock);
        log::info!("{:?}", quantized_macroblock);
        let mut rle_buf = Vec::new();
        encode_quantized_macroblock(&quantized_macroblock, &mut rle_buf);
        let (decoded_quantized_macroblock, remaining) = decode_quantized_macroblock(&rle_buf);
        assert!(remaining.is_empty());
        assert_eq!(quantized_macroblock, decoded_quantized_macroblock);
        let decoded_macroblock = dequantize_macroblock(&decoded_quantized_macroblock);

        log::info!("{:?}", decoded_macroblock);

        // check that all values within the decoded macroblock are within epsilon of the original
        let epsilon = 20;
        for (original, decoded) in macroblock.y0.iter().flatten().zip(decoded_macroblock.y0.iter().flatten()) {
            assert!((*original as i8 - *decoded as i8).abs() < epsilon);
        }
    }
}