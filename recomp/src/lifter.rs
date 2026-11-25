use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as FmtWrite;
use crate::m68k::{self, BitOp, Cond, Ea, Insn, Op, ShiftKind, Size};

pub const MAIN_RAM_BASE: u32 = 0x00001000;

/// Discover reachable 68000 instructions, call targets, and basic block leaders.
pub struct Cfg {
    pub insns: BTreeMap<u32, Insn>,
    pub functions: BTreeSet<u32>,
    pub block_leaders: BTreeSet<u32>,
}

/// Additional known entry points (ISRs, jump tables, VM handlers, dynamically discovered targets).
pub fn default_roots(mem: &[u8]) -> Vec<u32> {
    let mut roots = vec![
        0x001994, // Main relocated entry point
        0x00225E, // Level 3 Copper/VBlank ISR ($6C)
        0x001D34, // Level 2 CIA-A Keyboard ISR ($68)
        0x0021F8, // Exception handler ($0C)
        0x00229A, // Default RTE handler
        0x00664E, // HLE Disk Sector Read
        0x0066AE, // HLE Disk Sector Write
        0x0066D8, // Floppy motor timer
        // LZW decompressor indirect targets (called via jsr (a4) / jsr (a6))
        0x017698,
        0x0176AA,
        0x0176BC,
        0x0176CE,
        0x0176E0,
        0x017704,
        0x01771E,
    ];

    // Add all 10 entries of the inline-VM table at $001756
    for i in 0..10u32 {
        let off = (0x001756 - MAIN_RAM_BASE + i * 4) as usize;
        if off + 4 <= mem.len() {
            let ptr = u32::from_be_bytes([mem[off], mem[off + 1], mem[off + 2], mem[off + 3]]);
            if ptr >= MAIN_RAM_BASE && ptr < MAIN_RAM_BASE + mem.len() as u32 && (ptr & 1) == 0 {
                roots.push(ptr);
            }
        }
    }

    // Also scan for 32-bit absolute code pointers stored in common pointer tables inside the binary
    // (we start conservative with explicit roots + dynamic discovery so we don't lift random data).
    roots
}

pub fn build_cfg(mem: &[u8], extra_roots: &[u32]) -> Cfg {
    let mut insns = BTreeMap::new();
    let mut functions = BTreeSet::new();
    let mut block_leaders = BTreeSet::new();
    let mut queue = VecDeque::new();
    let max_addr = MAIN_RAM_BASE + mem.len() as u32;

    let mut roots = default_roots(mem);
    roots.extend_from_slice(extra_roots);

    for r in roots {
        if r >= MAIN_RAM_BASE && r < max_addr && (r & 1) == 0 {
            functions.insert(r);
            block_leaders.insert(r);
            queue.push_back(r);
        }
    }

    while let Some(pc) = queue.pop_front() {
        if pc < MAIN_RAM_BASE || pc >= max_addr || (pc & 1) != 0 || insns.contains_key(&pc) {
            continue;
        }
        // Don't trace inside HLE-hooked low-level floppy driver ($005B26..$00639F)
        if pc == 0x00664E || pc == 0x0066AE {
            let insn = m68k::decode(mem, MAIN_RAM_BASE, pc);
            insns.insert(pc, insn);
            continue;
        }

        let insn = m68k::decode(mem, MAIN_RAM_BASE, pc);
        let next = pc + insn.len;
        let mut falls_through = true;

        match insn.op {
            Op::Bra { target } => {
                block_leaders.insert(target);
                queue.push_back(target);
                falls_through = false;
            }
            Op::Bsr { target } => {
                functions.insert(target);
                block_leaders.insert(target);
                queue.push_back(target);
                if target == 0x0016E2 {
                    if let Some(resume) = skip_inline_vm(mem, next) {
                        block_leaders.insert(resume);
                        queue.push_back(resume);
                    }
                    falls_through = false;
                } else {
                    block_leaders.insert(next);
                }
            }
            Op::Bcc { target, .. } | Op::Dbcc { target, .. } => {
                block_leaders.insert(target);
                block_leaders.insert(next);
                queue.push_back(target);
            }
            Op::Jmp { target } => {
                falls_through = false;
                if let Some(t) = static_ea_target(target) {
                    block_leaders.insert(t);
                    queue.push_back(t);
                }
            }
            Op::Jsr { target } => {
                if let Some(t) = static_ea_target(target) {
                    functions.insert(t);
                    block_leaders.insert(t);
                    queue.push_back(t);
                    if t == 0x0016E2 {
                        if let Some(resume) = skip_inline_vm(mem, next) {
                            block_leaders.insert(resume);
                            queue.push_back(resume);
                        }
                        falls_through = false;
                    } else {
                        block_leaders.insert(next);
                    }
                } else {
                    block_leaders.insert(next);
                }
            }
            Op::MoveToSr { .. } | Op::OriSr { .. } | Op::AndiSr { .. } | Op::EoriSr { .. } => {
                block_leaders.insert(next);
            }
            Op::Rts | Op::Rte | Op::Rtr | Op::Illegal | Op::Unknown(_) => {
                falls_through = false;
            }
            Op::Move { src: Ea::Imm(v), dst: Ea::AbsW(d), .. } if (d as u16) >= 0x0008 && (d as u16) <= 0x00FC => {
                // Interrupt / exception vector install!
                if v >= MAIN_RAM_BASE && v < max_addr && (v & 1) == 0 {
                    functions.insert(v);
                    block_leaders.insert(v);
                    queue.push_back(v);
                }
            }
            _ => {}
        }

        insns.insert(pc, insn);
        if falls_through {
            queue.push_back(next);
        }
    }

    Cfg { insns, functions, block_leaders }
}

