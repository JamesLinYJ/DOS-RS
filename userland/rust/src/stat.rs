use core::ffi::CStr;
use core::ptr;

#[repr(C)]
#[derive(Default)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub size: i64,
    pub mode: u32,
    pub rdev: u32,
}
const _: () = assert!(size_of::<Stat>() == 32);

unsafe extern "C" {
    fn dosrs_file_info(name: *const i8, fd: i32, result: *mut Stat) -> i32;
}
impl Stat {
    fn get(path: *const i8, fd: i32) -> Result<Self, i32> {
        let mut result = Self::default();
        if unsafe { dosrs_file_info(path, fd, &mut result) } != 0 {
            Err(crate::errno())
        } else {
            Ok(result)
        }
    }
    pub fn fd(fd: i32) -> Result<Self, i32> {
        Self::get(ptr::null(), fd)
    }
    pub fn file(path: Option<&CStr>) -> Result<Self, i32> {
        if let Some(path) = path.filter(|p| p.to_bytes() != b"-") {
            Self::get(path.as_ptr(), -1)
        } else {
            Self::fd(0)
        }
    }
}
