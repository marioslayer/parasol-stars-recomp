mod bus;
mod cpu;
mod m68k_fallback;
mod ocs;
#[cfg(windows)]
mod win32_platform;

#[allow(unused_variables, unused_mut, unused_assignments, dead_code, unreachable_code, unused_parens)]
mod lifted {
    include!(concat!(env!("OUT_DIR"), "/lifted.rs"));
}

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bus::Bus;
use cpu::{Cpu, SR_T};
use ocs::{SCREEN_H, SCREEN_W};

struct Machine {
    cpu: Cpu,
    bus: Bus,
    lifted_dispatches: u64,
    fallback_steps: u64,
    fallback_pcs: BTreeMap<u32, u64>,
}

impl Machine {
    fn new(adf: Vec<u8>) -> Self {
        let cpu = Cpu::new();
        let bus = Bus::new(adf);
        Self {
            cpu,
            bus,
            lifted_dispatches: 0,
            fallback_steps: 0,
            fallback_pcs: BTreeMap::new(),
        }
    }

    fn run_frame(&mut self) {
        self.bus.ocs.frame_ready = false;
        self.bus.ocs.audio_buf.clear();

        let mut safety = 0u64;
        while !self.bus.ocs.frame_ready {
            // Deliver highest-priority pending Amiga interrupt if unmasked
            let ipl = self.bus.ocs.pending_ipl();
            if ipl > self.cpu.ipl() || ipl == 7 {
                self.cpu.take_interrupt(&mut self.bus, ipl);
            }

            let before_cycles = self.cpu.cycles;
            if (self.cpu.sr & SR_T) == 0
                && !self.bus.is_code_dirty(self.cpu.pc)
                && lifted::dispatch_lifted(&mut self.cpu, &mut self.bus)
                && self.cpu.cycles != before_cycles
            {
                self.lifted_dispatches += 1;
            } else {
                let pc = self.cpu.pc;
                *self.fallback_pcs.entry(pc).or_insert(0) += 1;
                self.fallback_steps += 1;
                m68k_fallback::step_one(&mut self.cpu, &mut self.bus);
            }

            safety += 1;
            if safety > 2_000_000 {
                eprintln!(
                    "[WARN] Frame safety limit hit! pc=${:06X} sr=${:04X} vpos={} intena=${:04X} intreq=${:04X} dmacon=${:04X}",
                    self.cpu.pc, self.cpu.sr, self.bus.ocs.vpos,
                    self.bus.ocs.intena, self.bus.ocs.intreq, self.bus.ocs.dmacon
                );
                break;
            }
        }
    }
}

fn find_default_adf() -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from("game/Parasol Stars (1992)(Ocean)[cr].zip"),
        PathBuf::from("../game/Parasol Stars (1992)(Ocean)[cr].zip"),
        PathBuf::from("assets/parasol_stars.adf"),
        PathBuf::from("parasol_stars.adf"),
    ];
    for c in candidates {
        if c.exists() {
            return Ok(c);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in ["game/Parasol Stars (1992)(Ocean)[cr].zip", "../../game/Parasol Stars (1992)(Ocean)[cr].zip", "../../../game/Parasol Stars (1992)(Ocean)[cr].zip"] {
                let p = dir.join(rel);
                if p.exists() { return Ok(p); }
            }
        }
    }
    Err("Could not find Parasol Stars ADF/ZIP. Pass path as first argument.".into())
}

