#![feature(result_flattening)]

use crate::datastructures::{FromQueryString, NotifyTextMessage, TransmissionCommand};
use crate::input_thread::InputThread;
use crate::tslib::TeamspeakConnection;
use anyhow::anyhow;
use clap::{arg, command};
use kstool::prelude::get_current_duration;
use log::{error, info, warn, LevelFilter};
use std::hint::unreachable_unchecked;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

mod datastructures;
mod input_thread;
mod tslib;

const DEFAULT_VARIABLE_NAME: &str = "TS_CLIENT_QUERY_APIKEY";
const TRANSMISSION_DEADLINE: u64 = 180;

async fn real_staff(
    mut conn: TeamspeakConnection,
    last_transmission: Arc<AtomicU64>,
    mut command_receiver: mpsc::Receiver<TransmissionCommand>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            Some(data) = command_receiver.recv() => {
                match data {
                    TransmissionCommand::Data(s) => {
                        let server_id = conn
                            .get_current_server_tab()
                            .await
                            .map_err(|e| warn!("Can't get current server tab: {:?}", e))
                            .map(|r| r.schandler_id())
                            .ok()
                            .unwrap_or(1);
                        conn.send_channel_message(server_id, &s)
                            .await
                            .map_err(|e| error!("Unable send channel message: {:?}", e))
                            .ok();
                        last_transmission.store(get_current_duration().as_secs(), Ordering::Relaxed);
                    }
                    TransmissionCommand::KeepAlive => {
                        conn.keep_alive()
                            .await
                            .map_err(|e| {
                                error!("Got error while write data in keep alive function: {:?}", e)
                            })
                            .ok();
                        last_transmission.store(get_current_duration().as_secs(), Ordering::Relaxed);
                    }
                    TransmissionCommand::Terminate => {
                        return Ok(());
                    }
                }
            }
            ret = conn.wait_readable() => {
                if !ret? {
                    continue;
                }

                let data = conn
                    .read_data()
                    .await
                    .map_err(|e| anyhow!("Got error while read data: {:?}", e))?;
                //trace!("Read data end");

                if !data.as_ref().is_some_and(|x| !x.is_empty()) {
                    continue;
                }
                let data = data.unwrap();

                //trace!("message loop start");
                for line in data.lines().map(|line| line.trim()) {
                    if line.is_empty() {
                        continue;
                    }

                    if line.contains("notifytextmessage") {
                        let view = NotifyTextMessage::from_query(line)
                            .map_err(|e| anyhow!("Got error while deserialize moved view: {:?}", e))?;

                        println!(
                            "[{time}] {sender}: {msg}",
                            time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                            sender = view.invoker_name(),
                            msg = view.msg()
                        );
                        continue;
                    }
                }
            }
        }
    }
}

async fn staff(
    api_key: &str,
    server: String,
    port: u16,
    command_sender: mpsc::Sender<TransmissionCommand>,
    command_receiver: mpsc::Receiver<TransmissionCommand>,
) -> anyhow::Result<()> {
    let mut conn = TeamspeakConnection::connect(&server, port)
        .await
        .map_err(|e| anyhow!("Connect error: {:?}", e))?;
    conn.login(api_key).await?;
    conn.register_event().await?;

    let last_transmission = Arc::new(AtomicU64::new(get_current_duration().as_secs()));
    tokio::select! {
        _ = async move {
            tokio::signal::ctrl_c().await.unwrap();
            info!("Recv SIGINT again, force exit.");
            std::process::exit(137);
        } => {
            unsafe { unreachable_unchecked() }
        }
        ret = real_staff(conn, last_transmission.clone(), command_receiver) => {
            ret?;
        }
        _ = async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if get_current_duration().as_secs() - last_transmission.load(Ordering::Relaxed) > TRANSMISSION_DEADLINE {
                    command_sender.send(TransmissionCommand::KeepAlive).await.map_err(|_| error!("Unable send keep alive command")).ok();
                }
            }
        } => {}
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let matches = command!()
        .args(&[
            arg!(<API_KEY> "Teamspeak client query api key").env(DEFAULT_VARIABLE_NAME),
            arg!(--server <SERVER> "Specify server"),
            arg!(--port <PORT> "Specify port"),
            arg!(--dbginput "Debug input function"),
            arg!(--debug "Enable other module log output in debug/trace level"),
        ])
        .get_matches();

    let mut logger_ = env_logger::Builder::from_default_env();
    if !matches.get_flag("dbginput") {
        logger_.filter_module("rustyline", LevelFilter::Warn);
    }
    if !matches.get_flag("debug") {
        logger_.filter_module("mio", LevelFilter::Warn);
    }
    logger_.init();

    let (sender, command_receiver) = mpsc::channel(4096);

    let input_handler = InputThread::start(sender.clone());

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
            sender,
            command_receiver,
        ))?;

    if input_handler.alive() {
        warn!("Input thread still alive!");
    } else {
        input_handler.join()?;
    }

    Ok(())
}
