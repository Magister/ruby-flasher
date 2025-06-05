use anyhow::{Error, Result};
use async_trait::async_trait;
use client::{Handle, Msg};
use keys::ssh_key;
use log::{error, info};
use russh::*;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use std::{net::IpAddr, str::FromStr, sync::Arc};
use std::str;

struct Client;

const TIMEOUT_TINY: u64 = 5;
const TIMEOUT_MAIN: u64 = 60;

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
        Duration::from_secs(TIMEOUT_TINY),
        russh::client::connect(Arc::new(config), (ip, port), sh)
    ).await??;
    session.authenticate_password("root", "12345").await?;
    return Ok(session);
}

// Helper function to wait for SCP acknowledgment
async fn wait_for_acknowledgment(channel: &mut russh::Channel<Msg>) -> Result<()> {
    let timeout_duration = Duration::from_secs(TIMEOUT_MAIN);

    match tokio::time::timeout(timeout_duration, channel.wait()).await {
        Ok(Some(russh::ChannelMsg::Data { ref data })) => {
            if data.len() >= 1 {
                match data[0] {
                    0 => Ok(()), // Success
                    1 => {
                        let error_msg = if data.len() > 1 {
                            String::from_utf8_lossy(&data[1..]).to_string()
                        } else {
                            "Unknown SCP error".to_string()
                        };
                        Err(anyhow::anyhow!("SCP error: {}", error_msg).into())
                    },
                    2 => Err(anyhow::anyhow!("SCP fatal error").into()),
                    _ => Err(anyhow::anyhow!("Unknown SCP response: {}", data[0]).into()),
                }
            } else {
                Err(anyhow::anyhow!("Empty SCP response").into())
            }
        },
        Ok(Some(russh::ChannelMsg::Success)) => Ok(()),
        Ok(Some(msg)) => {
            info!("Received msg: {:?}", msg);
            Ok(())
        },
        Ok(None) => Err(anyhow::anyhow!("Channel closed unexpectedly").into()),
        Err(_) => Err(anyhow::anyhow!("Timeout waiting for SCP acknowledgment").into()),
    }
}

