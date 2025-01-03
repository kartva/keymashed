#![allow(dead_code)]

mod fft2d_dct {
    use fft2d::slice::dcst::{dct_2d, idct_2d};

    // From https://en.wikipedia.org/wiki/JPEG#JPEG_codec_example
    pub fn dct2d(block: &[[u8; 8]; 8]) -> [[f64; 8]; 8] {
        let mut slice = [0.0; 64];

        for i in 0..8 {
            for j in 0..8 {
                // convert to [0.0, 1.0]
                slice[i * 8 + j] = (block[i][j] as f64) / 255.0;
            }
        }

        dct_2d(8, 8, &mut slice);
        let mut out = [[0.0; 8]; 8];
        for i in 0..8 {
            for j in 0..8 {
                out[i][j] = slice[i * 8 + j];
            }
        }
        out
    }

    pub fn inverse_dct2d(block: &[[f64; 8]; 8]) -> [[u8; 8]; 8] {
        let mut slice = [0.0; 64];

        for i in 0..8 {
            for j in 0..8 {
                slice[i * 8 + j] = block[i][j];
            }
        }

        idct_2d(8, 8, &mut slice);
        let mut out = [[0; 8]; 8];
        let fft_coeff = 4.0 / (8.0 * 8.0);
        for i in 0..8 {
            for j in 0..8 {
                out[i][j] = ((slice[i * 8 + j] * fft_coeff).max(0.0).min(1.0) * 255.0) as u8;
            }
        }
        out
    }
}

mod naive_dct {
    fn dct_alpha(u: usize) -> f64 {
        if u == 0 {
            1.0 / (2.0f64).sqrt()
        } else {
            1.0
        }
    }

    // From https://en.wikipedia.org/wiki/JPEG#JPEG_codec_example
    pub fn dct2d(block: &[[u8; 8]; 8]) -> [[f64; 8]; 8] {
        let mut out = [[0.0; 8]; 8];
        for u in 0..8 {
            for v in 0..8 {
                let mut sum = 0.0;
                for x in 0..8 {
                    for y in 0..8 {
                        sum += dct_alpha(u)
                            * dct_alpha(v)
                            * (block[x][y] as f64 - 128.0)
                            * (std::f64::consts::PI * (2.0 * (x as f64) + 1.0) * (u as f64) / 16.0)
                                .cos()
                            * (std::f64::consts::PI * (2.0 * (y as f64) + 1.0) * (v as f64) / 16.0)
                                .cos();
                    }
                }
                out[u][v] = sum / 4.0;
            }
        }
        out
    }
    pub fn inverse_dct2d(block: &[[f64; 8]; 8]) -> [[u8; 8]; 8] {
        let mut out = [[0; 8]; 8];
        for x in 0..8 {
            for y in 0..8 {
                let mut sum = 0.0;
                for u in 0..8 {
                    for v in 0..8 {
                        sum += dct_alpha(u)
                            * dct_alpha(v)
                            * block[u][v]
                            * (std::f64::consts::PI * (2.0 * (x as f64) + 1.0) * (u as f64) / 16.0)
                                .cos()
                            * (std::f64::consts::PI * (2.0 * (y as f64) + 1.0) * (v as f64) / 16.0)
                                .cos();
                    }
                }
                out[x][y] = (sum / 4.0 + 128.0) as u8;
            }
        }
        out
    }
}

mod fixed_fast_dct {
    /*
     * Fast discrete cosine transform algorithms (Rust)
     *
     * Copyright (c) 2020 Project Nayuki. (MIT License)
     * https://www.nayuki.io/page/fast-discrete-cosine-transform-algorithms
     *
     * Permission is hereby granted, free of charge, to any person obtaining a copy of
     * this software and associated documentation files (the "Software"), to deal in
     * the Software without restriction, including without limitation the rights to
     * use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
     * the Software, and to permit persons to whom the Software is furnished to do so,
     * subject to the following conditions:
     * - The above copyright notice and this permission notice shall be included in
     *   all copies or substantial portions of the Software.
     * - The Software is provided "as is", without warranty of any kind, express or
     *   implied, including but not limited to the warranties of merchantability,
     *   fitness for a particular purpose and noninfringement. In no event shall the
     *   authors or copyright holders be liable for any claim, damages or other
     *   liability, whether in an action of contract, tort or otherwise, arising from,
     *   out of or in connection with the Software or the use or other dealings in the
     *   Software.
     */

