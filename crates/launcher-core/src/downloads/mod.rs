mod plan;
mod verify;
mod worker;

pub use plan::DownloadSpec;
pub(crate) use verify::verify_file;
pub use worker::{
    DownloadCancellationToken, DownloadHttpClient, DownloadProgress, DownloadService,
    DownloadTimeouts, Jitter, ProgressSink, Sleeper,
};

#[cfg(test)]
mod tests;
