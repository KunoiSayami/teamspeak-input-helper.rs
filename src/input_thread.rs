mod inner {
    use anyhow::anyhow;
    use log::trace;
    use tokio::io::{stdin, AsyncBufReadExt, BufReader};
    use tokio::sync::mpsc;

    #[derive(Clone, Debug)]
    pub enum DataType {
        Data(String),
        Terminate,
    }

    pub async fn get_input(sender: mpsc::Sender<DataType>) -> anyhow::Result<()> {
        let mut reader = BufReader::new(stdin());
        let mut s = String::new();

        loop {
            let size = reader
                .read_line(&mut s)
                .await
                .map_err(|e| anyhow!("Got error while read from stdin: {:?}", e))?;
            if size == 0 {
                continue;
            }
            sender
                .send(DataType::Data(s.trim().to_string()))
                .await
                .map_err(|_| anyhow!("Unable to send text"))
                .ok();
            s.clear();
            trace!("Read {} bytes from stdin", size);
        }
    }
}

pub use inner::{get_input, DataType};
