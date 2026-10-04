//! Bounded setup client for the fixture's private daemon only.
//! It registers real folders/devices; it never creates a pane capability.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

pub struct Renderer {
    stream: BufReader<TcpStream>,
    snapshot: Value,
}

impl Renderer {
    pub fn connect(port: u16, token: &str) -> Result<Self> {
        let mut socket = TcpStream::connect(("127.0.0.1", port))?;
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        socket.set_write_timeout(Some(Duration::from_secs(2)))?;
        write!(
            socket,
            "GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        )?;
        let mut stream = BufReader::new(socket);
        let mut line = String::new();
        stream.read_line(&mut line)?;
        ensure!(
            line.starts_with("HTTP/1.1 101"),
            "private renderer upgrade refused"
        );
        let mut accepted = false;
        let mut header_bytes = line.len();
        loop {
            line.clear();
            stream.read_line(&mut line)?;
            header_bytes += line.len();
            ensure!(header_bytes <= 8 * 1024, "private renderer header cap");
            if line == "\r\n" {
                break;
            }
            if line
                .to_ascii_lowercase()
                .starts_with("sec-websocket-accept:")
            {
                accepted = line
                    .split_once(':')
                    .is_some_and(|(_, value)| value.trim() == "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
            }
        }
        ensure!(
            accepted,
            "private renderer handshake differs from RFC challenge"
        );
        let mut renderer = Self {
            stream,
            snapshot: Value::Null,
        };
        renderer.send(
            1,
            &serde_json::to_vec(&json!({"token":token,"schema_version":2}))?,
        )?;
        for _ in 0..8 {
            let frame = renderer.read()?;
            if frame["type"] == "snapshot" {
                renderer.snapshot = frame["payload"]["rest"].clone();
                return Ok(renderer);
            }
        }
        bail!("private renderer did not provide its initial snapshot")
    }

    pub fn snapshot(&self) -> &Value {
        &self.snapshot
    }

    pub fn event(&mut self, kind: &str, payload: Value) -> Result<()> {
        self.send(
            1,
            &serde_json::to_vec(&json!({"schema_version":2,"kind":kind,"payload":payload}))?,
        )
    }

    fn send(&mut self, opcode: u8, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= 64 * 1024, "private renderer output cap");
        let mask = [31_u8, 17, 89, 203];
        let mut frame = vec![0x80 | opcode];
        if bytes.len() < 126 {
            frame.push(0x80 | bytes.len() as u8);
        } else {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        frame.extend(
            bytes
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        self.stream.get_mut().write_all(&frame)?;
        Ok(())
    }

    fn read(&mut self) -> Result<Value> {
        for _ in 0..8 {
            let mut header = [0; 2];
            self.stream.read_exact(&mut header)?;
            ensure!(
                header[0] & 0x80 != 0 && header[1] & 0x80 == 0,
                "unexpected private renderer frame"
            );
            let length = match header[1] {
                126 => {
                    let mut size = [0; 2];
                    self.stream.read_exact(&mut size)?;
                    u16::from_be_bytes(size).into()
                }
                127 => {
                    let mut size = [0; 8];
                    self.stream.read_exact(&mut size)?;
                    u64::from_be_bytes(size)
                }
                size => u64::from(size),
            };
            ensure!(length <= 8 * 1024 * 1024, "private renderer input cap");
            let mut data = vec![0; length as usize];
            self.stream.read_exact(&mut data)?;
            match header[0] & 15 {
                1 => return serde_json::from_slice(&data).context("private renderer JSON"),
                9 => self.send(10, &data)?,
                8 => bail!("private renderer closed before snapshot"),
                _ => bail!("unexpected private renderer opcode"),
            }
        }
        bail!("private renderer ping cap")
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.send(8, &[]);
        let _ = self.stream.get_mut().shutdown(Shutdown::Both);
    }
}
