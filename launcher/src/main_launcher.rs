#![windows_subsystem = "windows"]

use ct_lib::bitmap::*;
use ct_lib::system;
use ct_lib::system::PathHelper;

use ct_lib::serde_derive::Deserialize;

use ct_lib::log;

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
#[cfg(debug_assertions)]
fn get_image_filepath_from_commandline() -> Option<String> {
    Some("examples/kers.png".to_string())
    // None
}

#[cfg(not(debug_assertions))]
fn get_image_filepath_from_commandline() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        args.last().cloned()
    } else {
        // NOTE: The first argument is the executable path
        None
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Low level bitmap helper function

#[repr(C)]
#[derive(Deserialize)]
struct PngPhys {
    pixel_per_unit_x: u32,
    pixel_per_unit_y: u32,
    unit_is_meter: u8,
}

type PngMetadataChunks = HashMap<String, Vec<u8>>;

fn png_extract_chunks_to_copy(image_filepath: &str) -> Result<PngMetadataChunks, String> {
    let file_bytes = std::fs::read(image_filepath)
        .map_err(|error| format!("Could not open file '{}' : {}", &image_filepath, error))?;
    let decoding_error_message = format!("Could not decode png file '{}'", &image_filepath);

    // Check header
    const PNG_HEADER: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if file_bytes.len() < PNG_HEADER.len() || file_bytes[0..8] != PNG_HEADER {
        return Err(decoding_error_message);
    }

    let mut chunks = HashMap::new();
    let mut chunk_begin_pos = PNG_HEADER.len();
    while chunk_begin_pos < file_bytes.len() {
        let chunk_data_length = {
            let mut deserializer = ct_lib::bincode::config();
            deserializer.big_endian();
            deserializer
                .deserialize::<u32>(&file_bytes[chunk_begin_pos..])
                .map_err(|error| format!("{} : {}", &decoding_error_message, error))?
                as usize
        };
        let chunk_complete_length = 4 + 4 + chunk_data_length + 4;

        let remaining_bytes = file_bytes.len() - chunk_begin_pos;
        if chunk_complete_length > remaining_bytes {
            return Err(decoding_error_message);
        }

        let chunk_type =
            std::str::from_utf8(&file_bytes[(chunk_begin_pos + 4)..(chunk_begin_pos + 8)])
                .map_err(|error| format!("{} : {}", &decoding_error_message, error))?;

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

    Ok(chunks)
}

fn load_bitmap(image_filepath: &str) -> Result<Bitmap, String> {
    if system::path_to_extension(&image_filepath).ends_with("png") {
        Bitmap::from_png_file(&image_filepath)
    } else {
        Err("We only support PNG images".to_string())
    }
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
    additional_chunks: &PngMetadataChunks,
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
    png_metadata: &PngMetadataChunks,
) -> Result<Option<f64>, String> {
    if let Some(metadata) = png_metadata.get("pHYs") {
        let info = {
            let mut deserializer = ct_lib::bincode::config();
            deserializer.big_endian();
            let info = deserializer
                .deserialize::<PngPhys>(metadata)
                .map_err(|error| {
                    format!(
                        "Could not read DPI metadata from '{}' : {}",
                        &image_filepath, error
                    )
                })?;

            info
        };

        if info.unit_is_meter != 1 {
            log::warn!(
                "Physical unit of image '{}' seems to be wrong, please re-export image",
                image_filepath,
            );
            return Ok(None);
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
            return Ok(None);
        }

        Ok(Some(ppi_x))
    } else {
        Ok(None)
    }
}

fn create_pattern_png(
    png_output_filepath: &str,
    image: &Bitmap,
    png_metadata: &PngMetadataChunks,
    result_pixel_width: i32,
    result_pixel_height: i32,
) -> Result<(), String> {
    let mut result_image = Bitmap::new(result_pixel_width as u32, result_pixel_height as u32);

    {
        let _timer = ct_lib::TimerScoped::new_scoped("Compositing", true);

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
        let _timer = ct_lib::TimerScoped::new_scoped("Writing", true);
        encode_png(&result_image, &png_output_filepath, &png_metadata).map_err(|error| {
            format!(
                "Could not write png file to '{}' : {}",
                png_output_filepath, error
            )
        })
    }
}

struct Image {
    pub filepath: String,
    pub bitmap: Bitmap,
    pub png_metadata: PngMetadataChunks,
    pub ppi: Option<f64>,
}

impl Image {
    fn new(filepath: &str) -> Result<Image, String> {
        let bitmap = load_bitmap(&filepath)?;
        let png_metadata = png_extract_chunks_to_copy(&filepath)?;
        let ppi = get_ppi_from_png_metadata(&filepath, &png_metadata)?;
        Ok(Image {
            filepath: filepath.to_string(),
            bitmap,
            png_metadata,
            ppi,
        })
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
    Row, Settings, Subscription, Text, TextInput,
};
const LABEL_SIZE_DEFAULT: u16 = 20;
const LABEL_SIZE_INVALID: u16 = 25;
const COLOR_DEFAULT: iced::Color = iced::Color::BLACK;
const COLOR_INVALID: iced::Color = iced::Color::from_rgb(1.0, 0.0, 0.0);
const DEFAULT_PPI: f64 = 72.0;

#[derive(Debug, Clone)]
enum GuiEvent {
    ChangedRepeatCountX(String),
    ChangedRepeatCountY(String),
    ChangedDimensionMillimeterX(String),
    ChangedDimensionMillimeterY(String),
    PressedStartButton,
    WindowEvent(iced_native::Event),
}

enum ProcessState {
    NoInput,
    Idle,
    Running,
    Finished,
}

fn get_image_width_height_pixel_per_mm(image: &Image) -> (f64, f64, f64) {
    let width = image.bitmap.width as f64;
    let height = image.bitmap.height as f64;
    let pixel_per_mm = pixel_per_inch_in_pixel_per_millimeter(image.ppi.unwrap_or(72.0));
    (width, height, pixel_per_mm)
}

fn get_output_pixel_width_height_filepath(
    image: &Image,
    repeat_x: f64,
    repeat_y: f64,
    dim_mm_x: f64,
    dim_mm_y: f64,
) -> (i32, i32, String) {
    let suffix_text = format!(
        "__{}x{}__{}x{}mm",
        pretty_print_float(repeat_x),
        pretty_print_float(repeat_y),
        pretty_print_float(dim_mm_x),
        pretty_print_float(dim_mm_y)
    );
    let png_output_filepath = get_image_output_filepath(&image.filepath, &suffix_text) + ".png";
    (
        (repeat_x * image.bitmap.width as f64).round() as i32,
        (repeat_y * image.bitmap.height as f64).round() as i32,
        png_output_filepath,
    )
}

struct RepeatyGui {
    image: Option<Image>,

    repeat_x: f64,
    repeat_y: f64,

    dim_mm_x: f64,
    dim_mm_y: f64,

    repeat_x_text: String,
    repeat_y_text: String,

    dim_mm_x_text: String,
    dim_mm_y_text: String,

    start_button_widget: button::State,

    repeat_x_widget: text_input::State,
    repeat_y_widget: text_input::State,

    dim_mm_x_widget: text_input::State,
    dim_mm_y_widget: text_input::State,

    process_state: ProcessState,

    current_error: Option<String>,
}

impl RepeatyGui {
    fn new() -> RepeatyGui {
        let mut result = RepeatyGui {
            image: None,
            repeat_x: 0.0,
            repeat_y: 0.0,
            dim_mm_x: 0.0,
            dim_mm_y: 0.0,
            repeat_x_text: "".to_string(),
            repeat_y_text: "".to_string(),
            dim_mm_x_text: "".to_string(),
            dim_mm_y_text: "".to_string(),
            start_button_widget: button::State::new(),
            repeat_x_widget: text_input::State::new(),
            repeat_y_widget: text_input::State::new(),
            dim_mm_x_widget: text_input::State::new(),
            dim_mm_y_widget: text_input::State::new(),
            process_state: ProcessState::Idle,

            current_error: None,
        };

        if let Some(image_filepath) = get_image_filepath_from_commandline() {
            result.load_image(&image_filepath);
        }

        result
    }

    fn load_image(&mut self, image_filepath: &str) {
        let image = {
            let image_result = Image::new(&image_filepath);
            if let Err(error_message) = Image::new(&image_filepath) {
                self.current_error = Some(error_message);
                return;
            }
            self.current_error = None;
            image_result.unwrap()
        };

        self.image = Some(image);

        if self.repeat_x <= 0.0
            || self.repeat_y <= 0.0
            || self.dim_mm_x <= 0.0
            || self.dim_mm_y <= 0.0
            || self.repeat_x.is_nan()
            || self.repeat_y.is_nan()
            || self.dim_mm_x.is_nan()
            || self.dim_mm_y.is_nan()
        {
            self.set_repeat_count_x(5.0);
            self.set_repeat_count_y(5.0);
            self.repeat_x_text = pretty_print_float(self.repeat_x);
            self.repeat_y_text = pretty_print_float(self.repeat_y);
        }
    }

    fn set_repeat_count_x(&mut self, value: f64) {
        if let Some(image) = &self.image {
            let (input_width, _input_height, pixel_per_mm) =
                get_image_width_height_pixel_per_mm(image);

            self.repeat_x = value;
            self.dim_mm_x = self.repeat_x * input_width / pixel_per_mm;
            self.dim_mm_x_text = pretty_print_float(self.dim_mm_x);

            self.process_state = ProcessState::Idle;
        }
    }

    fn set_repeat_count_y(&mut self, value: f64) {
        if let Some(image) = &self.image {
            let (_input_width, input_height, pixel_per_mm) =
                get_image_width_height_pixel_per_mm(image);

            self.repeat_y = value;
            self.dim_mm_y = self.repeat_y * input_height / pixel_per_mm;
            self.dim_mm_y_text = pretty_print_float(self.dim_mm_y);

            self.process_state = ProcessState::Idle;
        }
    }

    fn set_dimension_millimeter_x(&mut self, value: f64) {
        if let Some(image) = &self.image {
            let (input_width, _input_height, pixel_per_mm) =
                get_image_width_height_pixel_per_mm(image);

            self.dim_mm_x = value;
            self.repeat_x = self.dim_mm_x * pixel_per_mm / input_width;
            self.repeat_x_text = pretty_print_float(self.repeat_x);

            self.process_state = ProcessState::Idle;
        }
    }

    fn set_dimension_millimeter_y(&mut self, value: f64) {
        if let Some(image) = &self.image {
            let (_input_width, input_height, pixel_per_mm) =
                get_image_width_height_pixel_per_mm(image);

            self.dim_mm_y = value;
            self.repeat_y = self.dim_mm_y * pixel_per_mm / input_height;
            self.repeat_y_text = pretty_print_float(self.repeat_y);

            self.process_state = ProcessState::Idle;
        }
    }
}

impl Application for RepeatyGui {
    type Executor = iced::executor::Default;
    type Message = GuiEvent;
    type Flags = ();

    fn new(_flags: ()) -> (RepeatyGui, Command<Self::Message>) {
        (RepeatyGui::new(), Command::none())
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
                self.dim_mm_x_text = value_str;
                if let Some(value) = self.dim_mm_x_text.parse::<f64>().ok() {
                    self.set_dimension_millimeter_x(value);
                }
            }
            GuiEvent::ChangedDimensionMillimeterY(value_str) => {
                self.dim_mm_y_text = value_str;
                if let Some(value) = self.dim_mm_y_text.parse::<f64>().ok() {
                    self.set_dimension_millimeter_y(value);
                }
            }
            GuiEvent::PressedStartButton => {
                if let Some(image) = &self.image {
                    if self.repeat_x <= 0.0
                        || self.repeat_y <= 0.0
                        || self.dim_mm_x <= 0.0
                        || self.dim_mm_y <= 0.0
                        || self.repeat_x.is_nan()
                        || self.repeat_y.is_nan()
                        || self.dim_mm_x.is_nan()
                        || self.dim_mm_y.is_nan()
                    {
                        self.current_error =
                            Some("Some of the input values above are incorrect".to_string());
                    } else {
                        self.process_state = ProcessState::Running;

                        let (
                            output_image_pixel_width,
                            output_image_pixel_height,
                            png_output_filepath,
                        ) = get_output_pixel_width_height_filepath(
                            image,
                            self.repeat_x,
                            self.repeat_y,
                            self.dim_mm_x,
                            self.dim_mm_y,
                        );

                        if let Err(error_message) = create_pattern_png(
                            &png_output_filepath,
                            &image.bitmap,
                            &image.png_metadata,
                            output_image_pixel_width,
                            output_image_pixel_height,
                        ) {
                            self.current_error = Some(error_message);
                            self.process_state = ProcessState::Idle;
                        } else {
                            self.current_error = None;
                            self.process_state = ProcessState::Finished;
                        }
                    }
                }
            }
            GuiEvent::WindowEvent(window_event) => match window_event {
                iced_native::Event::Window(window_event) => match window_event {
                    iced_native::window::Event::FileDropped(filepath) => {
                        self.load_image(&filepath.to_string_borrowed());
                    }
                    _ => {}
                },
                iced_native::Event::Keyboard(key_event) => match key_event {
                    iced_native::input::keyboard::Event::Input { key_code, .. } => {
                        if key_code == iced_native::input::keyboard::KeyCode::Escape {
                            std::process::exit(0);
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
        }

        Command::none()
    }

    fn subscription(&self) -> Subscription<GuiEvent> {
        iced_native::subscription::events().map(GuiEvent::WindowEvent)
    }

    fn view(&mut self) -> Element<Self::Message> {
        let input_image_stats = if let Some(image) = &self.image {
            draw_input_image_stats(image)
        } else {
            Column::new()
                .spacing(10)
                .padding(20)
                .align_items(Align::Center)
                .width(FillPortion(1))
                .push(
                    Text::new("Please drag and drop an image into this window!")
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .size(30)
                        .color(iced::Color::from_rgb(0.8, 0.0, 0.1))
                        .width(FillPortion(1)),
                )
        };

        let output_image_stats = if let Some(image) = &self.image {
            let (output_image_pixel_width, output_image_pixel_height, png_output_filepath) =
                get_output_pixel_width_height_filepath(
                    image,
                    self.repeat_x,
                    self.repeat_y,
                    self.dim_mm_x,
                    self.dim_mm_y,
                );
            let ppi = image.ppi.unwrap_or(DEFAULT_PPI);
            draw_output_image_stats(
                &png_output_filepath,
                ppi,
                output_image_pixel_width,
                output_image_pixel_height,
            )
        } else {
            Column::new().spacing(10)
        };

        let input_fields = draw_textinput_fields(
            &self.repeat_x_text,
            &self.repeat_y_text,
            &self.dim_mm_x_text,
            &self.dim_mm_y_text,
            &mut self.repeat_x_widget,
            &mut self.repeat_y_widget,
            &mut self.dim_mm_x_widget,
            &mut self.dim_mm_y_widget,
        );

        let result = Column::new()
            .spacing(10)
            .padding(20)
            .align_items(Align::Center)
            .push(input_image_stats)
            .push(input_fields)
            .push(output_image_stats)
            .push(
                Button::new(&mut self.start_button_widget, Text::new("Create Pattern"))
                    .on_press(GuiEvent::PressedStartButton),
            );

        let result = if let Some(error_message) = &self.current_error {
            result.push(
                Text::new(format!("Error: {}", error_message))
                    .horizontal_alignment(iced::HorizontalAlignment::Center)
                    .size(30)
                    .color(iced::Color::from_rgb(0.8, 0.0, 0.1))
                    .width(FillPortion(1)),
            )
        } else {
            result
        };

        match self.process_state {
            ProcessState::NoInput => result
                .push(
                    Text::new("Please drag and drop an image into this window!")
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .size(30)
                        .color(iced::Color::from_rgb(0.8, 0.0, 0.1))
                        .width(FillPortion(1)),
                )
                .into(),
            ProcessState::Idle => result.into(),
            ProcessState::Running => result
                .push(
                    Text::new("Creating pattern - please wait!")
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .size(30)
                        .color(iced::Color::from_rgb(0.0, 0.0, 0.5))
                        .width(FillPortion(1)),
                )
                .into(),
            ProcessState::Finished => result
                .push(
                    Text::new("Finished creating pattern. Enjoy!")
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .size(30)
                        .color(iced::Color::from_rgb(0.0, 0.5, 0.0))
                        .width(FillPortion(1)),
                )
                .into(),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Draw Elements

fn get_label_size_and_color(text: &str) -> (iced::Color, u16) {
    if let Some(value) = text.parse::<f64>().ok() {
        if value > 0.0 {
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
fn pretty_print_float(value: f64) -> String {
    if (value - value.round()).abs() < 0.01 {
        format!("{:.0}", value.round())
    } else {
        format!("{:.2}", value)
    }
}

fn draw_input_image_stats<'a>(image: &Image) -> Column<'a, GuiEvent> {
    let ppi = image.ppi.unwrap_or(DEFAULT_PPI);
    let (ppi_label_color, ppi_label_size) = get_ppi_label_size_and_color(ppi);

    Column::new()
        .spacing(10)
        .padding(20)
        .align_items(Align::Center)
        .width(FillPortion(1))
        .push(
            Text::new("Input image:".to_string())
                .horizontal_alignment(iced::HorizontalAlignment::Left)
                .size(LABEL_SIZE_DEFAULT + 5)
                .color(COLOR_DEFAULT),
        )
        .push(
            Text::new(image.filepath.to_string())
                .size(LABEL_SIZE_DEFAULT)
                .color(COLOR_DEFAULT),
        )
        .push(
            Text::new(format!("{}x{}", image.bitmap.width, image.bitmap.height))
                .horizontal_alignment(iced::HorizontalAlignment::Left)
                .size(LABEL_SIZE_DEFAULT),
        )
        .push(
            Text::new(format!("DPI: {}", pretty_print_float(ppi)))
                .horizontal_alignment(iced::HorizontalAlignment::Left)
                .size(ppi_label_size)
                .color(ppi_label_color),
        )
}

fn draw_output_image_stats<'a>(
    filepath: &str,
    ppi: f64,
    pixel_width: i32,
    pixel_height: i32,
) -> Column<'a, GuiEvent> {
    let (ppi_label_color, ppi_label_size) = get_ppi_label_size_and_color(ppi);

    Column::new()
        .spacing(10)
        .padding(20)
        .align_items(Align::Center)
        .width(FillPortion(1))
        .push(
            Text::new("Output image:".to_string())
                .horizontal_alignment(iced::HorizontalAlignment::Left)
                .size(LABEL_SIZE_DEFAULT + 5)
                .color(COLOR_DEFAULT),
        )
        .push(
            Text::new(system::path_to_filename(&filepath))
                .size(LABEL_SIZE_DEFAULT)
                .color(COLOR_DEFAULT),
        )
        .push(
            Text::new(format!("{}x{}", pixel_width, pixel_height))
                .horizontal_alignment(iced::HorizontalAlignment::Left)
                .size(LABEL_SIZE_DEFAULT),
        )
        .push(
            Text::new(format!("DPI: {}", ppi))
                .horizontal_alignment(iced::HorizontalAlignment::Left)
                .size(ppi_label_size)
                .color(ppi_label_color),
        )
}

fn draw_textinput_field<'a, OnChangeEvent>(
    label_text: &str,
    input_text: &str,
    input_widget: &'a mut iced::text_input::State,
    on_change: OnChangeEvent,
) -> Row<'a, GuiEvent>
where
    OnChangeEvent: 'static + Fn(String) -> GuiEvent,
{
    let (label_color, label_size) = get_label_size_and_color(&input_text);
    let repeat_count_x_label = Text::new(label_text.to_string() + ": ")
        .size(label_size)
        .color(label_color)
        .width(FillPortion(1));
    let repeat_count_x_input = TextInput::new(input_widget, "", &input_text, on_change)
        .padding(15)
        .size(label_size)
        .width(FillPortion(1));

    Row::new()
        .padding(20)
        .align_items(Align::Center)
        .push(repeat_count_x_label)
        .push(repeat_count_x_input)
}

fn draw_textinput_fields<'a>(
    repeat_x_text: &str,
    repeat_y_text: &str,
    dim_x_text: &str,
    dim_y_text: &str,
    dim_x_widget: &'a mut iced::text_input::State,
    dim_y_widget: &'a mut iced::text_input::State,
    repeat_x_widget: &'a mut iced::text_input::State,
    repeat_y_widget: &'a mut iced::text_input::State,
) -> Column<'a, GuiEvent> {
    let repeat_x = draw_textinput_field(
        "Repeat horizontal",
        repeat_x_text,
        repeat_x_widget,
        GuiEvent::ChangedRepeatCountX,
    );
    let repeat_y = draw_textinput_field(
        "Repeat vertical",
        repeat_y_text,
        repeat_y_widget,
        GuiEvent::ChangedRepeatCountY,
    );
    let dim_x = draw_textinput_field(
        "Image width (mm)",
        dim_x_text,
        dim_x_widget,
        GuiEvent::ChangedDimensionMillimeterX,
    );
    let dim_y = draw_textinput_field(
        "Image height (mm)",
        dim_y_text,
        dim_y_widget,
        GuiEvent::ChangedDimensionMillimeterY,
    );

    let column_repeats = Column::new()
        .padding(10)
        .align_items(Align::Center)
        .width(FillPortion(1))
        .push(repeat_x)
        .push(repeat_y);
    let column_dimensions = Column::new()
        .padding(10)
        .align_items(Align::Center)
        .width(FillPortion(1))
        .push(dim_x)
        .push(dim_y);

    Column::new()
        .spacing(10)
        .padding(20)
        .align_items(Align::Center)
        .push(
            Row::new()
                .align_items(Align::Center)
                .push(column_repeats)
                .push(column_dimensions),
        )
}
