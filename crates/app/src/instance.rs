//! One app per user session: the first start owns a socket in
//! `$XDG_RUNTIME_DIR`; a later start asks it to bring its window to the
//! front and exits, so two apps never write the same `sessions.json`.

use std::fs;
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::fs::{FileTypeExt as _, OpenOptionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

const SOCKET_NAME: &str = "my-terminal-instance.sock";
const ACTIVATE: &str = "activate";
/// How long a later start waits for the running app's socket.
const CONNECT_TRIES: u32 = 50;
const CONNECT_PAUSE: std::time::Duration = std::time::Duration::from_millis(20);
/// A client that sends nothing is dropped after this.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// Result of trying to be the running app.
#[derive(Debug)]
pub enum Claim {
    /// No other app runs: this one listens on the socket and holds the
    /// lock file (released when the process ends).
    Primary(UnixListener, fs::File),
    /// Another app runs and was asked to come to the front.
    Secondary,
}

/// `$XDG_RUNTIME_DIR/my-terminal-instance.sock`; `None` without a private
/// runtime directory (then every start runs on its own).
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(SOCKET_NAME))
}

/// Becomes the running app, or asks the running one to come to the front.
/// A lock file next to the socket decides who runs, so two starts at the
/// same moment never both win. A socket left by a crashed app is replaced;
/// any other file at `path` is never touched.
pub fn claim(path: &Path) -> io::Result<Claim> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path.with_extension("lock"))?;
    match lock.try_lock() {
        Ok(()) => {
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_socket() => fs::remove_file(path)?,
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("{} exists and is not a socket", path.display()),
                    ));
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            Ok(Claim::Primary(UnixListener::bind(path)?, lock))
        }
        Err(fs::TryLockError::WouldBlock) => {
            // The running app may still be binding its socket.
            for _ in 0..CONNECT_TRIES {
                if let Ok(mut stream) = UnixStream::connect(path) {
                    stream.write_all(format!("{ACTIVATE}\n").as_bytes())?;
                    return Ok(Claim::Secondary);
                }
                std::thread::sleep(CONNECT_PAUSE);
            }
            Err(io::Error::other(
                "another my-terminal holds the lock but does not answer",
            ))
        }
        Err(fs::TryLockError::Error(err)) => Err(err),
    }
}

/// Forwards every activation request to `sender` from a background thread,
/// until the receiver is dropped.
pub fn listen(listener: UnixListener, sender: Sender<()>) {
    let spawned = std::thread::Builder::new()
        .name("instance-listener".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                if stream.set_read_timeout(Some(READ_TIMEOUT)).is_err() {
                    continue;
                }
                let mut line = String::new();
                // One short line per connection; anything else is ignored.
                let read = BufReader::new(stream.take(64)).read_line(&mut line);
                if read.is_ok() && line.trim() == ACTIVATE && sender.send(()).is_err() {
                    return;
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("my-terminal: cannot listen for other starts: {err}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "my-terminal-instance-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_start_activates_the_first() {
        let dir = temp_dir("second");
        let path = dir.join(SOCKET_NAME);
        let Claim::Primary(listener, _lock) = claim(&path).unwrap() else {
            panic!("first start must be primary");
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        listen(listener, sender);
        assert!(matches!(claim(&path).unwrap(), Claim::Secondary));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(()));
        assert!(matches!(claim(&path).unwrap(), Claim::Secondary));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn socket_of_a_crashed_app_is_replaced() {
        let dir = temp_dir("stale");
        let path = dir.join(SOCKET_NAME);
        drop(UnixListener::bind(&path).unwrap());
        assert!(
            path.exists(),
            "the socket file stays after the listener is gone"
        );
        assert!(matches!(claim(&path).unwrap(), Claim::Primary(..)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn other_files_are_left_alone() {
        let dir = temp_dir("other");
        let path = dir.join(SOCKET_NAME);
        fs::write(&path, "not a socket").unwrap();
        assert!(claim(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not a socket");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn simultaneous_starts_make_exactly_one_primary() {
        let dir = temp_dir("race");
        let path = dir.join(SOCKET_NAME);
        for _ in 0..200 {
            let claims: Vec<Claim> = std::thread::scope(|scope| {
                let a = scope.spawn(|| claim(&path).unwrap());
                let b = scope.spawn(|| claim(&path).unwrap());
                vec![a.join().unwrap(), b.join().unwrap()]
            });
            let primaries = claims
                .iter()
                .filter(|c| matches!(c, Claim::Primary(..)))
                .count();
            assert_eq!(primaries, 1);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_silent_client_does_not_block_activation() {
        let dir = temp_dir("silent");
        let path = dir.join(SOCKET_NAME);
        let Claim::Primary(listener, _lock) = claim(&path).unwrap() else {
            panic!("first start must be primary");
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        listen(listener, sender);
        let _silent = UnixStream::connect(&path).unwrap();
        assert!(matches!(claim(&path).unwrap(), Claim::Secondary));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(()));
        fs::remove_dir_all(dir).unwrap();
    }
}
