use crate::{Result, ServingError as E};
use std::{
    io::{Read, Write},
    net::TcpStream,
    path::Path,
    time::Duration,
};
fn record(stream: &mut TcpStream, kind: u8, body: &[u8]) -> Result<()> {
    if body.len() > 65535 {
        return Err(E::ProbeFailed);
    }
    let header = [
        1,
        kind,
        0,
        1,
        (body.len() >> 8) as u8,
        body.len() as u8,
        0,
        0,
    ];
    stream.write_all(&header)?;
    stream.write_all(body)?;
    Ok(())
}
fn length(out: &mut Vec<u8>, len: usize) {
    if len < 128 {
        out.push(len as u8);
    } else {
        out.extend(((len as u32) | 0x80000000).to_be_bytes());
    }
}
pub(crate) fn probe(port: u16, script: &Path, token: &str, version: &str) -> Result<()> {
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(3))
            .map_err(|_| E::ProbeFailed)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    record(&mut stream, 1, &[0, 1, 0, 0, 0, 0, 0, 0])?;
    let path = dunce::simplified(script)
        .to_str()
        .ok_or(E::InvalidConfiguration)?;
    let pairs = [
        ("GATEWAY_INTERFACE", "CGI/1.1"),
        ("REQUEST_METHOD", "GET"),
        ("SCRIPT_FILENAME", path),
        ("SCRIPT_NAME", "/health.php"),
        ("REQUEST_URI", "/health.php"),
        ("SERVER_PROTOCOL", "HTTP/1.1"),
        ("SERVER_NAME", "localhost"),
        ("SERVER_PORT", "80"),
        ("REMOTE_ADDR", "127.0.0.1"),
        ("REDIRECT_STATUS", "200"),
    ];
    let mut params = Vec::new();
    for (name, value) in pairs {
        length(&mut params, name.len());
        length(&mut params, value.len());
        params.extend(name.as_bytes());
        params.extend(value.as_bytes());
    }
    record(&mut stream, 4, &params)?;
    record(&mut stream, 4, &[])?;
    record(&mut stream, 5, &[])?;
    let mut output = Vec::new();
    loop {
        let mut head = [0u8; 8];
        stream.read_exact(&mut head).map_err(|_| E::ProbeFailed)?;
        if head[0] != 1 || head[2..4] != [0, 1] {
            return Err(E::ProbeFailed);
        }
        let len = u16::from_be_bytes([head[4], head[5]]) as usize;
        if output.len() + len > 128 * 1024 {
            return Err(E::ProbeFailed);
        }
        let mut body = vec![0; len + head[6] as usize];
        stream.read_exact(&mut body).map_err(|_| E::ProbeFailed)?;
        if head[1] == 6 {
            output.extend_from_slice(&body[..len]);
        }
        if head[1] == 3 {
            break;
        }
    }
    let text = std::str::from_utf8(&output).map_err(|_| E::ProbeFailed)?;
    let json = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .map(|(_, body)| body)
        .ok_or(E::ProbeFailed)?;
    let data: serde_json::Value = serde_json::from_str(json).map_err(|_| E::ProbeFailed)?;
    if data["token"] != token
        || !data["php"]
            .as_str()
            .is_some_and(|v| v.starts_with(version.split('-').next().unwrap()))
    {
        return Err(E::ProbeFailed);
    }
    Ok(())
}
