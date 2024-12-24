use core::f64;
use rayon::prelude::*;
use run_louder::{video::{
    dct::dct2d, dequantize_macroblock, quantize_macroblock, Macroblock, MacroblockWithPosition,
    MutableYUVFrame, YUVFrame, YUVFrameMacroblockIterator, YUYV422Sample,
}, wpm};
use sdl2::{pixels::{Color, PixelFormatEnum}, rect::Rect};
use std::{path::Path, sync::Mutex, time::Duration};
use zerocopy::IntoBytes;

const FILE_PATH: &str = "/home/kart/Downloads/Rick_Astley_Never_Gonna_Give_You_Up.mp4";

const GRID_PADDING: u32 = 10;

struct DisplayBuffer<'a> {
    texture: sdl2::render::Texture<'a>,
    rect: Rect,
}

impl<'a> DisplayBuffer<'a> {
    fn new(
        texture_creator: &'a sdl2::render::TextureCreator<sdl2::video::WindowContext>,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            texture: texture_creator
                .create_texture_streaming(PixelFormatEnum::YUY2, width, height)
                .unwrap(),
            rect: Rect::new(x, y, width, height),
        }
    }
}

fn calculate_quality(x: usize, _y: usize, video_width: usize, _video_height: usize) -> f64 {
    if x < video_width / 3 {
        0.04
    } else {
        let x_scale = (2 * video_width / 3) as f64;
        let x = x as f64 - (video_width as f64 - x_scale);
        0.04 + 0.3 * (x / x_scale)
    }

    // let width = width as f64;
    // let x = x as f64;
    // let scaling_factor = 1.0 / (1.0 + f64::consts::E.powf(((x - width / 2.0) / width) * 6.0));
    // 0.04 + 1.0 * scaling_factor
}

