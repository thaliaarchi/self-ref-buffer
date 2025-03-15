use std::{
    cell::UnsafeCell,
    io::{self, Read},
    mem,
};

use crate::{buffer::Buf, reader::SharedReader};

/// Parsed data, paired with the buffers it was parsed from.
pub struct BufPair<T> {
    dependent: T,
    owner: Vec<Buf>,
}

impl<T> BufPair<T> {
    pub fn new<R: Read, F, E>(reader: &mut SharedReader<R>, make: F) -> Result<Self, E>
    where
        F: for<'a> FnOnce(&'a BufBuilder<'_, R>) -> Result<T, E>,
    {
        let builder = BufBuilder::new(reader);
        let dependent = make(&builder)?;
        Ok(BufPair {
            dependent,
            owner: builder.bufs.into_inner(),
        })
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
