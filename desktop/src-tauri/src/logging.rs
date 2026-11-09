//! File + stderr logging with auth-token censoring and simple rotation,
//! mirroring the Qt `Log` helper (`profiles/<id>/logs/jellyfin-desktop.log`,
//! previous runs kept as `.log.1` ... `.log.10`).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

const MAX_LOGS: usize = 10;

/// Replaces the 32 characters following known token markers with `x`.
pub fn censor(msg: &str) -> String {
    const MARKERS: [&str; 6] = [
        "api_key=",
        "X-MediaBrowser-Token%3D",
        "X-MediaBrowser-Token=",
        "ApiKey=",
        "AccessToken=",
        "AccessToken\":\"",
    ];
    let mut out = msg.to_string();
    for marker in MARKERS {
        let mut start = 0;
        while let Some(pos) = out[start..].find(marker) {
            let begin = start + pos + marker.len();
            let end = (begin + 32).min(out.len());
            // Avoid slicing inside a multi-byte char.
            let end = (begin..=end).rev().find(|&i| out.is_char_boundary(i)).unwrap_or(begin);
            if end > begin {
                out.replace_range(begin..end, &"x".repeat(end - begin));
            }
            start = end;
        }
    }
    out
}

struct CensoringFile(Mutex<File>);

struct CensoringWriter<'a>(&'a CensoringFile);

impl Write for CensoringWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let censored = censor(&text);
        let mut file = self.0 .0.lock().unwrap();
        file.write_all(censored.as_bytes())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0 .0.lock().unwrap().flush()
    }
}

impl<'a> MakeWriter<'a> for CensoringFile {
    type Writer = CensoringWriter<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        CensoringWriter(self)
    }
}

fn rotate(log_dir: &Path) -> PathBuf {
    let main = log_dir.join("jellyfin-desktop.log");
    for i in (1..MAX_LOGS).rev() {
        let from = log_dir.join(format!("jellyfin-desktop.log.{i}"));
        let to = log_dir.join(format!("jellyfin-desktop.log.{}", i + 1));
        if from.exists() {
            let _ = fs::remove_file(&to);
            let _ = fs::rename(&from, &to);
        }
    }
    if main.exists() {
        let first = log_dir.join("jellyfin-desktop.log.1");
        let _ = fs::remove_file(&first);
        let _ = fs::rename(&main, &first);
    }
    main
}

/// `level` is the file log level name (debug/info/warn/error); the terminal
/// gets the same unless RUST_LOG overrides it.
pub fn init(log_dir: &Path, level: &str) -> PathBuf {
    let _ = fs::create_dir_all(log_dir);
    let path = rotate(log_dir);
    let file = File::create(&path).or_else(|_| File::create(std::env::temp_dir().join("jellyfin-desktop.log")));

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(io::stderr).with_target(true);
    match file {
        Ok(f) => {
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(CensoringFile(Mutex::new(f)))
                .with_ansi(false)
                .with_target(true);
            tracing_subscriber::registry().with(filter).with(stderr_layer).with(file_layer).init();
        }
        Err(_) => {
            tracing_subscriber::registry().with(filter).with(stderr_layer).init();
        }
    }
    path
}