fn static_ea_target(ea: Ea) -> Option<u32> {
    match ea {
        Ea::AbsL(t) => Some(t),
        Ea::AbsW(t) => Some(t as i32 as u32),
        Ea::PcDisp(b, d) => Some(b.wrapping_add(d as i32 as u32)),
        _ => None,
    }
}

/// Skip the inline VM bytecode stream immediately following `bsr/jsr $0016E2`
/// and return the resume PC where `$0016E2` returns (`end_of_stream + 2`).
fn skip_inline_vm(mem: &[u8], start_pc: u32) -> Option<u32> {
    let mut pc = start_pc;
    for _ in 0..256 {
        let off = pc.checked_sub(MAIN_RAM_BASE)? as usize;
        if off + 2 > mem.len() { return None; }
        let op = mem[off];
        match op {
            0x00 => return Some(pc + 2),
            0x04 => pc += 6,
            0x08 | 0x0C | 0x10 | 0x14 | 0x18 => pc += 4,
            0x1C => pc += 2,
            _ => return None,
        }
    }
    None
}

fn size_fn(s: Size) -> &'static str {
    match s {
        Size::Byte => "b",
        Size::Word => "w",
        Size::Long => "l",
    }
}

fn cond_expr(c: Cond) -> &'static str {
    match c {
        Cond::T => "Cond::T", Cond::F => "Cond::F", Cond::Hi => "Cond::Hi", Cond::Ls => "Cond::Ls",
        Cond::Cc => "Cond::Cc", Cond::Cs => "Cond::Cs", Cond::Ne => "Cond::Ne", Cond::Eq => "Cond::Eq",
        Cond::Vc => "Cond::Vc", Cond::Vs => "Cond::Vs", Cond::Pl => "Cond::Pl", Cond::Mi => "Cond::Mi",
        Cond::Ge => "Cond::Ge", Cond::Lt => "Cond::Lt", Cond::Gt => "Cond::Gt", Cond::Le => "Cond::Le",
    }
}

fn shift_expr(k: ShiftKind) -> &'static str {
    match k {
        ShiftKind::Asl => "ShiftKind::Asl", ShiftKind::Asr => "ShiftKind::Asr",
        ShiftKind::Lsl => "ShiftKind::Lsl", ShiftKind::Lsr => "ShiftKind::Lsr",
        ShiftKind::Roxl => "ShiftKind::Roxl", ShiftKind::Roxr => "ShiftKind::Roxr",
        ShiftKind::Rol => "ShiftKind::Rol", ShiftKind::Ror => "ShiftKind::Ror",
    }
}

fn bitop_expr(b: BitOp) -> &'static str {
    match b {
        BitOp::Btst => "BitOp::Btst", BitOp::Bchg => "BitOp::Bchg",
        BitOp::Bclr => "BitOp::Bclr", BitOp::Bset => "BitOp::Bset",
    }
}

fn emit_ea_addr(out: &mut String, ea: Ea, size: Size, var: &str) {
    let step = match size {
        Size::Byte => 1u32,
        Size::Word => 2u32,
        Size::Long => 4u32,
    };
    match ea {
        Ea::AnInd(r) => writeln!(out, "let {} = cpu.a[{}];", var, r).unwrap(),
        Ea::AnPostInc(r) => {
            let inc = if r == 7 && step == 1 { 2 } else { step };
            writeln!(out, "let {} = cpu.a[{}]; cpu.a[{}] = cpu.a[{}].wrapping_add({});", var, r, r, r, inc).unwrap();
        }
        Ea::AnPreDec(r) => {
            let dec = if r == 7 && step == 1 { 2 } else { step };
            writeln!(out, "cpu.a[{}] = cpu.a[{}].wrapping_sub({}); let {} = cpu.a[{}];", r, r, dec, var, r).unwrap();
        }
        Ea::AnDisp(r, d) => {
            writeln!(out, "let {} = cpu.a[{}].wrapping_add({}i32 as u32);", var, r, d).unwrap();
        }
        Ea::AnIndex(r, idx, d) => {
            let reg_expr = if idx.is_addr {
                format!("cpu.a[{}]", idx.reg)
            } else {
                format!("cpu.d[{}]", idx.reg)
            };
            let idx_expr = if idx.is_long {
                reg_expr
            } else {
                format!("({} as i16 as i32 as u32)", reg_expr)
            };
            writeln!(out, "let {} = cpu.a[{}].wrapping_add({}).wrapping_add({}i32 as u32);", var, r, idx_expr, d).unwrap();
        }
        Ea::AbsW(v) => {
            writeln!(out, "let {} = 0x{:08X}u32;", var, v as i32 as u32).unwrap();
        }
        Ea::AbsL(v) => {
            writeln!(out, "let {} = 0x{:08X}u32;", var, v).unwrap();
        }
        Ea::PcDisp(base, d) => {
            let target = base.wrapping_add(d as i32 as u32);
            writeln!(out, "let {} = 0x{:08X}u32;", var, target).unwrap();
        }
        Ea::PcIndex(base, idx, d) => {
            let target = base.wrapping_add(d as i32 as u32);
            let reg_expr = if idx.is_addr {
                format!("cpu.a[{}]", idx.reg)
            } else {
                format!("cpu.d[{}]", idx.reg)
            };
            let idx_expr = if idx.is_long {
                reg_expr
            } else {
                format!("({} as i16 as i32 as u32)", reg_expr)
            };
            writeln!(out, "let {} = 0x{:08X}u32.wrapping_add({});", var, target, idx_expr).unwrap();
        }
        _ => unreachable!("emit_ea_addr called on non-memory EA: {:?}", ea),
    }
}

