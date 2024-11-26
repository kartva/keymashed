use sdl2::{self, audio, pixels::{PixelFormat, PixelFormatEnum}};

fn init() {
    let sdl_context = sdl2::init().unwrap();
    let audio_subsystem = sdl_context.audio().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem.window("rust-userspace", 800, 600)
        .position_centered()
        .build().unwrap();
    let renderer = window.into_canvas().present_vsync().accelerated().build().unwrap();

    let texture_creator = renderer.texture_creator();
    let (width, height) = (256, 256);
    let mut texture = texture_creator.create_texture_streaming(PixelFormatEnum::RGB888, width, height).unwrap();
    // texture.
}

