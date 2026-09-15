//! Filesystem identity used to distinguish journaled writes from replacements.

use std::fs::{File, Metadata};
use std::io;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Identity {
    device: u64,
    file: u64,
}

#[cfg(unix)]
impl Identity {
    pub(super) fn of(file: &File) -> io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(Self {
            device: metadata.dev(),
            file: metadata.ino(),
        })
    }
}

#[cfg(windows)]
impl Identity {
    pub(super) fn of(file: &File) -> io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: file owns a live handle and information points to a correctly sized writable buffer.
        let result =
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful API call initialized the entire structure.
        let information = unsafe { information.assume_init() };
        Ok(Self {
            device: u64::from(information.dwVolumeSerialNumber),
            file: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        })
    }
}

pub(super) fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(unix)]
    {
        metadata.file_type().is_symlink()
    }
}