fn emit_read_ea(out: &mut String, ea: Ea, size: Size, var: &str) {
    let sf = size_fn(size);
    match ea {
        Ea::Dn(r) => match size {
            Size::Byte => writeln!(out, "let {} = (cpu.d[{}] & 0xFF) as u8;", var, r).unwrap(),
            Size::Word => writeln!(out, "let {} = (cpu.d[{}] & 0xFFFF) as u16;", var, r).unwrap(),
            Size::Long => writeln!(out, "let {} = cpu.d[{}];", var, r).unwrap(),
        },
        Ea::An(r) => match size {
            Size::Byte => writeln!(out, "let {} = (cpu.a[{}] & 0xFF) as u8;", var, r).unwrap(),
            Size::Word => writeln!(out, "let {} = (cpu.a[{}] & 0xFFFF) as u16;", var, r).unwrap(),
            Size::Long => writeln!(out, "let {} = cpu.a[{}];", var, r).unwrap(),
        },
        Ea::Imm(v) => match size {
            Size::Byte => writeln!(out, "let {} = 0x{:02X}u8;", var, v & 0xFF).unwrap(),
            Size::Word => writeln!(out, "let {} = 0x{:04X}u16;", var, v & 0xFFFF).unwrap(),
            Size::Long => writeln!(out, "let {} = 0x{:08X}u32;", var, v).unwrap(),
        },
        _ => {
            let avar = format!("{}_addr", var);
            emit_ea_addr(out, ea, size, &avar);
            writeln!(out, "let {} = bus.read_{}({});", var, sf, avar).unwrap();
        }
    }
}

fn emit_write_ea(out: &mut String, ea: Ea, size: Size, val_expr: &str) {
    let sf = size_fn(size);
    match ea {
        Ea::Dn(r) => match size {
            Size::Byte => writeln!(out, "cpu.set_d_b({}, {});", r, val_expr).unwrap(),
            Size::Word => writeln!(out, "cpu.set_d_w({}, {});", r, val_expr).unwrap(),
            Size::Long => writeln!(out, "cpu.d[{}] = {};", r, val_expr).unwrap(),
        },
        Ea::An(r) => match size {
            Size::Word => writeln!(out, "cpu.a[{}] = ({} as i16 as i32 as u32);", r, val_expr).unwrap(),
            Size::Long => writeln!(out, "cpu.a[{}] = {};", r, val_expr).unwrap(),
            Size::Byte => writeln!(out, "cpu.a[{}] = (cpu.a[{}] & 0xFFFFFF00) | ({} as u32);", r, r, val_expr).unwrap(),
        },
        _ => {
            emit_ea_addr(out, ea, size, "dst_addr");
            writeln!(out, "bus.write_{}(dst_addr, {});", sf, val_expr).unwrap();
        }
    }
}

fn emit_rmw_ea<F: FnOnce(&mut String, &str, &str)>(out: &mut String, ea: Ea, size: Size, body: F) {
    let sf = size_fn(size);
    match ea {
        Ea::Dn(r) => {
            match size {
                Size::Byte => writeln!(out, "let dst_v = (cpu.d[{}] & 0xFF) as u8;", r).unwrap(),
                Size::Word => writeln!(out, "let dst_v = (cpu.d[{}] & 0xFFFF) as u16;", r).unwrap(),
                Size::Long => writeln!(out, "let dst_v = cpu.d[{}];", r).unwrap(),
            }
            body(out, "dst_v", "res_v");
            match size {
                Size::Byte => writeln!(out, "cpu.set_d_b({}, res_v);", r).unwrap(),
                Size::Word => writeln!(out, "cpu.set_d_w({}, res_v);", r).unwrap(),
                Size::Long => writeln!(out, "cpu.d[{}] = res_v;", r).unwrap(),
            }
        }
        Ea::An(r) => {
            match size {
                Size::Word => writeln!(out, "let dst_v = (cpu.a[{}] & 0xFFFF) as u16;", r).unwrap(),
                Size::Long => writeln!(out, "let dst_v = cpu.a[{}];", r).unwrap(),
                Size::Byte => writeln!(out, "let dst_v = (cpu.a[{}] & 0xFF) as u8;", r).unwrap(),
            }
            body(out, "dst_v", "res_v");
            match size {
                Size::Word => writeln!(out, "cpu.a[{}] = res_v as i16 as i32 as u32;", r).unwrap(),
                Size::Long => writeln!(out, "cpu.a[{}] = res_v;", r).unwrap(),
                Size::Byte => writeln!(out, "cpu.a[{}] = (cpu.a[{}] & 0xFFFFFF00) | (res_v as u32);", r, r).unwrap(),
            }
        }
        _ => {
            emit_ea_addr(out, ea, size, "rmw_addr");
            writeln!(out, "let dst_v = bus.read_{}(rmw_addr);", sf).unwrap();
            body(out, "dst_v", "res_v");
            writeln!(out, "bus.write_{}(rmw_addr, res_v);", sf).unwrap();
        }
    }
}

