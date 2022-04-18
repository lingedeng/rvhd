use crate::{traits, Result};
use std::fs::File;
use std::io::{SeekFrom, prelude::*};
use std::cell::RefCell;

pub trait AsByteSlice {
    /// # Safety
    /// The method is unsafe because any padding bytes in the struct may be uninitialized memory (giving undefined behavior).
    /// Also, there are not any Endianness assumtions. The caller should care about it.
    unsafe fn as_byte_slice(&self) -> &[u8];
}

pub trait AsByteSliceMut {
    unsafe fn as_byte_slice_mut(&mut self) -> &mut [u8];
}

macro_rules! impl_int {
    ($name:ty) => {
        impl AsByteSlice for $name {
            unsafe fn as_byte_slice(&self) -> &[u8] {
                let byte_size = std::mem::size_of::<$name>();
                std::slice::from_raw_parts(self as *const _ as *const u8, byte_size)
            }
        }

        impl AsByteSlice for [$name] {
            unsafe fn as_byte_slice(&self) -> &[u8] {
                let byte_size = self.len() * std::mem::size_of::<$name>();
                std::slice::from_raw_parts(self.as_ptr() as *const u8, byte_size)
            }
        }

        impl AsByteSlice for Vec<$name> {
            unsafe fn as_byte_slice(&self) -> &[u8] {
                let byte_size = self.len() * std::mem::size_of::<$name>();
                std::slice::from_raw_parts(self.as_ptr() as *const u8, byte_size)
            }
        }

        impl AsByteSliceMut for $name {
            unsafe fn as_byte_slice_mut(&mut self) -> &mut [u8] {
                let byte_size = std::mem::size_of::<$name>();
                std::slice::from_raw_parts_mut(self as *mut _ as *mut u8, byte_size)
            }
        }

        impl AsByteSliceMut for [$name] {
            unsafe fn as_byte_slice_mut(&mut self) -> &mut [u8] {
                let byte_size = self.len() * std::mem::size_of::<$name>();
                std::slice::from_raw_parts_mut(self.as_ptr() as *mut u8, byte_size)
            }
        }

        impl AsByteSliceMut for Vec<$name> {
            unsafe fn as_byte_slice_mut(&mut self) -> &mut [u8] {
                let byte_size = self.len() * std::mem::size_of::<$name>();
                std::slice::from_raw_parts_mut(self.as_ptr() as *mut u8, byte_size)
            }
        }
    };
}

impl_int!(u8);
impl_int!(u16);
impl_int!(u32);
impl_int!(u64);
impl_int!(i8);
impl_int!(i16);
impl_int!(i32);
impl_int!(i64);

#[derive(Clone)]
pub struct StructBuffer<T: Sized> {
    buffer: Vec<u8>,
    _marker: std::marker::PhantomData<T>,
}

#[allow(clippy::len_without_is_empty)]
impl<T: Sized + Copy + Clone> StructBuffer<T> {
    /// Creates a buffer capable to hold the value of type `T`.
    ///
    /// # Safety
    /// The buffer is uninitialized!
    pub unsafe fn new() -> Self {
        Self {
            buffer: alloc_buffer(std::mem::size_of::<T>()),
            _marker: std::marker::PhantomData,
        }
    }

    /// Creates a buffer capable to hold the value of type `T` plus `ext_size` bytes.
    ///
    /// # Safety
    /// The buffer is uninitialized!
    pub unsafe fn with_ext(size: usize) -> Self {
        Self {
            buffer: alloc_buffer(std::mem::size_of::<T>() + size),
            _marker: std::marker::PhantomData,
        }
    }

    /// Creates a StructBuffer for the type `T` using supplied `buffer`.
    ///
    /// # Safety
    /// The buffer size should be >= mem::size_of::<T>() !
    pub unsafe fn with_buffer(buffer: Vec<u8>) -> Self {
        if buffer.len() < std::mem::size_of::<T>() {
            panic!("Insufficient buffer size");
        }

        Self {
            buffer,
            _marker: std::marker::PhantomData,
        }
    }

