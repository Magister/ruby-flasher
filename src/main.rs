#![windows_subsystem = "windows"]

use std::sync::{Arc, Mutex};

use fltk::{app::{self}, button::Button, enums::{self, Color, Font}, frame::Frame, group::Flex, image::IcoImage, input::{Input, InputType}, prelude::*, text::{StyleTableEntry, TextBuffer, TextDisplay}, window::Window};
use log::{error, info, LevelFilter};
use rust_embed::RustEmbed;
#[derive(RustEmbed)]
#[folder = "assets/"]
struct Asset;

mod flasher;

#[derive(Clone)]
struct DisplayState {
    disp: TextDisplay,
    text_buf: TextBuffer,
    style_buf: TextBuffer,
}

impl DisplayState {
    fn new() -> Self {
        let mut disp = TextDisplay::default();
        disp.wrap_mode(fltk::text::WrapMode::AtBounds, 0);
        let text_buf = TextBuffer::default();
        let style_buf = TextBuffer::default();
        disp.set_buffer(text_buf.clone());
        
        // Define style table (maps single-character style tags to colors)
        let styles = vec![
            StyleTableEntry { color: Color::Black, font: Font::Courier, size: 12 },  // 'A' = Black
            StyleTableEntry { color: Color::Red, font: Font::Courier, size: 12 },    // 'B' = Red
            StyleTableEntry { color: Color::Green, font: Font::Courier, size: 12 },  // 'C' = Green
            StyleTableEntry { color: Color::Blue, font: Font::Courier, size: 12 },   // 'D' = Blue
            StyleTableEntry { color: Color::Magenta, font: Font::Courier, size: 12 },// 'E' = Magenta
            StyleTableEntry { color: Color::Cyan, font: Font::Courier, size: 12 },   // 'F' = Cyan
            StyleTableEntry { color: Color::Dark3, font: Font::Courier, size: 12 },  // 'G' = Non-printable ASCII (Gray)
        ];

        // Apply styles
        disp.set_highlight_data(style_buf.clone(), styles);

        DisplayState { disp, text_buf, style_buf }
    }

    fn append_text(&mut self, text: &str) {
        let mut plain_text = String::new();
        let mut style_text = String::new(); // Style characters

        let mut current_style = 'A'; // Default: Black

        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            // ANSI Escape Sequences
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                chars.next(); // Skip '['
                let mut code = String::new();

                // Collect ANSI escape sequence (e.g., "0;31m")
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() || next == ';' || next == 'm' {
                        code.push(chars.next().unwrap());
                        if next == 'm' {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                // Parse ANSI codes
                let mut new_style = 'A'; // Default reset
                for part in code.trim_end_matches('m').split(';') {
                    match part {
                        "0" => new_style = 'A',  // Reset style
                        "31" => new_style = 'B', // Red
                        "32" => new_style = 'C', // Green
                        "34" => new_style = 'D', // Blue
                        "35" => new_style = 'E', // Magenta
                        "36" => new_style = 'F', // Cyan
                        _ => {} // Ignore unsupported codes
                    }
                }
                current_style = new_style; // Apply last parsed style

            } else {
                // ASCII Highlighting
                let ascii_style = if ch.is_ascii_control() {
                    'G' // Gray for non-printable ASCII
                } else if ch.is_ascii_graphic() {
                    current_style // Keep ANSI color
                } else {
                    'A' // Default Black
                };

                plain_text.push(ch);
                for _ in 0..ch.len_utf8() {
                    style_text.push(ascii_style);
                }
            }
        }

        self.text_buf.append(&plain_text);
        self.style_buf.append(&style_text);

        // Scroll to bottom
        let text_len = self.text_buf.length();
        self.disp.set_insert_position(text_len);
        let line_count = self.disp.count_lines(0, text_len, true) as i32;
        self.disp.scroll(line_count - 1, 0);

        app::awake();
        app::redraw();
    }

}

fn update_status(text_display: &mut DisplayState, status: &str) {
    info!("{}", status);
    text_display.append_text(format!("{}\n", status).as_str());
}

fn choose_file(soc: &str) -> Option<String> {
    let mut dialog = fltk::dialog::NativeFileChooser::new(fltk::dialog::NativeFileChooserType::BrowseFile);
    dialog.set_option(fltk::dialog::NativeFileChooserOptions::UseFilterExt);
    dialog.set_filter(format!("{}_rubyfpv_*.tgz", soc).as_str());
    match dialog.try_show() {
        Err(e) => {
            error!("error: {:?}", e);
            None
        },
        Ok(res) => match res {
            fltk::dialog::NativeFileChooserAction::Success => {
                let res = dialog.filename();
                match res.as_os_str().to_str() {
                    Some(res) => Some(res.to_owned()),
                    None => None,
                }
            }
            fltk::dialog::NativeFileChooserAction::Cancelled => None,
        }
    }
}

pub fn center() -> (i32, i32) {
    (
        (app::screen_size().0 / 2.0) as i32,
        (app::screen_size().1 / 2.0) as i32,
    )
}

#[derive(Copy, Clone)]
enum Message {
    PortChanged,
    IpChanged,
    DetectSoc,
    Flash,
}

#[derive(Default)]
struct State {
    soc: String,
    ip: String,
    port: String,
}

struct RubyFlasher {
    app: app::App,
    receiver: app::Receiver<Message>,
    display: Arc<Mutex<DisplayState>>,
    ip_input: Input,
    port_input: Input,
    btn_detect: Button,
    btn_flash: Button,
    state: Arc<Mutex<State>>,
}

