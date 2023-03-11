mod inner {
    use crate::datastructures::TransmissionCommand;
    use anyhow::anyhow;
    use log::{error, trace};
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;
    use std::thread::JoinHandle;
    use tempfile::NamedTempFile;
    use tokio::sync::mpsc;
    use tokio::time::Instant;

    #[derive(Debug)]
    pub struct InputThread {
        handle: JoinHandle<anyhow::Result<()>>,
    }

    impl InputThread {
        fn send_data(
            sender: mpsc::Sender<TransmissionCommand>,
            data: TransmissionCommand,
        ) -> Option<()> {
            let start = Instant::now();
            let ret = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(sender.send(data))
                .map_err(|_| error!("Unable to send text"))
                .ok();
            trace!("Measure end => {:?}", start.elapsed());
            ret
        }

        // Known issue, may override C-c function after program exit
        pub fn get_input(sender: mpsc::Sender<TransmissionCommand>) -> anyhow::Result<()> {
            let tmp_file = NamedTempFile::new()
                .map_err(|e| error!("[Can be safety ignore] Unable create temp file: {:?}", e))
                .ok();
            let mut rl = DefaultEditor::new()?;

            let mut success = false;
            if let Some(file) = tmp_file {
                rl.load_history(file.path())
                    .map(|_| success = true)
                    .map_err(|e| error!("[Can be safety ignore] Unable load file history. {:?}", e))
                    .ok();
            }

            loop {
                match rl.readline(">> ") {
                    Ok(line) => {
                        if line.is_empty() {
                            continue;
                        }
                        if success {
                            rl.add_history_entry(line.trim()).ok();
                        }
                        Self::send_data(
                            sender.clone(),
                            TransmissionCommand::Data(line.trim().to_string()),
                        );
                        trace!("Read {} bytes from stdin", line.len());
                    }
                    Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
                        Self::send_data(sender, TransmissionCommand::Terminate);
                        trace!("Send exit signal");
                        break;
                    }
                    Err(e) => return Err(anyhow!("Got error while read line: {:?}", e)),
                }
            }

            Ok(())
        }

        pub fn start(sender: mpsc::Sender<TransmissionCommand>) -> Self {
            Self {
                handle: std::thread::spawn(|| Self::get_input(sender)),
            }
        }

        pub fn alive(&self) -> bool {
            !self.handle.is_finished()
        }

        pub fn join(self) -> anyhow::Result<()> {
            self.handle
                .join()
                .map_err(|e| anyhow!("Unable join thread: {:?}", e))
                .flatten()
        }
    }
}

pub use inner::InputThread;
