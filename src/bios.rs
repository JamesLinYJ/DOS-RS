//! Resident BIOS pieces translated from v4.0 `BIOS/`.
//! Includes disk and character devices, MSLOAD and the modeled SYSINIT routines.

pub mod msaux;
pub mod msdisk;
pub mod mslpt;
pub mod msload;
pub mod sysimes;
pub mod sysinit1;
pub mod sysinit2;
pub mod msclock;
pub mod mscon;
pub mod msbio1;
pub(crate) mod ata;
pub(crate) mod pc;
