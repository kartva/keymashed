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

pub use fft2d_dct::{dct2d, inverse_dct2d};