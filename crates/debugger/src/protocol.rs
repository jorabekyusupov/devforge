//! DBGP framing and bounded XML. No eval, property mutation or arbitrary command passthrough.
use crate::{Error, Result};
use std::{collections::BTreeMap, io::Read, net::TcpStream};
pub const MAX_FRAME: usize = 1024 * 1024;
#[derive(Default)]
pub struct Frames {
    bytes: Vec<u8>,
}
impl Frames {
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        if self.bytes.len() + bytes.len() > MAX_FRAME + 32 {
            return Err(Error::Protocol);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    pub fn pop(&mut self) -> Result<Option<Vec<u8>>> {
        let Some(end) = self.bytes.iter().position(|b| *b == 0) else {
            if self.bytes.len() > 8 {
                return Err(Error::Protocol);
            }
            return Ok(None);
        };
        if end == 0 || end > 8 || !self.bytes[..end].iter().all(u8::is_ascii_digit) {
            return Err(Error::Protocol);
        }
        let n = std::str::from_utf8(&self.bytes[..end])
            .map_err(|_| Error::Protocol)?
            .parse::<usize>()
            .map_err(|_| Error::Protocol)?;
        if n == 0 || n > MAX_FRAME {
            return Err(Error::Protocol);
        }
        if self.bytes.len() < end + 1 + n + 1 {
            return Ok(None);
        }
        if self.bytes[end + 1 + n] != 0 {
            return Err(Error::Protocol);
        }
        let body = self.bytes[end + 1..end + 1 + n].to_vec();
        self.bytes.drain(..end + 1 + n + 1);
        Ok(Some(body))
    }
    pub fn receive(&mut self, stream: &mut TcpStream) -> Result<Option<Vec<u8>>> {
        if let Some(frame) = self.pop()? {
            return Ok(Some(frame));
        }
        let mut bytes = [0u8; 4096];
        match stream.read(&mut bytes) {
            Ok(0) => Err(Error::Unavailable),
            Ok(n) => {
                self.push(&bytes[..n])?;
                self.pop()
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(_) => Err(Error::Unavailable),
        }
    }
}
#[derive(Debug, Default, Clone)]
pub struct Element {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub text: String,
    pub children: Vec<Element>,
}
impl Element {
    pub fn attr(&self, key: &str) -> &str {
        self.attrs.get(key).map(String::as_str).unwrap_or("")
    }
    pub fn child(&self, key: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == key)
    }
}
fn start(bytes: &quick_xml::events::BytesStart<'_>) -> Result<Element> {
    let name = bytes.local_name().as_ref().to_owned();
    if name.len() > 80 {
        return Err(Error::Protocol);
    }
    let mut attrs = BTreeMap::new();
    for a in bytes.attributes() {
        let a = a.map_err(|_| Error::Protocol)?;
        let key = a.key.as_ref().to_owned();
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|_| Error::Protocol)?
            .into_owned();
        if key.len() > 80 || value.len() > 65536 || attrs.len() > 32 {
            return Err(Error::Protocol);
        }
        attrs.insert(key, value);
    }
    Ok(Element {
        name,
        attrs,
        ..Default::default()
    })
}
pub fn xml(bytes: &[u8]) -> Result<Element> {
    use quick_xml::events::Event;
    if bytes.len() > MAX_FRAME {
        return Err(Error::Protocol);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Protocol)?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    let mut count = 0;
    loop {
        match reader.read_event().map_err(|_| Error::Protocol)? {
            Event::Start(e) => {
                count += 1;
                if count > 2048 || stack.len() > 16 {
                    return Err(Error::Protocol);
                }
                stack.push(start(&e)?);
            }
            Event::Empty(e) => {
                count += 1;
                if count > 2048 {
                    return Err(Error::Protocol);
                }
                let e = start(&e)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(e);
                } else if root.replace(e).is_some() {
                    return Err(Error::Protocol);
                }
            }
            Event::End(_) => {
                let e = stack.pop().ok_or(Error::Protocol)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(e);
                } else if root.replace(e).is_some() {
                    return Err(Error::Protocol);
                }
            }
            Event::Text(e) => {
                if let Some(parent) = stack.last_mut() {
                    let value = e.as_ref();
                    parent.text.push_str(
                        &quick_xml::escape::unescape(value).map_err(|_| Error::Protocol)?,
                    );
                }
            }
            Event::CData(e) => {
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(e.as_ref());
                }
            }
            Event::GeneralRef(e) => {
                if let Some(parent) = stack.last_mut() {
                    let reference = e.as_ref();
                    parent.text.push_str(
                        &quick_xml::escape::unescape(&format!("&{reference};"))
                            .map_err(|_| Error::Protocol)?,
                    );
                }
            }
            Event::DocType(_) => return Err(Error::Protocol),
            Event::Eof => break,
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(Error::Protocol);
    }
    root.ok_or(Error::Protocol)
}
pub fn constant_time(a: &str, b: &str) -> bool {
    let mut diff = a.len() ^ b.len();
    for (i, x) in a.bytes().enumerate() {
        diff |= (x ^ b.as_bytes().get(i).copied().unwrap_or_default()) as usize;
    }
    diff == 0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_frames_and_hostile_xml_are_bounded() {
        let mut f = Frames::default();
        f.push(b"4\0<a").unwrap();
        assert!(f.pop().unwrap().is_none());
        f.push(b"/>\0").unwrap();
        assert_eq!(f.pop().unwrap().unwrap(), b"<a/>");
        assert!(f.pop().unwrap().is_none());
        assert!(xml(b"<!DOCTYPE a [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><a>&e;</a>").is_err());
        let p = xml(b"<response status='break'><property name='v'>&lt;x&gt;</property></response>")
            .unwrap();
        assert_eq!(p.child("property").unwrap().text, "<x>");
        assert!(constant_time("hello", "hello"));
        assert!(!constant_time("hello", "hell"));
        let mut f = Frames::default();
        f.push(b"99999999\0").unwrap();
        assert!(f.pop().is_err());
    }
}
