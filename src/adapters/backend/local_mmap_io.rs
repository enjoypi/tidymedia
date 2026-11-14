use std::fs::File;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use memmap2::{Mmap, MmapOptions};

use crate::entities::backend::{MappedBytes, MappedFile};

struct Mapping {
    map: Mmap,
    file: File,
    len: u64,
    modified: Option<SystemTime>,
}

impl MappedBytes for Mapping {
    fn bytes(&self) -> &[u8] {
        &self.map
    }

    fn unchanged(&self) -> bool {
        self.file
            .metadata()
            .is_ok_and(|meta| meta.len() == self.len && meta.modified().ok() == self.modified)
    }
}

pub(super) fn map_path(path: &Path) -> io::Result<Option<MappedFile>> {
    let file = File::open(path)?;
    let meta = file.metadata()?;
    let len = meta.len();
    let Ok(map_len) = usize::try_from(len) else {
        return Ok(None);
    };
    if map_len == 0 {
        return Ok(None);
    }
    let map = unsafe { MmapOptions::new().len(map_len).map(&file)? };
    Ok(Some(MappedFile::new(Box::new(Mapping {
        map,
        file,
        len,
        modified: meta.modified().ok(),
    }))))
}

#[cfg(target_os = "macos")]
pub(super) fn is_fixed_volume(path: &Path) -> bool {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return false;
    }
    let stat = unsafe { stat.assume_init() };
    stat.f_flags & libc::MNT_LOCAL.cast_unsigned() != 0
}

#[cfg(windows)]
pub(super) fn is_fixed_volume(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};

    const DRIVE_FIXED: u32 = 3;
    const ROOT_CAPACITY: u32 = 1024;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = [0_u16; ROOT_CAPACITY as usize];
    let found = unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), ROOT_CAPACITY) };
    if found == 0 {
        return false;
    }
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_FIXED }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn is_fixed_volume(_path: &Path) -> bool {
    false
}
