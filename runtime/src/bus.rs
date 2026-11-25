use crate::cpu::Cpu;
use crate::ocs::Ocs;

pub const CHIP_RAM_SIZE: usize = 1024 * 1024; // 1 MB Chip RAM ($000000..$100000)
pub const SLOW_RAM_BASE: u32 = 0x00C0_0000;
pub const SLOW_RAM_SIZE: usize = 512 * 1024;  // 512 KB Ranger/Slow RAM ($C00000..$C80000)

const MAIN_DISK_OFFSET: usize = 0x2C00;
const MAIN_LOAD_SIZE: usize = 0x186A0;
const MAIN_RAM_BASE: usize = 0x1000;

pub struct Bus {
    pub chip: Vec<u8>,
    pub slow: Vec<u8>,
    pub adf: Vec<u8>,
    pub ocs: Ocs,
    pub disk_reads: u64,
    pub trace_disk: bool,
    orig_code: Vec<u8>,  // Initial bytes of $000000..$01FFFF for exact self-mod detection
    code_dirty: Vec<u64>, // 1 bit per 16-bit word for $000000..$01FFFF (1024 * 64 = 65536 words)
}

impl Bus {
    pub fn new(adf: Vec<u8>) -> Self {
        let mut chip = vec![0u8; CHIP_RAM_SIZE];
        // Pre-load the relocated 100,000-byte Parasol Stars main binary directly at $00001000!
        // (Matches exactly what the Ocean bootblock + $00193E relocator does on Amiga.)
        chip[MAIN_RAM_BASE..MAIN_RAM_BASE + MAIN_LOAD_SIZE]
            .copy_from_slice(&adf[MAIN_DISK_OFFSET..MAIN_DISK_OFFSET + MAIN_LOAD_SIZE]);
        let orig_code = chip[0..0x020000].to_vec();

        Self {
            chip,
            slow: vec![0u8; SLOW_RAM_SIZE],
            adf,
            ocs: Ocs::new(),
            disk_reads: 0,
            trace_disk: false,
            orig_code,
            code_dirty: vec![0u64; 1024],
        }
    }

    #[inline(always)]
    fn sync_dirty_word(&mut self, a: u32) {
        if a < 0x0002_0000 {
            let o = (a & !1) as usize;
            let w = o >> 1;
            let changed = self.chip[o] != self.orig_code[o] || self.chip[o + 1] != self.orig_code[o + 1];
            if changed {
                self.code_dirty[w >> 6] |= 1u64 << (w & 63);
            } else {
                self.code_dirty[w >> 6] &= !(1u64 << (w & 63));
            }
        }
    }

    #[inline(always)]
    pub fn is_code_dirty(&self, pc: u32) -> bool {
        let a = pc & 0x00FF_FFFF;
        if a < 0x0002_0000 {
            let w = (a >> 1) as usize;
            (self.code_dirty[w >> 6] & (1u64 << (w & 63))) != 0
        } else {
            true
        }
    }

    #[inline(always)]
    pub fn is_range_dirty(&self, start: u32, end: u32) -> bool {
        let mut w = ((start & 0x00FF_FFFF) >> 1) as usize;
        let w_end = (((end & 0x00FF_FFFF).min(0x0002_0000)) >> 1) as usize;
        while w < w_end {
            let idx = w >> 6;
            let lo = w & 63;
            let hi = (w_end - (idx << 6)).min(64);
            let mask = if hi == 64 { !0u64 << lo } else { ((1u64 << (hi - lo)) - 1) << lo };
            if (self.code_dirty[idx] & mask) != 0 {
                return true;
            }
            w = (idx + 1) << 6;
        }
        false
    }

    #[inline(always)]
    pub fn advance_cycles(&mut self, cycles: u32) {
        self.ocs.advance_cycles(&mut self.chip, cycles);
    }

    #[inline(always)]
    pub fn should_yield(&self, cpu: &Cpu) -> bool {
        if self.ocs.frame_ready { return true; }
        let ipl = self.ocs.pending_ipl();
        if ipl > cpu.ipl() || ipl == 7 { return true; }
        if (cpu.sr & crate::cpu::SR_T) != 0 { return true; }
        false
    }

