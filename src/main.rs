use std::fmt;
use std::{ error::Error, path::PathBuf, process::Command, io::BufWriter };
use std::io::Write;
use clap::Parser;
use fltk::frame::Frame;
use fltk::input::FloatInput;
use fltk::{
    app,
    button::Button,
    dialog::*,
    enums::{ Event, Shortcut },
    group::Flex,
    menu::{ MenuFlag, SysMenuBar },
    prelude::*,
    utils::oncelock::Lazy,
    window::Window,
};
use fltk_theme::{ widget_themes, ThemeType, WidgetTheme };

static STATE: Lazy<app::GlobalState<State>> = Lazy::new(app::GlobalState::<State>::get);

#[derive(Debug, Parser)]
#[clap(name = "Video Editor", version = "0.1.0", author = "Gabriel Kaszewski")]
struct Args {
    #[clap(short, long)]
    input: Option<Vec<String>>,
    #[clap(short, long)]
    output: Option<String>,
    #[clap(short, long, default_value = "0.70")]
    volume: f32,
    #[clap(short, long, default_value = "false")]
    cli_mode: bool,
}

#[derive(Debug)]
struct MyError {
    message: String,
}

impl MyError {
    fn new(message: &str) -> MyError {
        MyError {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for MyError {}

impl From<std::io::Error> for MyError {
    fn from(error: std::io::Error) -> Self {
        MyError::new(&error.to_string())
    }
}

impl From<Box<dyn Error>> for MyError {
    fn from(error: Box<dyn Error>) -> Self {
        MyError::new(&error.to_string())
    }
}

#[derive(Debug)]
struct State {
    video_files: Vec<PathBuf>,
    volume: f32,
}

impl State {
    fn new() -> Self {
        Self {
            video_files: Vec::new(),
            volume: 0.7,
        }
    }
}

fn create_menu(menu_bar: &mut SysMenuBar) {
    menu_bar.set_frame(widget_themes::OS_BG_BOX);
    menu_bar.add(
        "&File/Import new videos...\t",
        Shortcut::Ctrl | 'i',
        MenuFlag::Normal,
        menu_callback
    );
}

fn menu_callback(menu_bar: &mut impl MenuExt) {
    if let Ok(menu_path) = menu_bar.item_pathname(None) {
        match menu_path.as_str() {
            "&File/Import new videos...\t" => {
                videos_import_callback();
            }
            _ => println!("Unknown menu item: {}", menu_path),
        }
    }
}

fn videos_import_callback() {
    let mut file_dialog = FileDialog::new(FileDialogType::BrowseMultiFile);
    file_dialog.set_option(FileDialogOptions::UseFilterExt);
    file_dialog.set_filter("Video Files\t*.{mp4,mkv}\n");
    file_dialog.show();
    let file_names = file_dialog.filenames();
    println!("Selected videos: {:?}", file_names);
    STATE.with(move |s| {
        s.video_files = file_names
            .iter()
            .map(|f| PathBuf::from(f))
            .collect();
    });
}

fn window_callback(_wind: &mut Window) {
    if app::event() == Event::Close {
        app::quit();
    }
}

fn remove_extension(path: &PathBuf) -> String {
    match path.file_stem() {
        Some(stem) => {
            let mut new_path = path.clone();
            new_path.set_file_name(stem);

            new_path.to_string_lossy().into_owned()
        }
        None => path.to_string_lossy().into_owned(),
    }
}

fn extract_and_adjust_audio(
    input_file: &PathBuf,
    track_index: usize,
    volume: f32
) -> Result<(PathBuf, Vec<PathBuf>), MyError> {
    let output_file = format!("{}_track-{}.ogg", remove_extension(input_file), track_index);
    let temp_files: Vec<PathBuf> = vec![PathBuf::from(output_file.clone())];

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-i")
        .arg(input_file)
        .args(&["-map", &format!("0:a:{}", track_index)])
        .arg("-af")
        .arg(&format!("volume={}", volume))
        .arg("-acodec")
        .arg("libvorbis")
        .arg(&output_file)
        .spawn()?
        .wait()?;

    if !status.success() {
        cleanup_temp_files(temp_files);
        return Err(
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to extract audio").into()
        );
    }

    Ok((PathBuf::from(output_file), temp_files))
}

fn merge_audio_tracks(audio_files: Vec<PathBuf>, output_file: PathBuf) -> Result<PathBuf, MyError> {
    let mut input_options: Vec<String> = Vec::new();
    for input_file in &audio_files {
        input_options.push("-i".to_string());
        input_options.push(input_file.to_string_lossy().to_string());
    }

    // Create the FFmpeg command
    let ffmpeg = Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .args(&input_options)
        .arg("-filter_complex")
        .arg("amerge")
        .arg("-ac")
        .arg(format!("{}", audio_files.len()))
        .arg("-c:a")
        .arg("libvorbis")
        .arg(&output_file)
        .spawn()?
        .wait()?;

    if !ffmpeg.success() {
        cleanup_temp_files(vec![output_file.clone()]);
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "Failed to merge audio").into());
    }

    Ok(output_file.clone())
}

fn concatenate_audio_files(
    audio_files: Vec<PathBuf>,
    output_file: PathBuf
) -> Result<PathBuf, MyError> {
    let temp_file = tempfile::NamedTempFile::new()?;
    let mut file = BufWriter::new(temp_file.reopen()?);

    for audio_file in &audio_files {
        writeln!(file, "file '{}'", audio_file.to_string_lossy())?;
    }

    file.flush()?;

    let ffmpeg = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-y")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(temp_file.path().to_string_lossy().to_string())
        .arg("-c")
        .arg("copy")
        .arg(&output_file)
        .spawn()?
        .wait()?;

    if !ffmpeg.success() {
        cleanup_temp_files(vec![output_file.clone()]);
        return Err(
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to concatenate audio").into()
        );
    }

    Ok(output_file.clone())
}

fn concatenate_video_files(
    video_files: Vec<PathBuf>,
    output_file: PathBuf
) -> Result<PathBuf, MyError> {
    let temp_file = tempfile::NamedTempFile::new()?;
    let mut file = BufWriter::new(temp_file.reopen()?);

    // Write file paths to the temporary file
    for video_file in video_files {
        writeln!(file, "file '{}'", video_file.to_string_lossy())?;
    }

    // Flush and finish writing to the temporary file
    file.flush()?;

    let ffmpeg = Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(temp_file.path().to_string_lossy().to_string())
        .arg("-c")
        .arg("copy")
        .arg("-an")
        .arg(&output_file)
        .spawn()?
        .wait()?;

    if !ffmpeg.success() {
        return Err(
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to concatenate video").into()
        );
    }

    Ok(output_file.clone())
}

fn combine_video_and_audio(
    video_file: PathBuf,
    audio_file: PathBuf,
    output_file: PathBuf
) -> Result<(), Box<dyn Error>> {
    let ffmpeg = Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-i")
        .arg(&video_file)
        .arg("-i")
        .arg(&audio_file)
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg("-strict")
        .arg("experimental")
        .arg(&output_file)
        .spawn()?
        .wait()?;

    if !ffmpeg.success() {
        return Err(
            std::io::Error
                ::new(std::io::ErrorKind::Other, "Failed to combine video and audio")
                .into()
        );
    }

    Ok(())
}

fn combine_and_encode_videos(
    input_files: Vec<PathBuf>,
    output_file: PathBuf,
    volume: f32
) -> Result<(), Box<dyn Error>> {
    let mut merged_audio_files: Vec<PathBuf> = Vec::new();
    let mut temp_files_to_delete: Vec<PathBuf> = Vec::new();

    for file_path in &input_files {
        let (background_audio, temp_bg_file) = extract_and_adjust_audio(file_path, 0, volume)?;
        let (voiceover_audio, temp_voice_file) = extract_and_adjust_audio(file_path, 1, 1.0)?;
        let merged_audio_path = PathBuf::from(
            format!("{}_merged_audio.ogg", remove_extension(file_path))
        );
        let temp_merged = merge_audio_tracks(
            vec![background_audio, voiceover_audio],
            merged_audio_path.clone()
        )?;
        merged_audio_files.push(merged_audio_path);
        temp_bg_file.iter().for_each(|f| temp_files_to_delete.push(f.clone()));
        temp_voice_file.iter().for_each(|f| temp_files_to_delete.push(f.clone()));
        temp_files_to_delete.push(temp_merged);
    }

    let concantenated_video_file = PathBuf::from(
        format!("{}_concatenated_video.mkv", remove_extension(&output_file))
    );
    let temp_concat_video = concatenate_video_files(
        input_files.clone(),
        concantenated_video_file.clone()
    )?;

    let final_audio_file = PathBuf::from(
        format!("{}_final_audio.ogg", remove_extension(&output_file))
    );
    let temp_concat_audio = concatenate_audio_files(merged_audio_files, final_audio_file.clone())?;

    temp_files_to_delete.push(temp_concat_video);
    temp_files_to_delete.push(temp_concat_audio);

    match combine_video_and_audio(concantenated_video_file, final_audio_file, output_file) {
        Ok(_) => {
            cleanup_temp_files(temp_files_to_delete);
            println!("Successfully combined videos");
        }
        Err(e) => {
            cleanup_temp_files(temp_files_to_delete);
            println!("Failed to combine videos: {}", e);
        }
    }

    Ok(())
}

fn cleanup_temp_files(temp_files: Vec<PathBuf>) {
    for temp_file in temp_files {
        if temp_file.exists() {
            println!("Deleting temp file: {:?}", temp_file);
            std::fs::remove_file(temp_file).expect("Failed to delete temp file");
        }
    }
}

fn combine_button_callback() {
    let videos = STATE.with(|s| s.video_files.clone());
    let vol: FloatInput = app::widget_from_id("volume_input").unwrap();
    let volume = vol.value().parse().unwrap_or(0.7);

    STATE.with(move |s| {
        s.volume = volume;
    });

    if videos.len() < 2 {
        return;
    }

    let mut file_dialog = FileDialog::new(FileDialogType::BrowseSaveFile);
    file_dialog.set_option(FileDialogOptions::UseFilterExt);
    file_dialog.set_filter("Video Files\t*.{mkv,mp4}\n");
    file_dialog.show();
    let output_file = file_dialog.filename();
    println!("Output file: {:?}", output_file);
    combine_and_encode_videos(videos, output_file, volume).expect("Failed to combine videos");
}

fn main() {
    let args = Args::parse();
    let input = args.input.unwrap_or(Vec::new());
    let output = args.output.unwrap_or("".to_string());

    if args.cli_mode {
        if input.len() == 0 || output == "" {
            println!("Please provide input and output files");
            return;
        }
        let output_ffmpeg = Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-version")
            .output()
            .expect("Failed to run ffmpeg");
        println!("ffmpeg version: {}", String::from_utf8_lossy(&output_ffmpeg.stdout));
        combine_and_encode_videos(
            input
                .iter()
                .map(|f| PathBuf::from(f))
                .collect(),
            PathBuf::from(output),
            args.volume
        ).expect("Failed to combine videos");
    } else {
        init_app();
    }
}

fn init_app() {
    let app = app::App::default();
    app::get_system_colors();
    let widget_theme = WidgetTheme::new(ThemeType::Aero);
    widget_theme.apply();

    let state = State::new();
    app::GlobalState::new(state);

    let mut wind = Window::new(100, 100, 400, 300, "Video editor");
    {
        let mut col = Flex::default_fill().column();
        col.begin();
        let mut menu_bar = SysMenuBar::default().with_size(wind.width(), 30);
        create_menu(&mut menu_bar);
        // create input box for volume
        let row = Flex::default_fill().row();
        Frame::default().with_size(100, 30).with_label("Volume:");
        FloatInput::default().with_size(100, 30).with_id("volume_input");
        row.end();
        let mut button = Button::default().with_size(100, 30).with_label("Combine");
        button.set_callback(move |_| combine_button_callback());
        wind.resizable(&col);
        col.fixed(&menu_bar, 30);
        col.end();
    }
    wind.end();
    wind.show();

    app.run().unwrap();
    wind.set_callback(window_callback);
}
