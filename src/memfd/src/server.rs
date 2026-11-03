use memmap2::MmapMut;
use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
use std::env;
use std::fs::{self, File, Permissions};
use std::io::{self, IoSlice, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const BUFFER_SIZE: usize = 4096;
const HEADER_SIZE: usize = 4;
const MAX_PAYLOAD: usize = BUFFER_SIZE - HEADER_SIZE;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Removes only the filesystem socket created by this server.
struct SocketCleanup {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl SocketCleanup {
    fn new(path: PathBuf) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_socket() {
            return Err(io::Error::other("the bound socket path was replaced"));
        }
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn handle_signals(&self) -> io::Result<()> {
        let path = self.path.clone();
        let device = self.device;
        let inode = self.inode;
        ctrlc::set_handler(move || {
            let status = match remove_socket(&path, device, inode) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("memfd-server: socket cleanup: {error}");
                    1
                }
            };
            std::process::exit(status);
        })
        .map_err(io::Error::other)
    }
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if let Err(error) = remove_socket(&self.path, self.device, self.inode) {
            eprintln!("memfd-server: socket cleanup: {error}");
        }
    }
}

fn remove_socket(path: &Path, device: u64, inode: u64) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket() || (metadata.dev(), metadata.ino()) != (device, inode) {
        return Err(io::Error::other(format!(
            "refusing to remove a replaced socket path: {}",
            path.display()
        )));
    }
    fs::remove_file(path)
}

fn create_buffer() -> io::Result<(File, MmapMut)> {
    let file = File::from(memfd_create(
        c"memfd-demo",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )?);
    file.set_len(BUFFER_SIZE as u64)?;
    fcntl_add_seals(&file, SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL)?;
    // The sealed length cannot change. Cooperative peers access bytes only on
    // their turn, and no references into the mapping survive a socket handoff.
    let mapping = unsafe { MmapMut::map_mut(&file)? };
    Ok((file, mapping))
}

fn send_buffer(stream: &UnixStream, file: &File) -> io::Result<()> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    let descriptors = [file.as_fd()];
    if !ancillary.push(SendAncillaryMessage::ScmRights(&descriptors)) {
        return Err(io::Error::other("cannot encode the buffer descriptor"));
    }
    loop {
        match sendmsg(
            stream,
            &[IoSlice::new(b"F")],
            &mut ancillary,
            SendFlags::NOSIGNAL,
        ) {
            Ok(1) => return Ok(()),
            Ok(_) => return Err(io::Error::new(io::ErrorKind::WriteZero, "sending buffer")),
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(io::Error::from(error)),
        }
    }
}

fn expect_control(stream: &mut UnixStream, expected: u8) -> io::Result<()> {
    let mut message = [0];
    stream.read_exact(&mut message).map_err(|error| {
        let detail = match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
                "I/O timed out after 5 seconds".to_owned()
            }
            _ => error.to_string(),
        };
        io::Error::new(
            error.kind(),
            format!("waiting for '{}': {detail}", char::from(expected)),
        )
    })?;
    if message[0] != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "expected control '{}', received {:#04x}",
                char::from(expected),
                message[0]
            ),
        ));
    }
    Ok(())
}

fn payload_length(header: [u8; HEADER_SIZE]) -> io::Result<usize> {
    let length = u32::from_le_bytes(header);
    if length > MAX_PAYLOAD as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("payload length {length} exceeds {MAX_PAYLOAD}"),
        ));
    }
    Ok(length as usize)
}

fn exchange(mut stream: UnixStream) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    expect_control(&mut stream, b'B')?;
    let (file, mut mapping) = create_buffer()?;
    send_buffer(&stream, &file)?;
    expect_control(&mut stream, b'W')?;
    {
        let mut header = [0; HEADER_SIZE];
        header.copy_from_slice(&mapping[..HEADER_SIZE]);
        let length = payload_length(header)?;
        let payload = &mut mapping[HEADER_SIZE..HEADER_SIZE + length];
        println!("Read: {}", String::from_utf8_lossy(payload));
        payload.make_ascii_uppercase();
    }
    stream.write_all(b"D")
}

pub fn run() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    let path = PathBuf::from(args.next().unwrap_or_else(|| "/tmp/memfd-demo.sock".into()));
    if path.as_os_str().is_empty() || args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: memfd-server [socket-path]",
        ));
    }
    let listener = UnixListener::bind(&path).map_err(|error| {
        io::Error::new(error.kind(), format!("binding {}: {error}", path.display()))
    })?;
    let cleanup = SocketCleanup::new(path.clone())?;
    fs::set_permissions(&path, Permissions::from_mode(0o600))?;
    cleanup.handle_signals()?;
    println!("Listening on {}", path.display());

    loop {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = exchange(stream) {
            eprintln!("memfd-server: client exchange: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::fs::fcntl_get_seals;
    use std::os::unix::fs::FileExt;

    #[test]
    fn accepts_empty_and_maximum_payloads() {
        for length in [0, 1, MAX_PAYLOAD as u32] {
            assert_eq!(
                payload_length(length.to_le_bytes()).unwrap(),
                length as usize
            );
        }
    }

    #[test]
    fn rejects_oversized_payloads() {
        for length in [MAX_PAYLOAD as u32 + 1, u32::MAX] {
            assert_eq!(
                payload_length(length.to_le_bytes()).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn buffer_is_shared_and_cannot_be_resized() {
        let (file, mut mapping) = create_buffer().unwrap();
        assert_eq!(file.metadata().unwrap().len(), BUFFER_SIZE as u64);
        assert_eq!(mapping.len(), BUFFER_SIZE);
        assert!(
            fcntl_get_seals(&file)
                .unwrap()
                .contains(SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL)
        );
        assert!(file.set_len(BUFFER_SIZE as u64 - 1).is_err());
        assert!(file.set_len(BUFFER_SIZE as u64 + 1).is_err());

        mapping[HEADER_SIZE] = b'a';
        let mut byte = [0];
        file.read_exact_at(&mut byte, HEADER_SIZE as u64).unwrap();
        assert_eq!(byte, *b"a");
        file.write_all_at(b"b", HEADER_SIZE as u64).unwrap();
        assert_eq!(mapping[HEADER_SIZE], b'b');
    }

    #[test]
    fn validates_control_and_disconnects() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender.write_all(b"BX").unwrap();
        expect_control(&mut receiver, b'B').unwrap();
        assert_eq!(
            expect_control(&mut receiver, b'W').unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        drop(sender);
        assert_eq!(
            expect_control(&mut receiver, b'W').unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
