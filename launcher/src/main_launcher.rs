//#![windows_subsystem = "windows"]

use ct_lib::bitmap::*;
use ct_lib::system;
use ct_lib::system::PathHelper;

use ct_lib::serde_derive::Deserialize;

use ct_lib::log;

use gif::SetParameter;
use mtpng;
use rayon::prelude::*;
use winapi;

use std::{collections::HashMap, fs::File};

mod main_launcher_info;

////////////////////////////////////////////////////////////////////////////////////////////////////
// Unit conversion

fn millimeter_in_meter(millimeter: f64) -> f64 {
    millimeter * (1.0 / 1000.0)
}

fn meter_in_millimeter(meter: f64) -> f64 {
    meter * 1000.0
}

fn millimeter_in_inch(millimeter: f64) -> f64 {
    millimeter * (1.0 / 25.4)
}

fn inch_in_millimeter(inch: f64) -> f64 {
    inch * 25.4
}

fn meter_in_inch(meter: f64) -> f64 {
    millimeter_in_inch(meter_in_millimeter(meter))
}

fn inch_in_meter(inch: f64) -> f64 {
    millimeter_in_meter(inch_in_millimeter(inch))
}

fn pixel_per_millimeter_in_pixel_per_meter(pixels_per_millimeter: f64) -> f64 {
    pixels_per_millimeter / millimeter_in_meter(1.0)
}

fn pixel_per_meter_in_pixel_per_millimeter(pixels_per_meter: f64) -> f64 {
    pixels_per_meter / meter_in_millimeter(1.0)
}

fn pixel_per_meter_in_pixel_per_inch(pixels_per_meter: f64) -> f64 {
    pixels_per_meter / meter_in_inch(1.0)
}

fn pixel_per_inch_in_pixel_per_meter(pixels_per_inch: f64) -> f64 {
    pixels_per_inch / inch_in_meter(1.0)
}

fn pixel_per_millimeter_in_pixel_per_inch(pixels_per_millimeter: f64) -> f64 {
    pixels_per_millimeter / millimeter_in_inch(1.0)
}

fn pixel_per_inch_in_pixel_per_millimeter(pixels_per_inch: f64) -> f64 {
    pixels_per_inch / inch_in_millimeter(1.0)
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

fn get_ppi_from_png_metadata(
    image_filepath: &str,
    png_metadata: &HashMap<String, Vec<u8>>,
) -> Option<f64> {
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

        if info.unit_is_meter != 1 {
            log::warn!(
                "Physical unit of image '{}' seems to be wrong, please re-export image",
                image_filepath,
            );
            return None;
        }

        let ppi_x = pixel_per_meter_in_pixel_per_inch(info.pixel_per_unit_x as f64);
        let ppi_y = pixel_per_meter_in_pixel_per_inch(info.pixel_per_unit_y as f64);
        if info.pixel_per_unit_x != info.pixel_per_unit_y {
            log::warn!(
                "Horizontal and Vertical DPI of image '{}' do not match: {:.2}x{:.2}",
                image_filepath,
                ppi_x,
                ppi_y
            );
            return None;
        }

        Some(ppi_x)
    } else {
        None
    }
}

fn create_pattern_png(
    image_filepath: &str,
    image: &Bitmap,
    png_metadata: &HashMap<String, Vec<u8>>,
    repeat_count_x: i32,
    repeat_count_y: i32,
) {
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

fn init_logging(logfile_path: &str, loglevel: log::LevelFilter) -> Result<(), String> {
    let logfile = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(false)
        .open(logfile_path)
        .map_err(|error| format!("Could not create logfile at '{}' : {}", logfile_path, error))?;

    fern::Dispatch::new()
        .format(|out, message, record| out.finish(format_args!("{}: {}", record.level(), message)))
        .level(loglevel)
        .chain(std::io::stdout())
        .chain(logfile)
        .apply()
        .map_err(|error| format!("Could initialize logger: {}", error))?;

    Ok(())
}

fn main() {
    let logfile_path = system::path_join(&get_executable_dir(), "logging.txt");
    if let Err(error) = init_logging(&logfile_path, log::LevelFilter::Error) {
        show_messagebox(
            main_launcher_info::LAUNCHER_WINDOW_TITLE,
            &format!("Logger initialization failed : {}", error,),
            true,
        );
        std::process::abort();
    }

    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("{}", panic_info);
    }));

    RepeatyGui::run(Settings::default());
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// GUI

use iced::{
    button, text_input, Align, Application, Button, Column, Command, Element, Length::FillPortion,
    Row, Settings, Text, TextInput,
};

#[derive(Default)]
struct RepeatyGui {
    image: Option<Image>,

    pixels_per_millimeter: f64,
    image_pixel_width: f64,
    image_pixel_height: f64,

    repeat_x: f64,
    repeat_y: f64,

    dim_x: f64,
    dim_y: f64,

    repeat_x_text: String,
    repeat_y_text: String,

    dim_x_text: String,
    dim_y_text: String,

