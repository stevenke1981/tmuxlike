//! Wire protocol between client and server: u32-LE length prefix + JSON.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command {
    Attach {
        session: Option<String>,
        create: bool,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        /// create the session without attaching (new -d)
        #[serde(default)]
        detached: bool,
    },
    List,
    Kill {
        session: String,
    },
    KillServer,
    /// Run a server command (source-file, set, bind-key ...)
    Run {
        line: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMsg {
    Hello(Command),
    Event(crossterm::event::Event),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMsg {
    Output(String),
    Text(String),
    Exit(String),
    Error(String),
}

pub fn write_msg<T: Serialize>(w: &mut dyn Write, msg: &T) -> io::Result<()> {
    let data = serde_json::to_vec(msg).map_err(io::Error::other)?;
    let mut buf = Vec::with_capacity(data.len() + 4);
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(&data);
    w.write_all(&buf)?;
    w.flush()
}

pub fn read_msg<T: DeserializeOwned>(r: &mut dyn Read) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > 64 * 1024 * 1024 {
        return Err(io::Error::other("message too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let mut v = Vec::new();
        write_msg(&mut v, &ServerMsg::Text("hi".into())).unwrap();
        let m: ServerMsg = read_msg(&mut &v[..]).unwrap();
        assert!(matches!(m, ServerMsg::Text(s) if s == "hi"));
    }
}
