use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    pub fn suffix(self) -> &'static str {
        match self {
            Size::Byte => "b",
            Size::Word => "w",
            Size::Long => "l",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexReg {
    pub is_addr: bool,
    pub reg: u8,
    pub is_long: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ea {
    Dn(u8),
    An(u8),
    AnInd(u8),
    AnPostInc(u8),
    AnPreDec(u8),
    AnDisp(u8, i16),
    AnIndex(u8, IndexReg, i8),
    AbsW(i16),
    AbsL(u32),
    PcDisp(u32, i16),            // (base_pc_of_ext_word, disp)
    PcIndex(u32, IndexReg, i8),  // (base_pc_of_ext_word, idx, disp)
    Imm(u32),
}

impl fmt::Display for Ea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Ea::Dn(r) => write!(f, "d{}", r),
            Ea::An(r) => write!(f, "a{}", r),
            Ea::AnInd(r) => write!(f, "(a{})", r),
            Ea::AnPostInc(r) => write!(f, "(a{})+", r),
            Ea::AnPreDec(r) => write!(f, "-(a{})", r),
            Ea::AnDisp(r, d) => {
                if d < 0 { write!(f, "-${:x}(a{})", -(d as i32), r) } else { write!(f, "${:x}(a{})", d, r) }
            }
            Ea::AnIndex(r, idx, d) => {
                let ir = if idx.is_addr { 'a' } else { 'd' };
                let sz = if idx.is_long { 'l' } else { 'w' };
                write!(f, "{}(a{},{}{}.{})", d, r, ir, idx.reg, sz)
            }
            Ea::AbsW(v) => write!(f, "${:04x}.w", v as u16),
            Ea::AbsL(v) => write!(f, "${:06x}", v),
            Ea::PcDisp(base, d) => {
                let target = base.wrapping_add(d as i32 as u32);
                write!(f, "${:06x}(pc)", target)
            }
            Ea::PcIndex(base, idx, d) => {
                let target = base.wrapping_add(d as i32 as u32);
                let ir = if idx.is_addr { 'a' } else { 'd' };
                let sz = if idx.is_long { 'l' } else { 'w' };
                write!(f, "${:06x}(pc,{}{}.{})", target, ir, idx.reg, sz)
            }
            Ea::Imm(v) => write!(f, "#${:x}", v),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    T, F, Hi, Ls, Cc, Cs, Ne, Eq, Vc, Vs, Pl, Mi, Ge, Lt, Gt, Le,
}

impl Cond {
    pub fn from_bits(c: u8) -> Self {
        match c & 0xF {
            0x0 => Cond::T,  0x1 => Cond::F,  0x2 => Cond::Hi, 0x3 => Cond::Ls,
            0x4 => Cond::Cc, 0x5 => Cond::Cs, 0x6 => Cond::Ne, 0x7 => Cond::Eq,
            0x8 => Cond::Vc, 0x9 => Cond::Vs, 0xA => Cond::Pl, 0xB => Cond::Mi,
            0xC => Cond::Ge, 0xD => Cond::Lt, 0xE => Cond::Gt, _   => Cond::Le,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Cond::T => "t", Cond::F => "f", Cond::Hi => "hi", Cond::Ls => "ls",
            Cond::Cc => "cc", Cond::Cs => "cs", Cond::Ne => "ne", Cond::Eq => "eq",
            Cond::Vc => "vc", Cond::Vs => "vs", Cond::Pl => "pl", Cond::Mi => "mi",
            Cond::Ge => "ge", Cond::Lt => "lt", Cond::Gt => "gt", Cond::Le => "le",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftKind { Asl, Asr, Lsl, Lsr, Roxl, Roxr, Rol, Ror }

impl ShiftKind {
    pub fn name(self) -> &'static str {
        match self {
            ShiftKind::Asl => "asl", ShiftKind::Asr => "asr",
            ShiftKind::Lsl => "lsl", ShiftKind::Lsr => "lsr",
            ShiftKind::Roxl => "roxl", ShiftKind::Roxr => "roxr",
            ShiftKind::Rol => "rol", ShiftKind::Ror => "ror",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitOp { Btst, Bchg, Bclr, Bset }

impl BitOp {
    pub fn name(self) -> &'static str {
        match self {
            BitOp::Btst => "btst", BitOp::Bchg => "bchg",
            BitOp::Bclr => "bclr", BitOp::Bset => "bset",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Move { size: Size, src: Ea, dst: Ea },
    Movea { size: Size, src: Ea, dst_an: u8 },
    Moveq { imm: i8, dst_dn: u8 },
    MoveFromSr { dst: Ea },
    MoveToCcr { src: Ea },
    MoveToSr { src: Ea },
    MoveUsp { an: u8, to_usp: bool },
    Movem { size: Size, to_mem: bool, mask: u16, ea: Ea },
    Movep { size: Size, to_mem: bool, dn: u8, an: u8, disp: i16 },
    Lea { src: Ea, dst_an: u8 },
    Pea { src: Ea },
    Exg { rx: u8, ry: u8, mode: u8 }, // mode: 0=Dx,Dy 1=Ax,Ay 2=Dx,Ay
    Swap { dn: u8 },
    Ext { size: Size, dn: u8 },
    Add { size: Size, src: Ea, dst: Ea },
    Adda { size: Size, src: Ea, dst_an: u8 },
    Addq { size: Size, imm: u8, dst: Ea },
    Addx { size: Size, rx: u8, ry: u8, mem: bool },
    Sub { size: Size, src: Ea, dst: Ea },
    Suba { size: Size, src: Ea, dst_an: u8 },
    Subq { size: Size, imm: u8, dst: Ea },
    Subx { size: Size, rx: u8, ry: u8, mem: bool },
    Cmp { size: Size, src: Ea, dst_dn: u8 },
    Cmpa { size: Size, src: Ea, dst_an: u8 },
    Cmpi { size: Size, imm: u32, dst: Ea },
    Cmpm { size: Size, ax: u8, ay: u8 },
    And { size: Size, src: Ea, dst: Ea },
    AndiCcr { imm: u8 },
    AndiSr { imm: u16 },
    Or { size: Size, src: Ea, dst: Ea },
    OriCcr { imm: u8 },
    OriSr { imm: u16 },
    Eor { size: Size, src: Ea, dst: Ea },
    EoriCcr { imm: u8 },
    EoriSr { imm: u16 },
    Not { size: Size, dst: Ea },
    Neg { size: Size, dst: Ea },
    Negx { size: Size, dst: Ea },
    Clr { size: Size, dst: Ea },
    Tst { size: Size, ea: Ea },
    Tas { ea: Ea },
    Mulu { src: Ea, dst_dn: u8 },
    Muls { src: Ea, dst_dn: u8 },
    Divu { src: Ea, dst_dn: u8 },
    Divs { src: Ea, dst_dn: u8 },
    Abcd { rx: u8, ry: u8, mem: bool },
    Sbcd { rx: u8, ry: u8, mem: bool },
    Nbcd { ea: Ea },
    Bit { op: BitOp, bit_src: Ea, dst: Ea },
    ShiftReg { kind: ShiftKind, size: Size, count_is_reg: bool, count: u8, dst_dn: u8 },
    ShiftMem { kind: ShiftKind, dst: Ea },
    Bra { target: u32 },
    Bsr { target: u32 },
    Bcc { cond: Cond, target: u32 },
    Dbcc { cond: Cond, dn: u8, target: u32 },
    Scc { cond: Cond, dst: Ea },
    Jmp { target: Ea },
    Jsr { target: Ea },
    Rts,
    Rte,
    Rtr,
    Nop,
    Reset,
    Stop { imm: u16 },
    Trap { vector: u8 },
    Trapv,
    Chk { src: Ea, dn: u8 },
    Link { an: u8, disp: i16 },
    Unlk { an: u8 },
    Movec { to_ctrl: bool, reg_is_addr: bool, reg: u8, ctrl: u16 },
    Illegal,
    Unknown(u16),
}

#[derive(Debug, Clone)]
pub struct Insn {
    pub pc: u32,
    pub len: u32,
    pub op: Op,
}

impl fmt::Display for Insn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.op {
            Op::Move { size, src, dst } => write!(f, "move.{} {},{}", size.suffix(), src, dst),
            Op::Movea { size, src, dst_an } => write!(f, "movea.{} {},a{}", size.suffix(), src, dst_an),
            Op::Moveq { imm, dst_dn } => write!(f, "moveq #{},d{}", imm, dst_dn),
            Op::MoveFromSr { dst } => write!(f, "move sr,{}", dst),
            Op::MoveToCcr { src } => write!(f, "move {},ccr", src),
            Op::MoveToSr { src } => write!(f, "move {},sr", src),
            Op::MoveUsp { an, to_usp } => {
                if to_usp { write!(f, "move a{},usp", an) } else { write!(f, "move usp,a{}", an) }
            }
            Op::Movem { size, to_mem, mask, ea } => {
                if to_mem {
                    write!(f, "movem.{} #${:04x},{}", size.suffix(), mask, ea)
                } else {
                    write!(f, "movem.{} {},#${:04x}", size.suffix(), ea, mask)
                }
            }
            Op::Movep { size, to_mem, dn, an, disp } => {
                if to_mem {
                    write!(f, "movep.{} d{},{}(a{})", size.suffix(), dn, disp, an)
                } else {
                    write!(f, "movep.{} {}(a{}),d{}", size.suffix(), disp, an, dn)
                }
            }
            Op::Lea { src, dst_an } => write!(f, "lea {},a{}", src, dst_an),
            Op::Pea { src } => write!(f, "pea {}", src),
            Op::Exg { rx, ry, mode } => match mode {
                0 => write!(f, "exg d{},d{}", rx, ry),
                1 => write!(f, "exg a{},a{}", rx, ry),
                _ => write!(f, "exg d{},a{}", rx, ry),
            },
            Op::Swap { dn } => write!(f, "swap d{}", dn),
            Op::Ext { size, dn } => write!(f, "ext.{} d{}", size.suffix(), dn),
            Op::Add { size, src, dst } => write!(f, "add.{} {},{}", size.suffix(), src, dst),
            Op::Adda { size, src, dst_an } => write!(f, "adda.{} {},a{}", size.suffix(), src, dst_an),
            Op::Addq { size, imm, dst } => write!(f, "addq.{} #{},{}", size.suffix(), imm, dst),
            Op::Addx { size, rx, ry, mem } => {
                if mem { write!(f, "addx.{} -(a{}),-(a{})", size.suffix(), ry, rx) }
                else { write!(f, "addx.{} d{},d{}", size.suffix(), ry, rx) }
            }
            Op::Sub { size, src, dst } => write!(f, "sub.{} {},{}", size.suffix(), src, dst),
            Op::Suba { size, src, dst_an } => write!(f, "suba.{} {},a{}", size.suffix(), src, dst_an),
            Op::Subq { size, imm, dst } => write!(f, "subq.{} #{},{}", size.suffix(), imm, dst),
            Op::Subx { size, rx, ry, mem } => {
                if mem { write!(f, "subx.{} -(a{}),-(a{})", size.suffix(), ry, rx) }
                else { write!(f, "subx.{} d{},d{}", size.suffix(), ry, rx) }
            }
            Op::Cmp { size, src, dst_dn } => write!(f, "cmp.{} {},d{}", size.suffix(), src, dst_dn),
            Op::Cmpa { size, src, dst_an } => write!(f, "cmpa.{} {},a{}", size.suffix(), src, dst_an),
            Op::Cmpi { size, imm, dst } => write!(f, "cmpi.{} #${:x},{}", size.suffix(), imm, dst),
            Op::Cmpm { size, ax, ay } => write!(f, "cmpm.{} (a{})+,(a{})+", size.suffix(), ay, ax),
            Op::And { size, src, dst } => write!(f, "and.{} {},{}", size.suffix(), src, dst),
            Op::AndiCcr { imm } => write!(f, "andi #${:02x},ccr", imm),
            Op::AndiSr { imm } => write!(f, "andi #${:04x},sr", imm),
            Op::Or { size, src, dst } => write!(f, "or.{} {},{}", size.suffix(), src, dst),
            Op::OriCcr { imm } => write!(f, "ori #${:02x},ccr", imm),
            Op::OriSr { imm } => write!(f, "ori #${:04x},sr", imm),
            Op::Eor { size, src, dst } => write!(f, "eor.{} {},{}", size.suffix(), src, dst),
            Op::EoriCcr { imm } => write!(f, "eori #${:02x},ccr", imm),
            Op::EoriSr { imm } => write!(f, "eori #${:04x},sr", imm),
            Op::Not { size, dst } => write!(f, "not.{} {}", size.suffix(), dst),
            Op::Neg { size, dst } => write!(f, "neg.{} {}", size.suffix(), dst),
            Op::Negx { size, dst } => write!(f, "negx.{} {}", size.suffix(), dst),
            Op::Clr { size, dst } => write!(f, "clr.{} {}", size.suffix(), dst),
            Op::Tst { size, ea } => write!(f, "tst.{} {}", size.suffix(), ea),
            Op::Tas { ea } => write!(f, "tas {}", ea),
            Op::Mulu { src, dst_dn } => write!(f, "mulu.w {},d{}", src, dst_dn),
            Op::Muls { src, dst_dn } => write!(f, "muls.w {},d{}", src, dst_dn),
            Op::Divu { src, dst_dn } => write!(f, "divu.w {},d{}", src, dst_dn),
            Op::Divs { src, dst_dn } => write!(f, "divs.w {},d{}", src, dst_dn),
            Op::Abcd { rx, ry, mem } => {
                if mem { write!(f, "abcd -(a{}),-(a{})", ry, rx) } else { write!(f, "abcd d{},d{}", ry, rx) }
            }
            Op::Sbcd { rx, ry, mem } => {
                if mem { write!(f, "sbcd -(a{}),-(a{})", ry, rx) } else { write!(f, "sbcd d{},d{}", ry, rx) }
            }
            Op::Nbcd { ea } => write!(f, "nbcd {}", ea),
            Op::Bit { op, bit_src, dst } => write!(f, "{} {},{}", op.name(), bit_src, dst),
            Op::ShiftReg { kind, size, count_is_reg, count, dst_dn } => {
                if count_is_reg {
                    write!(f, "{}.{} d{},d{}", kind.name(), size.suffix(), count, dst_dn)
                } else {
                    write!(f, "{}.{} #{},d{}", kind.name(), size.suffix(), count, dst_dn)
                }
            }
            Op::ShiftMem { kind, dst } => write!(f, "{}.w {}", kind.name(), dst),
            Op::Bra { target } => write!(f, "bra ${:06x}", target),
            Op::Bsr { target } => write!(f, "bsr ${:06x}", target),
            Op::Bcc { cond, target } => write!(f, "b{} ${:06x}", cond.name(), target),
            Op::Dbcc { cond, dn, target } => write!(f, "db{} d{},${:06x}", cond.name(), dn, target),
            Op::Scc { cond, dst } => write!(f, "s{} {}", cond.name(), dst),
            Op::Jmp { target } => write!(f, "jmp {}", target),
            Op::Jsr { target } => write!(f, "jsr {}", target),
            Op::Rts => write!(f, "rts"),
            Op::Rte => write!(f, "rte"),
            Op::Rtr => write!(f, "rtr"),
            Op::Nop => write!(f, "nop"),
            Op::Reset => write!(f, "reset"),
            Op::Stop { imm } => write!(f, "stop #${:04x}", imm),
            Op::Trap { vector } => write!(f, "trap #{}", vector),
            Op::Trapv => write!(f, "trapv"),
            Op::Chk { src, dn } => write!(f, "chk.w {},d{}", src, dn),
            Op::Link { an, disp } => write!(f, "link a{},#{}", an, disp),
            Op::Unlk { an } => write!(f, "unlk a{}", an),
            Op::Movec { to_ctrl, reg_is_addr, reg, ctrl } => {
                let r = if reg_is_addr { 'a' } else { 'd' };
                if to_ctrl { write!(f, "movec {}{},#${:03x}", r, reg, ctrl) }
                else { write!(f, "movec #${:03x},{}{}", ctrl, r, reg) }
            }
            Op::Illegal => write!(f, "illegal"),
            Op::Unknown(w) => write!(f, "dc.w ${:04x}", w),
        }
    }
}

struct Reader<'a> {
    mem: &'a [u8],
    base_addr: u32,
    pc: u32,
}

impl<'a> Reader<'a> {
    fn new(mem: &'a [u8], base_addr: u32, pc: u32) -> Self {
        Self { mem, base_addr, pc }
    }
    fn read_u16(&mut self) -> Option<u16> {
        let off = self.pc.checked_sub(self.base_addr)? as usize;
        if off + 2 > self.mem.len() { return None; }
        let v = u16::from_be_bytes([self.mem[off], self.mem[off + 1]]);
        self.pc += 2;
        Some(v)
    }
    fn read_i16(&mut self) -> Option<i16> { Some(self.read_u16()? as i16) }
    fn read_u32(&mut self) -> Option<u32> {
        let hi = self.read_u16()? as u32;
        let lo = self.read_u16()? as u32;
        Some((hi << 16) | lo)
    }
    fn read_imm(&mut self, size: Size) -> Option<u32> {
        match size {
            Size::Byte => Some((self.read_u16()? & 0xFF) as u32),
            Size::Word => Some(self.read_u16()? as u32),
            Size::Long => self.read_u32(),
        }
    }
    fn read_ea(&mut self, mode: u8, reg: u8, size: Size) -> Option<Ea> {
        match mode {
            0 => Some(Ea::Dn(reg)),
            1 => Some(Ea::An(reg)),
            2 => Some(Ea::AnInd(reg)),
            3 => Some(Ea::AnPostInc(reg)),
            4 => Some(Ea::AnPreDec(reg)),
            5 => Some(Ea::AnDisp(reg, self.read_i16()?)),
            6 => {
                let ext = self.read_u16()?;
                let idx = IndexReg {
                    is_addr: (ext & 0x8000) != 0,
                    reg: ((ext >> 12) & 7) as u8,
                    is_long: (ext & 0x0800) != 0,
                };
                Some(Ea::AnIndex(reg, idx, (ext & 0xFF) as i8))
            }
            7 => match reg {
                0 => Some(Ea::AbsW(self.read_i16()?)),
                1 => Some(Ea::AbsL(self.read_u32()?)),
                2 => {
                    let ext_pc = self.pc;
                    Some(Ea::PcDisp(ext_pc, self.read_i16()?))
                }
                3 => {
                    let ext_pc = self.pc;
                    let ext = self.read_u16()?;
                    let idx = IndexReg {
                        is_addr: (ext & 0x8000) != 0,
                        reg: ((ext >> 12) & 7) as u8,
                        is_long: (ext & 0x0800) != 0,
                    };
                    Some(Ea::PcIndex(ext_pc, idx, (ext & 0xFF) as i8))
                }
                4 => Some(Ea::Imm(self.read_imm(size)?)),
                _ => None,
            },
            _ => None,
        }
    }
}

fn decode_size2(bits: u16) -> Option<Size> {
    match bits & 3 {
        0 => Some(Size::Byte),
        1 => Some(Size::Word),
        2 => Some(Size::Long),
        _ => None,
    }
}

fn decode_shift_kind(kind_bits: u16, dir_left: bool) -> ShiftKind {
    match (kind_bits & 3, dir_left) {
        (0, true) => ShiftKind::Asl,  (0, false) => ShiftKind::Asr,
        (1, true) => ShiftKind::Lsl,  (1, false) => ShiftKind::Lsr,
        (2, true) => ShiftKind::Roxl, (2, false) => ShiftKind::Roxr,
        (_, true) => ShiftKind::Rol,  (_, false) => ShiftKind::Ror,
    }
}

pub fn decode(mem: &[u8], base_addr: u32, pc: u32) -> Insn {
    let mut r = Reader::new(mem, base_addr, pc);
    let Some(w) = r.read_u16() else {
        return Insn { pc, len: 2, op: Op::Unknown(0) };
    };
    let op = decode_inner(&mut r, w).unwrap_or(Op::Unknown(w));
    let len = if matches!(op, Op::Unknown(_)) { 2 } else { r.pc - pc };
    Insn { pc, len, op }
}

fn decode_inner(r: &mut Reader<'_>, w: u16) -> Option<Op> {
    let top = (w >> 12) & 0xF;
    let ea_mode = ((w >> 3) & 7) as u8;
    let ea_reg = (w & 7) as u8;
    let hi_reg = ((w >> 9) & 7) as u8;

    match top {
        0x0 => {
            // Dynamic bit ops or MOVEP: bit 8 == 1
            if (w & 0x0100) != 0 {
                if ea_mode == 1 {
                    let opmode = (w >> 6) & 3;
                    let size = if (opmode & 1) == 0 { Size::Word } else { Size::Long };
                    let to_mem = (opmode & 2) != 0;
                    let disp = r.read_i16()?;
                    return Some(Op::Movep { size, to_mem, dn: hi_reg, an: ea_reg, disp });
                }
                let bit_op = match (w >> 6) & 3 {
                    0 => BitOp::Btst, 1 => BitOp::Bchg, 2 => BitOp::Bclr, _ => BitOp::Bset,
                };
                let dst = r.read_ea(ea_mode, ea_reg, Size::Byte)?;
                return Some(Op::Bit { op: bit_op, bit_src: Ea::Dn(hi_reg), dst });
            }
            // Static bit ops: 0000 1000 xx eamode eareg
            if (w & 0xFF00) == 0x0800 {
                let bit_op = match (w >> 6) & 3 {
                    0 => BitOp::Btst, 1 => BitOp::Bchg, 2 => BitOp::Bclr, _ => BitOp::Bset,
                };
                let bit_num = (r.read_u16()? & 0xFF) as u32;
                let dst = r.read_ea(ea_mode, ea_reg, Size::Byte)?;
                return Some(Op::Bit { op: bit_op, bit_src: Ea::Imm(bit_num), dst });
            }
            // Immediate ops: ORI(0), ANDI(2), SUBI(4), ADDI(6), EORI(A), CMPI(C)
            let kind = (w >> 9) & 7;
            let sz_bits = (w >> 6) & 3;
            if ea_mode == 7 && ea_reg == 4 {
                // To CCR / SR
                if sz_bits == 0 {
                    let imm = (r.read_u16()? & 0xFF) as u8;
                    return match kind {
                        0 => Some(Op::OriCcr { imm }),
                        1 => Some(Op::AndiCcr { imm }),
                        5 => Some(Op::EoriCcr { imm }),
                        _ => None,
                    };
                } else if sz_bits == 1 {
                    let imm = r.read_u16()?;
                    return match kind {
                        0 => Some(Op::OriSr { imm }),
                        1 => Some(Op::AndiSr { imm }),
                        5 => Some(Op::EoriSr { imm }),
                        _ => None,
                    };
                }
                return None;
            }
            let size = decode_size2(sz_bits)?;
            let imm = r.read_imm(size)?;
            let dst = r.read_ea(ea_mode, ea_reg, size)?;
            match kind {
                0 => Some(Op::Or { size, src: Ea::Imm(imm), dst }),
                1 => Some(Op::And { size, src: Ea::Imm(imm), dst }),
                2 => Some(Op::Sub { size, src: Ea::Imm(imm), dst }),
                3 => Some(Op::Add { size, src: Ea::Imm(imm), dst }),
                5 => Some(Op::Eor { size, src: Ea::Imm(imm), dst }),
                6 => Some(Op::Cmpi { size, imm, dst }),
                _ => None,
            }
        }
        0x1 | 0x2 | 0x3 => {
            let size = match top { 1 => Size::Byte, 3 => Size::Word, _ => Size::Long };
            let src = r.read_ea(ea_mode, ea_reg, size)?;
            let dst_mode = ((w >> 6) & 7) as u8;
            let dst_reg = hi_reg;
            if dst_mode == 1 {
                if size == Size::Byte { return None; }
                return Some(Op::Movea { size, src, dst_an: dst_reg });
            }
            let dst = r.read_ea(dst_mode, dst_reg, size)?;
            Some(Op::Move { size, src, dst })
        }
        0x4 => {
            if w == 0x4AFC { return Some(Op::Illegal); }
            if w == 0x4E70 { return Some(Op::Reset); }
            if w == 0x4E71 { return Some(Op::Nop); }
            if w == 0x4E72 { return Some(Op::Stop { imm: r.read_u16()? }); }
            if w == 0x4E73 { return Some(Op::Rte); }
            if w == 0x4E75 { return Some(Op::Rts); }
            if w == 0x4E76 { return Some(Op::Trapv); }
            if w == 0x4E77 { return Some(Op::Rtr); }
            if w == 0x4E7A || w == 0x4E7B {
                let ext = r.read_u16()?;
                return Some(Op::Movec {
                    to_ctrl: w == 0x4E7B,
                    reg_is_addr: (ext & 0x8000) != 0,
                    reg: ((ext >> 12) & 7) as u8,
                    ctrl: ext & 0x0FFF,
                });
            }
            if (w & 0xFFF0) == 0x4E40 { return Some(Op::Trap { vector: (w & 0xF) as u8 }); }
            if (w & 0xFFF8) == 0x4E50 { return Some(Op::Link { an: ea_reg, disp: r.read_i16()? }); }
            if (w & 0xFFF8) == 0x4E58 { return Some(Op::Unlk { an: ea_reg }); }
            if (w & 0xFFF0) == 0x4E60 {
                return Some(Op::MoveUsp { an: ea_reg, to_usp: (w & 8) == 0 });
            }
            if (w & 0xFFC0) == 0x4E80 {
                return Some(Op::Jsr { target: r.read_ea(ea_mode, ea_reg, Size::Long)? });
            }
            if (w & 0xFFC0) == 0x4EC0 {
                return Some(Op::Jmp { target: r.read_ea(ea_mode, ea_reg, Size::Long)? });
            }
            if (w & 0xF1C0) == 0x41C0 {
                return Some(Op::Lea { src: r.read_ea(ea_mode, ea_reg, Size::Long)?, dst_an: hi_reg });
            }
            if (w & 0xF1C0) == 0x4180 {
                return Some(Op::Chk { src: r.read_ea(ea_mode, ea_reg, Size::Word)?, dn: hi_reg });
            }
            if (w & 0xFFC0) == 0x40C0 {
                return Some(Op::MoveFromSr { dst: r.read_ea(ea_mode, ea_reg, Size::Word)? });
            }
            if (w & 0xFFC0) == 0x44C0 {
                return Some(Op::MoveToCcr { src: r.read_ea(ea_mode, ea_reg, Size::Word)? });
            }
            if (w & 0xFFC0) == 0x46C0 {
                return Some(Op::MoveToSr { src: r.read_ea(ea_mode, ea_reg, Size::Word)? });
            }
            if (w & 0xFFC0) == 0x4800 {
                return Some(Op::Nbcd { ea: r.read_ea(ea_mode, ea_reg, Size::Byte)? });
            }
            if (w & 0xFFF8) == 0x4840 {
                return Some(Op::Swap { dn: ea_reg });
            }
            if (w & 0xFFC0) == 0x4840 {
                return Some(Op::Pea { src: r.read_ea(ea_mode, ea_reg, Size::Long)? });
            }
            if (w & 0xFFF8) == 0x4880 {
                return Some(Op::Ext { size: Size::Word, dn: ea_reg });
            }
            if (w & 0xFFF8) == 0x48C0 {
                return Some(Op::Ext { size: Size::Long, dn: ea_reg });
            }
            if (w & 0xFB80) == 0x4880 {
                let to_mem = (w & 0x0400) == 0;
                let size = if (w & 0x0040) == 0 { Size::Word } else { Size::Long };
                let mask = r.read_u16()?;
                let ea = r.read_ea(ea_mode, ea_reg, size)?;
                return Some(Op::Movem { size, to_mem, mask, ea });
            }
            if (w & 0xFFC0) == 0x4AC0 {
                return Some(Op::Tas { ea: r.read_ea(ea_mode, ea_reg, Size::Byte)? });
            }
            let sub = (w >> 8) & 0xF;
            let size = decode_size2((w >> 6) & 3)?;
            let ea = r.read_ea(ea_mode, ea_reg, size)?;
            match sub {
                0x0 => Some(Op::Negx { size, dst: ea }),
                0x2 => Some(Op::Clr { size, dst: ea }),
                0x4 => Some(Op::Neg { size, dst: ea }),
                0x6 => Some(Op::Not { size, dst: ea }),
                0xA => Some(Op::Tst { size, ea }),
                _ => None,
            }
        }
        0x5 => {
            if ((w >> 6) & 3) == 3 {
                let cond = Cond::from_bits(((w >> 8) & 0xF) as u8);
                if ea_mode == 1 {
                    let base = r.pc;
                    let disp = r.read_i16()?;
                    return Some(Op::Dbcc { cond, dn: ea_reg, target: base.wrapping_add(disp as i32 as u32) });
                }
                let dst = r.read_ea(ea_mode, ea_reg, Size::Byte)?;
                return Some(Op::Scc { cond, dst });
            }
            let size = decode_size2((w >> 6) & 3)?;
            let mut imm = hi_reg;
            if imm == 0 { imm = 8; }
            let dst = r.read_ea(ea_mode, ea_reg, size)?;
            if (w & 0x0100) == 0 {
                Some(Op::Addq { size, imm, dst })
            } else {
                Some(Op::Subq { size, imm, dst })
            }
        }
        0x6 => {
            let cond_bits = ((w >> 8) & 0xF) as u8;
            let disp8 = (w & 0xFF) as i8;
            let base = r.pc;
            let disp: i32 = if disp8 == 0 { r.read_i16()? as i32 } else { disp8 as i32 };
            let target = base.wrapping_add(disp as u32);
            match cond_bits {
                0 => Some(Op::Bra { target }),
                1 => Some(Op::Bsr { target }),
                c => Some(Op::Bcc { cond: Cond::from_bits(c), target }),
            }
        }
        0x7 => {
            if (w & 0x0100) != 0 { return None; }
            Some(Op::Moveq { imm: (w & 0xFF) as i8, dst_dn: hi_reg })
        }
        0x8 => {
            let opmode = (w >> 6) & 7;
            if opmode == 3 {
                return Some(Op::Divu { src: r.read_ea(ea_mode, ea_reg, Size::Word)?, dst_dn: hi_reg });
            }
            if opmode == 7 {
                return Some(Op::Divs { src: r.read_ea(ea_mode, ea_reg, Size::Word)?, dst_dn: hi_reg });
            }
            if (w & 0x01F0) == 0x0100 {
                return Some(Op::Sbcd { rx: hi_reg, ry: ea_reg, mem: (w & 8) != 0 });
            }
            let size = decode_size2(opmode & 3)?;
            let ea = r.read_ea(ea_mode, ea_reg, size)?;
            if opmode < 4 {
                Some(Op::Or { size, src: ea, dst: Ea::Dn(hi_reg) })
            } else {
                Some(Op::Or { size, src: Ea::Dn(hi_reg), dst: ea })
            }
        }
        0x9 | 0xD => {
            let is_add = top == 0xD;
            let opmode = (w >> 6) & 7;
            if opmode == 3 || opmode == 7 {
                let size = if opmode == 3 { Size::Word } else { Size::Long };
                let src = r.read_ea(ea_mode, ea_reg, size)?;
                return Some(if is_add {
                    Op::Adda { size, src, dst_an: hi_reg }
                } else {
                    Op::Suba { size, src, dst_an: hi_reg }
                });
            }
            if (w & 0x0100) != 0 && (ea_mode == 0 || ea_mode == 1) {
                let size = decode_size2(opmode & 3)?;
                let mem = ea_mode == 1;
                return Some(if is_add {
                    Op::Addx { size, rx: hi_reg, ry: ea_reg, mem }
                } else {
                    Op::Subx { size, rx: hi_reg, ry: ea_reg, mem }
                });
            }
            let size = decode_size2(opmode & 3)?;
            let ea = r.read_ea(ea_mode, ea_reg, size)?;
            let (src, dst) = if opmode < 4 { (ea, Ea::Dn(hi_reg)) } else { (Ea::Dn(hi_reg), ea) };
            Some(if is_add { Op::Add { size, src, dst } } else { Op::Sub { size, src, dst } })
        }
        0xB => {
            let opmode = (w >> 6) & 7;
            if opmode == 3 || opmode == 7 {
                let size = if opmode == 3 { Size::Word } else { Size::Long };
                return Some(Op::Cmpa { size, src: r.read_ea(ea_mode, ea_reg, size)?, dst_an: hi_reg });
            }
            let size = decode_size2(opmode & 3)?;
            if opmode < 4 {
                Some(Op::Cmp { size, src: r.read_ea(ea_mode, ea_reg, size)?, dst_dn: hi_reg })
            } else if ea_mode == 1 {
                Some(Op::Cmpm { size, ax: hi_reg, ay: ea_reg })
            } else {
                Some(Op::Eor { size, src: Ea::Dn(hi_reg), dst: r.read_ea(ea_mode, ea_reg, size)? })
            }
        }
        0xC => {
            let opmode = (w >> 6) & 7;
            if opmode == 3 {
                return Some(Op::Mulu { src: r.read_ea(ea_mode, ea_reg, Size::Word)?, dst_dn: hi_reg });
            }
            if opmode == 7 {
                return Some(Op::Muls { src: r.read_ea(ea_mode, ea_reg, Size::Word)?, dst_dn: hi_reg });
            }
            if (w & 0x01F0) == 0x0100 {
                return Some(Op::Abcd { rx: hi_reg, ry: ea_reg, mem: (w & 8) != 0 });
            }
            if (w & 0x01F8) == 0x0140 {
                return Some(Op::Exg { rx: hi_reg, ry: ea_reg, mode: 0 });
            }
            if (w & 0x01F8) == 0x0148 {
                return Some(Op::Exg { rx: hi_reg, ry: ea_reg, mode: 1 });
            }
            if (w & 0x01F8) == 0x0188 {
                return Some(Op::Exg { rx: hi_reg, ry: ea_reg, mode: 2 });
            }
            let size = decode_size2(opmode & 3)?;
            let ea = r.read_ea(ea_mode, ea_reg, size)?;
            if opmode < 4 {
                Some(Op::And { size, src: ea, dst: Ea::Dn(hi_reg) })
            } else {
                Some(Op::And { size, src: Ea::Dn(hi_reg), dst: ea })
            }
        }
        0xE => {
            let dir_left = (w & 0x0100) != 0;
            if ((w >> 6) & 3) == 3 {
                let kind = decode_shift_kind((w >> 9) & 3, dir_left);
                let dst = r.read_ea(ea_mode, ea_reg, Size::Word)?;
                return Some(Op::ShiftMem { kind, dst });
            }
            let size = decode_size2((w >> 6) & 3)?;
            let kind = decode_shift_kind((w >> 3) & 3, dir_left);
            let count_is_reg = (w & 0x0020) != 0;
            let mut count = hi_reg;
            if !count_is_reg && count == 0 { count = 8; }
            Some(Op::ShiftReg { kind, size, count_is_reg, count, dst_dn: ea_reg })
        }
        _ => None,
    }
}
