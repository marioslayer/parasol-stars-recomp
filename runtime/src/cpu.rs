use crate::bus::Bus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    T, F, Hi, Ls, Cc, Cs, Ne, Eq, Vc, Vs, Pl, Mi, Ge, Lt, Gt, Le,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftKind {
    Asl, Asr, Lsl, Lsr, Roxl, Roxr, Rol, Ror,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitOp {
    Btst, Bchg, Bclr, Bset,
}

pub const FLAG_C: u16 = 0x0001;
pub const FLAG_V: u16 = 0x0002;
pub const FLAG_Z: u16 = 0x0004;
pub const FLAG_N: u16 = 0x0008;
pub const FLAG_X: u16 = 0x0010;
pub const SR_S: u16 = 0x2000;
pub const SR_T: u16 = 0x8000;

pub struct Cpu {
    pub d: [u32; 8],
    pub a: [u32; 8],
    pub usp: u32,
    pub ssp: u32,
    pub sr: u16,
    pub pc: u32,
    pub cycles: u64,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            d: [0; 8],
            a: [0; 8],
            usp: 0x0007F000,
            ssp: 0x0007FFFE,
            sr: 0x2700, // Supervisor mode, interrupts masked
            pc: 0x00001994,
            cycles: 0,
        }
    }

    #[inline(always)]
    pub fn set_d_b(&mut self, r: usize, v: u8) {
        self.d[r] = (self.d[r] & 0xFFFF_FF00) | (v as u32);
    }

    #[inline(always)]
    pub fn set_d_w(&mut self, r: usize, v: u16) {
        self.d[r] = (self.d[r] & 0xFFFF_0000) | (v as u32);
    }

    #[inline(always)]
    pub fn flag_c(&self) -> bool { (self.sr & FLAG_C) != 0 }
    #[inline(always)]
    pub fn flag_v(&self) -> bool { (self.sr & FLAG_V) != 0 }
    #[inline(always)]
    pub fn flag_z(&self) -> bool { (self.sr & FLAG_Z) != 0 }
    #[inline(always)]
    pub fn flag_n(&self) -> bool { (self.sr & FLAG_N) != 0 }
    #[inline(always)]
    pub fn flag_x(&self) -> bool { (self.sr & FLAG_X) != 0 }
    #[inline(always)]
    pub fn ipl(&self) -> u8 { ((self.sr >> 8) & 7) as u8 }

    #[inline(always)]
    pub fn set_ccr(&mut self, ccr: u8) {
        self.sr = (self.sr & 0xFF00) | ((ccr as u16) & 0x1F);
    }

    #[inline(always)]
    pub fn set_sr(&mut self, new_sr: u16) {
        let old_s = (self.sr & SR_S) != 0;
        let new_s = (new_sr & SR_S) != 0;
        if old_s != new_s {
            if old_s {
                self.ssp = self.a[7];
                self.a[7] = self.usp;
            } else {
                self.usp = self.a[7];
                self.a[7] = self.ssp;
            }
        }
        self.sr = new_sr & 0xA71F;
    }

    #[inline(always)]
    pub fn push_w(&mut self, bus: &mut Bus, v: u16) {
        self.a[7] = self.a[7].wrapping_sub(2);
        bus.write_w(self.a[7], v);
    }

    #[inline(always)]
    pub fn push_l(&mut self, bus: &mut Bus, v: u32) {
        self.a[7] = self.a[7].wrapping_sub(4);
        bus.write_l(self.a[7], v);
    }

    #[inline(always)]
    pub fn pop_w(&mut self, bus: &mut Bus) -> u16 {
        let v = bus.read_w(self.a[7]);
        self.a[7] = self.a[7].wrapping_add(2);
        v
    }

    #[inline(always)]
    pub fn pop_l(&mut self, bus: &mut Bus) -> u32 {
        let v = bus.read_l(self.a[7]);
        self.a[7] = self.a[7].wrapping_add(4);
        v
    }

    #[inline(always)]
    pub fn tick(&mut self, bus: &mut Bus, cycles: u32) {
        self.cycles += cycles as u64;
        bus.advance_cycles(cycles);
    }

    pub fn exception(&mut self, bus: &mut Bus, vector: u8, return_pc: u32) {
        let old_sr = self.sr;
        // Enter supervisor mode and clear trace bit
        if (self.sr & SR_S) == 0 {
            self.usp = self.a[7];
            self.a[7] = self.ssp;
        }
        self.sr = (self.sr | SR_S) & !SR_T;
        self.push_l(bus, return_pc);
        self.push_w(bus, old_sr);
        let vec_addr = (vector as u32) * 4;
        self.pc = bus.read_l(vec_addr);
    }

    pub fn take_interrupt(&mut self, bus: &mut Bus, level: u8) {
        let old_sr = self.sr;
        if (self.sr & SR_S) == 0 {
            self.usp = self.a[7];
            self.a[7] = self.ssp;
        }
        self.sr = ((self.sr & !0x0700) | ((level as u16) << 8) | SR_S) & !SR_T;
        self.push_l(bus, self.pc);
        self.push_w(bus, old_sr);
        let vec_addr = (24 + level as u32) * 4;
        self.pc = bus.read_l(vec_addr);
    }

    #[inline(always)]
    pub fn flags_clr(&mut self) {
        self.sr = (self.sr & !0x0F) | FLAG_Z;
    }

    #[inline(always)]
    pub fn flags_nz_b(&mut self, v: u8) {
        let mut f = self.sr & !0x0F;
        if v == 0 { f |= FLAG_Z; }
        if (v & 0x80) != 0 { f |= FLAG_N; }
        self.sr = f;
    }

    #[inline(always)]
    pub fn flags_nz_w(&mut self, v: u16) {
        let mut f = self.sr & !0x0F;
        if v == 0 { f |= FLAG_Z; }
        if (v & 0x8000) != 0 { f |= FLAG_N; }
        self.sr = f;
    }

    #[inline(always)]
    pub fn flags_nz_l(&mut self, v: u32) {
        let mut f = self.sr & !0x0F;
        if v == 0 { f |= FLAG_Z; }
        if (v & 0x8000_0000) != 0 { f |= FLAG_N; }
        self.sr = f;
    }

    #[inline(always)]
    pub fn cond(&self, c: Cond) -> bool {
        let c_f = self.flag_c();
        let v_f = self.flag_v();
        let z_f = self.flag_z();
        let n_f = self.flag_n();
        match c {
            Cond::T => true,
            Cond::F => false,
            Cond::Hi => !c_f && !z_f,
            Cond::Ls => c_f || z_f,
            Cond::Cc => !c_f,
            Cond::Cs => c_f,
            Cond::Ne => !z_f,
            Cond::Eq => z_f,
            Cond::Vc => !v_f,
            Cond::Vs => v_f,
            Cond::Pl => !n_f,
            Cond::Mi => n_f,
            Cond::Ge => n_f == v_f,
            Cond::Lt => n_f != v_f,
            Cond::Gt => !z_f && (n_f == v_f),
            Cond::Le => z_f || (n_f != v_f),
        }
    }

    // --- ADD ---
    #[inline(always)]
    pub fn op_add_b(&mut self, s: u8, d: u8) -> u8 {
        let (r, carry) = d.overflowing_add(s);
        let ovf = ((!(s ^ d) & (s ^ r)) & 0x80) != 0;
        let mut f = self.sr & !0x1F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x80) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if carry { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    #[inline(always)]
    pub fn op_add_w(&mut self, s: u16, d: u16) -> u16 {
        let (r, carry) = d.overflowing_add(s);
        let ovf = ((!(s ^ d) & (s ^ r)) & 0x8000) != 0;
        let mut f = self.sr & !0x1F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x8000) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if carry { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    #[inline(always)]
    pub fn op_add_l(&mut self, s: u32, d: u32) -> u32 {
        let (r, carry) = d.overflowing_add(s);
        let ovf = ((!(s ^ d) & (s ^ r)) & 0x8000_0000) != 0;
        let mut f = self.sr & !0x1F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x8000_0000) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if carry { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    // --- SUB ---
    #[inline(always)]
    pub fn op_sub_b(&mut self, s: u8, d: u8) -> u8 {
        let (r, borrow) = d.overflowing_sub(s);
        let ovf = (((s ^ d) & (d ^ r)) & 0x80) != 0;
        let mut f = self.sr & !0x1F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x80) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if borrow { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    #[inline(always)]
    pub fn op_sub_w(&mut self, s: u16, d: u16) -> u16 {
        let (r, borrow) = d.overflowing_sub(s);
        let ovf = (((s ^ d) & (d ^ r)) & 0x8000) != 0;
        let mut f = self.sr & !0x1F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x8000) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if borrow { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    #[inline(always)]
    pub fn op_sub_l(&mut self, s: u32, d: u32) -> u32 {
        let (r, borrow) = d.overflowing_sub(s);
        let ovf = (((s ^ d) & (d ^ r)) & 0x8000_0000) != 0;
        let mut f = self.sr & !0x1F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x8000_0000) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if borrow { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    // --- CMP (does NOT modify X!) ---
    #[inline(always)]
    pub fn op_cmp_b(&mut self, s: u8, d: u8) {
        let (r, borrow) = d.overflowing_sub(s);
        let ovf = (((s ^ d) & (d ^ r)) & 0x80) != 0;
        let mut f = self.sr & !0x0F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x80) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if borrow { f |= FLAG_C; }
        self.sr = f;
    }

    #[inline(always)]
    pub fn op_cmp_w(&mut self, s: u16, d: u16) {
        let (r, borrow) = d.overflowing_sub(s);
        let ovf = (((s ^ d) & (d ^ r)) & 0x8000) != 0;
        let mut f = self.sr & !0x0F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x8000) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if borrow { f |= FLAG_C; }
        self.sr = f;
    }

    #[inline(always)]
    pub fn op_cmp_l(&mut self, s: u32, d: u32) {
        let (r, borrow) = d.overflowing_sub(s);
        let ovf = (((s ^ d) & (d ^ r)) & 0x8000_0000) != 0;
        let mut f = self.sr & !0x0F;
        if r == 0 { f |= FLAG_Z; }
        if (r & 0x8000_0000) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if borrow { f |= FLAG_C; }
        self.sr = f;
    }

    // --- ADDX / SUBX / NEGX ---
    fn addx_core(&mut self, s: u64, d: u64, bits: u32) -> u64 {
        let x = if self.flag_x() { 1u64 } else { 0u64 };
        let mask = (1u64 << bits) - 1;
        let sign = 1u64 << (bits - 1);
        let sum = d + s + x;
        let r = sum & mask;
        let carry = (sum >> bits) != 0;
        let ovf = ((!(s ^ d) & (s ^ r)) & sign) != 0;
        let old_z = self.flag_z();
        let mut f = self.sr & !0x1F;
        if old_z && r == 0 { f |= FLAG_Z; }
        if (r & sign) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if carry { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    fn subx_core(&mut self, s: u64, d: u64, bits: u32) -> u64 {
        let x = if self.flag_x() { 1u64 } else { 0u64 };
        let mask = (1u64 << bits) - 1;
        let sign = 1u64 << (bits - 1);
        let diff = d.wrapping_sub(s).wrapping_sub(x);
        let r = diff & mask;
        let borrow = d < (s + x);
        let ovf = (((s ^ d) & (d ^ r)) & sign) != 0;
        let old_z = self.flag_z();
        let mut f = self.sr & !0x1F;
        if old_z && r == 0 { f |= FLAG_Z; }
        if (r & sign) != 0 { f |= FLAG_N; }
        if ovf { f |= FLAG_V; }
        if borrow { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        r
    }

    pub fn op_addx_b(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(if ry == 7 { 2 } else { 1 });
            let s = bus.read_b(self.a[ry]) as u64;
            self.a[rx] = self.a[rx].wrapping_sub(if rx == 7 { 2 } else { 1 });
            let d = bus.read_b(self.a[rx]) as u64;
            let r = self.addx_core(s, d, 8) as u8;
            bus.write_b(self.a[rx], r);
        } else {
            let r = self.addx_core((self.d[ry] & 0xFF) as u64, (self.d[rx] & 0xFF) as u64, 8) as u8;
            self.set_d_b(rx, r);
        }
    }

    pub fn op_addx_w(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(2);
            let s = bus.read_w(self.a[ry]) as u64;
            self.a[rx] = self.a[rx].wrapping_sub(2);
            let d = bus.read_w(self.a[rx]) as u64;
            let r = self.addx_core(s, d, 16) as u16;
            bus.write_w(self.a[rx], r);
        } else {
            let r = self.addx_core((self.d[ry] & 0xFFFF) as u64, (self.d[rx] & 0xFFFF) as u64, 16) as u16;
            self.set_d_w(rx, r);
        }
    }

    pub fn op_addx_l(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(4);
            let s = bus.read_l(self.a[ry]) as u64;
            self.a[rx] = self.a[rx].wrapping_sub(4);
            let d = bus.read_l(self.a[rx]) as u64;
            let r = self.addx_core(s, d, 32) as u32;
            bus.write_l(self.a[rx], r);
        } else {
            let r = self.addx_core(self.d[ry] as u64, self.d[rx] as u64, 32) as u32;
            self.d[rx] = r;
        }
    }

    pub fn op_subx_b(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(if ry == 7 { 2 } else { 1 });
            let s = bus.read_b(self.a[ry]) as u64;
            self.a[rx] = self.a[rx].wrapping_sub(if rx == 7 { 2 } else { 1 });
            let d = bus.read_b(self.a[rx]) as u64;
            let r = self.subx_core(s, d, 8) as u8;
            bus.write_b(self.a[rx], r);
        } else {
            let r = self.subx_core((self.d[ry] & 0xFF) as u64, (self.d[rx] & 0xFF) as u64, 8) as u8;
            self.set_d_b(rx, r);
        }
    }

    pub fn op_subx_w(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(2);
            let s = bus.read_w(self.a[ry]) as u64;
            self.a[rx] = self.a[rx].wrapping_sub(2);
            let d = bus.read_w(self.a[rx]) as u64;
            let r = self.subx_core(s, d, 16) as u16;
            bus.write_w(self.a[rx], r);
        } else {
            let r = self.subx_core((self.d[ry] & 0xFFFF) as u64, (self.d[rx] & 0xFFFF) as u64, 16) as u16;
            self.set_d_w(rx, r);
        }
    }

    pub fn op_subx_l(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(4);
            let s = bus.read_l(self.a[ry]) as u64;
            self.a[rx] = self.a[rx].wrapping_sub(4);
            let d = bus.read_l(self.a[rx]) as u64;
            let r = self.subx_core(s, d, 32) as u32;
            bus.write_l(self.a[rx], r);
        } else {
            let r = self.subx_core(self.d[ry] as u64, self.d[rx] as u64, 32) as u32;
            self.d[rx] = r;
        }
    }

    pub fn op_negx_b(&mut self, d: u8) -> u8 { self.subx_core(d as u64, 0, 8) as u8 }
    pub fn op_negx_w(&mut self, d: u16) -> u16 { self.subx_core(d as u64, 0, 16) as u16 }
    pub fn op_negx_l(&mut self, d: u32) -> u32 { self.subx_core(d as u64, 0, 32) as u32 }

    // --- MUL / DIV ---
    pub fn op_mulu(&mut self, s: u16, dn: usize) {
        let r = (s as u32) * (self.d[dn] & 0xFFFF);
        self.d[dn] = r;
        self.flags_nz_l(r);
    }

    pub fn op_muls(&mut self, s: u16, dn: usize) {
        let r = ((s as i16 as i32) * (self.d[dn] as i16 as i32)) as u32;
        self.d[dn] = r;
        self.flags_nz_l(r);
    }

    pub fn op_divu(&mut self, bus: &mut Bus, s: u16, dn: usize, next_pc: u32) {
        if s == 0 {
            self.exception(bus, 5, next_pc);
            return;
        }
        let num = self.d[dn];
        let den = s as u32;
        let q = num / den;
        let r = num % den;
        if q > 0xFFFF {
            self.sr = (self.sr & !FLAG_C) | FLAG_V;
        } else {
            self.d[dn] = (r << 16) | (q & 0xFFFF);
            self.flags_nz_w(q as u16);
        }
    }

    pub fn op_divs(&mut self, bus: &mut Bus, s: u16, dn: usize, next_pc: u32) {
        if s == 0 {
            self.exception(bus, 5, next_pc);
            return;
        }
        let num = self.d[dn] as i32;
        let den = s as i16 as i32;
        if num == i32::MIN && den == -1 {
            self.sr = (self.sr & !FLAG_C) | FLAG_V;
            return;
        }
        let q = num / den;
        let r = num % den;
        if q < -32768 || q > 32767 {
            self.sr = (self.sr & !FLAG_C) | FLAG_V;
        } else {
            self.d[dn] = (((r as i16 as u16) as u32) << 16) | ((q as i16 as u16) as u32);
            self.flags_nz_w(q as i16 as u16);
        }
    }

    // --- BCD ---
    fn abcd_byte(&mut self, s: u8, d: u8) -> u8 {
        let x = if self.flag_x() { 1u16 } else { 0 };
        let mut lo = (s & 0xF) as u16 + (d & 0xF) as u16 + x;
        let mut carry = 0u16;
        if lo > 9 { lo += 6; }
        if lo > 0xF { carry = 1; lo &= 0xF; }
        let mut hi = ((s >> 4) as u16) + ((d >> 4) as u16) + carry;
        let mut out_c = false;
        if hi > 9 { hi += 6; }
        if hi > 0xF { out_c = true; hi &= 0xF; }
        let res = ((hi << 4) | lo) as u8;
        let old_z = self.flag_z();
        let mut f = self.sr & !0x1F;
        if old_z && res == 0 { f |= FLAG_Z; }
        if out_c { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        res
    }

    fn sbcd_byte(&mut self, s: u8, d: u8) -> u8 {
        let x = if self.flag_x() { 1i16 } else { 0 };
        let mut lo = (d & 0xF) as i16 - (s & 0xF) as i16 - x;
        let mut borrow = 0i16;
        if lo < 0 { lo += 10; borrow = 1; }
        let mut hi = (d >> 4) as i16 - (s >> 4) as i16 - borrow;
        let mut out_c = false;
        if hi < 0 { hi += 10; out_c = true; }
        let res = (((hi & 0xF) << 4) | (lo & 0xF)) as u8;
        let old_z = self.flag_z();
        let mut f = self.sr & !0x1F;
        if old_z && res == 0 { f |= FLAG_Z; }
        if out_c { f |= FLAG_C | FLAG_X; }
        self.sr = f;
        res
    }

    pub fn op_abcd(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(if ry == 7 { 2 } else { 1 });
            let s = bus.read_b(self.a[ry]);
            self.a[rx] = self.a[rx].wrapping_sub(if rx == 7 { 2 } else { 1 });
            let d = bus.read_b(self.a[rx]);
            let r = self.abcd_byte(s, d);
            bus.write_b(self.a[rx], r);
        } else {
            let r = self.abcd_byte(self.d[ry] as u8, self.d[rx] as u8);
            self.set_d_b(rx, r);
        }
    }

    pub fn op_sbcd(&mut self, bus: &mut Bus, rx: usize, ry: usize, mem: bool) {
        if mem {
            self.a[ry] = self.a[ry].wrapping_sub(if ry == 7 { 2 } else { 1 });
            let s = bus.read_b(self.a[ry]);
            self.a[rx] = self.a[rx].wrapping_sub(if rx == 7 { 2 } else { 1 });
            let d = bus.read_b(self.a[rx]);
            let r = self.sbcd_byte(s, d);
            bus.write_b(self.a[rx], r);
        } else {
            let r = self.sbcd_byte(self.d[ry] as u8, self.d[rx] as u8);
            self.set_d_b(rx, r);
        }
    }

    pub fn op_nbcd(&mut self, d: u8) -> u8 {
        self.sbcd_byte(d, 0)
    }

    // --- BIT OPS ---
    #[inline(always)]
    pub fn op_bit_dn(&mut self, op: BitOp, bnum: u8, dn: usize) {
        let bit = (bnum & 31) as u32;
        let mask = 1u32 << bit;
        let z = (self.d[dn] & mask) == 0;
        if z { self.sr |= FLAG_Z; } else { self.sr &= !FLAG_Z; }
        match op {
            BitOp::Btst => {}
            BitOp::Bchg => self.d[dn] ^= mask,
            BitOp::Bclr => self.d[dn] &= !mask,
            BitOp::Bset => self.d[dn] |= mask,
        }
    }

    #[inline(always)]
    pub fn op_btst_mem(&mut self, bnum: u8, val: u8) {
        let mask = 1u8 << (bnum & 7);
        if (val & mask) == 0 { self.sr |= FLAG_Z; } else { self.sr &= !FLAG_Z; }
    }

    #[inline(always)]
    pub fn op_bit_mem(&mut self, op: BitOp, bnum: u8, val: u8) -> u8 {
        let mask = 1u8 << (bnum & 7);
        if (val & mask) == 0 { self.sr |= FLAG_Z; } else { self.sr &= !FLAG_Z; }
        match op {
            BitOp::Btst => val,
            BitOp::Bchg => val ^ mask,
            BitOp::Bclr => val & !mask,
            BitOp::Bset => val | mask,
        }
    }

    // --- SHIFTS & ROTATES ---
    fn shift_core(&mut self, kind: ShiftKind, val: u32, bits: u32, count: u32) -> u32 {
        let mask = if bits == 32 { 0xFFFF_FFFFu32 } else { (1u32 << bits) - 1 };
        let sign = 1u32 << (bits - 1);
        let mut v = val & mask;
        if count == 0 {
            let mut f = self.sr & !0x0F;
            if v == 0 { f |= FLAG_Z; }
            if (v & sign) != 0 { f |= FLAG_N; }
            if matches!(kind, ShiftKind::Roxl | ShiftKind::Roxr) && self.flag_x() {
                f |= FLAG_C;
            }
            self.sr = f;
            return v;
        }
        let mut c_out = false;
        let mut v_flag = false;
        let mut x_flag = self.flag_x();
        for _ in 0..count {
            match kind {
                ShiftKind::Asl => {
                    let old_s = (v & sign) != 0;
                    c_out = old_s;
                    x_flag = c_out;
                    v = (v << 1) & mask;
                    if ((v & sign) != 0) != old_s { v_flag = true; }
                }
                ShiftKind::Asr => {
                    let old_s = v & sign;
                    c_out = (v & 1) != 0;
                    x_flag = c_out;
                    v = (v >> 1) | old_s;
                }
                ShiftKind::Lsl => {
                    c_out = (v & sign) != 0;
                    x_flag = c_out;
                    v = (v << 1) & mask;
                }
                ShiftKind::Lsr => {
                    c_out = (v & 1) != 0;
                    x_flag = c_out;
                    v >>= 1;
                }
                ShiftKind::Roxl => {
                    let next_x = (v & sign) != 0;
                    v = ((v << 1) | if x_flag { 1 } else { 0 }) & mask;
                    x_flag = next_x;
                    c_out = next_x;
                }
                ShiftKind::Roxr => {
                    let next_x = (v & 1) != 0;
                    v = (v >> 1) | if x_flag { sign } else { 0 };
                    x_flag = next_x;
                    c_out = next_x;
                }
                ShiftKind::Rol => {
                    c_out = (v & sign) != 0;
                    v = ((v << 1) | if c_out { 1 } else { 0 }) & mask;
                }
                ShiftKind::Ror => {
                    c_out = (v & 1) != 0;
                    v = (v >> 1) | if c_out { sign } else { 0 };
                }
            }
        }
        let mut f = self.sr & !0x1F;
        if x_flag { f |= FLAG_X; }
        if v == 0 { f |= FLAG_Z; }
        if (v & sign) != 0 { f |= FLAG_N; }
        if v_flag { f |= FLAG_V; }
        if c_out { f |= FLAG_C; }
        self.sr = f;
        v
    }

    pub fn op_shift_reg_b(&mut self, kind: ShiftKind, count: u32, dn: usize) {
        let r = self.shift_core(kind, self.d[dn] & 0xFF, 8, count) as u8;
        self.set_d_b(dn, r);
    }

    pub fn op_shift_reg_w(&mut self, kind: ShiftKind, count: u32, dn: usize) {
        let r = self.shift_core(kind, self.d[dn] & 0xFFFF, 16, count) as u16;
        self.set_d_w(dn, r);
    }

    pub fn op_shift_reg_l(&mut self, kind: ShiftKind, count: u32, dn: usize) {
        let r = self.shift_core(kind, self.d[dn], 32, count);
        self.d[dn] = r;
    }

    pub fn op_shift_mem(&mut self, kind: ShiftKind, val: u16) -> u16 {
        self.shift_core(kind, val as u32, 16, 1) as u16
    }

    // --- CHK ---
    pub fn op_chk(&mut self, bound: u16, dn: usize) -> bool {
        let val = self.d[dn] as i16;
        let bnd = bound as i16;
        if val < 0 {
            self.sr |= FLAG_N;
            true
        } else if val > bnd {
            self.sr &= !FLAG_N;
            true
        } else {
            false
        }
    }

    // --- MOVEM ---
    pub fn movem_to_predec_w(&mut self, bus: &mut Bus, an: usize, mask: u16) {
        let mut addr = self.a[an];
        // Predecrement bit order: bit 0 = A7, bit 7 = A0, bit 8 = D7, bit 15 = D0
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                addr = addr.wrapping_sub(2);
                let v = if i < 8 { self.a[7 - i] as u16 } else { self.d[15 - i] as u16 };
                bus.write_w(addr, v);
            }
        }
        self.a[an] = addr;
    }

    pub fn movem_to_predec_l(&mut self, bus: &mut Bus, an: usize, mask: u16) {
        let mut addr = self.a[an];
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                addr = addr.wrapping_sub(4);
                let v = if i < 8 { self.a[7 - i] } else { self.d[15 - i] };
                bus.write_l(addr, v);
            }
        }
        self.a[an] = addr;
    }

    pub fn movem_to_mem_w(&mut self, bus: &mut Bus, mut addr: u32, mask: u16) {
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                let v = if i < 8 { self.d[i] as u16 } else { self.a[i - 8] as u16 };
                bus.write_w(addr, v);
                addr = addr.wrapping_add(2);
            }
        }
    }

    pub fn movem_to_mem_l(&mut self, bus: &mut Bus, mut addr: u32, mask: u16) {
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                let v = if i < 8 { self.d[i] } else { self.a[i - 8] };
                bus.write_l(addr, v);
                addr = addr.wrapping_add(4);
            }
        }
    }

    pub fn movem_from_postinc_w(&mut self, bus: &mut Bus, an: usize, mask: u16) {
        let mut addr = self.a[an];
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                let v = bus.read_w(addr) as i16 as i32 as u32;
                addr = addr.wrapping_add(2);
                if i < 8 { self.d[i] = v; } else { self.a[i - 8] = v; }
            }
        }
        if (mask & (1 << (8 + an))) == 0 {
            self.a[an] = addr;
        }
    }

    pub fn movem_from_postinc_l(&mut self, bus: &mut Bus, an: usize, mask: u16) {
        let mut addr = self.a[an];
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                let v = bus.read_l(addr);
                addr = addr.wrapping_add(4);
                if i < 8 { self.d[i] = v; } else { self.a[i - 8] = v; }
            }
        }
        if (mask & (1 << (8 + an))) == 0 {
            self.a[an] = addr;
        }
    }

    pub fn movem_from_mem_w(&mut self, bus: &mut Bus, mut addr: u32, mask: u16) {
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                let v = bus.read_w(addr) as i16 as i32 as u32;
                addr = addr.wrapping_add(2);
                if i < 8 { self.d[i] = v; } else { self.a[i - 8] = v; }
            }
        }
    }

    pub fn movem_from_mem_l(&mut self, bus: &mut Bus, mut addr: u32, mask: u16) {
        for i in 0..16 {
            if (mask & (1 << i)) != 0 {
                let v = bus.read_l(addr);
                addr = addr.wrapping_add(4);
                if i < 8 { self.d[i] = v; } else { self.a[i - 8] = v; }
            }
        }
    }

    // --- MOVEP ---
    pub fn movep_to_mem_w(&mut self, bus: &mut Bus, addr: u32, dn: usize) {
        let v = self.d[dn];
        bus.write_b(addr, ((v >> 8) & 0xFF) as u8);
        bus.write_b(addr.wrapping_add(2), (v & 0xFF) as u8);
    }

    pub fn movep_to_mem_l(&mut self, bus: &mut Bus, addr: u32, dn: usize) {
        let v = self.d[dn];
        bus.write_b(addr, ((v >> 24) & 0xFF) as u8);
        bus.write_b(addr.wrapping_add(2), ((v >> 16) & 0xFF) as u8);
        bus.write_b(addr.wrapping_add(4), ((v >> 8) & 0xFF) as u8);
        bus.write_b(addr.wrapping_add(6), (v & 0xFF) as u8);
    }

    pub fn movep_from_mem_w(&mut self, bus: &mut Bus, addr: u32, dn: usize) {
        let hi = bus.read_b(addr) as u16;
        let lo = bus.read_b(addr.wrapping_add(2)) as u16;
        self.set_d_w(dn, (hi << 8) | lo);
    }

    pub fn movep_from_mem_l(&mut self, bus: &mut Bus, addr: u32, dn: usize) {
        let b0 = bus.read_b(addr) as u32;
        let b1 = bus.read_b(addr.wrapping_add(2)) as u32;
        let b2 = bus.read_b(addr.wrapping_add(4)) as u32;
        let b3 = bus.read_b(addr.wrapping_add(6)) as u32;
        self.d[dn] = (b0 << 24) | (b1 << 16) | (b2 << 8) | b3;
    }
}
