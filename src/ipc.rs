//! Local socket naming: named pipes on Windows, abstract/unix sockets elsewhere.
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Name, Stream, prelude::*,
};
use std::io;

pub fn base_name() -> String {
    if let Ok(s) = std::env::var("TMUXLIKE_SOCKET") {
        return s;
    }
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "default".into());
    format!("tmuxlike-{user}.sock")
}

fn name() -> io::Result<Name<'static>> {
    let b = base_name();
    if GenericNamespaced::is_supported() {
        b.to_ns_name::<GenericNamespaced>()
    } else {
        std::env::temp_dir().join(b).to_fs_name::<GenericFilePath>()
    }
}

pub fn connect() -> io::Result<Stream> {
    Stream::connect(name()?)
}

pub fn listen() -> io::Result<interprocess::local_socket::Listener> {
    ListenerOptions::new()
        .name(name()?)
        .try_overwrite(true)
        .create_sync()
}