pub fn emit_rust(cfg: &Cfg) -> String {
    let mut out = String::with_capacity(2 * 1024 * 1024);
    writeln!(out, "// AUTO-GENERATED BY Parasol Stars 68000 Static Recompiler").unwrap();
    writeln!(out, "// DO NOT EDIT BY HAND — regenerate with `cargo run -p recomp`").unwrap();
    writeln!(out, "use crate::cpu::{{BitOp, Cond, Cpu, ShiftKind}};").unwrap();
    writeln!(out, "use crate::bus::Bus;\n").unwrap();

    // Assign every instruction PC to its owning function (nearest function start <= pc).
    // Also split any huge function (>200 insns) into smaller sub-chunks for fast rustc compile!
    let mut func_starts: Vec<u32> = cfg.functions.iter().copied().collect();
    // Ensure earliest insn is a function start
    if let Some(&first_pc) = cfg.insns.keys().next() {
        if func_starts.is_empty() || func_starts[0] > first_pc {
            func_starts.insert(0, first_pc);
        }
    }
    let func_set: BTreeSet<u32> = func_starts.iter().copied().collect();

    let mut fn_insns: BTreeMap<u32, Vec<&Insn>> = BTreeMap::new();
    for (&pc, insn) in &cfg.insns {
        let owner = func_set.range(..=pc).next_back().copied().unwrap_or(func_starts[0]);
        fn_insns.entry(owner).or_default().push(insn);
    }

    // Build top-level dispatcher mapping any lifted PC -> its owning function!
    // Every basic block leader gets an entry in `dispatch_lifted`, so jumping to ANY
    // lifted label (even mid-function) enters the right Rust function immediately!
    writeln!(out, "pub fn dispatch_lifted(cpu: &mut Cpu, bus: &mut Bus) -> bool {{").unwrap();
    writeln!(out, "    match cpu.pc {{").unwrap();
    for (&owner, list) in &fn_insns {
        let mut leaders: Vec<u32> = list.iter()
            .map(|i| i.pc)
            .filter(|pc| *pc == owner || cfg.block_leaders.contains(pc))
            .collect();
        leaders.dedup();
        if leaders.is_empty() { continue; }
        let pat = leaders.iter().map(|pc| format!("0x{:06X}", pc)).collect::<Vec<_>>().join(" | ");
        writeln!(out, "        {} => {{ sub_{:06x}(cpu, bus); true }}", pat, owner).unwrap();
    }
    writeln!(out, "        _ => false,").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}\n").unwrap();

    // Emit each lifted function!
    for (&owner, list) in &fn_insns {
        let fn_pcs: BTreeSet<u32> = list.iter().map(|i| i.pc).collect();
        writeln!(out, "fn sub_{:06x}(cpu: &mut Cpu, bus: &mut Bus) {{", owner).unwrap();

        // HLE Hooks for low-level floppy sector read / write!
        if owner == 0x00664E {
            writeln!(out, "    bus.hle_disk_read(cpu);").unwrap();
            writeln!(out, "    cpu.pc = cpu.pop_l(bus);").unwrap();
            writeln!(out, "}}\n").unwrap();
            continue;
        }
        if owner == 0x0066AE {
            writeln!(out, "    bus.hle_disk_write(cpu);").unwrap();
            writeln!(out, "    cpu.pc = cpu.pop_l(bus);").unwrap();
            writeln!(out, "}}\n").unwrap();
            continue;
        }

        // Partition `list` into basic blocks starting at block_leaders (or owner).
        let mut blocks: Vec<Vec<&Insn>> = Vec::new();
        let mut cur: Vec<&Insn> = Vec::new();
        let mut prev_end = 0u32;
        for &insn in list {
            let start_new = cur.is_empty()
                || insn.pc == owner
                || cfg.block_leaders.contains(&insn.pc)
                || insn.pc != prev_end;
            if start_new && !cur.is_empty() {
                blocks.push(cur);
                cur = Vec::new();
            }
            prev_end = insn.pc + insn.len;
            cur.push(insn);
        }
        if !cur.is_empty() {
            blocks.push(cur);
        }

        writeln!(out, "    loop {{").unwrap();
        writeln!(out, "        if bus.is_code_dirty(cpu.pc) {{ return; }}").unwrap();
        writeln!(out, "        match cpu.pc {{").unwrap();
        for blk in &blocks {
            let bpc = blk[0].pc;
            let last = blk.last().unwrap();
            let bend = last.pc + last.len;
            writeln!(out, "            0x{:06X} => {{", bpc).unwrap();
            writeln!(out, "                if bus.is_range_dirty(0x{:06X}, 0x{:06X}) {{ return; }}", bpc, bend).unwrap();
            let mut terminated = false;
            let mut blk_cycles = 0u32;
            for insn in blk {
                blk_cycles += 8;
                terminated = emit_insn(&mut out, insn, &fn_pcs, &func_set, blk_cycles);
                if terminated { break; }
            }
            if !terminated {
                let last = blk.last().unwrap();
                let next_pc = last.pc + last.len;
                writeln!(out, "                cpu.tick(bus, {});", blk_cycles).unwrap();
                writeln!(out, "                cpu.pc = 0x{:06X};", next_pc).unwrap();
                if !fn_pcs.contains(&next_pc) {
                    writeln!(out, "                return;").unwrap();
                } else {
                    writeln!(out, "                if bus.should_yield(cpu) {{ return; }}").unwrap();
                }
            }
            writeln!(out, "            }}").unwrap();
        }
        writeln!(out, "            _ => return,").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "}}\n").unwrap();
    }

    out
}