fn write_bmp(path: &Path, fb: &[u32]) -> std::io::Result<()> {
    let row_bytes = SCREEN_W * 3;
    let img_size = (row_bytes * SCREEN_H) as u32;
    let file_size = 54 + img_size;
    let mut out = Vec::with_capacity(file_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    // DIB header (BITMAPINFOHEADER)
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(SCREEN_W as i32).to_le_bytes());
    out.extend_from_slice(&(-(SCREEN_H as i32)).to_le_bytes()); // top-down
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&img_size.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for &px in fb {
        let r = ((px >> 16) & 0xFF) as u8;
        let g = ((px >> 8) & 0xFF) as u8;
        let b = (px & 0xFF) as u8;
        out.push(b);
        out.push(g);
        out.push(r);
    }
    fs::write(path, out)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut adf_path: Option<PathBuf> = None;
    let mut headless_frames: Option<u64> = None;
    let mut dump_dir: Option<PathBuf> = None;
    let mut trace_disk = false;
    let mut fire_frame: Option<u64> = None;
    let mut start_fullscreen = false;
    let mut start_crt_on = true; // Subtle CRT scanlines enabled by default (toggle with F9)

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--headless" => {
                i += 1;
                headless_frames = Some(args.get(i).and_then(|s| s.parse().ok()).unwrap_or(300));
            }
            "--dump" => {
                i += 1;
                dump_dir = args.get(i).map(PathBuf::from);
            }
            "--trace-disk" => trace_disk = true,
            "--fullscreen" | "-f" => start_fullscreen = true,
            "--no-crt" => start_crt_on = false,
            "--fire-at" => {
                i += 1;
                fire_frame = args.get(i).and_then(|s| s.parse().ok());
            }
            other if !other.starts_with("--") => {
                adf_path = Some(PathBuf::from(other));
            }
            _ => {}
        }
        i += 1;
    }

    let path = adf_path.unwrap_or_else(|| find_default_adf().expect("ADF not found"));
    let adf = recomp::zip_adf::load_adf_from_path(&path).expect("Failed to load ADF");
    let mut m = Machine::new(adf);
    m.bus.trace_disk = trace_disk;

    if let Some(frames) = headless_frames {
        if let Some(ref d) = dump_dir {
            let _ = fs::create_dir_all(d);
        }
        let t0 = Instant::now();
        let mut non_zero_audio = 0u64;
        for f in 1..=frames {
            if let Some(ff) = fire_frame {
                m.bus.ocs.joy1.fire = f >= ff && f < ff + 15;
            }
            m.run_frame();
            if m.bus.ocs.audio_buf.iter().any(|&s| s != 0) {
                non_zero_audio += 1;
            }
            if let Some(ref d) = dump_dir {
                if f == 60 || f == 400 || f == 620 || f == 750 || f == 900 || f == frames {
                    let p = d.join(format!("frame_{:04}.bmp", f));
                    let _ = write_bmp(&p, &m.bus.ocs.fb);
                }
            }
        }
        let elapsed = t0.elapsed();
        let non_black = m.bus.ocs.fb.iter().filter(|&&c| c != 0).count();
        println!(
            "Headless {} frames in {:.2?} ({:.1} fps): pc=${:06X} blits={} disk_reads={} non_black_px={} audio_frames={}",
            frames,
            elapsed,
            frames as f64 / elapsed.as_secs_f64(),
            m.cpu.pc,
            m.bus.ocs.blit_count,
            m.bus.disk_reads,
            non_black,
            non_zero_audio
        );
        println!(
            "OCS: dmacon=${:04X} intena=${:04X} intreq=${:04X} bplcon0=${:04X} bplcon1=${:04X} bplcon2=${:04X} diw=${:04X}/${:04X} ddf=${:04X}/${:04X} mod={}/{}",
            m.bus.ocs.dmacon, m.bus.ocs.intena, m.bus.ocs.intreq,
            m.bus.ocs.bplcon0, m.bus.ocs.bplcon1, m.bus.ocs.bplcon2,
            m.bus.ocs.diwstrt, m.bus.ocs.diwstop, m.bus.ocs.ddfstrt, m.bus.ocs.ddfstop,
            m.bus.ocs.bpl1mod, m.bus.ocs.bpl2mod
        );
        for s in 0..8 {
            let sp = m.bus.ocs.sprites[s];
            println!("  SPR{}: ptr=${:06X} vs={} ve={} hs={} att={} act={}",
                s, sp.ptr, sp.vstart, sp.vstop, sp.hstart, sp.attached, sp.active_line);
        }
        let pc_base = m.cpu.pc.saturating_sub(16) & !1;
        print!("RAM around PC ${:06X}:", m.cpu.pc);
        for o in (0..32u32).step_by(2) {
            print!(" {:04X}", m.bus.read_w(pc_base + o));
        }
        println!();
        println!(
            "Recomp stats: lifted_dispatches={}, fallback_steps={}, unique_fallback_pcs={}",
            m.lifted_dispatches, m.fallback_steps, m.fallback_pcs.len()
        );
        let mut top_fb: Vec<(u32, u64)> = m.fallback_pcs.into_iter().collect();
        top_fb.sort_by(|a, b| b.1.cmp(&a.1));
        for (pc, cnt) in top_fb.iter().take(20) {
            println!("  fallback PC ${:06X}: {} steps", pc, cnt);
        }
        return;
    }

    #[cfg(windows)]
    {
        let init_crt = if start_crt_on {
            win32_platform::CrtMode::Subtle
        } else {
            win32_platform::CrtMode::Off
        };
        let mut win = win32_platform::Win32Window::new(
            "Parasol Stars (1992) — Rust Static Recomp",
            3,
            start_fullscreen,
            init_crt,
        ).expect("Failed to create window");
        println!("Parasol Stars running! (XInput gamepad: {})", if win.has_xinput() { "enabled" } else { "unavailable" });
        println!("Display:  F11 / Alt+Enter = Fullscreen, F9 = CRT Scanlines (Off/Subtle/Classic)");
        println!("Keyboard: Arrows / WASD = Move/Jump, Z / Space / Ctrl = Parasol/Fire, P = Pause");
        println!("Xbox Pad: D-Pad/Stick = Move, A = Jump, X/B/RB/RT = Parasol/Fire, Start = Pause");

        let frame_dur = Duration::from_micros(20_000); // 50 Hz PAL
        let mut next_frame = Instant::now();
        loop {
            let alive = win.poll_events(&mut |sc, pressed| {
                m.bus.ocs.push_amiga_key(sc, pressed);
            });
            if !alive { break; }

            let (j0, j1) = win.poll_joysticks(&mut |sc, pressed| {
                m.bus.ocs.push_amiga_key(sc, pressed);
            });
            m.bus.ocs.joy0 = j0;
            m.bus.ocs.joy1 = j1;

            m.run_frame();
            win.present(&m.bus.ocs.fb);
            win.submit_audio(&m.bus.ocs.audio_buf);

            next_frame += frame_dur;
            let now = Instant::now();
            if next_frame > now {
                std::thread::sleep(next_frame - now);
            } else {
                next_frame = now;
            }
        }
    }
}
