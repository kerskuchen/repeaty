//#![windows_subsystem = "windows"]

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
// Unit conversion

fn millimeter_in_inch(millimeter: f64) -> f64 {
    millimeter * (1.0 / 2.54)
}

fn inch_in_millimeter(inch: f64) -> f64 {
    inch * 2.54
}

fn meter_in_inch(meter: f64) -> f64 {
    meter * (1.0 / 0.0254)
}

fn inch_in_meter(inch: f64) -> f64 {
    inch * 0.0254
}

fn ppm_to_ppi(pixels_per_meter: f64) -> f64 {
    pixels_per_meter * 0.0254
}

fn ppi_to_ppm(pixels_per_inch: f64) -> f64 {
    pixels_per_inch * (1.0 / 0.0254)
}

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

fn open_image(image_filepath: &str) -> (Bitmap, f64) {
    if system::path_to_extension(&image_filepath).ends_with("gif") {
        //bitmap_create_from_gif_file(&image_filepath)
        todo!()
    } else if system::path_to_extension(&image_filepath).ends_with("png") {
        let ppi = {
            let mut decoder = ct_lib::lodepng::Decoder::new();
            let _ = decoder
                .decode_file(image_filepath)
                .expect(&format!("Could not decode png file '{}'", image_filepath));
            let info = decoder.info_png();

            let ppi_x = ppm_to_ppi(info.phys_x as f64);
            let ppi_y = ppm_to_ppi(info.phys_y as f64);
            assert_eq!(
                info.phys_x, info.phys_y,
                "Horizontal and Vertical DPI of image '{}' do not match: {:.2}x{:.2}",
                image_filepath, ppi_x, ppi_y
            );

            let ppi = ppi_x;
            assert!(
                f64::abs(ppi - 300.0) < 0.1,
                "Expected and DPI of image '{}' to be 300 but got {:.2}",
                image_filepath,
                ppi
            );

            assert_eq!(
                info.phys_unit, 1,
                "Physical unit of image '{}' seems to be wrong, please re-export image",
                image_filepath,
            );

            ppi
        };

        (Bitmap::create_from_png_file(&image_filepath), ppi)
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

        show_messagebox("Repeaty Error", &final_message, true);

        // NOTE: This forces the other threads to shutdown as well
        std::process::abort();
    }));
}

fn main() {
    set_panic_hook();

    let image_filepath = get_image_filepath_from_commandline();
    let (image, ppi) = open_image(&image_filepath);

    let repeat_count_x = 3;
    let repeat_count_y = 3;

    let result_pixel_width = image.width * repeat_count_x;
    let result_pixel_height = image.height * repeat_count_y;

    let mut result_image = Bitmap::new(result_pixel_width as u32, result_pixel_height as u32);

    fn copy_pixels(
        input_image: &Bitmap,
        output_image_width: i32,
        output_image_buffer: &mut [PixelRGBA],
        start_index: usize,
    ) {
        for index in 0..output_image_buffer.len() {
            let output_x = (index + start_index) % output_image_width as usize;
            let output_y = (index + start_index) / output_image_width as usize;

            let input_x = output_x as i32 % input_image.width;
            let input_y = output_y as i32 % input_image.height;

            output_image_buffer[index] = input_image.get(input_x, input_y);
        }
    }

    {
        let _timer = ct_lib::TimerScoped::new_scoped("Compositing", false);

        let chunk_size = 4 * 1024 * 1024;
        let result_image_width = result_image.width;
        result_image
            .data
            .par_chunks_mut(chunk_size)
            .enumerate()
            .for_each(|(chunk_index, chunk)| {
                let start_index = chunk_index * chunk_size;
                copy_pixels(&image, result_image_width, chunk, start_index);
            });
    }

    {
        let _timer = ct_lib::TimerScoped::new_scoped("Writing", false);
        Bitmap::write_to_png_file(&result_image, "output.png");
    }

    let image_width_mm = show_messagebox("Repeaty", "Finished creating patterns. Enjoy!", false);
}
