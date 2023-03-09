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

mod schandler_id {
    use crate::datastructures::FromQueryString;
    use serde_derive::Deserialize;
    #[derive(Copy, Clone, Debug, Deserialize)]
    pub struct SchandlerId {
        #[serde(rename = "schandlerid")]
        schandler_id: i64,
    }
    impl SchandlerId {
        pub fn schandler_id(&self) -> i64 {
            self.schandler_id
        }
    }
    impl FromQueryString for SchandlerId {}
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
pub use schandler_id::SchandlerId;
use serde::Deserialize;