    start_button_widget: button::State,

    repeat_x_widget: text_input::State,
    repeat_y_widget: text_input::State,

    dim_x_widget: text_input::State,
    dim_y_widget: text_input::State,
}

#[derive(Debug, Clone)]
enum GuiEvent {
    ChangedRepeatCountX(String),
    ChangedRepeatCountY(String),
    ChangedDimensionMillimeterX(String),
    ChangedDimensionMillimeterY(String),
    PressedStartButton,
}

impl RepeatyGui {
    fn set_repeat_count_x(&mut self, value: f64) {
        self.repeat_x = value;
        self.dim_x = self.repeat_x * self.image_pixel_width / self.pixels_per_millimeter;
        self.dim_x_text = format!("{:.2}", self.dim_x);
    }

    fn set_repeat_count_y(&mut self, value: f64) {
        self.repeat_y = value;
        self.dim_y = self.repeat_y * self.image_pixel_height / self.pixels_per_millimeter;
        self.dim_y_text = format!("{:.2}", self.dim_y);
    }

    fn set_dimension_millimeter_x(&mut self, value: f64) {
        self.dim_x = value;
        self.repeat_x = self.dim_x * self.pixels_per_millimeter / self.image_pixel_width;
        self.repeat_x_text = format!("{:.2}", self.repeat_x);
    }

    fn set_dimension_millimeter_y(&mut self, value: f64) {
        self.dim_y = value;
        self.repeat_y = self.dim_y * self.pixels_per_millimeter / self.image_pixel_height;
        self.repeat_y_text = format!("{:.2}", self.repeat_y);
    }
}

const LABEL_SIZE_DEFAULT: u16 = 20;
const LABEL_SIZE_INVALID: u16 = 25;
const COLOR_DEFAULT: iced::Color = iced::Color::BLACK;
const COLOR_INVALID: iced::Color = iced::Color::from_rgb(1.0, 0.0, 0.0);
fn get_label_size_and_color(text: &str) -> (iced::Color, u16) {
    if let Some(value) = text.parse::<f64>().ok() {
        if value != 0.0 {
            return (COLOR_DEFAULT, LABEL_SIZE_DEFAULT);
        }
    }
    (COLOR_INVALID, LABEL_SIZE_INVALID)
}
fn get_ppi_label_size_and_color(ppi: f64) -> (iced::Color, u16) {
    if (ppi - 300.0).abs() <= 0.1 {
        (COLOR_DEFAULT, LABEL_SIZE_DEFAULT)
    } else {
        (COLOR_INVALID, LABEL_SIZE_INVALID)
    }
}

struct Image {
    filepath: String,
    bitmap: Bitmap,
    png_metadata: HashMap<String, Vec<u8>>,
    ppi: Option<f64>,
}

impl Image {
    fn new(filepath: &str) -> Image {
        let bitmap = load_bitmap(&filepath);
        let png_metadata = png_extract_chunks_to_copy(&filepath);
        let ppi = get_ppi_from_png_metadata(&filepath, &png_metadata);
        Image {
            filepath: filepath.to_string(),
            bitmap,
            png_metadata,
            ppi,
        }
    }
}

impl Application for RepeatyGui {
    type Executor = iced::executor::Default;
    type Message = GuiEvent;
    type Flags = ();

    fn new(_flags: ()) -> (RepeatyGui, Command<Self::Message>) {
        let image_filepath = get_image_filepath_from_commandline();
        let image = Image::new(&image_filepath);

        let mut result = RepeatyGui::default();
        result.pixels_per_millimeter =
            pixel_per_inch_in_pixel_per_millimeter(image.ppi.unwrap_or(72.0));
        result.image_pixel_width = image.bitmap.width as f64;
        result.image_pixel_height = image.bitmap.height as f64;
        result.image = Some(image);

        result.set_repeat_count_x(5.0);
        result.set_repeat_count_y(5.0);

        result.repeat_x_text = format!("{:.2}", result.repeat_x);
        result.repeat_y_text = format!("{:.2}", result.repeat_y);
        result.dim_x_text = format!("{:.2}", result.dim_x);
        result.dim_y_text = format!("{:.2}", result.dim_y);

        (result, Command::none())
    }

    fn title(&self) -> String {
        String::from(main_launcher_info::LAUNCHER_WINDOW_TITLE)
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            GuiEvent::ChangedRepeatCountX(value_str) => {
                self.repeat_x_text = value_str;
                if let Some(value) = self.repeat_x_text.parse::<f64>().ok() {
                    self.set_repeat_count_x(value);
                }
            }
            GuiEvent::ChangedRepeatCountY(value_str) => {
                self.repeat_y_text = value_str;
                if let Some(value) = self.repeat_y_text.parse::<f64>().ok() {
                    self.set_repeat_count_y(value);
                }
            }
            GuiEvent::ChangedDimensionMillimeterX(value_str) => {
                self.dim_x_text = value_str;
                if let Some(value) = self.dim_x_text.parse::<f64>().ok() {
                    self.set_dimension_millimeter_x(value);
                }
            }
            GuiEvent::ChangedDimensionMillimeterY(value_str) => {
                self.dim_y_text = value_str;
                if let Some(value) = self.dim_y_text.parse::<f64>().ok() {
                    self.set_dimension_millimeter_y(value);
                }
            }
            GuiEvent::PressedStartButton => todo!(),
        }

