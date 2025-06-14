#![windows_subsystem = "windows"]

#[cfg(not(target_os = "windows"))]
use std::process::Command;

use std::sync::{Arc, Mutex};

use fltk::{
    app,
    button::Button,
    enums::{self, Color, Font},
    frame::Frame,
    group::Flex,
    image::IcoImage,
    input::{Input, InputType},
    menu::MenuButton,
    prelude::*,
    text::{StyleTableEntry, TextBuffer, TextDisplay},
    window::Window,
};
use fltk_theme::{color_themes, ColorTheme};
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

fn is_dark_mode() -> bool {
    // macOS-specific check
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("defaults")
            .args(["read", "NSGlobalDomain", "AppleInterfaceStyle"])
            .output();
        match output {
            Ok(output) => {
                let result = String::from_utf8_lossy(&output.stdout);
                result.trim() == "Dark"
            }
            Err(_) => false, // Default to light mode if check fails
        }
    }

    // Windows-specific check
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        let hkey = RegKey::predef(HKEY_CURRENT_USER);
        match hkey.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        {
            Ok(key) => {
                match key.get_value::<u32, _>("AppsUseLightTheme") {
                    Ok(value) => value == 0, // 0 means dark mode, 1 means light mode
                    Err(_) => false,         // Default to light mode if value not found
                }
            }
            Err(_) => false, // Default to light mode if registry key not accessible
        }
    }

    // Linux-specific check (GNOME example)
    #[cfg(target_os = "linux")]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();

        // KDE check
        if desktop.contains("kde") || desktop.contains("plasma") {
            if let Ok(output) = Command::new("kreadconfig5")
                .args([
                    "--file",
                    "kdeglobals",
                    "--group",
                    "KDE",
                    "--key",
                    "widgetStyle",
                ])
                .output()
            {
                let result = String::from_utf8_lossy(&output.stdout);
                return result.to_lowercase().contains("dark");
            }
        }

        // GNOME check
        if desktop.contains("gnome") || desktop.contains("gtk") {
            if let Ok(output) = Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
                .output()
            {
                let result = String::from_utf8_lossy(&output.stdout);
                return result.to_lowercase().contains("dark");
            }
        }

        // Fallback: check QT_STYLE_OVERRIDE or GTK_THEME
        std::env::var("QT_STYLE_OVERRIDE")
            .map(|theme| theme.to_lowercase().contains("dark"))
            .unwrap_or(false)
            || std::env::var("GTK_THEME")
                .map(|theme| theme.to_lowercase().contains("dark"))
                .unwrap_or(false)
    }

    // Fallback for unsupported platforms
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        false // Default to light mode
    }
}