    /*
     * Computes the scaled DCT type II on the given length-8 array in place.
     * The inverse of this function is inverse_transform(), except for rounding errors.
     */
    pub fn transform(vector: &mut [f64; 8]) {
        // Algorithm by Arai, Agui, Nakajima, 1988. For details, see:
        // https://web.stanford.edu/class/ee398a/handouts/lectures/07-TransformCoding.pdf#page=30
        let v0 = vector[0] + vector[7];
        let v1 = vector[1] + vector[6];
        let v2 = vector[2] + vector[5];
        let v3 = vector[3] + vector[4];
        let v4 = vector[3] - vector[4];
        let v5 = vector[2] - vector[5];
        let v6 = vector[1] - vector[6];
        let v7 = vector[0] - vector[7];

        let v8 = v0 + v3;
        let v9 = v1 + v2;
        let v10 = v1 - v2;
        let v11 = v0 - v3;
        let v12 = -v4 - v5;
        let v13 = (v5 + v6) * A[3];
        let v14 = v6 + v7;

        let v15 = v8 + v9;
        let v16 = v8 - v9;
        let v17 = (v10 + v11) * A[1];
        let v18 = (v12 + v14) * A[5];

        let v19 = -v12 * A[2] - v18;
        let v20 = v14 * A[4] - v18;

        let v21 = v17 + v11;
        let v22 = v11 - v17;
        let v23 = v13 + v7;
        let v24 = v7 - v13;

        let v25 = v19 + v24;
        let v26 = v23 + v20;
        let v27 = v23 - v20;
        let v28 = v24 - v19;

        vector[0] = (S[0] * v15) / 8.0f64.sqrt();
        vector[1] = (S[1] * v26) / 2.0;
        vector[2] = (S[2] * v21) / 2.0;
        vector[3] = (S[3] * v28) / 2.0;
        vector[4] = (S[4] * v16) / 2.0;
        vector[5] = (S[5] * v25) / 2.0;
        vector[6] = (S[6] * v22) / 2.0;
        vector[7] = (S[7] * v27) / 2.0;
    }
    /*
     * Computes the scaled DCT type III on the given length-8 array in place.
     * The inverse of this function is transform(), except for rounding errors.
     */
    pub fn inverse_transform(vector: &mut [f64; 8]) {
        vector[0] *= 8.0f64.sqrt();
        for i in 1..8 {
            vector[i] *= 2.0;
        }
        // A straightforward inverse of the forward algorithm
        let v15 = vector[0] / S[0];
        let v26 = vector[1] / S[1];
        let v21 = vector[2] / S[2];
        let v28 = vector[3] / S[3];
        let v16 = vector[4] / S[4];
        let v25 = vector[5] / S[5];
        let v22 = vector[6] / S[6];
        let v27 = vector[7] / S[7];

        let v19 = (v25 - v28) / 2.0;
        let v20 = (v26 - v27) / 2.0;
        let v23 = (v26 + v27) / 2.0;
        let v24 = (v25 + v28) / 2.0;

        let v7 = (v23 + v24) / 2.0;
        let v11 = (v21 + v22) / 2.0;
        let v13 = (v23 - v24) / 2.0;
        let v17 = (v21 - v22) / 2.0;

        let v8 = (v15 + v16) / 2.0;
        let v9 = (v15 - v16) / 2.0;

        let v18 = (v19 - v20) * A[5]; // Different from original
        let v12 = (v19 * A[4] - v18) / (A[2] * A[5] - A[2] * A[4] - A[4] * A[5]);
        let v14 = (v18 - v20 * A[2]) / (A[2] * A[5] - A[2] * A[4] - A[4] * A[5]);

        let v6 = v14 - v7;
        let v5 = v13 / A[3] - v6;
        let v4 = -v5 - v12;
        let v10 = v17 / A[1] - v11;

        let v0 = (v8 + v11) / 2.0;
        let v1 = (v9 + v10) / 2.0;
        let v2 = (v9 - v10) / 2.0;
        let v3 = (v8 - v11) / 2.0;

        vector[0] = (v0 + v7) / 2.0;
        vector[1] = (v1 + v6) / 2.0;
        vector[2] = (v2 + v5) / 2.0;
        vector[3] = (v3 + v4) / 2.0;
        vector[4] = (v3 - v4) / 2.0;
        vector[5] = (v2 - v5) / 2.0;
        vector[6] = (v1 - v6) / 2.0;
        vector[7] = (v0 - v7) / 2.0;
    }
    /*---- Tables of constants ----*/
    const S: [f64; 8] = [
        0.353553390593273762200422,
        0.254897789552079584470970,
        0.270598050073098492199862,
        0.300672443467522640271861,
        0.353553390593273762200422,
        0.449988111568207852319255,
        0.653281482438188263928322,
        1.281457723870753089398043,
    ];
    const A: [f64; 6] = [
        std::f64::NAN,
        0.707106781186547524400844,
        0.541196100146196984399723,
        0.707106781186547524400844,
        1.306562964876376527856643,
        0.382683432365089771728460,
    ];

