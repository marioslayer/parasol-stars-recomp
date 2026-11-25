// Exact Motorola 68000 single-instruction fallback executor.
// Used ONLY if execution jumps to dynamically loaded/generated code outside the
// statically recompiled functions (e.g., the 4 `$34567893` stubs or newly discovered jump-table targets),
// or while the 68000 Trace flag (SR bit 15) is active.
use crate::bus::Bus;
use crate::cpu::{BitOp, Cond, Cpu, ShiftKind, SR_T};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sz { B, W, L }

impl Sz {
    fn bytes(self) -> u32 { match self { Sz::B => 1, Sz::W => 2, Sz::L => 4 } }
    fn from2(bits: u16) -> Option<Self> {
        match bits & 3 { 0 => Some(Sz::B), 1 => Some(Sz::W), 2 => Some(Sz::L), _ => None }
    }
}

fn cond_from_bits(c: u8) -> Cond {
    match c & 0xF {
        0x0 => Cond::T,  0x1 => Cond::F,  0x2 => Cond::Hi, 0x3 => Cond::Ls,
        0x4 => Cond::Cc, 0x5 => Cond::Cs, 0x6 => Cond::Ne, 0x7 => Cond::Eq,
        0x8 => Cond::Vc, 0x9 => Cond::Vs, 0xA => Cond::Pl, 0xB => Cond::Mi,
        0xC => Cond::Ge, 0xD => Cond::Lt, 0xE => Cond::Gt, _   => Cond::Le,
    }
}

fn shift_kind(bits: u16, left: bool) -> ShiftKind {
    match (bits & 3, left) {
        (0, true) => ShiftKind::Asl,  (0, false) => ShiftKind::Asr,
        (1, true) => ShiftKind::Lsl,  (1, false) => ShiftKind::Lsr,
        (2, true) => ShiftKind::Roxl, (2, false) => ShiftKind::Roxr,
        (_, true) => ShiftKind::Rol,  (_, false) => ShiftKind::Ror,
    }
}

fn fetch_w(cpu: &mut Cpu, bus: &mut Bus) -> u16 {
    let v = bus.read_w(cpu.pc);
    cpu.pc = cpu.pc.wrapping_add(2);
    v
}

fn fetch_l(cpu: &mut Cpu, bus: &mut Bus) -> u32 {
    let hi = fetch_w(cpu, bus) as u32;
    let lo = fetch_w(cpu, bus) as u32;
    (hi << 16) | lo
}

fn fetch_imm(cpu: &mut Cpu, bus: &mut Bus, sz: Sz) -> u32 {
    match sz {
        Sz::B => (fetch_w(cpu, bus) & 0xFF) as u32,
        Sz::W => fetch_w(cpu, bus) as u32,
        Sz::L => fetch_l(cpu, bus),
    }
}

enum Loc {
    Dn(usize),
    An(usize),
    Mem(u32),
    Imm(u32),
}

