#![feature(is_some_and)]

use crate::datastructures::{FromQueryString, NotifyTextMessage, SocketConn};
use crate::input_thread::{get_input, DataType};
use anyhow::anyhow;
use clap::{arg, command};
use log::{error, info};
use rustyline_async::SharedWriter;
use std::hint::unreachable_unchecked;
//use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

mod datastructures;
mod input_thread;

async fn real_staff(
    mut conn: SocketConn,
    alt_signal: Arc<AtomicBool>,
    mut input_receiver: mpsc::Receiver<DataType>,
    _writer: SharedWriter,
) -> anyhow::Result<()> {
    let mut received = true;

    loop {
        if let Ok(Some(data)) =
            tokio::time::timeout(Duration::from_secs(1), input_receiver.recv()).await
        {
            match data {
                DataType::Data(s) => {
                    /*println!(
                        "[{}] /me: {}",
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                        &s
                    );*/
                    conn.send_channel_message(&s)
                        .await
                        .map_err(|e| error!("Unable send channel message: {:?}", e))
                        .ok();
                }
                DataType::Terminate => {
                    return Ok(());
                }
            }
        }

        let data = conn
            .read_data()
            .await
            .map_err(|e| anyhow!("Got error while read data: {:?}", e))?;
        //trace!("Read data end");

        if !data.as_ref().is_some_and(|x| !x.is_empty()) {
            let signal = alt_signal.load(Ordering::Relaxed);
            if signal {
                if !received {
                    error!("Not received answer after period of time");
                    return Err(anyhow!("Server disconnected"));
                }
                received = false;
                conn.keep_alive()
                    .await
                    .map_err(|e| {
                        error!("Got error while write data in keep alive function: {:?}", e)
                    })
                    .ok();
                alt_signal.store(false, Ordering::Relaxed);
            }
            continue;
        }
        let data = data.unwrap();

        //trace!("message loop start");
        for line in data.lines().map(|line| line.trim()) {
            if line.is_empty() {
                continue;
            }

            if line.contains("notifytextmessage") {
                let current_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let view = NotifyTextMessage::from_query(line)
                    .map_err(|e| anyhow!("Got error while deserialize moved view: {:?}", e))?;

                /*writer
                .write_all(
                    .as_bytes(),
                )
                .map_err(|e| anyhow!("Unable write to console: {:?}", e))?;*/

                println!(
                    "[{time}] {sender}: {msg}",
                    time = current_time,
                    sender = view.invoker_name(),
                    msg = view.msg()
                );
                continue;
            }

            if line.contains("schandlerid=") {
                received = true;
            }
        }
    }
}

async fn staff(api_key: &str, server: String, port: u16) -> anyhow::Result<()> {
    let mut conn = SocketConn::connect(&server, port)
        .await
        .map_err(|e| anyhow!("Connect error: {:?}", e))?;
    conn.login(api_key).await?;
    conn.register_event().await?;

    let (readline, shared_writer) = rustyline_async::Readline::new(">> ".to_string())
        .map_err(|e| anyhow!("Unable to create readline {:?}", e))?;

    let keepalive_signal = Arc::new(AtomicBool::new(false));
    let alt_signal = keepalive_signal.clone();
    let (sender, input_receiver) = mpsc::channel(4096);
    tokio::select! {
        ret = get_input(readline, sender.clone()) =>{
            ret?;
        }
        _ = async move {
            tokio::signal::ctrl_c().await.unwrap();
            info!("Recv SIGINT again, force exit.");
            std::process::exit(137);
        } => {
            unsafe { unreachable_unchecked() }
        }
        _ = async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                keepalive_signal.store(true, Ordering::Relaxed);
            }
        } => {}
        ret = real_staff(conn, alt_signal, input_receiver, shared_writer) => {
            ret?;
            // We really need this?
            std::process::exit(0);
        }
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let matches = command!()
        .args(&[
            arg!([API_KEY] "Teamspeak client query api key"),
            arg!(--server <SERVER> "Specify server"),
            arg!(--port <PORT> "Specify port"),
        ])
        .get_matches();

    env_logger::Builder::from_default_env().init();

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(staff(
            matches.get_one::<String>("API_KEY").unwrap(),
            matches
                .get_one("server")
                .map(|s: &String| s.to_string())
                .unwrap_or_else(|| "localhost".to_string()),
            matches
                .get_one("port")
                .map(|s: &String| s.to_string())
                .and_then(|s| {
                    s.parse()
                        .map_err(|e| error!("Got parse error, use default 25639 instead. {:?}", e))
                        .ok()
                })
                .unwrap_or(25639),
        ))?;

    Ok(())
}
