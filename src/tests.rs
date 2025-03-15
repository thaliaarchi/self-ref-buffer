use std::{
    borrow::Cow,
    io::{self, Read},
};

use crate::{pair::BufPair, reader::SharedReader};

struct LimitReader<R> {
    reader: R,
    limit: usize,
}
impl<R: Read> LimitReader<R> {
    pub fn new(reader: R, limit: usize) -> Self {
        LimitReader { reader, limit }
    }
}
impl<R: Read> Read for LimitReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = buf.len().min(self.limit);
        self.reader.read(&mut buf[..len])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Ident<'a> {
    author: &'a [u8],
    committer: &'a [u8],
}

#[test]
fn self_ref() {
    let s = "author: Author
committer: Committer";
    let mut b = LimitReader::new(s.as_bytes(), 8);
    let mut r = SharedReader::new(&mut b, 100);

    let ident = BufPair::new(&mut r, |builder| -> io::Result<_> {
        let author = strip_lf(builder.read_line()?);
        let Some(author) = author.strip_prefix(b"author: ") else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected author directive",
            ));
        };
        let committer = strip_lf(builder.read_line()?);
        let Some(committer) = committer.strip_prefix(b"committer: ") else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected committer directive",
            ));
        };
        // Does not compile: lifetime may not live long enough
        Ok(Ident { author, committer })
    })
    .unwrap();
    assert_eq!(
        ident.dependent(),
        &Ident {
            author: b"Author",
            committer: b"Committer"
        }
    );
}

#[test]
fn read_line() {
    let s = "Lorem ipsum dolor sit amet,
consectetur adipiscing elit,
sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.

Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.";
    let mut b = LimitReader::new(s.as_bytes(), 8);
    let mut r = SharedReader::new(&mut b, 100);

    let init_buf = r.buffer();
    let line1 = r.read_line().unwrap();
    assert_eq!(utf8(line1.slice()), "Lorem ipsum dolor sit amet,\n");
    assert_eq!(line1.buf(), init_buf);
    let line2 = r.read_line().unwrap();
    assert_eq!(utf8(line2.slice()), "consectetur adipiscing elit,\n");
    assert_eq!(line2.buf(), line1.buf());
    let line3 = r.read_line().unwrap();
    assert_eq!(
        utf8(line3.slice()),
        "sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n",
    );
    assert_ne!(line3.buf(), line2.buf());
    let line4 = r.read_line().unwrap();
    assert_eq!(utf8(line4.slice()), "\n");
    assert_eq!(line4.buf(), line3.buf());
    let line5 = r.read_line().unwrap();
    assert_eq!(
        utf8(line5.slice()),
        "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.",
    );
    assert_ne!(line5.buf(), line4.buf());
    let line6 = r.read_line().unwrap();
    assert_eq!(utf8(line6.slice()), "");
    assert_eq!(line6.buf(), line5.buf());
}

#[track_caller]
fn utf8(s: &[u8]) -> &str {
    match String::from_utf8_lossy(s) {
        Cow::Borrowed(s) => s,
        Cow::Owned(lossy) => panic!("not UTF-8: {lossy:?} ({s:?})"),
    }
}

fn strip_lf(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}
