// Character device backend framework.

use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

/// Trait for character device backends.
///
/// A chardev provides byte-level I/O used by serial ports,
/// consoles, and similar devices.
pub trait Chardev: Send {
    /// Read one byte if available.
    fn read(&mut self) -> Option<u8>;

    /// Write one byte to the backend.
    fn write(&mut self, data: u8);

    /// Returns `true` if data is available to read.
    fn can_read(&self) -> bool;

    /// Start delivering input bytes via the callback.
    /// The backend is responsible for how (thread, poll,
    /// etc.). The callback is invoked with each byte.
    fn start_input(&mut self, _cb: Arc<Mutex<dyn FnMut(u8) + Send>>) {}
}

// -- CharFrontend ------------------------------------------------

/// Bridges a device (frontend) to a chardev backend.
pub struct CharFrontend {
    backend: Box<dyn Chardev>,
}

impl CharFrontend {
    pub fn new(backend: Box<dyn Chardev>) -> Self {
        Self { backend }
    }

    /// Write a byte slice to the backend.
    pub fn write(&mut self, data: &[u8]) {
        for &b in data {
            self.backend.write(b);
        }
    }

    /// Start receiving input from the backend. The
    /// callback is invoked for each byte received.
    pub fn start_input(&mut self, cb: Arc<Mutex<dyn FnMut(u8) + Send>>) {
        self.backend.start_input(cb);
    }
}

// -- NullChardev -------------------------------------------------

/// Discards all output and never produces input.
pub struct NullChardev;

impl Chardev for NullChardev {
    fn read(&mut self) -> Option<u8> {
        None
    }

    fn write(&mut self, _data: u8) {}

    fn can_read(&self) -> bool {
        false
    }
}

// -- StdioChardev ------------------------------------------------

/// Wraps host stdin/stdout. Spawns a reader thread when
/// start_input() is called.
pub struct StdioChardev {
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl StdioChardev {
    pub fn new() -> Self {
        Self { _thread: None }
    }
}

impl Default for StdioChardev {
    fn default() -> Self {
        Self::new()
    }
}

impl Chardev for StdioChardev {
    fn read(&mut self) -> Option<u8> {
        None
    }

    fn write(&mut self, data: u8) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(&[data]);
        let _ = out.flush();
    }

    fn can_read(&self) -> bool {
        false
    }

    fn start_input(&mut self, cb: Arc<Mutex<dyn FnMut(u8) + Send>>) {
        let handle = std::thread::spawn(move || {
            use std::io::Read;
            let stdin = std::io::stdin();
            let mut buf = [0u8; 1];
            loop {
                match stdin.lock().read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(mut f) = cb.lock() {
                            f(buf[0]);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        self._thread = Some(handle);
    }
}

// -- SocketChardev -----------------------------------------------

/// Unix-socket backed chardev (for integration testing).
pub struct SocketChardev {
    stream: Option<UnixStream>,
}

impl SocketChardev {
    pub fn new() -> Self {
        Self { stream: None }
    }

    pub fn connect(&mut self, path: &str) -> std::io::Result<()> {
        let s = UnixStream::connect(path)?;
        s.set_nonblocking(true)?;
        self.stream = Some(s);
        Ok(())
    }
}

impl Default for SocketChardev {
    fn default() -> Self {
        Self::new()
    }
}

impl Chardev for SocketChardev {
    fn read(&mut self) -> Option<u8> {
        use std::io::Read;
        let stream = self.stream.as_mut()?;
        let mut buf = [0u8; 1];
        match stream.read(&mut buf) {
            Ok(1) => Some(buf[0]),
            _ => None,
        }
    }

    fn write(&mut self, data: u8) {
        if let Some(ref mut stream) = self.stream {
            let _ = stream.write_all(&[data]);
        }
    }

    fn can_read(&self) -> bool {
        self.stream.is_some()
    }
}
