mod inner {
    use anyhow::anyhow;
    use log::trace;
    use rustyline_async::{Readline, ReadlineError};
    use tokio::sync::mpsc;

    #[derive(Clone, Debug)]
    pub enum DataType {
        Data(String),
        Terminate,
    }

    pub async fn get_input(
        mut reader: Readline,
        sender: mpsc::Sender<DataType>,
    ) -> anyhow::Result<()> {
        loop {
            match reader.readline().await {
                Ok(line) => {
                    if line.is_empty() {
                        continue;
                    }
                    sender
                        .send(DataType::Data(line.trim().to_string()))
                        .await
                        .map_err(|_| anyhow!("Unable to send text"))
                        .ok();
                    trace!("Read {} bytes from stdin", line.len());
                }
                Err(ReadlineError::Interrupted) => {
                    sender.send(DataType::Terminate).await.ok();
                    ()
                }
                Err(ReadlineError::Eof) => {}
                Err(e) => return Err(anyhow!("Got error while read line: {:?}", e)),
            }
        }
    }
}

pub use inner::{get_input, DataType};