async fn transfer_file<F>(src: &str, dst: &str, session: &mut Handle<Client>, mut status_update: F) -> Result<()> where F: FnMut(&str) {
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

    // Wait for initial acknowledgment (0x00 byte)
    wait_for_acknowledgment(&mut channel).await?;

    let mut total_sent = 0;

    // Send the SCP command
    channel.data(cmd.as_bytes()).await?; // &[u8] still works here (might be coerced)
    total_sent += cmd.as_bytes().len();
    let percent = (total_sent as f64 / total_size as f64 * 100.0).min(100.0);
    status_update(&format!(
        "Progress: {:.1}% ({} / {} bytes)",
        percent, total_sent, total_size
    ));

    // Wait for acknowledgment of the command
    wait_for_acknowledgment(&mut channel).await?;

    // Send the file contents in chunks
    const CHUNK_SIZE: usize = 1024 * 64; // 64KB chunks

    for chunk_start in (0..file_contents.len()).step_by(CHUNK_SIZE) {
        let chunk_end = std::cmp::min(chunk_start + CHUNK_SIZE, file_contents.len());
        let chunk = &file_contents[chunk_start..chunk_end];

        tokio::time::timeout(Duration::from_secs(TIMEOUT_MAIN), channel.data(chunk)).await??;
        total_sent += chunk.len();

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

    // Wait for final acknowledgment
    wait_for_acknowledgment(&mut channel).await?;

    // Finish up
    tokio::time::timeout(Duration::from_secs(TIMEOUT_TINY), channel.eof()).await??;

    info!("consuming leftovers if any...");
    // consume leftovers
    loop {
       match tokio::time::timeout(Duration::from_secs(TIMEOUT_TINY), channel.wait()).await {
            Ok(msg) => {
                if msg.is_none() {
                    break;
                }
            },
            Err(_) => (),
        }
    }
    tokio::time::timeout(Duration::from_secs(TIMEOUT_TINY), channel.close()).await??;

    status_update("File sent successfully!");
    Ok(())
}

async fn run_command<F>(session: &mut Handle<Client>, command: &str, mut status_update: F) -> Result<String> where F: FnMut(&str) {
    info!("# {}", command);
    //tokio::time::sleep(Duration::from_secs(2)).await;
    status_update(format!("# {}", command).as_str());
    let mut result: Option<u32> = None;
    let mut res = String::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, command).await?;

    while let Some(msg) = tokio::time::timeout(Duration::from_secs(TIMEOUT_MAIN), channel.wait()).await? {
        match msg {
            russh::ChannelMsg::Data { ref data } => {
                buf.write_all(data)?;
                // Try to decode as much valid UTF-8 as possible
                let (valid_str, remaining) = match String::from_utf8(buf.clone()) {
                    Ok(msg) => {
                        // All data is valid UTF-8
                        (msg, Vec::new())
                    },
                    Err(e) => {
                        // Extract valid UTF-8 prefix
                        let valid_up_to = e.utf8_error().valid_up_to();
                        if valid_up_to > 0 {
                            let valid_part = String::from_utf8(buf[..valid_up_to].to_vec()).unwrap();
                            let remaining = buf[valid_up_to..].to_vec();
                            (valid_part, remaining)
                        } else {
                            // No valid UTF-8 found, wait for more data
                            (String::new(), buf.clone())
                        }
                    }
                };
                if !valid_str.is_empty() {
                    res.push_str(&valid_str);
                    for line in valid_str.split('\n') {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            status_update(trimmed);
                            if line.contains("Unconditional reboot") {
                                let _ = tokio::time::timeout(Duration::from_secs(TIMEOUT_TINY), channel.close()).await;
                                return Ok(res);
                            }
                        }
                    }
                }

                // Update buffer with remaining bytes
                buf = remaining;
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

    // Try to handle any remaining data in buffer
    if !buf.is_empty() {
        let msg = String::from_utf8_lossy(&buf);
        res.push_str(&msg);
        for line in msg.split('\n') {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                status_update(trimmed);
            }
        }
    }

    // Finish up
    tokio::time::timeout(Duration::from_secs(TIMEOUT_TINY), channel.eof()).await??;

    info!("consuming leftovers if any...");
    // consume leftovers
    loop {
       match tokio::time::timeout(Duration::from_secs(TIMEOUT_TINY), channel.wait()).await {
            Ok(msg) => {
                if msg.is_none() {
                    break;
                }
            },
            Err(_) => (),
        }
    }

    info!("closing channel");
    tokio::time::timeout(Duration::from_secs(TIMEOUT_TINY), channel.close()).await??;

    status_update(format!("command '{}' done.", command).as_str());

    // tokio::time::sleep(Duration::from_secs(1)).await;

    match result {
        Some(exit_status) if exit_status != 0 => {
            Err(anyhow::anyhow!("command '{}' failed with exit status: {}", command, exit_status).into())
        }
        _ => Ok(res), // Success or no exit status (treat as success)
    }
}

// fn replace_extension(filename: &str, new_ext: &str) -> String {
//     let path = Path::new(filename);
//     match path.extension() {
//         Some(_) => path.with_extension(new_ext).to_string_lossy().into_owned(),
//         None => filename.to_string(), // Return original if no extension
//     }
// }

fn extract_filename(src: &str) -> Result<String> {
    let path = Path::new(&src);
    let fname = path.file_name().unwrap_or_default().to_str();
    match fname {
        Some(str) => Ok(str.to_string()),
        None => Err(Error::msg(format!("invalid filename: {}", src)))
    }
}

pub(crate) async fn detect_soc<F>(ip_addr: &str, port: u16, mut status_update: F) -> Result<String> where F: FnMut(&str) {
    let ip = IpAddr::from_str(&ip_addr)?;
    let mut session = connect(ip, port).await?;
    let soc = run_command(&mut session, "fw_printenv -n soc", &mut status_update).await?.trim().to_string();
    session.disconnect(Disconnect::ByApplication, "", "en").await?;
    Ok(soc)
}

pub(crate) async fn flash<F>(ip_addr: &str, port: u16, src: &str, mut status_update: F) -> Result<()> where F: FnMut(&str) {
    let ip = IpAddr::from_str(&ip_addr)?;
    let fname = extract_filename(&src)?;
    let dst = format!("/tmp/{}", fname);
    // let dst_tar = replace_extension(&dst, "tar");
    status_update(format!("Connecting to {}:{}...", ip_addr, port).as_str());
    let mut session = connect(ip, port).await?;
    let soc = run_command(&mut session, "fw_printenv -n soc", &mut status_update).await?.trim().to_string();
    run_command(&mut session, "ruby_stop.sh || true", &mut status_update).await?;
    status_update(format!("Uploading firmware {}...", fname).as_str());
    transfer_file(&src, &dst, &mut session, &mut status_update).await?;
    run_command(&mut session, format!("sh -c 'gunzip -c {} | tar -xvC /tmp'", dst).as_str(), &mut status_update).await?;
    run_command(&mut session, format!("sysupgrade --kernel=/tmp/uImage.{} --rootfs=/tmp/rootfs.squashfs.{} -z", soc, soc).as_str(), &mut status_update).await?;
    session.disconnect(Disconnect::ByApplication, "", "en").await?;
    Ok(())
}

pub(crate) async fn reset_device<F>(ip_addr: &str, port: u16, mut status_update: F) -> Result<()> where F: FnMut(&str) {
    let ip = IpAddr::from_str(&ip_addr)?;
    status_update(format!("Connecting to {}:{}...", ip_addr, port).as_str());
    let mut session = connect(ip, port).await?;
    status_update("Executing firstboot command...");
    run_command(&mut session, "firstboot", &mut status_update).await?;
    session.disconnect(Disconnect::ByApplication, "", "en").await?;
    Ok(())
}
