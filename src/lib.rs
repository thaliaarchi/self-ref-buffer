use std::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    io::{self, Read},
    mem::{self, MaybeUninit},
    ptr::NonNull,
    rc::Rc,
    slice,
};

/// A persistent, reference counted buffer. Once data is written, it cannot be
/// overwritten. This limited version of the API does not allow accessing the
/// contents directly from the buffer.
#[derive(Clone)]
pub struct Buf {
    buf: Rc<UnsafeCell<[u8]>>,
}

impl Debug for Buf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buf").finish_non_exhaustive()
    }
}

impl PartialEq for Buf {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.buf, &other.buf)
    }
}

impl Eq for Buf {}

/// A mutable handle into the current buffer. Only one such handle can be
/// created, and it has exclusive mutable access for filling the buffer.
///
/// ```ignore
/// +----------+----------------+
/// | consumed |avail|          |
/// +----------+-----+----------+
/// |     filled     | unfilled |
/// +----------------+----------+
/// ```
pub struct BufMut {
    buf: Rc<UnsafeCell<[u8]>>,
    consumed: usize,
    filled: usize,
}

impl BufMut {
    pub fn new(capacity: usize) -> Self {
        let mut buf = Rc::<[u8]>::new_uninit_slice(capacity);
        Rc::get_mut(&mut buf).unwrap().fill(MaybeUninit::new(0));
        let buf = unsafe { mem::transmute(buf.assume_init()) };
        BufMut {
            buf,
            consumed: 0,
            filled: 0,
        }
    }

    pub fn consumed(&self) -> &[u8] {
        let buf = self.buf.get();
        unsafe { slice::from_raw_parts(buf as *mut u8, self.consumed) }
    }

    pub fn available(&self) -> &[u8] {
        let buf = self.buf.get();
        unsafe {
            let ptr = (buf as *mut u8).add(self.consumed);
            slice::from_raw_parts(ptr, self.filled - self.consumed)
        }
    }

    pub fn unfilled(&mut self) -> &mut [u8] {
        let buf = self.buf.get();
        unsafe {
            let ptr = (buf as *mut u8).add(self.filled);
            slice::from_raw_parts_mut(ptr, buf.len() - self.filled)
        }
    }

    pub fn consume(&mut self, n: usize) -> &[u8] {
        assert!(self.consumed + n <= self.filled);
        let buf = self.buf.get();
        let consumed = unsafe {
            let ptr = (buf as *mut u8).add(self.consumed);
            slice::from_raw_parts(ptr, n)
        };
        self.consumed += n;
        consumed
    }

    pub fn fill(&mut self, n: usize) -> &[u8] {
        let buf = self.buf.get();
        assert!(self.filled + n <= buf.len());
        let filled = unsafe {
            let ptr = (buf as *mut u8).add(self.filled);
            slice::from_raw_parts(ptr, n)
        };
        self.filled += n;
        filled
    }

    pub fn append(&mut self, data: &[u8]) {
        let unfilled = self.unfilled();
        unfilled[..data.len()].copy_from_slice(data);
        self.filled += data.len();
    }

    pub fn borrow(&self) -> Buf {
        Buf {
            buf: self.buf.clone(),
        }
    }
}

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

/// A reference to a buffer and a slice within it.
pub struct Bytes {
    buf: Buf,
    slice: NonNull<[u8]>,
}

impl Bytes {
    pub fn slice(&self) -> &[u8] {
        unsafe { &*self.slice.as_ptr() }
    }

    pub fn buf(&self) -> Buf {
        self.buf.clone()
    }
}

/// Parsed data, paired with the buffers it was parsed from.
pub struct BufPair<T> {
    dependent: T,
    owner: Vec<Buf>,
}

impl<T> BufPair<T> {
    pub fn new<R: Read, F>(reader: &mut SharedReader<R>, make: F) -> Self
    where
        F: for<'a> FnOnce(&'a BufBuilder<'_, R>) -> T,
    {
        let builder = BufBuilder::new(reader);
        let dependent = make(&builder);
        BufPair {
            dependent,
            owner: builder.bufs.into_inner(),
        }
    }

    pub fn dependent(&self) -> &T {
        &self.dependent
    }

    pub fn owner(&self) -> &[Buf] {
        &self.owner
    }
}

pub struct BufBuilder<'r, R> {
    reader: UnsafeCell<&'r mut SharedReader<R>>,
    bufs: UnsafeCell<Vec<Buf>>,
}

impl<'r, R: Read> BufBuilder<'r, R> {
    fn new(reader: &'r mut SharedReader<R>) -> Self {
        BufBuilder {
            reader: UnsafeCell::new(reader),
            bufs: UnsafeCell::new(Vec::new()),
        }
    }

    /// Reads a line from the reader and stores the buffer it came from, so
    pub fn read_line(&self) -> io::Result<&[u8]> {
        let reader = unsafe { &mut *self.reader.get() };
        let line = reader.read_line()?;
        // SAFETY: The vec only grows and any returned slices are not
        // invalidated if their `Buf` moves, since the buffers are boxed.
        let bufs = unsafe { &mut *self.bufs.get() };
        if !bufs.last().is_some_and(|last| last == &line.buf) {
            bufs.push(line.buf);
        }
        // Transmute the lifetime from 'r to 'self.
        // SAFETY: We are tracking the buffer, so the borrow now can live as
        // long as the chain.
        let slice = unsafe { mem::transmute(line.slice) };
        Ok(slice)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

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
}
