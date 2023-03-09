mod inner {
    use anyhow::anyhow;
    use log::{error, trace};
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;
    use std::thread::JoinHandle;
    use tokio::sync::mpsc;

    #[derive(Clone, Debug)]
    pub enum DataType {
        Data(String),
        Terminate,
    }

    #[derive(Debug)]
    pub struct InputThread {
        handle: JoinHandle<anyhow::Result<()>>,
    }

    impl InputThread {
        fn send_data(sender: mpsc::Sender<DataType>, data: DataType) -> Option<()> {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(sender.send(data))
                .map_err(|_| error!("Unable to send text"))
                .ok()
        }

        pub fn get_input(sender: mpsc::Sender<DataType>) -> anyhow::Result<()> {
            let mut rl = DefaultEditor::new()?;

            loop {
                match rl.readline(">> ") {
                    Ok(line) => {
                        if line.is_empty() {
                            continue;
                        }
                        Self::send_data(sender.clone(), DataType::Data(line.trim().to_string()));
                        trace!("Read {} bytes from stdin", line.len());
                    }
                    Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
                        Self::send_data(sender, DataType::Terminate);
                        break;
                    }
                    Err(e) => return Err(anyhow!("Got error while read line: {:?}", e)),
                }
            }

            Ok(())
        }

        pub fn start(sender: mpsc::Sender<DataType>) -> Self {
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

pub use inner::{DataType, InputThread};