        Command::none()
    }

    fn view(&mut self) -> Element<Self::Message> {
        let (ppi_label_color, ppi_label_size) = get_ppi_label_size_and_color(
            pixel_per_millimeter_in_pixel_per_inch(self.pixels_per_millimeter),
        );
        let input_image_stats = Column::new()
            .padding(20)
            .align_items(Align::Center)
            .width(FillPortion(1))
            .push(
                Text::new(format!(
                    "Image pixels per inch: {:.2}",
                    pixel_per_millimeter_in_pixel_per_inch(self.pixels_per_millimeter)
                ))
                .size(ppi_label_size)
                .color(ppi_label_color)
                .width(FillPortion(1)),
            )
            .push(
                Text::new(format!(
                    "Image size (pixels): {:.0}x{:.0}",
                    self.image_pixel_width, self.image_pixel_height
                ))
                .size(LABEL_SIZE_DEFAULT)
                .width(FillPortion(1)),
            );

        let repeat_count_x = {
            let (label_color, label_size) = get_label_size_and_color(&self.repeat_x_text);
            let repeat_count_x_label = Text::new("Repeat horizontal: ")
                .size(label_size)
                .color(label_color)
                .width(FillPortion(1));
            let repeat_count_x_input = TextInput::new(
                &mut self.repeat_x_widget,
                "",
                &self.repeat_x_text,
                GuiEvent::ChangedRepeatCountX,
            )
            .padding(15)
            .size(label_size)
            .width(FillPortion(1));

            Row::new()
                .padding(20)
                .align_items(Align::Center)
                .push(repeat_count_x_label)
                .push(repeat_count_x_input)
        };

        let repeat_count_y = {
            let (label_color, label_size) = get_label_size_and_color(&self.repeat_y_text);
            let repeat_count_y_label = Text::new("Repeat vertical: ")
                .size(label_size)
                .color(label_color)
                .width(FillPortion(1));
            let repeat_count_y_input = TextInput::new(
                &mut self.repeat_y_widget,
                "",
                &self.repeat_y_text,
                GuiEvent::ChangedRepeatCountY,
            )
            .padding(15)
            .size(label_size)
            .width(FillPortion(1));

            Row::new()
                .padding(20)
                .align_items(Align::Center)
                .push(repeat_count_y_label)
                .push(repeat_count_y_input)
        };

        let dimension_mm_x = {
            let (label_color, label_size) = get_label_size_and_color(&self.dim_x_text);
            let dimension_mm_x_label = Text::new("Image width (mm): ")
                .size(label_size)
                .color(label_color)
                .width(FillPortion(1));
            let dimension_mm_x_input = TextInput::new(
                &mut self.dim_x_widget,
                "",
                &self.dim_x_text,
                GuiEvent::ChangedDimensionMillimeterX,
            )
            .padding(15)
            .size(label_size)
            .width(FillPortion(1));

            Row::new()
                .padding(20)
                .align_items(Align::Center)
                .push(dimension_mm_x_label)
                .push(dimension_mm_x_input)
        };
        let dimension_mm_y = {
            let (label_color, label_size) = get_label_size_and_color(&self.dim_y_text);
            let dimension_mm_y_label = Text::new("Image height (mm): ")
                .size(label_size)
                .color(label_color)
                .width(FillPortion(1));
            let dimension_mm_y_input = TextInput::new(
                &mut self.dim_y_widget,
                "",
                &self.dim_y_text,
                GuiEvent::ChangedDimensionMillimeterY,
            )
            .padding(15)
            .size(label_size)
            .width(FillPortion(1));

            Row::new()
                .padding(20)
                .align_items(Align::Center)
                .push(dimension_mm_y_label)
                .push(dimension_mm_y_input)
        };

        let column_repeats = Column::new()
            .padding(20)
            .align_items(Align::Center)
            .width(FillPortion(1))
            .push(repeat_count_x)
            .push(repeat_count_y);
        let column_dimensions = Column::new()
            .padding(20)
            .align_items(Align::Center)
            .width(FillPortion(1))
            .push(dimension_mm_x)
            .push(dimension_mm_y);

        Column::new()
            .padding(20)
            .align_items(Align::Center)
            .push(input_image_stats)
            .push(
                Row::new()
                    .padding(20)
                    .align_items(Align::Center)
                    .push(column_repeats)
                    .push(column_dimensions),
            )
            .push(
                Button::new(&mut self.start_button_widget, Text::new("Create Pattern"))
                    .on_press(GuiEvent::PressedStartButton),
            )
            .into()
    }
}