fn eval_ea(cpu: &mut Cpu, bus: &mut Bus, mode: u8, reg: u8, sz: Sz) -> Loc {
    let r = reg as usize;
    match mode {
        0 => Loc::Dn(r),
        1 => Loc::An(r),
        2 => Loc::Mem(cpu.a[r]),
        3 => {
            let a = cpu.a[r];
            let step = if r == 7 && sz == Sz::B { 2 } else { sz.bytes() };
            cpu.a[r] = a.wrapping_add(step);
            Loc::Mem(a)
        }
        4 => {
            let step = if r == 7 && sz == Sz::B { 2 } else { sz.bytes() };
            cpu.a[r] = cpu.a[r].wrapping_sub(step);
            Loc::Mem(cpu.a[r])
        }
        5 => {
            let d = fetch_w(cpu, bus) as i16 as i32 as u32;
            Loc::Mem(cpu.a[r].wrapping_add(d))
        }
        6 => {
            let ext = fetch_w(cpu, bus);
            let ir = ((ext >> 12) & 7) as usize;
            let raw = if (ext & 0x8000) != 0 { cpu.a[ir] } else { cpu.d[ir] };
            let idx = if (ext & 0x0800) != 0 { raw } else { raw as i16 as i32 as u32 };
            let disp = (ext & 0xFF) as i8 as i32 as u32;
            Loc::Mem(cpu.a[r].wrapping_add(idx).wrapping_add(disp))
        }
        7 => match reg {
            0 => Loc::Mem(fetch_w(cpu, bus) as i16 as i32 as u32),
            1 => Loc::Mem(fetch_l(cpu, bus)),
            2 => {
                let base = cpu.pc;
                let d = fetch_w(cpu, bus) as i16 as i32 as u32;
                Loc::Mem(base.wrapping_add(d))
            }
            3 => {
                let base = cpu.pc;
                let ext = fetch_w(cpu, bus);
                let ir = ((ext >> 12) & 7) as usize;
                let raw = if (ext & 0x8000) != 0 { cpu.a[ir] } else { cpu.d[ir] };
                let idx = if (ext & 0x0800) != 0 { raw } else { raw as i16 as i32 as u32 };
                let disp = (ext & 0xFF) as i8 as i32 as u32;
                Loc::Mem(base.wrapping_add(idx).wrapping_add(disp))
            }
            4 => Loc::Imm(fetch_imm(cpu, bus, sz)),
            _ => Loc::Imm(0),
        },
        _ => Loc::Imm(0),
    }
}

fn read_loc(cpu: &Cpu, bus: &mut Bus, loc: &Loc, sz: Sz) -> u32 {
    match *loc {
        Loc::Dn(r) => match sz { Sz::B => cpu.d[r] & 0xFF, Sz::W => cpu.d[r] & 0xFFFF, Sz::L => cpu.d[r] },
        Loc::An(r) => match sz { Sz::B => cpu.a[r] & 0xFF, Sz::W => cpu.a[r] & 0xFFFF, Sz::L => cpu.a[r] },
        Loc::Imm(v) => v,
        Loc::Mem(a) => match sz {
            Sz::B => bus.read_b(a) as u32,
            Sz::W => bus.read_w(a) as u32,
            Sz::L => bus.read_l(a),
        },
    }
}

fn write_loc(cpu: &mut Cpu, bus: &mut Bus, loc: &Loc, sz: Sz, val: u32) {
    match *loc {
        Loc::Dn(r) => match sz {
            Sz::B => cpu.set_d_b(r, val as u8),
            Sz::W => cpu.set_d_w(r, val as u16),
            Sz::L => cpu.d[r] = val,
        },
        Loc::An(r) => match sz {
            Sz::W => cpu.a[r] = val as i16 as i32 as u32,
            Sz::L => cpu.a[r] = val,
            Sz::B => cpu.a[r] = (cpu.a[r] & 0xFFFFFF00) | (val & 0xFF),
        },
        Loc::Mem(a) => match sz {
            Sz::B => bus.write_b(a, val as u8),
            Sz::W => bus.write_w(a, val as u16),
            Sz::L => bus.write_l(a, val),
        },
        Loc::Imm(_) => {}
    }
}

fn flags_nz(cpu: &mut Cpu, sz: Sz, v: u32) {
    match sz {
        Sz::B => cpu.flags_nz_b(v as u8),
        Sz::W => cpu.flags_nz_w(v as u16),
        Sz::L => cpu.flags_nz_l(v),
    }
}

fn op_add(cpu: &mut Cpu, sz: Sz, s: u32, d: u32) -> u32 {
    match sz {
        Sz::B => cpu.op_add_b(s as u8, d as u8) as u32,
        Sz::W => cpu.op_add_w(s as u16, d as u16) as u32,
        Sz::L => cpu.op_add_l(s, d),
    }
}

