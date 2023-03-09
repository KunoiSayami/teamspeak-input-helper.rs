pub mod socket {
    use super::{FromQueryString, QueryResult, QueryStatus};
    use crate::datastructures::{NotifyTextMessage, QueryError};
    use anyhow::anyhow;
    use log::{error, trace, warn};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const BUFFER_SIZE: usize = 512;

    pub struct SocketConn {
        conn: TcpStream,
    }

    impl SocketConn {
        fn decode_status(content: String) -> QueryResult<String> {
            for line in content.lines() {
                if line.trim().starts_with("error ") {
                    let status = QueryStatus::try_from(line)?;

                    return status.into_result(content);
                }
            }
            Err(QueryError::static_empty_response())
        }

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

        pub(crate) async fn keep_alive(&mut self) -> anyhow::Result<()> {
            self.write_data("currentschandlerid\n\r").await
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
            client_id: i64,
            text: &str,
        ) -> QueryResult<()> {
            let payload = format!(
                "sendtextmessage targetmode={mode} target={client_id} msg={text}\n\r",
                mode = mode,
                client_id = client_id,
                text = Self::escape(text)
            );
            let data = self.write_and_read(&payload).await?;
            if data.starts_with("notifytextmessage") {
                let (_, data) = data
                    .split_once("notifytextmessage ")
                    .ok_or_else(|| QueryError::send_message_error(data.clone()))?;
                let r = NotifyTextMessage::from_query(data)
                    .map_err(|_| QueryError::decode_error(data))?;
                //debug!("{:?} {:?}", r.msg(), text);
                if !r.msg().eq(text) {
                    return Err(QueryError::send_message_error("None".to_string()));
                }
            } else {
                return Self::decode_status(data).map(|_| ());
            }
            Ok(())
        }

        #[allow(dead_code)]
        pub async fn send_private_message(
            &mut self,
            client_id: i64,
            text: &str,
        ) -> QueryResult<()> {
            self.send_text_message(1, client_id, text).await
        }

        pub async fn send_channel_message(&mut self, text: &str) -> QueryResult<()> {
            self.send_text_message(2, 0, text).await
        }
        fn escape(s: &str) -> String {
            s.replace('\\', "\\\\")
                .replace(' ', "\\s")
                .replace('/', "\\/")
        }
    }
}

pub trait FromQueryString: for<'de> Deserialize<'de> {
    fn from_query(data: &str) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        serde_teamspeak_querystring::from_str(data)
            .map_err(|e| anyhow::anyhow!("Got parser error: {:?}", e))
    }
}

impl FromQueryString for () {
    fn from_query(_data: &str) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        Ok(())
    }
}

mod notifies {
    use crate::datastructures::FromQueryString;
    use serde_derive::Deserialize;

    #[derive(Clone, Debug, Deserialize)]
    pub struct NotifyTextMessage {
        /*#[serde(rename = "targetmode", default)]
        target_mode: i8,*/
        msg: String,
        /*#[serde(rename = "invokerid", default)]
        invoker_id: i64,*/
        #[serde(rename = "invokername", default)]
        invoker_name: String,
        /*#[serde(rename = "invokeruid", default)]
        invoker_uid: String,*/
    }

    impl NotifyTextMessage {
        pub fn msg(&self) -> &str {
            &self.msg
        }
        pub fn invoker_name(&self) -> &str {
            &self.invoker_name
        }
    }

    impl FromQueryString for NotifyTextMessage {}
}

pub mod query_status {
    use crate::datastructures::{QueryError, QueryResult};
    use anyhow::anyhow;
    use serde_derive::Deserialize;

    #[derive(Clone, Debug, Deserialize)]
    pub struct WebQueryStatus {
        code: i32,
        message: String,
    }

    impl WebQueryStatus {
        pub fn into_status(self) -> QueryStatus {
            QueryStatus {
                id: self.code,
                msg: self.message,
            }
        }
    }

    impl From<WebQueryStatus> for QueryStatus {
        fn from(status: WebQueryStatus) -> Self {
            status.into_status()
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct QueryStatus {
        id: i32,
        msg: String,
    }

    impl Default for QueryStatus {
        fn default() -> Self {
            Self {
                id: 0,
                msg: "ok".to_string(),
            }
        }
    }

    impl QueryStatus {
        pub fn id(&self) -> i32 {
            self.id
        }
        pub fn msg(&self) -> &String {
            &self.msg
        }

        pub fn into_err(self) -> QueryError {
            QueryError::from(self)
        }

        pub fn into_result<T>(self, ret: T) -> QueryResult<T> {
            if self.id == 0 {
                return Ok(ret);
            }
            Err(self.into_err())
        }
    }

    impl TryFrom<&str> for QueryStatus {
        type Error = anyhow::Error;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            let (_, line) = value
                .split_once("error ")
                .ok_or_else(|| anyhow!("Split error: {}", value))?;
            serde_teamspeak_querystring::from_str(line)
                .map_err(|e| anyhow!("Got error while parse string: {:?} {:?}", line, e))
        }
    }
}

mod query_result {
    use crate::datastructures::QueryStatus;
    use anyhow::Error;
    use std::fmt::{Display, Formatter};

    pub type QueryResult<T> = Result<T, QueryError>;

    #[derive(Clone, Default, Debug)]
    pub struct QueryError {
        code: i32,
        message: String,
    }

    impl QueryError {
        pub fn static_empty_response() -> Self {
            Self {
                code: -1,
                message: "Expect result but none found.".to_string(),
            }
        }

        pub fn send_message_error(data: String) -> Self {
            Self {
                code: -2,
                message: format!("Unable to send message, raw data => {}", data),
            }
        }

        pub fn decode_error(data: &str) -> Self {
            Self {
                code: -3,
                message: format!("Decode result error: {}", data),
            }
        }
    }

    impl Display for QueryError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}({})", self.message, self.code)
        }
    }

    impl std::error::Error for QueryError {}

    impl From<QueryStatus> for QueryError {
        fn from(status: QueryStatus) -> Self {
            Self {
                code: status.id(),
                message: status.msg().clone(),
            }
        }
    }

    impl From<Error> for QueryError {
        fn from(s: Error) -> Self {
            Self {
                code: -2,
                message: s.to_string(),
            }
        }
    }
}

pub use notifies::NotifyTextMessage;
pub use query_result::{QueryError, QueryResult};
pub use query_status::QueryStatus;
use serde::Deserialize;
pub use socket::SocketConn;
