#[allow(dead_code)]
#[derive(Debug)]
pub enum RequestError {
    RequestErr(reqwless::Error),
    Utf8Error(core::str::Utf8Error),
    JsonErr(serde_json_core::de::Error),
}

impl From<reqwless::Error> for RequestError {
    fn from(value: reqwless::Error) -> Self {
        RequestError::RequestErr(value)
    }
}