    /* --- MIT License code ends here --- */

    pub fn dct2d(block: &[[u8; 8]; 8]) -> [[f64; 8]; 8] {
        let mut out = [[0.0; 8]; 8];
        // DCT over rows
        for i in 0..8 {
            out[i] = block[i].map(|x| x as f64);
            transform(&mut out[i]);
        }

        // DCT over columns
        for i in 0..8 {
            let mut column = [0.0; 8];
            for j in 0..8 {
                column[j] = out[j][i];
            }
            transform(&mut column);
            for j in 0..8 {
                out[j][i] = column[j];
            }
        }

        out
    }
    pub fn inverse_dct2d(block: &[[f64; 8]; 8]) -> [[u8; 8]; 8] {
        let mut output = [[0.0; 8]; 8];

        // IDCT over columns
        for i in 0..8 {
            let mut column = [0.0; 8];
            for j in 0..8 {
                column[j] = block[j][i];
            }
            inverse_transform(&mut column);
            for j in 0..8 {
                output[j][i] = column[j].round();
            }
        }
        
        // IDCT over rows
        for i in 0..8 {
            inverse_transform(&mut output[i]);
        }

        let mut rounded_output = [[0; 8]; 8];
        for i in 0..8 {
            for j in 0..8 {
                rounded_output[i][j] = output[i][j].round() as u8;
            }
        }

        rounded_output
    }
}

mod test_dct {

    #[test]
    fn test_dct_invertibility() {
        use super::fixed_fast_dct::{dct2d, inverse_dct2d};

        let block = [
            [52, 55, 61, 66, 70, 61, 64, 73],
            [63, 59, 55, 90, 109, 85, 69, 72],
            [62, 59, 68, 113, 144, 104, 66, 73],
            [63, 58, 71, 122, 154, 106, 70, 69],
            [67, 61, 68, 104, 126, 88, 68, 70],
            [79, 65, 60, 70, 77, 68, 58, 75],
            [85, 71, 64, 59, 55, 61, 65, 83],
            [87, 79, 69, 68, 65, 76, 78, 94],
        ];

        let dct = dct2d(&block);
        let inverse = inverse_dct2d(&dct);

        for i in 0..8 {
            for j in 0..8 {
                assert!((inverse[i][j] as i32 - block[i][j] as i32).abs() < 10);
            }
        }
    }
}

pub use fft2d_dct::{dct2d, inverse_dct2d};
