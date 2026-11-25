pub mod lifter;
pub mod m68k;
pub mod zip_adf;

pub const MAIN_DISK_OFFSET: usize = 0x2C00;
pub const MAIN_LOAD_SIZE: usize = 0x186A0;
pub const MAIN_RAM_BASE: u32 = 0x00001000;