fn emit_insn(
    out: &mut String,
    insn: &Insn,
    fn_pcs: &BTreeSet<u32>,
    func_set: &BTreeSet<u32>,
    blk_cycles: u32,
) -> bool {
    let next_pc = insn.pc + insn.len;
    let mut s = String::new();

    // Special check for the 1 self-modifying instruction at $002218 (patches $00221C to NOPs)
    if insn.pc == 0x002218 {
        writeln!(s, "if bus.read_l(0x00221C) != 0x4E714E71 {{").unwrap();
    }

    let mut terminated = false;
    match insn.op {
        Op::Move { size, src, dst } => {
            emit_read_ea(&mut s, src, size, "v");
            writeln!(s, "cpu.flags_nz_{}(v);", size_fn(size)).unwrap();
            emit_write_ea(&mut s, dst, size, "v");
        }
        Op::Movea { size, src, dst_an } => {
            emit_read_ea(&mut s, src, size, "v");
            match size {
                Size::Word => writeln!(s, "cpu.a[{}] = v as i16 as i32 as u32;", dst_an).unwrap(),
                Size::Long => writeln!(s, "cpu.a[{}] = v;", dst_an).unwrap(),
                Size::Byte => unreachable!(),
            }
        }
        Op::Moveq { imm, dst_dn } => {
            writeln!(s, "let v = {}i32 as u32; cpu.d[{}] = v; cpu.flags_nz_l(v);", imm, dst_dn).unwrap();
        }
        Op::MoveFromSr { dst } => {
            emit_write_ea(&mut s, dst, Size::Word, "cpu.sr");
        }
        Op::MoveToCcr { src } => {
            emit_read_ea(&mut s, src, Size::Word, "v");
            writeln!(s, "cpu.set_ccr(v as u8);").unwrap();
        }
        Op::MoveToSr { src } => {
            emit_read_ea(&mut s, src, Size::Word, "v");
            writeln!(s, "cpu.set_sr(v);").unwrap();
            writeln!(s, "cpu.tick(bus, {}); cpu.pc = 0x{:06X}; if bus.should_yield(cpu) {{ return; }}", blk_cycles, next_pc).unwrap();
            terminated = true;
        }
        Op::MoveUsp { an, to_usp } => {
            if to_usp { writeln!(s, "cpu.usp = cpu.a[{}];", an).unwrap(); }
            else { writeln!(s, "cpu.a[{}] = cpu.usp;", an).unwrap(); }
        }
        Op::Movem { size, to_mem, mask, ea } => {
            let sf = size_fn(size);
            if to_mem {
                match ea {
                    Ea::AnPreDec(r) => writeln!(s, "cpu.movem_to_predec_{}(bus, {}, 0x{:04X});", sf, r, mask).unwrap(),
                    _ => {
                        emit_ea_addr(&mut s, ea, size, "maddr");
                        writeln!(s, "cpu.movem_to_mem_{}(bus, maddr, 0x{:04X});", sf, mask).unwrap();
                    }
                }
            } else {
                match ea {
                    Ea::AnPostInc(r) => writeln!(s, "cpu.movem_from_postinc_{}(bus, {}, 0x{:04X});", sf, r, mask).unwrap(),
                    _ => {
                        emit_ea_addr(&mut s, ea, size, "maddr");
                        writeln!(s, "cpu.movem_from_mem_{}(bus, maddr, 0x{:04X});", sf, mask).unwrap();
                    }
                }
            }
        }
        Op::Movep { size, to_mem, dn, an, disp } => {
            let sf = size_fn(size);
            writeln!(s, "let maddr = cpu.a[{}].wrapping_add({}i32 as u32);", an, disp).unwrap();
            if to_mem { writeln!(s, "cpu.movep_to_mem_{}(bus, maddr, {});", sf, dn).unwrap(); }
            else { writeln!(s, "cpu.movep_from_mem_{}(bus, maddr, {});", sf, dn).unwrap(); }
        }
        Op::Lea { src, dst_an } => {
            emit_ea_addr(&mut s, src, Size::Long, "la");
            writeln!(s, "cpu.a[{}] = la;", dst_an).unwrap();
        }
        Op::Pea { src } => {
            emit_ea_addr(&mut s, src, Size::Long, "pa");
            writeln!(s, "cpu.push_l(bus, pa);").unwrap();
        }
        Op::Exg { rx, ry, mode } => match mode {
            0 => writeln!(s, "cpu.d.swap({}, {});", rx, ry).unwrap(),
            1 => writeln!(s, "cpu.a.swap({}, {});", rx, ry).unwrap(),
            _ => writeln!(s, "std::mem::swap(&mut cpu.d[{}], &mut cpu.a[{}]);", rx, ry).unwrap(),
        },
        Op::Swap { dn } => {
            writeln!(s, "let v = cpu.d[{}].rotate_left(16); cpu.d[{}] = v; cpu.flags_nz_l(v);", dn, dn).unwrap();
        }
        Op::Ext { size, dn } => match size {
            Size::Word => writeln!(s, "let v = (cpu.d[{}] as i8 as i16) as u16; cpu.set_d_w({}, v); cpu.flags_nz_w(v);", dn, dn).unwrap(),
            Size::Long => writeln!(s, "let v = (cpu.d[{}] as i16 as i32) as u32; cpu.d[{}] = v; cpu.flags_nz_l(v);", dn, dn).unwrap(),
            Size::Byte => unreachable!(),
        },
        Op::Add { size, src, dst } => {
            emit_read_ea(&mut s, src, size, "sv");
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = cpu.op_add_{}(sv, {});", rv, size_fn(size), dv).unwrap();
            });
        }
        Op::Adda { size, src, dst_an } => {
            emit_read_ea(&mut s, src, size, "sv");
            match size {
                Size::Word => writeln!(s, "cpu.a[{}] = cpu.a[{}].wrapping_add(sv as i16 as i32 as u32);", dst_an, dst_an).unwrap(),
                Size::Long => writeln!(s, "cpu.a[{}] = cpu.a[{}].wrapping_add(sv);", dst_an, dst_an).unwrap(),
                Size::Byte => unreachable!(),
            }
        }
        Op::Addq { size, imm, dst } => {
            if let Ea::An(r) = dst {
                // ADDQ to An NEVER affects flags and operates on full 32-bit An!
                writeln!(s, "cpu.a[{}] = cpu.a[{}].wrapping_add({});", r, r, imm).unwrap();
            } else {
                emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                    writeln!(s, "let {} = cpu.op_add_{}({}, {});", rv, size_fn(size), imm, dv).unwrap();
                });
            }
        }
        Op::Addx { size, rx, ry, mem } => {
            writeln!(s, "cpu.op_addx_{}(bus, {}, {}, {});", size_fn(size), rx, ry, mem).unwrap();
        }
        Op::Sub { size, src, dst } => {
            emit_read_ea(&mut s, src, size, "sv");
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = cpu.op_sub_{}(sv, {});", rv, size_fn(size), dv).unwrap();
            });
        }
        Op::Suba { size, src, dst_an } => {
            emit_read_ea(&mut s, src, size, "sv");
            match size {
                Size::Word => writeln!(s, "cpu.a[{}] = cpu.a[{}].wrapping_sub(sv as i16 as i32 as u32);", dst_an, dst_an).unwrap(),
                Size::Long => writeln!(s, "cpu.a[{}] = cpu.a[{}].wrapping_sub(sv);", dst_an, dst_an).unwrap(),
                Size::Byte => unreachable!(),
            }
        }
        Op::Subq { size, imm, dst } => {
            if let Ea::An(r) = dst {
                writeln!(s, "cpu.a[{}] = cpu.a[{}].wrapping_sub({});", r, r, imm).unwrap();
            } else {
                emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                    writeln!(s, "let {} = cpu.op_sub_{}({}, {});", rv, size_fn(size), imm, dv).unwrap();
                });
            }
        }
        Op::Subx { size, rx, ry, mem } => {
            writeln!(s, "cpu.op_subx_{}(bus, {}, {}, {});", size_fn(size), rx, ry, mem).unwrap();
        }
        Op::Cmp { size, src, dst_dn } => {
            emit_read_ea(&mut s, src, size, "sv");
            match size {
                Size::Byte => writeln!(s, "cpu.op_cmp_b(sv, cpu.d[{}] as u8);", dst_dn).unwrap(),
                Size::Word => writeln!(s, "cpu.op_cmp_w(sv, cpu.d[{}] as u16);", dst_dn).unwrap(),
                Size::Long => writeln!(s, "cpu.op_cmp_l(sv, cpu.d[{}]);", dst_dn).unwrap(),
            }
        }
        Op::Cmpa { size, src, dst_an } => {
            emit_read_ea(&mut s, src, size, "sv");
            match size {
                Size::Word => writeln!(s, "cpu.op_cmp_l(sv as i16 as i32 as u32, cpu.a[{}]);", dst_an).unwrap(),
                Size::Long => writeln!(s, "cpu.op_cmp_l(sv, cpu.a[{}]);", dst_an).unwrap(),
                Size::Byte => unreachable!(),
            }
        }
        Op::Cmpi { size, imm, dst } => {
            emit_read_ea(&mut s, dst, size, "dv");
            match size {
                Size::Byte => writeln!(s, "cpu.op_cmp_b(0x{:02X}u8, dv);", imm & 0xFF).unwrap(),
                Size::Word => writeln!(s, "cpu.op_cmp_w(0x{:04X}u16, dv);", imm & 0xFFFF).unwrap(),
                Size::Long => writeln!(s, "cpu.op_cmp_l(0x{:08X}u32, dv);", imm).unwrap(),
            }
        }
        Op::Cmpm { size, ax, ay } => {
            emit_read_ea(&mut s, Ea::AnPostInc(ay), size, "sv");
            emit_read_ea(&mut s, Ea::AnPostInc(ax), size, "dv");
            writeln!(s, "cpu.op_cmp_{}(sv, dv);", size_fn(size)).unwrap();
        }
        Op::And { size, src, dst } => {
            emit_read_ea(&mut s, src, size, "sv");
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = {} & sv; cpu.flags_nz_{}({});", rv, dv, size_fn(size), rv).unwrap();
            });
        }
        Op::AndiCcr { imm } => writeln!(s, "cpu.set_ccr((cpu.sr as u8) & 0x{:02X});", imm).unwrap(),
        Op::AndiSr { imm } => {
            writeln!(s, "cpu.set_sr(cpu.sr & 0x{:04X});", imm).unwrap();
            writeln!(s, "cpu.tick(bus, {}); cpu.pc = 0x{:06X}; if bus.should_yield(cpu) {{ return; }}", blk_cycles, next_pc).unwrap();
            terminated = true;
        }
        Op::Or { size, src, dst } => {
            emit_read_ea(&mut s, src, size, "sv");
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = {} | sv; cpu.flags_nz_{}({});", rv, dv, size_fn(size), rv).unwrap();
            });
        }
        Op::OriCcr { imm } => writeln!(s, "cpu.set_ccr((cpu.sr as u8) | 0x{:02X});", imm).unwrap(),
        Op::OriSr { imm } => {
            writeln!(s, "cpu.set_sr(cpu.sr | 0x{:04X});", imm).unwrap();
            writeln!(s, "cpu.tick(bus, {}); cpu.pc = 0x{:06X}; if bus.should_yield(cpu) {{ return; }}", blk_cycles, next_pc).unwrap();
            terminated = true;
        }
        Op::Eor { size, src, dst } => {
            emit_read_ea(&mut s, src, size, "sv");
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = {} ^ sv; cpu.flags_nz_{}({});", rv, dv, size_fn(size), rv).unwrap();
            });
        }
        Op::EoriCcr { imm } => writeln!(s, "cpu.set_ccr((cpu.sr as u8) ^ 0x{:02X});", imm).unwrap(),
        Op::EoriSr { imm } => {
            writeln!(s, "cpu.set_sr(cpu.sr ^ 0x{:04X});", imm).unwrap();
            writeln!(s, "cpu.tick(bus, {}); cpu.pc = 0x{:06X}; if bus.should_yield(cpu) {{ return; }}", blk_cycles, next_pc).unwrap();
            terminated = true;
        }
        Op::Not { size, dst } => {
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = !{}; cpu.flags_nz_{}({});", rv, dv, size_fn(size), rv).unwrap();
            });
        }
        Op::Neg { size, dst } => {
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = cpu.op_sub_{}({}, 0);", rv, size_fn(size), dv).unwrap();
            });
        }
        Op::Negx { size, dst } => {
            emit_rmw_ea(&mut s, dst, size, |s, dv, rv| {
                writeln!(s, "let {} = cpu.op_negx_{}({});", rv, size_fn(size), dv).unwrap();
            });
        }
        Op::Clr { size, dst } => {
            // Note: on 68000 CLR reads before writing (relevant for hardware regs, usually harmless, keep simple)
            writeln!(s, "cpu.flags_clr();").unwrap();
            emit_write_ea(&mut s, dst, size, "0");
        }
        Op::Tst { size, ea } => {
            emit_read_ea(&mut s, ea, size, "v");
            writeln!(s, "cpu.flags_nz_{}(v);", size_fn(size)).unwrap();
        }
        Op::Tas { ea } => {
            emit_rmw_ea(&mut s, ea, Size::Byte, |s, dv, rv| {
                writeln!(s, "cpu.flags_nz_b({}); let {} = {} | 0x80;", dv, rv, dv).unwrap();
            });
        }
        Op::Mulu { src, dst_dn } => {
            emit_read_ea(&mut s, src, Size::Word, "sv");
            writeln!(s, "cpu.op_mulu(sv, {});", dst_dn).unwrap();
        }
        Op::Muls { src, dst_dn } => {
            emit_read_ea(&mut s, src, Size::Word, "sv");
            writeln!(s, "cpu.op_muls(sv, {});", dst_dn).unwrap();
        }
        Op::Divu { src, dst_dn } => {
            emit_read_ea(&mut s, src, Size::Word, "sv");
            writeln!(s, "cpu.op_divu(bus, sv, {}, 0x{:06X});", dst_dn, next_pc).unwrap();
        }
        Op::Divs { src, dst_dn } => {
            emit_read_ea(&mut s, src, Size::Word, "sv");
            writeln!(s, "cpu.op_divs(bus, sv, {}, 0x{:06X});", dst_dn, next_pc).unwrap();
        }
        Op::Abcd { rx, ry, mem } => writeln!(s, "cpu.op_abcd(bus, {}, {}, {});", rx, ry, mem).unwrap(),
        Op::Sbcd { rx, ry, mem } => writeln!(s, "cpu.op_sbcd(bus, {}, {}, {});", rx, ry, mem).unwrap(),
        Op::Nbcd { ea } => {
            emit_rmw_ea(&mut s, ea, Size::Byte, |s, dv, rv| {
                writeln!(s, "let {} = cpu.op_nbcd({});", rv, dv).unwrap();
            });
        }
        Op::Bit { op, bit_src, dst } => {
            emit_read_ea(&mut s, bit_src, Size::Byte, "bnum");
            if let Ea::Dn(r) = dst {
                writeln!(s, "cpu.op_bit_dn({}, bnum, {});", bitop_expr(op), r).unwrap();
            } else if op == BitOp::Btst {
                emit_read_ea(&mut s, dst, Size::Byte, "mv");
                writeln!(s, "cpu.op_btst_mem(bnum, mv);").unwrap();
            } else {
                emit_rmw_ea(&mut s, dst, Size::Byte, |s, dv, rv| {
                    writeln!(s, "let {} = cpu.op_bit_mem({}, bnum, {});", rv, bitop_expr(op), dv).unwrap();
                });
            }
        }
        Op::ShiftReg { kind, size, count_is_reg, count, dst_dn } => {
            let cnt = if count_is_reg { format!("(cpu.d[{}] & 63)", count) } else { format!("{}u32", count) };
            writeln!(s, "cpu.op_shift_reg_{}({}, {}, {});", size_fn(size), shift_expr(kind), cnt, dst_dn).unwrap();
        }
        Op::ShiftMem { kind, dst } => {
            emit_rmw_ea(&mut s, dst, Size::Word, |s, dv, rv| {
                writeln!(s, "let {} = cpu.op_shift_mem({}, {});", rv, shift_expr(kind), dv).unwrap();
            });
        }
        Op::Bra { target } => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            writeln!(s, "cpu.pc = 0x{:06X};", target).unwrap();
            if target <= insn.pc || !fn_pcs.contains(&target) {
                writeln!(s, "if bus.should_yield(cpu) {{ return; }}").unwrap();
            }
            if !fn_pcs.contains(&target) {
                writeln!(s, "return;").unwrap();
            }
            terminated = true;
        }
        Op::Bsr { target } => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            writeln!(s, "cpu.push_l(bus, 0x{:06X});", next_pc).unwrap();
            writeln!(s, "cpu.pc = 0x{:06X};", target).unwrap();
            if func_set.contains(&target) {
                writeln!(s, "sub_{:06x}(cpu, bus);", target).unwrap();
                writeln!(s, "if cpu.pc != 0x{:06X} || bus.should_yield(cpu) {{ return; }}", next_pc).unwrap();
            } else {
                writeln!(s, "return;").unwrap();
            }
            terminated = true;
        }
        Op::Bcc { cond, target } => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            writeln!(s, "if cpu.cond({}) {{", cond_expr(cond)).unwrap();
            writeln!(s, "    cpu.pc = 0x{:06X};", target).unwrap();
            if !fn_pcs.contains(&target) {
                writeln!(s, "    return;").unwrap();
            } else if target <= insn.pc {
                writeln!(s, "    if bus.should_yield(cpu) {{ return; }}").unwrap();
            }
            writeln!(s, "}} else {{").unwrap();
            writeln!(s, "    cpu.pc = 0x{:06X};", next_pc).unwrap();
            if !fn_pcs.contains(&next_pc) {
                writeln!(s, "    return;").unwrap();
            }
            writeln!(s, "}}").unwrap();
            terminated = true;
        }
        Op::Dbcc { cond, dn, target } => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            writeln!(s, "if !cpu.cond({}) {{", cond_expr(cond)).unwrap();
            writeln!(s, "    let nv = (cpu.d[{}] as u16).wrapping_sub(1);", dn).unwrap();
            writeln!(s, "    cpu.set_d_w({}, nv);", dn).unwrap();
            writeln!(s, "    if nv != 0xFFFF {{").unwrap();
            writeln!(s, "        cpu.pc = 0x{:06X};", target).unwrap();
            if !fn_pcs.contains(&target) {
                writeln!(s, "        return;").unwrap();
            } else {
                writeln!(s, "        if bus.should_yield(cpu) {{ return; }}").unwrap();
            }
            writeln!(s, "    }} else {{ cpu.pc = 0x{:06X}; }}", next_pc).unwrap();
            writeln!(s, "}} else {{ cpu.pc = 0x{:06X}; }}", next_pc).unwrap();
            if !fn_pcs.contains(&next_pc) {
                writeln!(s, "if cpu.pc == 0x{:06X} {{ return; }}", next_pc).unwrap();
            }
            terminated = true;
        }
        Op::Scc { cond, dst } => {
            writeln!(s, "let sv = if cpu.cond({}) {{ 0xFFu8 }} else {{ 0x00u8 }};", cond_expr(cond)).unwrap();
            emit_write_ea(&mut s, dst, Size::Byte, "sv");
        }
        Op::Jmp { target } => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            emit_ea_addr(&mut s, target, Size::Long, "jaddr");
            writeln!(s, "cpu.pc = jaddr;").unwrap();
            if let Some(t) = static_ea_target(target) {
                if !fn_pcs.contains(&t) {
                    writeln!(s, "return;").unwrap();
                } else {
                    writeln!(s, "if bus.should_yield(cpu) {{ return; }}").unwrap();
                }
            } else {
                writeln!(s, "return;").unwrap();
            }
            terminated = true;
        }
        Op::Jsr { target } => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            emit_ea_addr(&mut s, target, Size::Long, "jaddr");
            writeln!(s, "cpu.push_l(bus, 0x{:06X});", next_pc).unwrap();
            writeln!(s, "cpu.pc = jaddr;").unwrap();
            if let Some(t) = static_ea_target(target) {
                if func_set.contains(&t) {
                    writeln!(s, "sub_{:06x}(cpu, bus);", t).unwrap();
                    writeln!(s, "if cpu.pc != 0x{:06X} || bus.should_yield(cpu) {{ return; }}", next_pc).unwrap();
                } else {
                    writeln!(s, "return;").unwrap();
                }
            } else {
                writeln!(s, "return;").unwrap();
            }
            terminated = true;
        }
        Op::Rts => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            writeln!(s, "cpu.pc = cpu.pop_l(bus);").unwrap();
            writeln!(s, "return;").unwrap();
            terminated = true;
        }
        Op::Rte => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            writeln!(s, "let nsr = cpu.pop_w(bus);").unwrap();
            writeln!(s, "cpu.pc = cpu.pop_l(bus);").unwrap();
            writeln!(s, "cpu.set_sr(nsr);").unwrap();
            writeln!(s, "return;").unwrap();
            terminated = true;
        }
        Op::Rtr => {
            writeln!(s, "cpu.tick(bus, {});", blk_cycles).unwrap();
            writeln!(s, "let nccr = cpu.pop_w(bus);").unwrap();
            writeln!(s, "cpu.pc = cpu.pop_l(bus);").unwrap();
            writeln!(s, "cpu.set_ccr(nccr as u8);").unwrap();
            writeln!(s, "return;").unwrap();
            terminated = true;
        }
        Op::Nop | Op::Reset => {}
        Op::Movec { .. } => {
            // 68020+ cache control instruction -> trigger illegal instruction trap $10 on 68000!
            writeln!(s, "cpu.exception(bus, 4, 0x{:06X}); return;", insn.pc).unwrap();
            terminated = true;
        }
        Op::Stop { imm } => {
            writeln!(s, "cpu.set_sr(0x{:04X}); cpu.tick(bus, 64); cpu.pc = 0x{:06X}; return;", imm, next_pc).unwrap();
            terminated = true;
        }
        Op::Trap { vector } => {
            writeln!(s, "cpu.exception(bus, {}, 0x{:06X}); return;", 32 + vector, next_pc).unwrap();
            terminated = true;
        }
        Op::Trapv => {
            writeln!(s, "if cpu.flag_v() {{ cpu.exception(bus, 7, 0x{:06X}); return; }}", next_pc).unwrap();
        }
        Op::Chk { src, dn } => {
            emit_read_ea(&mut s, src, Size::Word, "bound");
            writeln!(s, "if cpu.op_chk(bound, {}) {{ cpu.exception(bus, 6, 0x{:06X}); return; }}", dn, next_pc).unwrap();
        }
        Op::Link { an, disp } => {
            writeln!(s, "let old_a = cpu.a[{}]; cpu.push_l(bus, old_a); cpu.a[{}] = cpu.a[7]; cpu.a[7] = cpu.a[7].wrapping_add({}i32 as u32);", an, an, disp).unwrap();
        }
        Op::Unlk { an } => {
            writeln!(s, "cpu.a[7] = cpu.a[{}]; cpu.a[{}] = cpu.pop_l(bus);", an, an).unwrap();
        }
        Op::Illegal | Op::Unknown(_) => {
            writeln!(s, "cpu.exception(bus, 4, 0x{:06X}); return;", insn.pc).unwrap();
            terminated = true;
        }
    }

    if insn.pc == 0x002218 {
        writeln!(s, "}}").unwrap();
    }

    for line in s.lines() {
        writeln!(out, "                {}", line).unwrap();
    }
    terminated
}
