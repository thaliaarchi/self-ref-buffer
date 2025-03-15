use std::io::{self, Read};

use crate::buffer::{Buf, BufMut, Bytes};

/// A reader which does not overwrite its buffers, so slices can be freely
/// retained while reading.
pub struct SharedReader<R> {
    reader: R,
    buf: BufMut,
    /// The initial capacity for a new buffer.
    initial_capacity: usize,
}

impl<R: Read> SharedReader<R> {
    pub fn new(reader: R, initial_capacity: usize) -> Self {
        SharedReader {
            reader,
            buf: BufMut::new(initial_capacity),
            initial_capacity,
        }
    }

    /// Reads a line until LF or EOF. Returns a shared reference to a slice in
    /// the current buffer which contains the line.
    pub fn read_line(&mut self) -> io::Result<Bytes> {
        fn find_lf(buf: &[u8]) -> Option<usize> {
            buf.iter().position(|&b| b == b'\n')
        }

        let len = if let Some(i) = find_lf(self.buf.available()) {
            i + 1
        } else {
            let mut len = self.buf.available().len();
            loop {
                if self.buf.unfilled().is_empty() {
                    let partial = self.buf.available();
                    let mut new_buf = BufMut::new((partial.len() * 2).max(self.initial_capacity));
                    new_buf.append(partial);
                    self.buf = new_buf;
                }
                let n = self.reader.read(self.buf.unfilled())?;
                if n == 0 {
                    break;
                }
                if let Some(i) = find_lf(self.buf.fill(n)) {
                    len += i + 1;
                    break;
                }
                len += n;
            }
            len
        };
        let line = self.buf.consume(len).into();
        Ok(Bytes {
            buf: self.buf.borrow(),
            slice: line,
        })
    }

    pub fn buffer(&self) -> Buf {
        self.buf.borrow()
    }
}
