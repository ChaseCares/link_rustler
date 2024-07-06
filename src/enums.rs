use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub enum LinkType {
    Generic,
    Content,
    Unknown,
    Local,
    Mailto,
    InternalError,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct Validity {
    pub valid: Option<Vec<ValidReason>>,
    pub invalid: Option<Vec<InvalidReason>>,
    pub error: Option<CustomError>,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub enum ValidReason {
    CompressionExact,
    CompressionWithinTolerance,
    ScreenshotHashExact,
    ScreenshotHashWithinTolerance,
    PageHash,
    Title,
    Marker,
    Type,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub enum InvalidReason {
    Compression,
    PageHash,
    ScreenshotHash,
    Title,
    Type,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub enum CustomError {
    InsecureCertificate,
    Redirected,
    BadTitle,
    MarkerNotFound,
    UnknownLinkType,
    LinkTypeLocal,
    LinkTypeMailto,
    BadScreenshot,
    PageNotFound,
    PageError,
    Marker,
    Warning,
    WebDriverError,
}