impl DisplayState {
    fn new() -> Self {
        let mut disp = TextDisplay::default();
        disp.wrap_mode(fltk::text::WrapMode::AtBounds, 0);
        let text_buf = TextBuffer::default();
        let style_buf = TextBuffer::default();
        disp.set_buffer(text_buf.clone());

        // Define style tables for light and dark modes
        let light_styles = vec![
            StyleTableEntry {
                color: Color::from_rgb(0, 0, 0),
                font: Font::Courier,
                size: 12,
            }, // 'A' = Black
            StyleTableEntry {
                color: Color::from_rgb(200, 0, 0),
                font: Font::Courier,
                size: 12,
            }, // 'B' = Dark Red
            StyleTableEntry {
                color: Color::from_rgb(0, 200, 0),
                font: Font::Courier,
                size: 12,
            }, // 'C' = Dark Green
            StyleTableEntry {
                color: Color::from_rgb(0, 0, 200),
                font: Font::Courier,
                size: 12,
            }, // 'D' = Dark Blue
            StyleTableEntry {
                color: Color::from_rgb(200, 0, 200),
                font: Font::Courier,
                size: 12,
            }, // 'E' = Dark Magenta
            StyleTableEntry {
                color: Color::from_rgb(0, 200, 200),
                font: Font::Courier,
                size: 12,
            }, // 'F' = Dark Cyan
            StyleTableEntry {
                color: Color::from_rgb(90, 90, 90),
                font: Font::Courier,
                size: 12,
            }, // 'G' = Dark Gray
        ];

        let dark_styles = vec![
            StyleTableEntry {
                color: Color::from_rgb(255, 255, 255),
                font: Font::Courier,
                size: 12,
            }, // 'A' = White
            StyleTableEntry {
                color: Color::from_rgb(255, 100, 100),
                font: Font::Courier,
                size: 12,
            }, // 'B' = Light Red
            StyleTableEntry {
                color: Color::from_rgb(100, 255, 100),
                font: Font::Courier,
                size: 12,
            }, // 'C' = Light Green
            StyleTableEntry {
                color: Color::from_rgb(100, 100, 255),
                font: Font::Courier,
                size: 12,
            }, // 'D' = Light Blue
            StyleTableEntry {
                color: Color::from_rgb(255, 100, 255),
                font: Font::Courier,
                size: 12,
            }, // 'E' = Light Magenta
            StyleTableEntry {
                color: Color::from_rgb(100, 255, 255),
                font: Font::Courier,
                size: 12,
            }, // 'F' = Light Cyan
            StyleTableEntry {
                color: Color::from_rgb(127, 127, 127),
                font: Font::Courier,
                size: 12,
            }, // 'G' = Light Gray
        ];

        // Choose styles based on dark mode
        let styles = if is_dark_mode() {
            dark_styles
        } else {
            light_styles
        };

        // Apply styles
        disp.set_highlight_data(style_buf.clone(), styles);

        DisplayState {
            disp,
            text_buf,
            style_buf,
        }
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
                        "90" => new_style = 'G', // Gray
                        _ => {}                  // Ignore unsupported codes
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
    let mut dialog =
        fltk::dialog::NativeFileChooser::new(fltk::dialog::NativeFileChooserType::BrowseFile);
    dialog.set_option(fltk::dialog::NativeFileChooserOptions::UseFilterExt);
    dialog.set_filter(format!("*{}_rubyfpv_*.tgz", soc).as_str());
    match dialog.try_show() {
        Err(e) => {
            error!("error: {:?}", e);
            None
        }
        Ok(res) => match res {
            fltk::dialog::NativeFileChooserAction::Success => {
                let res = dialog.filename();
                match res.as_os_str().to_str() {
                    Some(res) => Some(res.to_owned()),
                    None => None,
                }
            }
            fltk::dialog::NativeFileChooserAction::Cancelled => None,
        },
    }
}

fn prompt_for_password() -> Option<String> {
    match fltk::dialog::input_default("Authentication failed.\nPlease enter the device password:", "") {
        Some(password) => Some(password.to_string()),
        _ => None,
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
    ResetDevice,
    EnterManualMode,
    ExitManualMode,
    ExecuteManualCommand,
    PromptPasswordAndRetry(RetryAction),
}

#[derive(Copy, Clone)]
enum RetryAction {
    DetectSoc,
    Flash,
    ResetDevice,
}

#[derive(Default)]
struct State {
    soc: String,
    ip: String,
    port: String,
    password: Option<String>,
}

struct RubyFlasher {
    app: app::App,
    receiver: app::Receiver<Message>,
    sender: app::Sender<Message>,
    display: Arc<Mutex<DisplayState>>,
    ip_input: Input,
    port_input: Input,
    btn_detect: Button,
    btn_flash: Button,
    menu_btn: MenuButton,
    manual_input: Input,
    manual_flex: Flex,
    container: Flex,
    state: Arc<Mutex<State>>,
}

impl RubyFlasher {
    pub fn new() -> Self {
        let app = app::App::default().with_scheme(app::Scheme::Gtk);

        // Apply theme based on system dark mode
        let theme = if is_dark_mode() {
            ColorTheme::new(color_themes::DARK_THEME) // Dark theme
        } else {
            ColorTheme::new(color_themes::GRAY_THEME) // Light theme
        };
        theme.apply();

        // Message channel
        let (s, receiver) = app::channel();

        let (x, y) = center();
        let (w, h) = (720, 576);
        let mut wind = Window::new(x - w / 2, y - h / 2, w, h, "RubyFPV simple flasher");
        wind.set_xclass(wind.label().as_str());
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

        let mut menu_btn = MenuButton::default().with_label("Actions");
        menu_btn.deactivate();

        flex.end();

        // Main display area
        let display = Arc::new(Mutex::new(DisplayState::new()));
        {
            let display_guard = display.lock().unwrap();
            container.add(&display_guard.disp);
        }

        // Manual command input area (initially hidden)
        let mut manual_flex = Flex::default().row();
        let manual_label = Frame::default().with_label("Command:");
        manual_flex.fixed(&manual_label, 70);
        let mut manual_input = Input::default();
        manual_flex.add(&manual_input);
        let mut manual_exit_btn = Button::default().with_label("Exit Manual Mode");
        manual_flex.fixed(&manual_exit_btn, 150);
        manual_flex.end();

        // Add manual flex to container and initially hide it
        container.fixed(&manual_flex, 29);
        manual_flex.hide(); // Initially hidden

        container.end();
        wind.end();
        wind.show();

        btn_detect.emit(s, Message::DetectSoc);
        btn_flash.emit(s, Message::Flash);

        // Set up the menu items
        menu_btn.add_choice("Reset device");
        menu_btn.add_choice("Manual command execution");

        // Set up menu callback
        let s_menu = s.clone();
        menu_btn.set_callback(move |m| {
            if let Some(choice) = m.choice() {
                match choice.as_str() {
                    "Reset device" => s_menu.send(Message::ResetDevice),
                    "Manual command execution" => s_menu.send(Message::EnterManualMode),
                    _ => {}
                }
            }
        });

        // Set up manual mode callbacks
        manual_input.set_trigger(fltk::enums::CallbackTrigger::EnterKey);
        manual_input.emit(s, Message::ExecuteManualCommand);
        manual_exit_btn.emit(s, Message::ExitManualMode);

        let state = Arc::new(Mutex::new(State {
            port: "22".to_string(),
            ..Default::default()
        }));
        Self {
            app,
            receiver,
            sender: s,
            ip_input,
            port_input,
            display,
            state,
            btn_detect,
            btn_flash,
            menu_btn,
            manual_input,
            manual_flex,
            container,
        }
    }

    pub fn run(mut self) {
        while self.app.wait() {
            if let Some(msg) = self.receiver.recv() {
                match msg {
                    Message::IpChanged => {
                        self.state.lock().unwrap().ip = self.ip_input.value();
                    }
                    Message::PortChanged => {
                        self.state.lock().unwrap().port = self.port_input.value();
                    }
                    Message::PromptPasswordAndRetry(action) => {
                        // Handle password prompting in main thread
                        if let Some(new_password) = prompt_for_password() {
                            self.state.lock().unwrap().password = Some(new_password);
                            // Retry the operation
                            match action {
                                RetryAction::DetectSoc => self.sender.send(Message::DetectSoc),
                                RetryAction::Flash => self.sender.send(Message::Flash),
                                RetryAction::ResetDevice => self.sender.send(Message::ResetDevice),
                            }
                        } else {
                            let mut display = self.display.lock().unwrap();
                            update_status(&mut display, "Authentication failed and no password provided.");
                            self.btn_detect.activate();
                            self.btn_flash.activate();
                            self.menu_btn.activate();
                        }
                    }
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
                        let mut menu_btn_clone = self.menu_btn.clone();
                        let sender_clone = self.sender.clone();
                        tokio::spawn(async move {
                            let ip = state_clone.lock().unwrap().ip.clone();
                            let password = state_clone.lock().unwrap().password.clone();

                            match flasher::detect_soc(ip.as_str(), port, |msg| {
                                update_status(&mut display_clone.lock().unwrap(), msg);
                            }, password.as_deref())
                            .await
                            {
                                Ok(soc) => {
                                    state_clone.lock().unwrap().soc = soc;
                                    update_status(&mut display_clone.lock().unwrap(), "Done.");
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                    menu_btn_clone.activate();
                                }
                                Err(e) if flasher::is_auth_error(&e) => {
                                    // Clear failed password
                                    state_clone.lock().unwrap().password = None;
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        "Authentication failed. Please enter password when prompted.",
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.deactivate();
                                    menu_btn_clone.deactivate();

                                    // Send message to main thread to handle password input
                                    app::awake();
                                    sender_clone.send(Message::PromptPasswordAndRetry(RetryAction::DetectSoc));
                                }
                                Err(e) => {
                                    error!("error: {:?}", e);
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        format!("Error: {}", e).as_str(),
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.deactivate();
                                    menu_btn_clone.deactivate();
                                }
                            }
                        });
                    }
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
                        self.menu_btn.deactivate();

                        let state_clone = self.state.clone();
                        let display_clone = self.display.clone();
                        let mut btn_detect_clone = self.btn_detect.clone();
                        let mut btn_flash_clone = self.btn_flash.clone();
                        let mut menu_btn_clone = self.menu_btn.clone();
                        let sender_clone = self.sender.clone();
                        tokio::spawn(async move {
                            let ip = state_clone.lock().unwrap().ip.clone();
                            let password = state_clone.lock().unwrap().password.clone();

                            match flasher::flash(ip.as_str(), port, &path, |msg| {
                                update_status(&mut display_clone.lock().unwrap(), msg);
                            }, password.as_deref())
                            .await
                            {
                                Ok(_) => {
                                    update_status(&mut display_clone.lock().unwrap(),"\n\
                                          \x1b[32mReview the log above to ensure everything went well.\n\
                                          The last log line should be like '\x1b[0m\x1b[1mUnconditional reboot\x1b[0m\x1b[32m'.\n\
                                          If the log shows no errors, the firmware flash is completed.\n\
                                          \x1b[1m\x1b[34mPlease wait 2-3 minutes for the device to completely initialize \
                                          and do not disconnect power during this time.\x1b[0m"
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                    menu_btn_clone.activate();
                                }
                                Err(e) if flasher::is_auth_error(&e) => {
                                    // Clear failed password
                                    state_clone.lock().unwrap().password = None;
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        "Authentication failed. Please enter password when prompted.",
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                    menu_btn_clone.activate();

                                    // Send message to main thread to handle password input
                                    app::awake();
                                    sender_clone.send(Message::PromptPasswordAndRetry(RetryAction::Flash));
                                }
                                Err(e) => {
                                    error!("error: {:?}", e);
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        format!("Error: {}", e).as_str(),
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                    menu_btn_clone.activate();
                                }
                            }
                        });
                    }
                    Message::ResetDevice => {
                        // Show confirmation dialog
                        let choice = fltk::dialog::choice2_default(
                            "Are you sure you want to reset the device?\n\nThis will clear all settings from the device and make it appear as newly flashed.\n\nThe device will need 2-3 minutes to completely initialize after reset and should not be disconnected from power during this time.",
                            "Cancel",
                            "Reset Device",
                            ""
                        );

                        // If user chose "Cancel" (returns Some(0)) or closed dialog (returns None), don't proceed
                        match choice {
                            Some(1) => {}  // User chose "Reset Device", proceed
                            _ => continue, // User chose "Cancel" or closed dialog, don't proceed
                        }

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
                        self.btn_flash.deactivate();
                        self.menu_btn.deactivate();

                        let state_clone = self.state.clone();
                        let display_clone = self.display.clone();
                        let mut btn_detect_clone = self.btn_detect.clone();
                        let mut btn_flash_clone = self.btn_flash.clone();
                        let mut menu_btn_clone = self.menu_btn.clone();
                        let sender_clone = self.sender.clone();
                        tokio::spawn(async move {
                            let ip = state_clone.lock().unwrap().ip.clone();
                            let password = state_clone.lock().unwrap().password.clone();

                            match flasher::reset_device(ip.as_str(), port, |msg| {
                                update_status(&mut display_clone.lock().unwrap(), msg);
                            }, password.as_deref())
                            .await
                            {
                                Ok(_) => {
                                    update_status(&mut display_clone.lock().unwrap(),"\n\
                                          \x1b[32mReview the log above to ensure everything went well.\n\
                                          The last log line should be like '\x1b[0m\x1b[1mUnconditional reboot\x1b[0m\x1b[32m'.\n\
                                          If the log shows no errors, the reset is completed.\n\
                                          \x1b[1m\x1b[34mPlease wait 2-3 minutes for the device to completely initialize \
                                          and do not disconnect power during this time.\x1b[0m"
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                    menu_btn_clone.activate();
                                }
                                Err(e) if flasher::is_auth_error(&e) => {
                                    // Clear failed password
                                    state_clone.lock().unwrap().password = None;
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        "Authentication failed. Please enter password when prompted.",
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                    menu_btn_clone.activate();

                                    // Send message to main thread to handle password input
                                    app::awake();
                                    sender_clone.send(Message::PromptPasswordAndRetry(RetryAction::ResetDevice));
                                }
                                Err(e) => {
                                    error!("error: {:?}", e);
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        format!("Error: {}", e).as_str(),
                                    );
                                    btn_detect_clone.activate();
                                    btn_flash_clone.activate();
                                    menu_btn_clone.activate();
                                }
                            }
                        });
                    }
                    Message::EnterManualMode => {
                        let state = self.state.lock().unwrap();
                        if !state.ip.is_empty() {
                            drop(state); // Release the lock before acquiring it again
                            self.btn_detect.deactivate();
                            self.btn_flash.deactivate();
                            self.menu_btn.deactivate();
                            // Show the manual flex
                            self.manual_flex.show();
                            self.container.layout();
                            // Use a safe focus operation instead of unwrap
                            if let Err(e) = self.manual_input.take_focus() {
                                error!("Failed to take focus: {:?}", e);
                            }
                            app::redraw();
                        } else {
                            drop(state); // Release the lock
                            let mut display = self.display.lock().unwrap();
                            update_status(&mut display, "Error: Please enter an IP address first.");
                        }
                    }
                    Message::ExitManualMode => {
                        // Hide the manual flex
                        self.manual_flex.hide();
                        self.container.layout();
                        self.btn_detect.activate();
                        self.btn_flash.activate();
                        self.menu_btn.activate();
                        self.manual_input.set_value("");
                        app::redraw();
                    }
                    Message::ExecuteManualCommand => {
                        // Only allow command execution if manual_flex is visible (manual mode)
                        if !self.manual_flex.visible() {
                            return;
                        }
                        let state = self.state.lock().unwrap();
                        let command = self.manual_input.value().trim().to_string();
                        if command.is_empty() {
                            return;
                        }

                        let mut display = self.display.lock().unwrap();
                        let port: u16 = match state.port.parse() {
                            Ok(v) => v,
                            Err(e) => {
                                error!("error: {:?}", e);
                                update_status(&mut display, "Error: invalid port specified.");
                                return;
                            }
                        };

                        // Clear the input for next command
                        self.manual_input.set_value("");

                        let state_clone = self.state.clone();
                        let display_clone = self.display.clone();
                        tokio::spawn(async move {
                            let ip = state_clone.lock().unwrap().ip.clone();
                            let password = state_clone.lock().unwrap().password.clone();

                            match flasher::execute_command(ip.as_str(), port, &command, |msg| {
                                update_status(&mut display_clone.lock().unwrap(), msg);
                            }, password.as_deref())
                            .await
                            {
                                Ok(_) => {
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        "Command completed.",
                                    );
                                }
                                Err(e) if flasher::is_auth_error(&e) => {
                                    // Clear failed password and show message
                                    state_clone.lock().unwrap().password = None;
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        "Authentication failed. Please set password in a regular operation first.",
                                    );
                                }
                                Err(e) => {
                                    error!("error: {:?}", e);
                                    update_status(
                                        &mut display_clone.lock().unwrap(),
                                        format!("Error: {}", e).as_str(),
                                    );
                                }
                            }
                        });
                    }
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
    env_logger::builder().filter_level(LevelFilter::Info).init();

    let a = RubyFlasher::new();
    a.run();
}
