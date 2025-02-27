use anyhow::{Error, Result};
use async_trait::async_trait;
use log::{error, info};
use russh::*;
use russh_keys::*;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;
use std::{net::IpAddr, str::FromStr, sync::Arc};
use std::str;

struct Client;

#[async_trait]
impl client::Handler for Client {
    type Error = Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        info!("server_key: {:?}", server_public_key);
        Ok(true)
    }
}

async fn connect(ip: IpAddr, port: u16) -> Result<russh::client::Handle<Client>> {
    let config = russh::client::Config::default();
    let sh = Client {};
    let mut session = tokio::time::timeout(
        Duration::from_secs(10),
        russh::client::connect(Arc::new(config), (ip, port), sh)
    ).await??;
    session.authenticate_password("root", "12345").await?;
    return Ok(session);
}

async fn transfer_file<F>(src: &str, dst: &str, session: &mut russh::client::Handle<Client>, mut status_update: F) -> Result<()> where F: FnMut(&str) {
    // Read the file into memory
    let mut src_file = File::open(src).await?;
    let mut file_contents = Vec::new();
    src_file.read_to_end(&mut file_contents).await?;
    let file_size = file_contents.len();
    let cmd = format!("C0644 {} filename\n", file_size);
    let total_size = file_size + cmd.as_bytes().len() + 1; // File + command + null byte

    // Open the channel and start SCP
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, format!("scp -t {}", dst)).await?;

    let mut total_sent = 0;

    // Send the SCP command
    channel.data(cmd.as_bytes()).await?; // &[u8] still works here (might be coerced)
    total_sent += cmd.as_bytes().len();
    let percent = (total_sent as f64 / total_size as f64 * 100.0).min(100.0);
    status_update(&format!(
        "Progress: {:.1}% ({} / {} bytes)",
        percent, total_sent, total_size
    ));

    // Send the file contents in chunks
    const CHUNK_SIZE: usize = 1024 * 64; // 64KB chunks
    let mut cursor = Cursor::new(file_contents);
    let mut buffer = vec![0u8; CHUNK_SIZE];
    
    loop {
        let pos = cursor.position() as usize;
        let slice = cursor.get_mut(); // Get mutable access to the underlying Vec, but only after pos
        if pos >= slice.len() {
            break; // End of file
        }
        let remaining = slice.len() - pos;
        let to_read = CHUNK_SIZE.min(remaining);
        buffer[..to_read].copy_from_slice(&slice[pos..pos + to_read]);
        cursor.set_position((pos + to_read) as u64);

        channel.data(&buffer[..to_read]).await?;
        total_sent += to_read;
        let percent = (total_sent as f64 / total_size as f64 * 100.0).min(100.0);
        status_update(&format!(
            "Progress: {:.1}% ({} / {} bytes)",
            percent, total_sent, total_size
        ));
    }

    // Send the null byte
    channel.data(&b"\0"[..]).await?;
    total_sent += 1;
    let percent = (total_sent as f64 / total_size as f64 * 100.0).min(100.0);
    status_update(&format!(
        "Progress: {:.1}% ({} / {} bytes)",
        percent, total_sent, total_size
    ));

    // Finish up
    channel.eof().await?;
    let _msg = channel
        .wait()
        .await
        .ok_or_else(|| anyhow::anyhow!("Channel closed unexpectedly"))?;

    status_update("File sent successfully!");
    Ok(())
}

async fn run_command<F>(session: &mut russh::client::Handle<Client>, command: &str, mut status_update: F) -> Result<String> where F: FnMut(&str) {
    info!("# {}", command);
    status_update(format!("# {}", command).as_str());
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, command).await?;
    let mut result: Option<u32> = None;
    let mut res = String::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { ref data } => {
                let str_msg = String::from_utf8_lossy(data);
                res.push_str(&str_msg);
                for line in str_msg.split("\n") {
                    let trimmed = line.trim();
                    if trimmed.len() > 0 {
                        info!("{}", trimmed);
                        status_update(trimmed);
                        if line.starts_with("Unconditional reboot") {
                            break;
                        }
                    }
                }
            }
            russh::ChannelMsg::ExtendedData { ref data, ext } => {
                if ext == 1 {
                    let str_msg = String::from_utf8_lossy(data);
                    for line in str_msg.split("\n") {
                        error!("stderr: {}", line);
                        status_update(format!("stderr: {}", line).as_str());
                    }
                }
            }
            // If we get an exit code report, store it, but crucially don't
            // assume this message means end of communications. The data might
            // not be finished yet!
            russh::ChannelMsg::ExitStatus { exit_status } => result = Some(exit_status),

            // We SHOULD get this EOF messagge, but 4254 sec 5.3 also permits
            // the channel to close without it being sent. And sometimes this
            // message can even precede the Data message, so don't handle it
            // russh::ChannelMsg::Eof => break,
            _ => {}
        }
    };
    // Finish up
    tokio::time::timeout(Duration::from_secs(5), channel.eof()).await??;
    tokio::time::timeout(Duration::from_secs(5), channel.close()).await??;

    match result {
        Some(exit_status) if exit_status != 0 => {
            Err(anyhow::anyhow!("command '{}' failed with exit status: {}", command, exit_status).into())
        }
        _ => Ok(res), // Success or no exit status (treat as success)
    }
}

fn replace_extension(filename: &str, new_ext: &str) -> String {
    let path = Path::new(filename);
    match path.extension() {
        Some(_) => path.with_extension(new_ext).to_string_lossy().into_owned(),
        None => filename.to_string(), // Return original if no extension
    }
}

fn extract_filename(src: &str) -> Result<String> {
    let path = Path::new(&src);
    let fname = path.file_name().unwrap_or_default().to_str();
    match fname {
        Some(str) => Ok(str.to_string()),
        None => Err(Error::msg(format!("invalid filename: {}", src)))
    }
}

pub(crate) async fn flash<F>(ip_addr: &str, port: u16, src: &str, mut status_update: F) -> Result<()> where F: FnMut(&str) {
    let ip = IpAddr::from_str(&ip_addr)?;
    let fname = extract_filename(&src)?;
    let dst = format!("/tmp/{}", fname);
    let dst_tar = replace_extension(&dst, "tar");
    status_update(format!("Connecting to {}:{}...", ip_addr, port).as_str());
    let mut session = connect(ip, port).await?;
    let soc = run_command(&mut session, "fw_printenv -n soc", &mut status_update).await?.trim().to_string();
    run_command(&mut session, "ruby_stop.sh || true", &mut status_update).await?;
    status_update(format!("Uploading firmware {}...", fname).as_str());
    transfer_file(&src, &dst, &mut session, &mut status_update).await?;
    run_command(&mut session, format!("gunzip {} && ls -ls {}", dst, dst_tar).as_str(), &mut status_update).await?;
    run_command(&mut session, format!("sleep 2 && tar -xvf {} -C /tmp && sync && sleep 2", dst_tar).as_str(), &mut status_update).await?;
    run_command(&mut session, format!("sleep 2 && sysupgrade --kernel=/tmp/uImage.{} --rootfs=/tmp/rootfs.squashfs.{} -z", soc, soc).as_str(), &mut status_update).await?;
    Ok(())
}
