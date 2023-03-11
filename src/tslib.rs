mod ts_socket {
    use crate::datastructures::{
        FromQueryString, NotifyTextMessage, QueryError, QueryResult, QueryStatus, SchandlerId,
    };
    use anyhow::anyhow;
    use log::{error, trace, warn};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const BUFFER_SIZE: usize = 512;

    pub struct TeamspeakConnection {
        conn: TcpStream,
    }

    impl TeamspeakConnection {
        fn decode_status(content: String) -> QueryResult<String> {
            for line in content.lines() {
                if line.trim().starts_with("error ") {
                    let status = QueryStatus::try_from(line)?;

                    return status.into_result(content);
                }
            }
            Err(QueryError::static_empty_response())
        }

        /*        pub async fn read_data_long(&mut self) -> anyhow::Result<Option<String>> {
            let mut buffer = [0u8; BUFFER_SIZE];
            let mut ret = String::new();

            let mut wait = Duration::from_millis(400);

            let mut loop_timer = 5;

            loop {
                let size = if let Ok(data) =
                    tokio::time::timeout(wait, self.conn.read(&mut buffer)).await
                {
                    match data {
                        Ok(size) => {
                            wait = Duration::from_millis(100);
                            ret.push_str(&dbg!(String::from_utf8_lossy(&buffer[..size])));
                            size
                        }
                        Err(e) => return Err(anyhow!("Got error while read data: {:?}", e)),
                    }
                } else {
                    0
                };
                loop_timer -= 1;

                if (loop_timer == 0 && size < BUFFER_SIZE)
                    || dbg!(ret.contains("error id=") && ret.ends_with("\n\r"))
                {
                    break;
                }
            }
            trace!("receive => {:?}", &ret);
            Ok(Some(ret))
        }*/

        pub async fn read_data(&mut self) -> anyhow::Result<Option<String>> {
            let mut buffer = [0u8; BUFFER_SIZE];
            let mut ret = String::new();
            loop {
                let size = if let Ok(data) =
                    tokio::time::timeout(Duration::from_secs(2), self.conn.read(&mut buffer)).await
                {
                    match data {
                        Ok(size) => size,
                        Err(e) => return Err(anyhow!("Got error while read data: {:?}", e)),
                    }
                } else {
                    return Ok(None);
                };

                ret.push_str(&String::from_utf8_lossy(&buffer[..size]));
                if size < BUFFER_SIZE || (ret.contains("error id=") && ret.ends_with("\n\r")) {
                    break;
                }
            }
            trace!("receive => {:?}", &ret);
            Ok(Some(ret))
        }

        async fn write_data(&mut self, payload: &str) -> anyhow::Result<()> {
            debug_assert!(payload.ends_with("\n\r"));
            trace!("send => {:?}", payload);
            self.conn
                .write(payload.as_bytes())
                .await
                .map(|size| {
                    if size != payload.as_bytes().len() {
                        error!(
                            "Error payload size mismatch! expect {} but {} found. payload: {:?}",
                            payload.as_bytes().len(),
                            size,
                            payload
                        )
                    }
                })
                .map_err(|e| anyhow!("Got error while send data: {:?}", e))?;
            Ok(())
        }

        pub async fn keep_alive(&mut self) -> QueryResult<bool> {
            let line = self.query_one_non_error::<String>("whoami\n\r").await?;
            Ok(line.contains("clid=") && line.contains("cid="))
        }

        async fn basic_operation(&mut self, payload: &str) -> QueryResult<()> {
            let data = self.write_and_read(payload).await?;
            Self::decode_status(data).map(|_| ())
        }

        async fn write_and_read(&mut self, payload: &str) -> anyhow::Result<String> {
            self.write_data(payload).await?;
            self.read_data()
                .await?
                .ok_or_else(|| anyhow!("Return data is None"))
        }

        pub async fn connect(server: &str, port: u16) -> anyhow::Result<Self> {
            let conn = TcpStream::connect(format!("{}:{}", server, port))
                .await
                .map_err(|e| anyhow!("Got error while connect to {}:{} {:?}", server, port, e))?;

            let mut self_ = Self { conn };

            tokio::time::sleep(Duration::from_millis(10)).await;

            let content = self_
                .read_data()
                .await
                .map_err(|e| anyhow!("Got error in connect while read content: {:?}", e))?;

            if content.is_none() {
                warn!("Read none data.");
            }

            Ok(self_)
        }

        pub async fn register_event(&mut self) -> QueryResult<()> {
            self.basic_operation("clientnotifyregister schandlerid=0 event=notifytextmessage\n\r")
                .await
        }

        pub async fn login(&mut self, api_key: &str) -> QueryResult<()> {
            let payload = format!("auth apikey={}\n\r", api_key);
            self.basic_operation(payload.as_str()).await
        }

        async fn send_text_message(
            &mut self,
            mode: i64,
            server_id: i64,
            client_id: i64,
            text: &str,
        ) -> QueryResult<()> {
            let payload = format!(
                "sendtextmessage schandlerid={server_id} targetmode={mode} target={client_id} msg={text}\n\r",
                server_id = server_id,
                mode = mode,
                client_id = client_id,
                text = Self::escape(text)
            );
            let data = self.write_and_read(&payload).await.map(|s| {
                if s.contains("\n\r") {
                    s.split_once("\n\r").unwrap().0.to_string()
                } else {
                    s
                }
            })?;
            if data.starts_with("notifytextmessage") {
                let (_, data) = data
                    .split_once("notifytextmessage ")
                    .ok_or_else(|| QueryError::send_message_error(data.clone()))?;
                let r = NotifyTextMessage::from_query(data)
                    .map_err(|_| QueryError::decode_error(data))?;
                //trace!("{:?} {:?}", r.msg(), text);
                if !r.msg().eq(text) {
                    return Err(QueryError::send_message_error(
                        "None (No equal)".to_string(),
                    ));
                }
            } else {
                return Self::decode_status(data).map(|_| ());
            }
            Ok(())
        }

        #[allow(dead_code)]
        pub async fn send_private_message(
            &mut self,
            server_id: i64,
            client_id: i64,
            text: &str,
        ) -> QueryResult<()> {
            self.send_text_message(1, server_id, client_id, text).await
        }

        pub async fn send_channel_message(
            &mut self,
            server_id: i64,
            text: &str,
        ) -> QueryResult<()> {
            self.send_text_message(2, server_id, 0, text).await
        }

        fn escape(s: &str) -> String {
            s.replace('\\', "\\\\")
                .replace(' ', "\\s")
                .replace('/', "\\/")
        }

        fn decode_status_with_result<T: FromQueryString + Sized>(
            data: String,
        ) -> QueryResult<Option<Vec<T>>> {
            let content = Self::decode_status(data)?;

            for line in content.lines() {
                if !line.starts_with("error ") {
                    let mut v = Vec::new();
                    for element in line.split('|') {
                        v.push(T::from_query(element)?);
                    }
                    return Ok(Some(v));
                }
            }
            Ok(None)
        }

        async fn query_operation_non_error<T: FromQueryString + Sized>(
            &mut self,
            payload: &str,
        ) -> QueryResult<Vec<T>> {
            let data = self.write_and_read(payload).await?;
            let ret = Self::decode_status_with_result(data)?;
            Ok(ret
                .ok_or_else(|| panic!("Can't find result line, payload => {}", payload))
                .unwrap())
        }

        async fn query_one_non_error<T: FromQueryString + Sized>(
            &mut self,
            payload: &str,
        ) -> QueryResult<T> {
            self.query_operation_non_error(payload)
                .await
                .map(|mut v| v.swap_remove(0))
        }

        // TODO: Need test in no connection
        pub async fn get_current_server_tab(&mut self) -> QueryResult<SchandlerId> {
            self.query_one_non_error("currentschandlerid\n\r").await
        }
    }
}

pub use ts_socket::TeamspeakConnection;