impl RubyFlasher {
    pub fn new() -> Self {

        let app = app::App::default().with_scheme(app::Scheme::Gtk);
        let (s, receiver) = app::channel();

        let (x, y) = center();
        let (w, h) = (600, 400);
        let mut wind = Window::new(x - w/2, y - h/2, w, h, "RubyFPV simple flasher");
        wind.make_resizable(true);
        let bytes = Asset::get("ruby.ico").unwrap();
        let image = IcoImage::from_data(&bytes.data).unwrap();
        wind.set_icon(Some(image));

        let mut container = Flex::default().size_of_parent().column();
        container.set_margin(12);
    
        let mut flex = Flex::default().size_of_parent().row();
        container.fixed(&flex, 29);
        let frame = Frame::default()
                .with_label("IP address:")
                .with_align(enums::Align::Inside);
        flex.fixed(&frame, 70);
        let mut ip_input = Input::default();
        ip_input.emit(s, Message::IpChanged);
        let frame = Frame::default()
                .with_label("port:")
                .with_align(enums::Align::Inside);
        flex.fixed(&frame, 30);
        let mut port_input = Input::default().with_type(InputType::Int);
        port_input.set_value("22");
        port_input.emit(s, Message::PortChanged);

        flex.fixed(&port_input, 70);
    
        let mut btn_detect = Button::default().with_label("Identify SOC");
    
        let mut btn_flash = Button::default().with_label("Flash firmware...");
        btn_flash.deactivate();
    
        flex.end();
        let flex2 = Flex::default().size_of_parent().row();
        let display = Arc::new(Mutex::new(DisplayState::new()));
        flex2.end();
        container.end();
        wind.end();
        wind.show();
    
        btn_detect.emit(s, Message::DetectSoc);
        btn_flash.emit(s, Message::Flash);
    
        let state = Arc::new(Mutex::new(State { port: "22".to_string(), ..Default::default() }));
        Self {
            app,
            receiver,
            ip_input,
            port_input,
            display,
            state,
            btn_detect,
            btn_flash,
        }
    }

    pub fn run(mut self) {
        while self.app.wait() {
            if let Some(msg) = self.receiver.recv() {
                match msg {
                    Message::IpChanged => {
                        let mut state = self.state.lock().unwrap();
                        state.ip = self.ip_input.value();
                    },
                    Message::PortChanged => {
                        let mut state = self.state.lock().unwrap();
                        state.port = self.port_input.value();
                    },
                    Message::DetectSoc => {
                        let state = self.state.lock().unwrap();
                        let mut display = self.display.lock().unwrap();
                        let port: u16 = match state.port.parse() {
                            Ok(v) => v,
                            Err(e) => {
                                error!("error: {:?}", e);
                                update_status(&mut display, "Error: invalid port specified.");
                                continue;
                            }
                        };
                        self.btn_detect.deactivate();
                        let state_clone = self.state.clone();
                        let display_clone = self.display.clone();
                        let mut btn_detect_clone = self.btn_detect.clone();
                        let mut btn_flash_clone = self.btn_flash.clone();
                        tokio::spawn(async move {
                            let ip = state_clone.lock().unwrap().ip.clone();
                            match flasher::detect_soc(ip.as_str(), port, |msg| {
                                update_status(&mut display_clone.lock().unwrap(), msg);
                            }).await {
                                Ok(soc) => {
                                    state_clone.lock().unwrap().soc = soc;
                                    update_status(&mut display_clone.lock().unwrap(), "Done.");
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                },
                                Err(e) => {
                                    error!("error: {:?}", e);
                                    update_status(&mut display_clone.lock().unwrap(), format!("Error: {}", e).as_str());
                                    btn_detect_clone.activate();
                                    btn_flash_clone.deactivate();
                                }
                            }
                        });
                    },
                    Message::Flash => {
                        let state = self.state.lock().unwrap();
                        let mut display = self.display.lock().unwrap();
                        let path = match choose_file(state.soc.as_str()) {
                            Some(path) => path,
                            None => continue,
                        };
                        let port: u16 = match state.port.parse() {
                            Ok(v) => v,
                            Err(e) => {
                                error!("error: {:?}", e);
                                update_status(&mut display, "Error: invalid port specified.");
                                continue;
                            }
                        };
                        self.btn_detect.deactivate();
                        self.btn_flash.deactivate();

                        let state_clone = self.state.clone();
                        let display_clone = self.display.clone();
                        let mut btn_detect_clone = self.btn_detect.clone();
                        let mut btn_flash_clone = self.btn_flash.clone();
                        tokio::spawn(async move {
                            let ip = state_clone.lock().unwrap().ip.clone();
                            match flasher::flash(ip.as_str(), port, &path, |msg| {
                                update_status(&mut display_clone.lock().unwrap(), msg);
                            }).await {
                                Ok(_) => {
                                    update_status(&mut display_clone.lock().unwrap(), "Done.");
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                },
                                Err(e) => {
                                    error!("error: {:?}", e);
                                    update_status(&mut display_clone.lock().unwrap(), format!("Error: {}", e).as_str());
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                }
                            }
                        });
                    },
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    #[cfg(target_os = "windows")]
    {
        use winapi::um::wincon::{AttachConsole, ATTACH_PARENT_PROCESS};
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .init();

    let a = RubyFlasher::new();
    a.run();
}