    #[inline(always)]
    pub fn read_b(&mut self, addr: u32) -> u8 {
        let a = addr & 0x00FF_FFFF;
        if (a as usize) < CHIP_RAM_SIZE {
            return self.chip[a as usize];
        }
        if a >= SLOW_RAM_BASE && a < SLOW_RAM_BASE + SLOW_RAM_SIZE as u32 {
            return self.slow[(a - SLOW_RAM_BASE) as usize];
        }
        if a >= 0x00DF_F000 && a < 0x00DF_F200 {
            let w = self.ocs.read_custom_w((a & 0x1FE) as u16);
            return if (a & 1) == 0 { (w >> 8) as u8 } else { (w & 0xFF) as u8 };
        }
        // CIA-A: odd addresses $BFE001..$BFEF01
        if (a & 0x00FF_F001) == 0x00BF_E001 {
            let reg = ((a >> 8) & 0xF) as u8;
            return self.ocs.cia_a.read(reg);
        }
        // CIA-B: even addresses $BFD000..$BFDF00
        if (a & 0x00FF_F001) == 0x00BF_D000 {
            let reg = ((a >> 8) & 0xF) as u8;
            return self.ocs.cia_b.read(reg);
        }
        0xFF
    }

    #[inline(always)]
    pub fn read_w(&mut self, addr: u32) -> u16 {
        let a = addr & 0x00FF_FFFF;
        if (a as usize) + 1 < CHIP_RAM_SIZE {
            return u16::from_be_bytes([self.chip[a as usize], self.chip[a as usize + 1]]);
        }
        if a >= SLOW_RAM_BASE && a + 1 < SLOW_RAM_BASE + SLOW_RAM_SIZE as u32 {
            let o = (a - SLOW_RAM_BASE) as usize;
            return u16::from_be_bytes([self.slow[o], self.slow[o + 1]]);
        }
        if a >= 0x00DF_F000 && a < 0x00DF_F200 {
            return self.ocs.read_custom_w((a & 0x1FE) as u16);
        }
        let hi = self.read_b(a) as u16;
        let lo = self.read_b(a.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    #[inline(always)]
    pub fn read_l(&mut self, addr: u32) -> u32 {
        let a = addr & 0x00FF_FFFF;
        if (a as usize) + 3 < CHIP_RAM_SIZE {
            let o = a as usize;
            return u32::from_be_bytes([self.chip[o], self.chip[o + 1], self.chip[o + 2], self.chip[o + 3]]);
        }
        if a >= SLOW_RAM_BASE && a + 3 < SLOW_RAM_BASE + SLOW_RAM_SIZE as u32 {
            let o = (a - SLOW_RAM_BASE) as usize;
            return u32::from_be_bytes([self.slow[o], self.slow[o + 1], self.slow[o + 2], self.slow[o + 3]]);
        }
        let hi = self.read_w(a) as u32;
        let lo = self.read_w(a.wrapping_add(2)) as u32;
        (hi << 16) | lo
    }

    #[inline(always)]
    pub fn write_b(&mut self, addr: u32, val: u8) {
        let a = addr & 0x00FF_FFFF;
        if (a as usize) < CHIP_RAM_SIZE {
            self.chip[a as usize] = val;
            self.sync_dirty_word(a);
            return;
        }
        if a >= SLOW_RAM_BASE && a < SLOW_RAM_BASE + SLOW_RAM_SIZE as u32 {
            self.slow[(a - SLOW_RAM_BASE) as usize] = val;
            return;
        }
        if (a & 0x00FF_F001) == 0x00BF_E001 {
            let reg = ((a >> 8) & 0xF) as u8;
            self.ocs.cia_a.write(reg, val);
            return;
        }
        if (a & 0x00FF_F001) == 0x00BF_D000 {
            let reg = ((a >> 8) & 0xF) as u8;
            self.ocs.cia_b.write(reg, val);
            return;
        }
        if a >= 0x00DF_F000 && a < 0x00DF_F200 {
            let w = ((val as u16) << 8) | (val as u16);
            self.ocs.write_custom_w(&mut self.chip, (a & 0x1FE) as u16, w);
        }
    }

    #[inline(always)]
    pub fn write_w(&mut self, addr: u32, val: u16) {
        let a = addr & 0x00FF_FFFF;
        if (a as usize) + 1 < CHIP_RAM_SIZE {
            let b = val.to_be_bytes();
            self.chip[a as usize] = b[0];
            self.chip[a as usize + 1] = b[1];
            self.sync_dirty_word(a);
            return;
        }
        if a >= SLOW_RAM_BASE && a + 1 < SLOW_RAM_BASE + SLOW_RAM_SIZE as u32 {
            let o = (a - SLOW_RAM_BASE) as usize;
            let b = val.to_be_bytes();
            self.slow[o] = b[0];
            self.slow[o + 1] = b[1];
            return;
        }
        if a >= 0x00DF_F000 && a < 0x00DF_F200 {
            self.ocs.write_custom_w(&mut self.chip, (a & 0x1FE) as u16, val);
            return;
        }
        self.write_b(a, (val >> 8) as u8);
        self.write_b(a.wrapping_add(1), (val & 0xFF) as u8);
    }

    #[inline(always)]
    pub fn write_l(&mut self, addr: u32, val: u32) {
        let a = addr & 0x00FF_FFFF;
        if (a as usize) + 3 < CHIP_RAM_SIZE {
            let b = val.to_be_bytes();
            let o = a as usize;
            self.chip[o] = b[0];
            self.chip[o + 1] = b[1];
            self.chip[o + 2] = b[2];
            self.chip[o + 3] = b[3];
            self.sync_dirty_word(a);
            self.sync_dirty_word(a.wrapping_add(2));
            return;
        }
        if a >= SLOW_RAM_BASE && a + 3 < SLOW_RAM_BASE + SLOW_RAM_SIZE as u32 {
            let b = val.to_be_bytes();
            let o = (a - SLOW_RAM_BASE) as usize;
            self.slow[o] = b[0];
            self.slow[o + 1] = b[1];
            self.slow[o + 2] = b[2];
            self.slow[o + 3] = b[3];
            return;
        }
        self.write_w(a, (val >> 16) as u16);
        self.write_w(a.wrapping_add(2), (val & 0xFFFF) as u16);
    }

    /// HLE Hook for $00664E: read `d2.w` 512-byte sectors starting at sector `d1.w` into `a0`.
    /// Returns `d0 = 0` on success.
    pub fn hle_disk_read(&mut self, cpu: &mut Cpu) {
        let start_sec = (cpu.d[1] & 0xFFFF) as usize;
        let count_sec = (cpu.d[2] & 0xFFFF) as usize;
        let dst = cpu.a[0] & 0x00FF_FFFF;
        let disk_off = start_sec * 512;
        let byte_len = count_sec * 512;
        self.disk_reads += 1;

        if self.trace_disk {
            // Try reading requested filename from $0068A6 if available
            let name_ptr = self.read_l(0x0068A6);
            let mut fname = String::new();
            if name_ptr >= 0x1000 && name_ptr < 0x80000 {
                for i in 0..16 {
                    let c = self.read_b(name_ptr + i);
                    if c == 0 || !c.is_ascii_graphic() { break; }
                    fname.push(c as char);
                }
            }
            eprintln!("[DISK READ #{}] sec={} cnt={} -> dst=${:06X} (file='{}')",
                self.disk_reads, start_sec, count_sec, dst, fname);
        }

        if disk_off + byte_len <= self.adf.len() {
            for i in 0..byte_len {
                let b = self.adf[disk_off + i];
                self.write_b(dst.wrapping_add(i as u32), b);
            }
            cpu.d[0] = 0;
            self.write_w(0x0068EA, 0x0064);
        } else {
            eprintln!("[DISK READ OOB] sec={} cnt={}", start_sec, count_sec);
            cpu.d[0] = 1;
        }
    }

    /// HLE Hook for $0066AE: write `d2.w` 512-byte sectors starting at sector `d1.w` from `a0`.
    pub fn hle_disk_write(&mut self, cpu: &mut Cpu) {
        let start_sec = (cpu.d[1] & 0xFFFF) as usize;
        let count_sec = (cpu.d[2] & 0xFFFF) as usize;
        let src = cpu.a[0] & 0x00FF_FFFF;
        let disk_off = start_sec * 512;
        let byte_len = count_sec * 512;
        if disk_off + byte_len <= self.adf.len() {
            for i in 0..byte_len {
                self.adf[disk_off + i] = self.read_b(src.wrapping_add(i as u32));
            }
            cpu.d[0] = 0;
        } else {
            cpu.d[0] = 1;
        }
    }
}