    /// Creates a StructBuffer for the type `T` using supplied `T` value.
    ///
    /// # Safety
    /// The buffer size should be >= mem::size_of::<T>() !
    pub unsafe fn with_value(value: &T) -> Self {
        let buffer = {
            let size = std::mem::size_of::<T>();
            let mut buf = alloc_buffer(size);
            let value_bytes = std::slice::from_raw_parts(value as *const _ as *const u8, size);
            buf.as_byte_slice_mut().copy_from_slice(value_bytes);
            buf
        };

        Self {
            buffer,
            _marker: std::marker::PhantomData,
        }
    }

    /// Creates the value of type `T` represented by the all-zero byte-pattern.
    pub fn zeroed() -> Self {
        Self {
            buffer: vec![0_u8; std::mem::size_of::<T>()],
            _marker: std::marker::PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn raw(&self) -> &T {
        #[allow(clippy::cast_ptr_alignment)]
        unsafe {
            &*(self.buffer.as_ptr() as *const T)
        }
    }

    pub fn raw_mut(&mut self) -> &mut T {
        #[allow(clippy::cast_ptr_alignment)]
        unsafe {
            &mut *(self.buffer.as_ptr() as *mut T)
        }
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    pub fn ext_buffer(&self) -> &[u8] {
        &self.buffer[std::mem::size_of::<T>()..]
    }

    pub fn ext_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[std::mem::size_of::<T>()..]
    }

    pub fn has_ext_buffer(&self) -> bool {
        !self.ext_buffer().is_empty()
    }

    pub fn copy(&self) -> T {
        *self.raw()
    }

    pub fn take(&self) -> T {
        *self.raw()
    }
}

impl<T: Sized + Copy + Clone> std::ops::Deref for StructBuffer<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.raw()
    }
}

impl<T: Sized + Copy + Clone> std::ops::DerefMut for StructBuffer<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.raw_mut()
    }
}

impl<T: Sized + Copy + Clone> AsByteSlice for StructBuffer<T> {
    unsafe fn as_byte_slice(&self) -> &[u8] {
        self.buffer.as_byte_slice()
    }
}

impl<T: Sized + Copy + Clone> AsByteSliceMut for StructBuffer<T> {
    unsafe fn as_byte_slice_mut(&mut self) -> &mut [u8] {
        self.buffer.as_byte_slice_mut()
    }
}

/// # Safety
/// The allocated buffer is uninitialized and should be entirely rewritten before read.
pub unsafe fn alloc_buffer(size: usize) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(size);
    buffer.set_len(size);
    buffer
}


/// vhd file open/create/size/read_at/write_at/flush
pub struct VhdFile(RefCell<File>);

impl traits::ReadAt for VhdFile {
    fn read_at(&self, offset: u64, data: &mut [u8]) -> Result<usize> {
        let mut file = self.0.borrow_mut();
        file.seek(SeekFrom::Start(offset))?;
        file.read(data).map_err(From::from)
    }
}

impl traits::WriteAt for VhdFile {
    fn write_at(&self, offset: u64, data: &[u8]) -> Result<usize> {
        let mut file = self.0.borrow_mut();
        file.seek(SeekFrom::Start(offset))?;
        file.write(data).map_err(From::from)
    }
}

impl traits::Flush for VhdFile {
    fn flush(&self) -> Result<()> {
        let mut file = self.0.borrow_mut();
        file.flush().map_err(From::from)
    }
}

impl VhdFile {
    pub fn open(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        Ok(VhdFile(
            RefCell::new(file)
        ))
    }

    pub fn create(path: &str, _size: u64) -> Result<Self> {
        let file = File::create(path)?;
        //file.seek(SeekFrom::Start(size))?;
        Ok(VhdFile(
            RefCell::new(file)
        ))
    }

