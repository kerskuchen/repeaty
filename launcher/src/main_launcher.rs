#![windows_subsystem = "windows"]

use ct_lib::bitmap::*;
use ct_lib::draw::*;
use ct_lib::font;
use ct_lib::font::BitmapFont;
use ct_lib::math::*;
use ct_lib::system;
use ct_lib::system::PathHelper;

use gif::SetParameter;
use indexmap::IndexMap;
use rayon::prelude::*;
use winapi;

use std::fs::File;

////////////////////////////////////////////////////////////////////////////////////////////////////
// Paths

fn get_executable_dir() -> String {
    if let Some(executable_path) = std::env::current_exe().ok() {
        system::path_without_filename(executable_path.to_string_borrowed())
    } else {
        ".".to_owned()
    }
}

/// Example:
/// exe path: "C:\bin\repeaty.exe"
/// imagepath: "D:\images\example_image.png"
/// output_dir_suffix: "__20x23__134x312mm"
///
/// This returns:
/// "C:\bin\example_image__20x23__134x312mm.png"
fn get_image_output_filepath(image_filepath: &str, image_suffix: &str) -> String {
    let output_dir_root = get_executable_dir();
    let image_filename = system::path_to_filename_without_extension(image_filepath) + image_suffix;
    system::path_join(&output_dir_root, &image_filename)
}

// NOTE: THIS IS FOR INTERNAL TESTING
#[cfg(debug_assertions)]
fn get_image_filepath_from_commandline() -> String {
    "examples/nathan.png".to_string()
}

#[cfg(not(debug_assertions))]
fn get_image_filepath_from_commandline() -> String {
    let mut args: Vec<String> = std::env::args().collect();

    // NOTE: The first argument is the executable path
    args.remove(0);

    assert_eq!(
        args.len(),
        1,
        "Please drag and drop one image onto the executable"
    );

    args
}

fn open_image(image_filepath: &str) -> Bitmap {
    if system::path_to_extension(&image_filepath).ends_with("gif") {
        bitmap_create_from_gif_file(&image_filepath)
    } else if system::path_to_extension(&image_filepath).ends_with("png") {
        let mut decoder = ct_lib::lodepng::Decoder::new();
        let image = decoder
            .decode_file(image_filepath)
            .expect(&format!("Could not decode png file '{}'", image_filepath));
        let info = decoder.info_png();
        dbg!(info);
        Bitmap::create_from_png_file(&image_filepath)
    } else {
        panic!("We only support GIF or PNG images");
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Low level bitmap helper function

fn bitmap_create_from_gif_file(image_filepath: &str) -> Bitmap {
    let mut decoder = gif::Decoder::new(
        File::open(image_filepath).expect(&format!("Cannot open file '{}'", image_filepath)),
    );
    decoder.set(gif::ColorOutput::RGBA);
    let mut decoder = decoder
        .read_info()
        .expect(&format!("Cannot decode file '{}'", image_filepath));
    let frame = decoder
        .read_next_frame()
        .expect(&format!(
            "Cannot decode first frame in '{}'",
            image_filepath
        ))
        .expect(&format!("No frame found in '{}'", image_filepath));
    let buffer: Vec<PixelRGBA> = frame
        .buffer
        .chunks_exact(4)
        .into_iter()
        .map(|color| PixelRGBA::new(color[0], color[1], color[2], color[3]))
        .collect();
    Bitmap::new_from_buffer(frame.width as u32, frame.height as u32, buffer)
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Main

#[cfg(windows)]
fn show_messagebox(caption: &str, message: &str, is_error: bool) {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use winapi::um::winuser::{MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK};

    let caption_wide: Vec<u16> = std::ffi::OsStr::new(caption)
        .encode_wide()
        .chain(once(0))
        .collect();
    let message_wide: Vec<u16> = std::ffi::OsStr::new(message)
        .encode_wide()
        .chain(once(0))
        .collect();

    unsafe {
        MessageBoxW(
            null_mut(),
            message_wide.as_ptr(),
            caption_wide.as_ptr(),
            MB_OK
                | if is_error {
                    MB_ICONERROR
                } else {
                    MB_ICONINFORMATION
                },
        )
    };
}

fn set_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let (message, location) = ct_lib::panic_message_split_to_message_and_location(panic_info);
        let final_message = format!("{}\n\nError occured at: {}", message, location);

        show_messagebox("Pixel Stitch Error", &final_message, true);

        // NOTE: This forces the other threads to shutdown as well
        std::process::abort();
    }));
}

fn main() {
    set_panic_hook();

    let image_filepath = get_image_filepath_from_commandline();
    let image = open_image(&image_filepath);

    show_messagebox("Repeaty", "Finished creating patterns. Enjoy!", false);
}
