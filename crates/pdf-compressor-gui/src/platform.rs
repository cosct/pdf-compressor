//! Desktop integration primitives shared by the GPUI entrypoint.

use std::{
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    thread,
};

use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Stream, prelude::*,
};

/// Keep the same argv contract as the Tauri shell: skip argv[0], preserve order,
/// and accept only case-insensitive `.pdf` paths. Existence is checked later by
/// the queue restore/analysis path so a file association can be handed off
/// before a network mount has finished appearing.
pub fn argv_pdf_paths<I, S>(args: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    args.into_iter()
        .map(Into::into)
        .skip(1)
        .map(PathBuf::from)
        .filter(|path| super::app::format::is_pdf(path))
        .collect()
}

pub fn argv_pdf_paths_from_os() -> Vec<PathBuf> {
    argv_pdf_paths(std::env::args_os().filter_map(|arg| arg.into_string().ok()))
}

/// Linux display preflight. Non-Linux platforms delegate to GPUI.
pub fn check_display_server() -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    {
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("ZED_HEADLESS").is_some() {
            return Ok(());
        }
        let wayland = std::env::var_os("WAYLAND_DISPLAY").filter(|value| !value.is_empty());
        let x11 = std::env::var_os("DISPLAY").filter(|value| !value.is_empty());
        match (wayland, x11) {
            (Some(display), _) => {
                let display_path = Path::new(&display);
                let socket = if display_path.is_absolute() {
                    display_path.to_path_buf()
                } else {
                    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
                        "WAYLAND_DISPLAY is relative but XDG_RUNTIME_DIR is not set.".to_owned()
                    })?;
                    PathBuf::from(runtime).join(display_path)
                };
                connect_display(&socket, "Wayland compositor")
            }
            (None, Some(display)) => {
                let display = display.to_string_lossy();
                let Some(number) = display.strip_prefix(':').and_then(|value| value.split('.').next()) else {
                    return Ok(());
                };
                if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Ok(());
                }
                connect_display(&PathBuf::from(format!("/tmp/.X11-unix/X{number}")), "X11 server")
            }
            (None, None) => Err(
                "No display server detected: WAYLAND_DISPLAY and DISPLAY are unset. Start the app inside a desktop session."
                    .to_owned(),
            ),
        }
    }
}

#[cfg(target_os = "linux")]
fn connect_display(socket: &Path, kind: &str) -> Result<(), String> {
    std::os::unix::net::UnixStream::connect(socket)
        .map(|_| ())
        .map_err(|error| format!("{kind} is unreachable at {}: {error}", socket.display()))
}

pub fn cjk_fonts_available() -> bool {
    match Command::new("fc-list")
        .args([":lang=zh", "family"])
        .output()
    {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| !line.trim().is_empty()),
        // Unknown is deliberately treated as available; an absent fontconfig
        // executable should not prevent startup on minimal systems.
        _ => true,
    }
}

/// One local-socket server per user session. The lock is kept alive by the
/// returned handle; the first process receives newline-delimited JSON messages.
pub struct SingleInstance {
    _listener: Arc<LocalSocketListener>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPdfMessage {
    pub paths: Vec<PathBuf>,
}

impl OpenPdfMessage {
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode(line: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(line)
    }
}

fn socket_name() -> io::Result<interprocess::local_socket::Name<'static>> {
    let id = format!(
        "pdf-compressor-{}",
        std::env::var("USER").unwrap_or_else(|_| "user".into())
    );
    if GenericNamespaced::is_supported() {
        id.to_ns_name::<GenericNamespaced>()
            .map(|name| name.into_owned())
    } else {
        let path = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("{id}.sock"));
        path.to_fs_name::<GenericFilePath>()
            .map(|name| name.into_owned())
    }
}

impl SingleInstance {
    /// Try to forward paths to an existing process. `Ok(true)` means the
    /// caller should exit; `Ok(false)` means no listener answered.
    pub fn forward(paths: &[PathBuf]) -> io::Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        let name = socket_name()?;
        let mut stream = match Stream::connect(name) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::ConnectionAborted
                ) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        stream.write_all(
            &OpenPdfMessage {
                paths: paths.to_vec(),
            }
            .encode()?,
        )?;
        stream.flush()?;
        Ok(true)
    }

    /// Acquire the first-instance listener and spawn a bounded connection
    /// accept loop. A second process should call [`forward`] and exit.
    pub fn acquire(on_paths: impl Fn(Vec<PathBuf>) + Send + Sync + 'static) -> io::Result<Self> {
        let name = socket_name()?;
        let listener = ListenerOptions::new().name(name).create_sync()?;
        let listener = Arc::new(listener);
        let callback = Arc::new(on_paths);
        let listener_for_thread = Arc::clone(&listener);
        thread::Builder::new()
            .name("pdf-compressor-instance".into())
            .spawn(move || {
                for connection in listener_for_thread.incoming() {
                    let Ok(stream) = connection else { continue };
                    let callback = Arc::clone(&callback);
                    let _ = thread::Builder::new()
                        .name("pdf-compressor-instance-client".into())
                        .spawn(move || {
                            let mut reader = BufReader::new(stream);
                            let mut line = String::new();
                            if reader.read_line(&mut line).is_ok()
                                && let Ok(message) = OpenPdfMessage::decode(line.as_bytes())
                            {
                                let paths = message
                                    .paths
                                    .into_iter()
                                    .filter(|path| super::app::format::is_pdf(path))
                                    .collect();
                                callback(paths);
                            }
                        });
                }
            })?;
        Ok(Self {
            _listener: listener,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_filter_skips_program_and_accepts_uppercase_pdf() {
        let paths = argv_pdf_paths(["pdf-compressor", "/tmp/a.PDF", "--verbose", "/tmp/b.txt"]);
        assert_eq!(paths, vec![PathBuf::from("/tmp/a.PDF")]);
    }

    #[test]
    fn message_roundtrips_and_preserves_order() {
        let message = OpenPdfMessage {
            paths: vec![PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/b.PDF")],
        };
        let decoded = OpenPdfMessage::decode(&message.encode().unwrap()).unwrap();
        assert_eq!(decoded.paths, message.paths);
    }

    #[test]
    fn cjk_parser_accepts_any_nonempty_line() {
        assert!(
            "Noto Sans CJK SC\n"
                .lines()
                .any(|line| !line.trim().is_empty())
        );
        assert!(!"\n \t\n".lines().any(|line| !line.trim().is_empty()));
    }
}
