use std::{
    cell::UnsafeCell,
    fmt::{self, Debug, Formatter},
    mem::{self, MaybeUninit},
    ptr::NonNull,
    rc::Rc,
    slice,
};

/// A reference to a buffer and a slice within it.
pub struct Bytes {
    pub(crate) buf: Buf,
    pub(crate) slice: NonNull<[u8]>,
}

impl Bytes {
    pub fn slice(&self) -> &[u8] {
        unsafe { &*self.slice.as_ptr() }
    }

    pub fn buf(&self) -> Buf {
        self.buf.clone()
    }
}

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
