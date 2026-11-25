mod lifter;
mod m68k;

use std::env;
use std::fs;
use m68k::Op;

const MAIN_DISK_OFFSET: usize = 0x2C00;
const MAIN_LOAD_SIZE: usize = 0x186A0; // 100,000 bytes relocated to $00001000
const MAIN_RAM_BASE: u32 = 0x00001000;

fn disasm_range(mem: &[u8], start: u32, end: u32) {
    let mut pc = start;
    while pc < end {
        let insn = m68k::decode(mem, MAIN_RAM_BASE, pc);
        let off = (pc - MAIN_RAM_BASE) as usize;
        let bytes = &mem[off..(off + insn.len as usize).min(mem.len())];
        let hex: String = bytes.chunks(2).map(|c| {
            if c.len() == 2 { format!("{:02X}{:02X}", c[0], c[1]) } else { format!("{:02X}", c[0]) }
        }).collect::<Vec<_>>().join(" ");
        println!("  ${:06X}: {:<24} {}", pc, hex, insn);
        pc += insn.len;
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let adf_path = args.get(1).map(|s| s.as_str()).unwrap_or(
        r"C:\Users\mario\.gemini\antigravity-cli\brain\de5793e5-247b-4c5a-9bf6-5dadd966a5bb\scratch\parasol_stars.adf"
    );
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("stats");
    let adf = fs::read(adf_path).expect("failed to read ADF");
    let main_bin = &adf[MAIN_DISK_OFFSET..MAIN_DISK_OFFSET + MAIN_LOAD_SIZE];

    match mode {
        "dis" => {
            let start = u32::from_str_radix(args[3].trim_start_matches("0x"), 16).unwrap();
            let end = u32::from_str_radix(args[4].trim_start_matches("0x"), 16).unwrap();
            disasm_range(main_bin, start, end);
        }
        "emit" => {
            let out_path = args.get(3).expect("usage: recomp <adf> emit <out.rs> [extra_root_hex ...]");
            let extra: Vec<u32> = args[4..].iter()
                .filter_map(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .collect();
            let cfg = lifter::build_cfg(main_bin, &extra);
            let unknowns: Vec<_> = cfg.insns.values().filter(|i| matches!(i.op, Op::Unknown(_))).collect();
            let code = lifter::emit_rust(&cfg);
            fs::write(out_path, &code).expect("failed to write lifted rust");
            println!(
                "Lifted {} instructions across {} functions ({} basic blocks, {} bytes Rust, {} unknowns) -> {}",
                cfg.insns.len(), cfg.functions.len(), cfg.block_leaders.len(), code.len(), unknowns.len(), out_path
            );
            for u in unknowns.iter().take(20) {
                println!("  UNKNOWN at ${:06X}: {}", u.pc, u);
            }
        }
        _ => {
            let cfg = lifter::build_cfg(main_bin, &[]);
            let unknowns: Vec<_> = cfg.insns.values().filter(|i| matches!(i.op, Op::Unknown(_))).collect();
            println!(
                "CFG stats: {} instructions, {} functions, {} block leaders, {} unknowns",
                cfg.insns.len(), cfg.functions.len(), cfg.block_leaders.len(), unknowns.len()
            );
            for u in unknowns.iter().take(20) {
                println!("  UNKNOWN at ${:06X}: {}", u.pc, u);
            }
        }
    }
}
