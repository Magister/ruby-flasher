use fltk::{app::{self}, button::Button, enums::{self, Color, Font}, frame::Frame, group::Flex, input::{Input, InputType}, prelude::*, text::{StyleTableEntry, TextBuffer, TextDisplay}, window::Window};
use log::{error, LevelFilter};

mod flasher;

fn replace_last_line(buf: &mut TextBuffer, new_text: &str) {
    let text = buf.text();
    if let Some(pos) = text.rfind('\n') {
        buf.replace((pos + 1) as i32, buf.length(), new_text);
    } else {
        // If no newline, replace the whole text
        buf.set_text(new_text);
    }
}

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
            StyleTableEntry { color: Color::Black, font: Font::Helvetica, size: 12 },  // 'A' = Black
            StyleTableEntry { color: Color::Red, font: Font::Helvetica, size: 12 },    // 'B' = Red
            StyleTableEntry { color: Color::Green, font: Font::Helvetica, size: 12 },  // 'C' = Green
            StyleTableEntry { color: Color::Blue, font: Font::Helvetica, size: 12 },   // 'D' = Blue
            StyleTableEntry { color: Color::Magenta, font: Font::Helvetica, size: 12 },// 'E' = Magenta
            StyleTableEntry { color: Color::Cyan, font: Font::Helvetica, size: 12 },   // 'F' = Cyan
            StyleTableEntry { color: Color::Dark3, font: Font::Helvetica, size: 12 },  // 'G' = Non-printable ASCII (Gray)
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
                match code.as_str() {
                    "0m" => current_style = 'A',  // Reset to Black
                    "31m" => current_style = 'B', // Red
                    "32m" => current_style = 'C', // Green
                    "34m" => current_style = 'D', // Blue
                    "35m" => current_style = 'E', // Magenta
                    "36m" => current_style = 'F', // Cyan
                    _ => {}
                }
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
                style_text.push(ascii_style);
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
    text_display.append_text(format!("{}\n", status).as_str());
}

fn choose_file() -> Option<String> {
    let mut dialog = fltk::dialog::NativeFileChooser::new(fltk::dialog::NativeFileChooserType::BrowseFile);
    dialog.set_option(fltk::dialog::NativeFileChooserOptions::UseFilterExt);
    dialog.set_filter("*ruby*.tgz");
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

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .init();

    let app = app::App::default().with_scheme(app::Scheme::Gtk);
    let (x, y) = center();
    let (w, h) = (600, 400);
    let mut wind = Window::new(x - w/2, y - h/2, w, h, "RubyFPV simple flasher");
    wind.make_resizable(true);
    let mut container = Flex::default().size_of_parent().column();
    container.set_margin(12);

    let mut flex = Flex::default().size_of_parent().row();
    container.fixed(&flex, 29);
    let frame = Frame::default()
            .with_label("IP address:")
            .with_align(enums::Align::Inside);
    flex.fixed(&frame, 70);
    let ip_field = Input::default();
    let frame = Frame::default()
            .with_label("port:")
            .with_align(enums::Align::Inside);
    flex.fixed(&frame, 30);
    let mut port_field = Input::default().with_type(InputType::Int);
    port_field.set_value("22");
    flex.fixed(&port_field, 70);

    let mut btn_flash = Button::default().with_label("Flash firmware...");
    flex.end();
    let flex2 = Flex::default().size_of_parent().row();
    let display_state = DisplayState::new();
    flex2.end();
    container.end();
    wind.end();
    wind.show();

    btn_flash.set_callback(move |btn_self| {
        let path = match choose_file() {
            Some(path) => path,
            None => return,
        };
        let mut buffer = display_state.clone();
        let ip_addr = ip_field.value();
        let port: u16 = match port_field.value().parse() {
            Ok(v) => v,
            Err(e) => {
                error!("error: {:?}", e);
                update_status(&mut buffer, "Error: invalid port specified.");
                return;
            }
        };
        btn_self.deactivate();
        let mut btn_ref = btn_self.clone();
        tokio::spawn(async move {
            match flasher::flash(&ip_addr, port, &path, |msg| {
                update_status(&mut buffer, msg);
            }).await {
                Ok(_) => {
                    update_status(&mut buffer, "Done.");
                    btn_ref.activate();
                },
                Err(e) => {
                    error!("error: {:?}", e);
                    update_status(&mut buffer, format!("Error: {}", e).as_str());
                    btn_ref.activate();
                }
            }
        });
    });
    app.run().unwrap();
}
