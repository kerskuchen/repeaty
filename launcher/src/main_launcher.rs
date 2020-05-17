//#![windows_subsystem = "windows"]

use ct_lib::bitmap::*;
use ct_lib::system;
use ct_lib::system::PathHelper;

use ct_lib::serde_derive::Deserialize;

use gif::SetParameter;
use mtpng;
use rayon::prelude::*;
use winapi;

use std::{collections::HashMap, fs::File};

mod main_launcher_info;

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
//#[cfg(debug_assertions)]
fn get_image_filepath_from_commandline() -> String {
    "examples/nathan.png".to_string()
}

/*
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

    args.first().unwrap().to_string()
}
*/

////////////////////////////////////////////////////////////////////////////////////////////////////
// Low level bitmap helper function

#[repr(C)]
#[derive(Deserialize)]
struct PngPhys {
    pixel_per_unit_x: u32,
    pixel_per_unit_y: u32,
    unit_is_meter: u8,
}

fn png_extract_chunks_to_copy(image_filepath: &str) -> HashMap<String, Vec<u8>> {
    let file_bytes =
        std::fs::read(image_filepath).expect(&format!("Could not open file '{}'", image_filepath));
    let decoding_error_message = format!("Could not decode png file '{}'", image_filepath);

    // Check header
    const PNG_HEADER: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    assert!(
        file_bytes.len() > PNG_HEADER.len(),
        "{}",
        decoding_error_message
    );
    assert!(file_bytes[0..8] == PNG_HEADER, "{}", decoding_error_message);

    let mut chunks = HashMap::new();
    let mut chunk_begin_pos = PNG_HEADER.len();
    while chunk_begin_pos < file_bytes.len() {
        let chunk_data_length = {
            let mut deserializer = ct_lib::bincode::config();
            deserializer.big_endian();
            deserializer
                .deserialize::<u32>(&file_bytes[chunk_begin_pos..])
                .expect(&decoding_error_message) as usize
        };
        let chunk_complete_length = 4 + 4 + chunk_data_length + 4;

        let remaining_bytes = file_bytes.len() - chunk_begin_pos;
        assert!(
            chunk_complete_length <= remaining_bytes,
            "{}",
            decoding_error_message
        );

        let chunk_type =
            std::str::from_utf8(&file_bytes[(chunk_begin_pos + 4)..(chunk_begin_pos + 8)])
                .expect(&decoding_error_message);

        let keep_chunk = match chunk_type {
            "cHRM" => true,
            "gAMA" => true,
            "iCCP" => true,
            "pHYs" => true,
            "sRGB" => true,
            _ => false,
        };
        let chunk_data_pos = chunk_begin_pos + 4 + 4;
        if keep_chunk {
            chunks.insert(
                chunk_type.to_string(),
                file_bytes[chunk_data_pos..(chunk_data_pos + chunk_data_length)].to_vec(),
            );
        }
        chunk_begin_pos += chunk_complete_length;
    }

    chunks
}

fn load_bitmap(image_filepath: &str) -> Bitmap {
    if system::path_to_extension(&image_filepath).ends_with("gif") {
        //bitmap_create_from_gif_file(&image_filepath)
        todo!()
    } else if system::path_to_extension(&image_filepath).ends_with("png") {
        Bitmap::create_from_png_file(&image_filepath)
    } else {
        panic!("We only support GIF or PNG images");
    }
}

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

fn encode_png(
    image: &Bitmap,
    output_filepath: &str,
    additional_chunks: &HashMap<String, Vec<u8>>,
) -> Result<(), std::io::Error> {
    let image_width = image.width as u32;
    let image_height = image.height as u32;
    let bytes = unsafe {
        let len = image.data.len() * std::mem::size_of::<u32>();
        let ptr = image.data.as_ptr() as *const u8;
        std::slice::from_raw_parts(ptr, len)
    };

    let options = mtpng::encoder::Options::new();
    let writer = File::create(output_filepath)?;
    let mut encoder = mtpng::encoder::Encoder::new(writer, &options);

    let mut header = mtpng::Header::new();
    header.set_size(image_width, image_height)?;
    header.set_color(mtpng::ColorType::TruecolorAlpha, 8)?;

    encoder.write_header(&header)?;
    for (chunktype, chunk) in additional_chunks {
        encoder.write_chunk(chunktype.as_bytes(), chunk)?;
    }
    encoder.write_image_rows(bytes)?;
    encoder.finish()?;

    Ok(())
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
    let image = load_bitmap(&image_filepath);
    let png_metadata = png_extract_chunks_to_copy(&image_filepath);
    let ppi = {
        if let Some(metadata) = png_metadata.get("pHYs") {
            let info = {
                let mut deserializer = ct_lib::bincode::config();
                deserializer.big_endian();
                let info = deserializer
                    .deserialize::<PngPhys>(metadata)
                    .expect(&format!(
                        "Could not read DPI metadata from '{}'",
                        &image_filepath
                    ));

                info
            };

            assert_eq!(
                info.unit_is_meter, 1,
                "Physical unit of image '{}' seems to be wrong, please re-export image",
                image_filepath,
            );

            let ppi_x = ppm_to_ppi(info.pixel_per_unit_x as f64);
            let ppi_y = ppm_to_ppi(info.pixel_per_unit_y as f64);
            assert_eq!(
                info.pixel_per_unit_x, info.pixel_per_unit_y,
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

            Some(ppi)
        } else {
            None
        }
    };

    let repeat_count_x = 1;
    let repeat_count_y = 1;

    let result_pixel_width = image.width * repeat_count_x;
    let result_pixel_height = image.height * repeat_count_y;

    let mut result_image = Bitmap::new(result_pixel_width as u32, result_pixel_height as u32);

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

        let png_output_filepath = get_image_output_filepath(
            &image_filepath,
            &format!("__{}x{}", repeat_count_x, repeat_count_y),
        );
        let png_output_filepath = "output.png";
        encode_png(&result_image, &png_output_filepath, &png_metadata).expect(&format!(
            "Could not write png file to '{}'",
            png_output_filepath
        ));
    }

    let image_width_mm = show_messagebox(
        main_launcher_info::LAUNCHER_WINDOW_TITLE,
        "Finished creating pattern. Enjoy!",
        false,
    );

    Hello::run(Settings::default())
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// GUI

use iced::{button, Align, Button, Column, Element, Sandbox, Settings, Text};

#[derive(Default)]
struct Hello {
    value: i32,
    button_plus: button::State,
    button_minus: button::State,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    PressedPlus,
    PressedMinus,
}

impl Sandbox for Hello {
    type Message = Message;

    fn new() -> Hello {
        Hello::default()
    }

    fn title(&self) -> String {
        String::from(main_launcher_info::LAUNCHER_WINDOW_TITLE)
    }

    fn update(&mut self, message: Self::Message) {
        match message {
            Message::PressedPlus => {
                self.value += 1;
            }
            Message::PressedMinus => {
                self.value -= 1;
            }
        }
    }

    fn view(&mut self) -> Element<Self::Message> {
        //Text::new("Hello, world!").into()
        Column::new()
            .padding(20)
            .align_items(Align::Center)
            .push(
                Button::new(&mut self.button_plus, Text::new("Increment"))
                    .on_press(Message::PressedPlus),
            )
            .push(Text::new(self.value.to_string()).size(50))
            .push(
                Button::new(&mut self.button_minus, Text::new("Decrement"))
                    .on_press(Message::PressedMinus),
            )
            .into()
    }
}