    pub fn size(&self) -> Result<u64> {
        let metadata = self.0.borrow().metadata()?;
        Ok(metadata.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C, packed)]
    #[derive(Copy, Clone)]
    struct S {
        byte: u8,
        word: u16,
    }

    #[test]
    fn as_byte_slice_for_vec() {
        let vec: Vec<u8> = vec![1, 2, 3];
        let bytes = unsafe { vec.as_byte_slice() };
        assert_eq!(3, bytes.len());

        let vec: Vec<u16> = vec![1, 2, 3];
        let bytes = unsafe { vec.as_byte_slice() };
        assert_eq!(6, bytes.len());

        let vec: Vec<u32> = vec![1, 2, 3];
        let bytes = unsafe { vec.as_byte_slice() };
        assert_eq!(12, bytes.len());
    }

    #[test]
    fn as_byte_slice_for_slice() {
        let vec: Vec<u8> = vec![1, 2, 3];
        let slice = vec.as_slice();
        let bytes = unsafe { slice.as_byte_slice() };
        assert_eq!(3, bytes.len());

        let vec: Vec<u16> = vec![1, 2, 3];
        let slice = vec.as_slice();
        let bytes = unsafe { slice.as_byte_slice() };
        assert_eq!(6, bytes.len());

        let vec: Vec<u32> = vec![1, 2, 3];
        let slice = vec.as_slice();
        let bytes = unsafe { slice.as_byte_slice() };
        assert_eq!(12, bytes.len());
    }

    #[test]
    fn as_byte_slice_for_struct() {
        let mut buffer = StructBuffer::<S>::zeroed();
        assert_eq!(3, buffer.len());

        unsafe {
            // packed fileds
            assert_eq!(0, buffer.byte);
            assert_eq!(0, buffer.word)
        }

        buffer.byte = 12;
        unsafe {
            assert_eq!(12, buffer.byte);
            assert_eq!(0, buffer.word)
        }

        let bytes = unsafe { buffer.as_byte_slice() };
        assert_eq!(3, bytes.len());

        let s = buffer.copy();
        unsafe {
            assert_eq!(12, s.byte);
            assert_eq!(0, s.word)
        }

        let s = buffer.take();
        unsafe {
            assert_eq!(12, s.byte);
            assert_eq!(0, s.word)
        }
    }

    #[test]
    fn as_byte_slice_for_primitive() {
        let b = 4_u8;
        let bytes = unsafe { b.as_byte_slice() };
        assert_eq!(1, bytes.len());

        let b = 4_u16;
        let bytes = unsafe { b.as_byte_slice() };
        assert_eq!(2, bytes.len());

        let b = 4_u32;
        let bytes = unsafe { b.as_byte_slice() };
        assert_eq!(4, bytes.len());

        let b = 4_u64;
        let bytes = unsafe { b.as_byte_slice() };
        assert_eq!(8, bytes.len());
    }

    #[test]
    fn ext_buffer() {
        let mut buffer = unsafe { StructBuffer::<S>::with_ext(4) };
        assert_eq!(7, buffer.len());
        assert!(buffer.has_ext_buffer());
        assert!(buffer.ext_buffer().len() == 4);
        assert!(buffer.ext_buffer_mut().len() == 4);

        let mut buffer = StructBuffer::<S>::zeroed();
        assert_eq!(3, buffer.len());
        assert!(!buffer.has_ext_buffer());
        assert!(buffer.ext_buffer().len() == 0);
        assert!(buffer.ext_buffer_mut().len() == 0);
    }

    #[test]
    fn with_value() {
        let mut buffer = unsafe { StructBuffer::<S>::with_value(&S{ byte: 78, word: 0x1326}) };
        assert_eq!(3, buffer.len());
        assert!(!buffer.has_ext_buffer());
        assert!(buffer.ext_buffer().len() == 0);
        assert!(buffer.ext_buffer_mut().len() == 0);

        assert!( buffer.byte == 78 );
        assert!( buffer.word == 0x1326 );
    }
}