fn op_sub(cpu: &mut Cpu, sz: Sz, s: u32, d: u32) -> u32 {
    match sz {
        Sz::B => cpu.op_sub_b(s as u8, d as u8) as u32,
        Sz::W => cpu.op_sub_w(s as u16, d as u16) as u32,
        Sz::L => cpu.op_sub_l(s, d),
    }
}

fn op_cmp(cpu: &mut Cpu, sz: Sz, s: u32, d: u32) {
    match sz {
        Sz::B => cpu.op_cmp_b(s as u8, d as u8),
        Sz::W => cpu.op_cmp_w(s as u16, d as u16),
        Sz::L => cpu.op_cmp_l(s, d),
    }
}

pub fn step_one(cpu: &mut Cpu, bus: &mut Bus) {
    let trace_before = (cpu.sr & SR_T) != 0;
    let insn_pc = cpu.pc;
    let w = fetch_w(cpu, bus);
    let top = (w >> 12) & 0xF;
    let ea_mode = ((w >> 3) & 7) as u8;
    let ea_reg = (w & 7) as u8;
    let hi_reg = ((w >> 9) & 7) as usize;
    let mut took_exception = false;

    match top {
        0x0 => {
            if (w & 0x0100) != 0 {
                if ea_mode == 1 {
                    let opmode = (w >> 6) & 3;
                    let disp = fetch_w(cpu, bus) as i16 as i32 as u32;
                    let addr = cpu.a[ea_reg as usize].wrapping_add(disp);
                    match opmode {
                        0 => cpu.movep_from_mem_w(bus, addr, hi_reg),
                        1 => cpu.movep_from_mem_l(bus, addr, hi_reg),
                        2 => cpu.movep_to_mem_w(bus, addr, hi_reg),
                        _ => cpu.movep_to_mem_l(bus, addr, hi_reg),
                    }
                } else {
                    let bop = match (w >> 6) & 3 { 0 => BitOp::Btst, 1 => BitOp::Bchg, 2 => BitOp::Bclr, _ => BitOp::Bset };
                    let bnum = (cpu.d[hi_reg] & 0xFF) as u8;
                    let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::B);
                    if let Loc::Dn(r) = loc {
                        cpu.op_bit_dn(bop, bnum, r);
                    } else {
                        let v = read_loc(cpu, bus, &loc, Sz::B) as u8;
                        let nv = cpu.op_bit_mem(bop, bnum, v);
                        if bop != BitOp::Btst { write_loc(cpu, bus, &loc, Sz::B, nv as u32); }
                    }
                }
            } else if (w & 0xFF00) == 0x0800 {
                let bop = match (w >> 6) & 3 { 0 => BitOp::Btst, 1 => BitOp::Bchg, 2 => BitOp::Bclr, _ => BitOp::Bset };
                let bnum = (fetch_w(cpu, bus) & 0xFF) as u8;
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::B);
                if let Loc::Dn(r) = loc {
                    cpu.op_bit_dn(bop, bnum, r);
                } else {
                    let v = read_loc(cpu, bus, &loc, Sz::B) as u8;
                    let nv = cpu.op_bit_mem(bop, bnum, v);
                    if bop != BitOp::Btst { write_loc(cpu, bus, &loc, Sz::B, nv as u32); }
                }
            } else {
                let kind = (w >> 9) & 7;
                let sz_bits = (w >> 6) & 3;
                if ea_mode == 7 && ea_reg == 4 {
                    let imm = fetch_w(cpu, bus);
                    if sz_bits == 0 {
                        let c = cpu.sr as u8;
                        let nc = match kind { 0 => c | (imm as u8), 1 => c & (imm as u8), _ => c ^ (imm as u8) };
                        cpu.set_ccr(nc);
                    } else {
                        let ns = match kind { 0 => cpu.sr | imm, 1 => cpu.sr & imm, _ => cpu.sr ^ imm };
                        cpu.set_sr(ns);
                    }
                } else if let Some(sz) = Sz::from2(sz_bits) {
                    let imm = fetch_imm(cpu, bus, sz);
                    let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                    let dv = read_loc(cpu, bus, &loc, sz);
                    match kind {
                        0 => { let r = dv | imm; flags_nz(cpu, sz, r); write_loc(cpu, bus, &loc, sz, r); }
                        1 => { let r = dv & imm; flags_nz(cpu, sz, r); write_loc(cpu, bus, &loc, sz, r); }
                        2 => { let r = op_sub(cpu, sz, imm, dv); write_loc(cpu, bus, &loc, sz, r); }
                        3 => { let r = op_add(cpu, sz, imm, dv); write_loc(cpu, bus, &loc, sz, r); }
                        5 => { let r = dv ^ imm; flags_nz(cpu, sz, r); write_loc(cpu, bus, &loc, sz, r); }
                        6 => { op_cmp(cpu, sz, imm, dv); }
                        _ => {}
                    }
                }
            }
        }
        0x1 | 0x2 | 0x3 => {
            let sz = match top { 1 => Sz::B, 3 => Sz::W, _ => Sz::L };
            let sloc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
            let v = read_loc(cpu, bus, &sloc, sz);
            let dmode = ((w >> 6) & 7) as u8;
            if dmode == 1 {
                cpu.a[hi_reg] = if sz == Sz::W { v as i16 as i32 as u32 } else { v };
            } else {
                flags_nz(cpu, sz, v);
                let dloc = eval_ea(cpu, bus, dmode, hi_reg as u8, sz);
                write_loc(cpu, bus, &dloc, sz, v);
            }
        }
        0x4 => {
            if w == 0x4E71 || w == 0x4E70 { /* NOP / RESET */ }
            else if w == 0x4E72 { let imm = fetch_w(cpu, bus); cpu.set_sr(imm); }
            else if w == 0x4E73 { let nsr = cpu.pop_w(bus); cpu.pc = cpu.pop_l(bus); cpu.set_sr(nsr); }
            else if w == 0x4E75 { cpu.pc = cpu.pop_l(bus); }
            else if w == 0x4E76 { if cpu.flag_v() { let ret = cpu.pc; cpu.exception(bus, 7, ret); took_exception = true; } }
            else if w == 0x4E77 { let nccr = cpu.pop_w(bus); cpu.pc = cpu.pop_l(bus); cpu.set_ccr(nccr as u8); }
            else if w == 0x4AFC || w == 0x4E7A || w == 0x4E7B {
                cpu.exception(bus, 4, insn_pc);
                took_exception = true;
            }
            else if (w & 0xFFF0) == 0x4E40 {
                let ret = cpu.pc;
                cpu.exception(bus, 32 + (w & 0xF) as u8, ret);
                took_exception = true;
            }
            else if (w & 0xFFF8) == 0x4E50 {
                let d = fetch_w(cpu, bus) as i16 as i32 as u32;
                let r = ea_reg as usize;
                let old = cpu.a[r];
                cpu.push_l(bus, old);
                cpu.a[r] = cpu.a[7];
                cpu.a[7] = cpu.a[7].wrapping_add(d);
            }
            else if (w & 0xFFF8) == 0x4E58 {
                let r = ea_reg as usize;
                cpu.a[7] = cpu.a[r];
                cpu.a[r] = cpu.pop_l(bus);
            }
            else if (w & 0xFFF0) == 0x4E60 {
                let r = ea_reg as usize;
                if (w & 8) == 0 { cpu.usp = cpu.a[r]; } else { cpu.a[r] = cpu.usp; }
            }
            else if (w & 0xFFC0) == 0x4E80 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::L);
                if let Loc::Mem(a) = loc { let ret = cpu.pc; cpu.push_l(bus, ret); cpu.pc = a; }
            }
            else if (w & 0xFFC0) == 0x4EC0 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::L);
                if let Loc::Mem(a) = loc { cpu.pc = a; }
            }
            else if (w & 0xF1C0) == 0x41C0 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::L);
                if let Loc::Mem(a) = loc { cpu.a[hi_reg] = a; }
            }
            else if (w & 0xF1C0) == 0x4180 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let bnd = read_loc(cpu, bus, &loc, Sz::W) as u16;
                if cpu.op_chk(bnd, hi_reg) { let ret = cpu.pc; cpu.exception(bus, 6, ret); took_exception = true; }
            }
            else if (w & 0xFFC0) == 0x40C0 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                write_loc(cpu, bus, &loc, Sz::W, cpu.sr as u32);
            }
            else if (w & 0xFFC0) == 0x44C0 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let v = read_loc(cpu, bus, &loc, Sz::W) as u8;
                cpu.set_ccr(v);
            }
            else if (w & 0xFFC0) == 0x46C0 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let v = read_loc(cpu, bus, &loc, Sz::W) as u16;
                cpu.set_sr(v);
            }
            else if (w & 0xFFC0) == 0x4800 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::B);
                let v = read_loc(cpu, bus, &loc, Sz::B) as u8;
                let r = cpu.op_nbcd(v);
                write_loc(cpu, bus, &loc, Sz::B, r as u32);
            }
            else if (w & 0xFFF8) == 0x4840 {
                let r = ea_reg as usize;
                let v = cpu.d[r].rotate_left(16);
                cpu.d[r] = v;
                cpu.flags_nz_l(v);
            }
            else if (w & 0xFFC0) == 0x4840 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::L);
                if let Loc::Mem(a) = loc { cpu.push_l(bus, a); }
            }
            else if (w & 0xFFF8) == 0x4880 {
                let r = ea_reg as usize;
                let v = (cpu.d[r] as i8 as i16) as u16;
                cpu.set_d_w(r, v);
                cpu.flags_nz_w(v);
            }
            else if (w & 0xFFF8) == 0x48C0 {
                let r = ea_reg as usize;
                let v = (cpu.d[r] as i16 as i32) as u32;
                cpu.d[r] = v;
                cpu.flags_nz_l(v);
            }
            else if (w & 0xFB80) == 0x4880 {
                let to_mem = (w & 0x0400) == 0;
                let is_l = (w & 0x0040) != 0;
                let mask = fetch_w(cpu, bus);
                if to_mem {
                    if ea_mode == 4 {
                        if is_l { cpu.movem_to_predec_l(bus, ea_reg as usize, mask); }
                        else { cpu.movem_to_predec_w(bus, ea_reg as usize, mask); }
                    } else if let Loc::Mem(a) = eval_ea(cpu, bus, ea_mode, ea_reg, if is_l { Sz::L } else { Sz::W }) {
                        if is_l { cpu.movem_to_mem_l(bus, a, mask); } else { cpu.movem_to_mem_w(bus, a, mask); }
                    }
                } else {
                    if ea_mode == 3 {
                        if is_l { cpu.movem_from_postinc_l(bus, ea_reg as usize, mask); }
                        else { cpu.movem_from_postinc_w(bus, ea_reg as usize, mask); }
                    } else if let Loc::Mem(a) = eval_ea(cpu, bus, ea_mode, ea_reg, if is_l { Sz::L } else { Sz::W }) {
                        if is_l { cpu.movem_from_mem_l(bus, a, mask); } else { cpu.movem_from_mem_w(bus, a, mask); }
                    }
                }
            }
            else if (w & 0xFFC0) == 0x4AC0 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::B);
                let v = read_loc(cpu, bus, &loc, Sz::B) as u8;
                cpu.flags_nz_b(v);
                write_loc(cpu, bus, &loc, Sz::B, (v | 0x80) as u32);
            }
            else if let Some(sz) = Sz::from2((w >> 6) & 3) {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                match (w >> 8) & 0xF {
                    0x0 => {
                        let v = read_loc(cpu, bus, &loc, sz);
                        let r = match sz { Sz::B => cpu.op_negx_b(v as u8) as u32, Sz::W => cpu.op_negx_w(v as u16) as u32, Sz::L => cpu.op_negx_l(v) };
                        write_loc(cpu, bus, &loc, sz, r);
                    }
                    0x2 => { cpu.flags_clr(); write_loc(cpu, bus, &loc, sz, 0); }
                    0x4 => { let v = read_loc(cpu, bus, &loc, sz); let r = op_sub(cpu, sz, v, 0); write_loc(cpu, bus, &loc, sz, r); }
                    0x6 => { let v = !read_loc(cpu, bus, &loc, sz); flags_nz(cpu, sz, v); write_loc(cpu, bus, &loc, sz, v); }
                    0xA => { let v = read_loc(cpu, bus, &loc, sz); flags_nz(cpu, sz, v); }
                    _ => {}
                }
            }
        }
        0x5 => {
            if ((w >> 6) & 3) == 3 {
                let c = cond_from_bits(((w >> 8) & 0xF) as u8);
                if ea_mode == 1 {
                    let base = cpu.pc;
                    let d = fetch_w(cpu, bus) as i16 as i32 as u32;
                    if !cpu.cond(c) {
                        let r = ea_reg as usize;
                        let nv = (cpu.d[r] as u16).wrapping_sub(1);
                        cpu.set_d_w(r, nv);
                        if nv != 0xFFFF { cpu.pc = base.wrapping_add(d); }
                    }
                } else {
                    let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::B);
                    write_loc(cpu, bus, &loc, Sz::B, if cpu.cond(c) { 0xFF } else { 0 });
                }
            } else if let Some(sz) = Sz::from2((w >> 6) & 3) {
                let imm = if hi_reg == 0 { 8 } else { hi_reg as u32 };
                let is_sub = (w & 0x0100) != 0;
                if ea_mode == 1 {
                    let r = ea_reg as usize;
                    cpu.a[r] = if is_sub { cpu.a[r].wrapping_sub(imm) } else { cpu.a[r].wrapping_add(imm) };
                } else {
                    let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                    let dv = read_loc(cpu, bus, &loc, sz);
                    let res = if is_sub { op_sub(cpu, sz, imm, dv) } else { op_add(cpu, sz, imm, dv) };
                    write_loc(cpu, bus, &loc, sz, res);
                }
            }
        }
        0x6 => {
            let cb = ((w >> 8) & 0xF) as u8;
            let disp8 = (w & 0xFF) as i8;
            let base = cpu.pc;
            let disp = if disp8 == 0 { fetch_w(cpu, bus) as i16 as i32 as u32 } else { disp8 as i32 as u32 };
            let target = base.wrapping_add(disp);
            match cb {
                0 => cpu.pc = target,
                1 => { let ret = cpu.pc; cpu.push_l(bus, ret); cpu.pc = target; }
                c => if cpu.cond(cond_from_bits(c)) { cpu.pc = target; }
            }
        }
        0x7 => {
            let v = (w & 0xFF) as i8 as i32 as u32;
            cpu.d[hi_reg] = v;
            cpu.flags_nz_l(v);
        }
        0x8 => {
            let opmode = (w >> 6) & 7;
            if opmode == 3 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let sv = read_loc(cpu, bus, &loc, Sz::W) as u16;
                let ret = cpu.pc;
                cpu.op_divu(bus, sv, hi_reg, ret);
            } else if opmode == 7 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let sv = read_loc(cpu, bus, &loc, Sz::W) as u16;
                let ret = cpu.pc;
                cpu.op_divs(bus, sv, hi_reg, ret);
            } else if (w & 0x01F0) == 0x0100 {
                cpu.op_sbcd(bus, hi_reg, ea_reg as usize, (w & 8) != 0);
            } else if let Some(sz) = Sz::from2(opmode & 3) {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                if opmode < 4 {
                    let r = read_loc(cpu, bus, &Loc::Dn(hi_reg), sz) | read_loc(cpu, bus, &loc, sz);
                    flags_nz(cpu, sz, r);
                    write_loc(cpu, bus, &Loc::Dn(hi_reg), sz, r);
                } else {
                    let r = read_loc(cpu, bus, &loc, sz) | read_loc(cpu, bus, &Loc::Dn(hi_reg), sz);
                    flags_nz(cpu, sz, r);
                    write_loc(cpu, bus, &loc, sz, r);
                }
            }
        }
        0x9 | 0xD => {
            let is_add = top == 0xD;
            let opmode = (w >> 6) & 7;
            if opmode == 3 || opmode == 7 {
                let sz = if opmode == 3 { Sz::W } else { Sz::L };
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                let sv = read_loc(cpu, bus, &loc, sz);
                let s32 = if sz == Sz::W { sv as i16 as i32 as u32 } else { sv };
                cpu.a[hi_reg] = if is_add { cpu.a[hi_reg].wrapping_add(s32) } else { cpu.a[hi_reg].wrapping_sub(s32) };
            } else if (w & 0x0100) != 0 && (ea_mode == 0 || ea_mode == 1) {
                if let Some(sz) = Sz::from2(opmode & 3) {
                    let mem = ea_mode == 1;
                    match (is_add, sz) {
                        (true, Sz::B) => cpu.op_addx_b(bus, hi_reg, ea_reg as usize, mem),
                        (true, Sz::W) => cpu.op_addx_w(bus, hi_reg, ea_reg as usize, mem),
                        (true, Sz::L) => cpu.op_addx_l(bus, hi_reg, ea_reg as usize, mem),
                        (false, Sz::B) => cpu.op_subx_b(bus, hi_reg, ea_reg as usize, mem),
                        (false, Sz::W) => cpu.op_subx_w(bus, hi_reg, ea_reg as usize, mem),
                        (false, Sz::L) => cpu.op_subx_l(bus, hi_reg, ea_reg as usize, mem),
                    }
                }
            } else if let Some(sz) = Sz::from2(opmode & 3) {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                if opmode < 4 {
                    let sv = read_loc(cpu, bus, &loc, sz);
                    let dv = read_loc(cpu, bus, &Loc::Dn(hi_reg), sz);
                    let r = if is_add { op_add(cpu, sz, sv, dv) } else { op_sub(cpu, sz, sv, dv) };
                    write_loc(cpu, bus, &Loc::Dn(hi_reg), sz, r);
                } else {
                    let sv = read_loc(cpu, bus, &Loc::Dn(hi_reg), sz);
                    let dv = read_loc(cpu, bus, &loc, sz);
                    let r = if is_add { op_add(cpu, sz, sv, dv) } else { op_sub(cpu, sz, sv, dv) };
                    write_loc(cpu, bus, &loc, sz, r);
                }
            }
        }
        0xB => {
            let opmode = (w >> 6) & 7;
            if opmode == 3 || opmode == 7 {
                let sz = if opmode == 3 { Sz::W } else { Sz::L };
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                let sv = read_loc(cpu, bus, &loc, sz);
                let s32 = if sz == Sz::W { sv as i16 as i32 as u32 } else { sv };
                cpu.op_cmp_l(s32, cpu.a[hi_reg]);
            } else if let Some(sz) = Sz::from2(opmode & 3) {
                if opmode < 4 {
                    let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                    let sv = read_loc(cpu, bus, &loc, sz);
                    let dv = read_loc(cpu, bus, &Loc::Dn(hi_reg), sz);
                    op_cmp(cpu, sz, sv, dv);
                } else if ea_mode == 1 {
                    let sl = eval_ea(cpu, bus, 3, ea_reg, sz);
                    let sv = read_loc(cpu, bus, &sl, sz);
                    let dl = eval_ea(cpu, bus, 3, hi_reg as u8, sz);
                    let dv = read_loc(cpu, bus, &dl, sz);
                    op_cmp(cpu, sz, sv, dv);
                } else {
                    let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                    let sv = read_loc(cpu, bus, &Loc::Dn(hi_reg), sz);
                    let dv = read_loc(cpu, bus, &loc, sz);
                    let r = dv ^ sv;
                    flags_nz(cpu, sz, r);
                    write_loc(cpu, bus, &loc, sz, r);
                }
            }
        }
        0xC => {
            let opmode = (w >> 6) & 7;
            if opmode == 3 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let sv = read_loc(cpu, bus, &loc, Sz::W) as u16;
                cpu.op_mulu(sv, hi_reg);
            } else if opmode == 7 {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let sv = read_loc(cpu, bus, &loc, Sz::W) as u16;
                cpu.op_muls(sv, hi_reg);
            } else if (w & 0x01F0) == 0x0100 {
                cpu.op_abcd(bus, hi_reg, ea_reg as usize, (w & 8) != 0);
            } else if (w & 0x01F8) == 0x0140 {
                cpu.d.swap(hi_reg, ea_reg as usize);
            } else if (w & 0x01F8) == 0x0148 {
                cpu.a.swap(hi_reg, ea_reg as usize);
            } else if (w & 0x01F8) == 0x0188 {
                std::mem::swap(&mut cpu.d[hi_reg], &mut cpu.a[ea_reg as usize]);
            } else if let Some(sz) = Sz::from2(opmode & 3) {
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, sz);
                if opmode < 4 {
                    let r = read_loc(cpu, bus, &Loc::Dn(hi_reg), sz) & read_loc(cpu, bus, &loc, sz);
                    flags_nz(cpu, sz, r);
                    write_loc(cpu, bus, &Loc::Dn(hi_reg), sz, r);
                } else {
                    let r = read_loc(cpu, bus, &loc, sz) & read_loc(cpu, bus, &Loc::Dn(hi_reg), sz);
                    flags_nz(cpu, sz, r);
                    write_loc(cpu, bus, &loc, sz, r);
                }
            }
        }
        0xE => {
            let left = (w & 0x0100) != 0;
            if ((w >> 6) & 3) == 3 {
                let k = shift_kind((w >> 9) & 3, left);
                let loc = eval_ea(cpu, bus, ea_mode, ea_reg, Sz::W);
                let v = read_loc(cpu, bus, &loc, Sz::W) as u16;
                let r = cpu.op_shift_mem(k, v);
                write_loc(cpu, bus, &loc, Sz::W, r as u32);
            } else if let Some(sz) = Sz::from2((w >> 6) & 3) {
                let k = shift_kind((w >> 3) & 3, left);
                let cnt = if (w & 0x0020) != 0 {
                    cpu.d[hi_reg] & 63
                } else if hi_reg == 0 { 8 } else { hi_reg as u32 };
                match sz {
                    Sz::B => cpu.op_shift_reg_b(k, cnt, ea_reg as usize),
                    Sz::W => cpu.op_shift_reg_w(k, cnt, ea_reg as usize),
                    Sz::L => cpu.op_shift_reg_l(k, cnt, ea_reg as usize),
                }
            }
        }
        _ => {
            // Line-A / Line-F exception
            let vec = if top == 0xA { 10 } else { 11 };
            cpu.exception(bus, vec, insn_pc);
            took_exception = true;
        }
    }

    cpu.tick(bus, 8);
    if trace_before && !took_exception {
        let ret = cpu.pc;
        cpu.exception(bus, 9, ret); // Vector 9 = $0024 Trace exception!
    }
}