use video_rs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = video_rs::Decoder::new(Path::new(FILE_PATH)).unwrap();
    let (video_width, video_height) = decoder.size();

    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem
        .window(
            "JPEG Compression Stages",
            GRID_PADDING * 3 + video_width * 2,
            GRID_PADDING * 3 + video_height * 2,
        )
        .position_centered()
        .build()?;

    let mut canvas = window.into_canvas().accelerated().build()?;
    let texture_creator = canvas.texture_creator();

    // Create display buffers for each visualization stage
    let mut buffers = vec![
        DisplayBuffer::new(
            &texture_creator,
            GRID_PADDING as i32,
            GRID_PADDING as i32,
            video_width,
            video_height,
        ), // Original
        DisplayBuffer::new(
            &texture_creator,
            (GRID_PADDING * 2 + video_width) as i32,
            GRID_PADDING as i32,
            video_width,
            video_height,
        ), // DCT
        // DisplayBuffer::new(&texture_creator, (GRID_PADDING * 3 + video_width * 2) as i32, GRID_PADDING as i32,
        //     video_width, video_height),  // Quantization Matrix
        DisplayBuffer::new(
            &texture_creator,
            GRID_PADDING as i32,
            (GRID_PADDING * 2 + video_height) as i32,
            video_width,
            video_height,
        ), // Quantized DCT
        DisplayBuffer::new(
            &texture_creator,
            (GRID_PADDING * 2 + video_width) as i32,
            (GRID_PADDING * 2 + video_height) as i32,
            video_width,
            video_height,
        ), // Reconstructed
    ];

    let mut event_pump = sdl_context.event_pump()?;

    let mut typing_metrics = wpm::TypingMetrics::new();
    let mut frame_buf = Vec::with_capacity(video_width as usize * video_height as usize * 2);
    'running: loop {
        let start_time = std::time::Instant::now();

        for event in event_pump.poll_iter() {
            match event {
                sdl2::event::Event::Quit {..} => return Ok(()),
                sdl2::event::Event::KeyDown { keycode, repeat: false, timestamp: _, .. } => {
                    match keycode {
                        Some(k) => {
                            let ik = k.into_i32();
                            typing_metrics.receive_char_stroke(ik);
                        },
                        _ => {}
                    }
                },
                _ => {}
            }
        }

        let wpm = typing_metrics.calc_wpm();
        log::info!("WPM: {}", wpm);

        let quality = wpm::wpm_to_jpeg_quality(wpm);

        canvas.set_draw_color(wpm::wpm_to_sdl_color(wpm, Color::RED));
        canvas.clear();

        let frame = match decoder.decode_raw() {
            Ok(f) => f,
            Err(video_rs::Error::DecodeExhausted) => break 'running,
            Err(e) => panic!("{:?}", e),
        };

        frame_buf.clear();
        // push YUYV422 samples from an RGB888 image
        for y in 0..video_height {
            for x in 0..(video_width / 2) {
                let start_index = (y * video_width * 3 + x * 3 * 2) as usize;
                let rgb = &frame.data(0)[start_index..start_index + 6];
                let yuyv = YUYV422Sample::from_rgb24(rgb.try_into().unwrap());

                frame_buf.extend_from_slice(&yuyv.as_bytes());
            }
        }

        // Capture frame
        let frame: &[u8] = frame_buf.as_ref();
        let yuv_frame = YUVFrame::new(video_width as usize, video_height as usize, &frame);

        // Original frame
        buffers[0]
            .texture
            .with_lock(None, |buffer: &mut [u8], _pitch: usize| {
                buffer.copy_from_slice(frame);
            })?;

        fn scale_down_to_u8(val: f64) -> u8 {
            (val * 255.0).clamp(0.0, 255.0) as u8
        }

        // DCT frame
        buffers[1]
            .texture
            .with_lock(None, |buffer: &mut [u8], _pitch: usize| {
                let output_yuv_frame = Mutex::new(MutableYUVFrame::new(
                    video_width as usize,
                    video_height as usize,
                    buffer,
                ));

                YUVFrameMacroblockIterator::new(&yuv_frame)
                    .par_bridge()
                    .into_par_iter()
                    .map(|MacroblockWithPosition { block, x, y }| {
                        // Perform DCT; scale float-points values to 0-255 range
                        let Macroblock {
                            y0,
                            y1,
                            y2,
                            y3,
                            u,
                            v,
                        } = block;

                        let dct_block = Macroblock {
                            y0: dct2d(&y0).map(|row| row.map(scale_down_to_u8)),
                            y1: dct2d(&y1).map(|row| row.map(scale_down_to_u8)),
                            y2: dct2d(&y2).map(|row| row.map(scale_down_to_u8)),
                            y3: dct2d(&y3).map(|row| row.map(scale_down_to_u8)),
                            u: dct2d(&u).map(|row| row.map(scale_down_to_u8)),
                            v: dct2d(&v).map(|row| row.map(scale_down_to_u8)),
                        };

                        MacroblockWithPosition {
                            block: dct_block,
                            x,
                            y,
                        }
                    })
                    .for_each(|MacroblockWithPosition { block, x, y }| {
                        block.copy_to_yuv422_frame(&mut output_yuv_frame.lock().unwrap(), x, y);
                    });
            })?;

        // Quantization matrix
        // buffers[2].texture.with_lock(None, |buffer: &mut [u8], _pitch: usize| {
        //     let output_yuv_frame = Mutex::new(MutableYUVFrame::new(video_width as usize, video_height as usize, buffer));

        //     YUVFrameMacroblockIterator::new(&yuv_frame).par_bridge().into_par_iter().map(|MacroblockWithPosition { block: _, x, y }| {
        //         let quality = calculate_quality(x, y, video_width as usize, video_height as usize);

        //         let quality_scaled_luminance_q_matrix =
        //             quality_scaled_q_matrix(&LUMINANCE_QUANTIZATION_TABLE, quality);
        //         let quality_scaled_chrominance_q_matrix =
        //             quality_scaled_q_matrix(&CHROMINANCE_QUANTIZATION_TABLE, quality);
        //         let block = Macroblock {
        //             y0: quality_scaled_luminance_q_matrix.map(|row| row.map(scale_down_to_u8)),
        //             y1: quality_scaled_luminance_q_matrix.map(|row| row.map(scale_down_to_u8)),
        //             y2: quality_scaled_luminance_q_matrix.map(|row| row.map(scale_down_to_u8)),
        //             y3: quality_scaled_luminance_q_matrix.map(|row| row.map(scale_down_to_u8)),
        //             u: quality_scaled_chrominance_q_matrix.map(|row| row.map(scale_down_to_u8)),
        //             v: quality_scaled_chrominance_q_matrix.map(|row| row.map(scale_down_to_u8)),
        //         };
        //         MacroblockWithPosition { block, x, y }
        //     })
        //     .for_each(|MacroblockWithPosition { block, x, y }| {
        //         block.copy_to_yuv422_frame(&mut output_yuv_frame.lock().unwrap(), x, y);
        //     });
        // })?;

        // Quantized DCT frame
        buffers[2]
            .texture
            .with_lock(None, |buffer: &mut [u8], _pitch: usize| {
                let output_yuv_frame = Mutex::new(MutableYUVFrame::new(
                    video_width as usize,
                    video_height as usize,
                    buffer,
                ));

                YUVFrameMacroblockIterator::new(&yuv_frame)
                    .par_bridge()
                    .into_par_iter()
                    .map(|MacroblockWithPosition { block, x, y }| {
                        let quantized_block = quantize_macroblock(&block, quality);
                        let output_macroblock = Macroblock {
                            y0: quantized_block.y0.map(|row| row.map(|val| val as u8)),
                            y1: quantized_block.y1.map(|row| row.map(|val| val as u8)),
                            y2: quantized_block.y2.map(|row| row.map(|val| val as u8)),
                            y3: quantized_block.y3.map(|row| row.map(|val| val as u8)),
                            u: quantized_block.u.map(|row| row.map(|val| val as u8)),
                            v: quantized_block.v.map(|row| row.map(|val| val as u8)),
                        };

                        MacroblockWithPosition {
                            block: output_macroblock,
                            x,
                            y,
                        }
                    })
                    .for_each(|MacroblockWithPosition { block, x, y }| {
                        block.copy_to_yuv422_frame(&mut output_yuv_frame.lock().unwrap(), x, y);
                    });
            })?;

        // Reconstructed
        buffers[3]
            .texture
            .with_lock(None, |buffer: &mut [u8], _pitch: usize| {
                let output_yuv_frame = Mutex::new(MutableYUVFrame::new(
                    video_width as usize,
                    video_height as usize,
                    buffer,
                ));

                YUVFrameMacroblockIterator::new(&yuv_frame)
                    .par_bridge()
                    .into_par_iter()
                    .map(|MacroblockWithPosition { block, x, y }| {
                        // let quality =
                        //     calculate_quality(x, y, video_width as usize, video_height as usize);

                        let quantized_block = quantize_macroblock(&block, quality);
                        let dequantized_block = dequantize_macroblock(&quantized_block, quality);
                        let output_macroblock = Macroblock {
                            y0: dequantized_block.y0.map(|row| row.map(|val| val as u8)),
                            y1: dequantized_block.y1.map(|row| row.map(|val| val as u8)),
                            y2: dequantized_block.y2.map(|row| row.map(|val| val as u8)),
                            y3: dequantized_block.y3.map(|row| row.map(|val| val as u8)),
                            u: dequantized_block.u.map(|row| row.map(|val| val as u8)),
                            v: dequantized_block.v.map(|row| row.map(|val| val as u8)),
                        };

                        MacroblockWithPosition {
                            block: output_macroblock,
                            x,
                            y,
                        }
                    })
                    .for_each(|MacroblockWithPosition { block, x, y }| {
                        block.copy_to_yuv422_frame(&mut output_yuv_frame.lock().unwrap(), x, y);
                    });
            })?;

        // Render all buffers
        canvas.clear();
        for buffer in &buffers {
            canvas.copy(&buffer.texture, None, buffer.rect)?;
        }
        canvas.present();

        // delay to hit target FPS
        let target_latency = Duration::from_secs_f64(1.0 / decoder.frame_rate() as f64);
        let elapsed = start_time.elapsed();
        if elapsed < target_latency {
            std::thread::sleep(target_latency - elapsed);
        } else {
            log::warn!(
                "Sender took too long sending; overshot frame deadline by {} ms",
                (elapsed - target_latency).as_millis()
            );
        }
    }

    Ok(())
}
