use std::ops::Deref;

pub trait MappedBytes: Send + Sync {
    fn bytes(&self) -> &[u8];
    fn unchanged(&self) -> bool;
}

pub struct MappedFile(Box<dyn MappedBytes>);

impl MappedFile {
    #[must_use]
    pub fn new(inner: Box<dyn MappedBytes>) -> Self {
        Self(inner)
    }
}

pub enum FileBytes {
    Heap(Vec<u8>),
    Mapped(MappedFile),
}

impl FileBytes {
    #[must_use]
    pub fn unchanged(&self) -> bool {
        match self {
            Self::Heap(_) => true,
            Self::Mapped(mapped) => mapped.0.unchanged(),
        }
    }
}

impl Deref for FileBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Self::Heap(bytes) => bytes,
            Self::Mapped(mapped) => mapped.0.bytes(),
        }
    }
}

#[cfg(test)]
#[path = "mapped_tests.rs"]
mod tests;
