use fltk::{app::{self, App}, button::Button, enums::Align, frame::Frame, group::Flex, prelude::*, window::Window};
use async_ssh2_tokio::client::{Client, AuthMethod, ServerCheckMethod};
use log::{error, info, LevelFilter};
use std::{net::IpAddr, path::{self, Path, PathBuf}, str::FromStr};

async fn flash(src: String) -> Result<(), async_ssh2_tokio::Error> {
    let auth_method = AuthMethod::with_password("Kitten");
    let client = Client::connect(
        ("192.168.13.200", 22),
        "misha",
        auth_method,
        ServerCheckMethod::NoCheck,
    ).await?;

    let path = Path::new(&src);
    let fname = path.file_name().unwrap_or_default().to_str().unwrap_or_default();
    let dst_path = format!("/tmp/{}", fname);
    client.upload_file(src, dst_path).await?;

    //let result = client.execute("cat /tmp/Cargo.toml").await?;
    
    Ok(())
}

fn update_status(frame: &mut Frame, status: &str) {
    frame.set_label(status);
    app::awake();
    app::redraw();
}

fn choose_file() -> Option<String> {
    let mut dialog = fltk::dialog::NativeFileChooser::new(fltk::dialog::NativeFileChooserType::BrowseFile);
    dialog.set_option(fltk::dialog::NativeFileChooserOptions::UseFilterExt);
    dialog.set_filter("*.img");
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

#[tokio::main]
async fn main() -> Result<(), async_ssh2_tokio::Error> {

    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .init();

    let app = app::App::default().with_scheme(app::Scheme::Gtk);
    let mut wind = Window::new(100, 100, 400, 300, "Hello from rust");
    wind.make_resizable(true);
    let mut container = Flex::default().size_of_parent().column();
    container.set_margin(12);
    let flex = Flex::default().size_of_parent().row();
    let mut but_inc = Button::default().with_label("Choose file...");
    let mut frame = Frame::default().with_label("0");
    flex.end();
    let flex2 = Flex::default().size_of_parent().row();
    let mut but_dec = Button::default().with_label("-");
    flex2.end();
    container.end();
    wind.end();
    wind.show();
    but_inc.set_callback(move |_| {
        let path = match choose_file() {
            Some(path) => path,
            None => return,
        };
        let mut frame_clone = frame.clone();
        tokio::spawn(async move {
            update_status(&mut frame_clone, "Uploading...");
            match flash(path).await {
                Ok(_) => {
                    update_status(&mut frame_clone, "Uploading complete!");
                },
                Err(e) => {
                    error!("error: {:?}", e);
                    update_status(&mut frame_clone, "Upload failed!");
                }
            };
        });
    });
    but_dec.set_callback(|this_button| this_button.set_label("Works"));
    app.run().unwrap();

    Ok(())
}
