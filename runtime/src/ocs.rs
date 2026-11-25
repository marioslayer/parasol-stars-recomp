pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 256;
pub const PAL_LINES: u16 = 312;
pub const CYCLES_PER_LINE: u32 = 455;
pub const AUDIO_RATE: u32 = 44_100;
const PAL_AUDIO_CLK: f64 = 3_546_895.0;

#[derive(Clone, Copy, Default)]
pub struct AudioChannel {
    pub lc: u32,
    pub len: u16,
    pub per: u16,
    pub vol: u16,
    pub cur_ptr: u32,
    pub cur_len_bytes: u32,
    pub phase: f64,
    pub sample: i8,
    pub dma_active: bool,
}

#[derive(Clone, Copy, Default)]
pub struct SpriteState {
    pub ptr: u32,
    pub vstart: u16,
    pub vstop: u16,
    pub hstart: u16,
    pub attached: bool,
    pub armed: bool,
    pub dat_a: u16,
    pub dat_b: u16,
    pub active_line: bool,
    pub dma_stopped: bool,
}

#[derive(Clone, Copy, Default)]
pub struct CiaTimer {
    pub latch: u16,
    pub counter: u16,
    pub running: bool,
    pub oneshot: bool,
}

pub struct Cia {
    pub pra: u8,
    pub prb: u8,
    pub ddra: u8,
    pub ddrb: u8,
    pub sdr: u8,
    pub icr_data: u8,
    pub icr_mask: u8,
    pub cra: u8,
    pub crb: u8,
    pub ta: CiaTimer,
    pub tb: CiaTimer,
    pub tod: u32,
    pub key_queue: std::collections::VecDeque<u8>,
}

impl Cia {
    pub fn new(pra_init: u8) -> Self {
        Self {
            pra: pra_init,
            prb: 0xFF,
            ddra: 0,
            ddrb: 0,
            sdr: 0,
            icr_data: 0,
            icr_mask: 0,
            cra: 0,
            crb: 0,
            ta: CiaTimer::default(),
            tb: CiaTimer::default(),
            tod: 0,
            key_queue: std::collections::VecDeque::new(),
        }
    }

    pub fn tick(&mut self, e_clocks: u32) -> bool {
        let mut fired = false;
        if self.ta.running && e_clocks > 0 {
            if e_clocks >= self.ta.counter as u32 {
                self.icr_data |= 0x01;
                if self.ta.oneshot {
                    self.ta.running = false;
                    self.cra &= !1;
                    self.ta.counter = self.ta.latch;
                } else {
                    let rem = (e_clocks - self.ta.counter as u32) % (self.ta.latch.max(1) as u32);
                    self.ta.counter = self.ta.latch.saturating_sub(rem as u16);
                }
                if (self.icr_mask & 0x01) != 0 {
                    self.icr_data |= 0x80;
                    fired = true;
                }
            } else {
                self.ta.counter -= e_clocks as u16;
            }
        }
        if self.tb.running && e_clocks > 0 {
            if e_clocks >= self.tb.counter as u32 {
                self.icr_data |= 0x02;
                if self.tb.oneshot {
                    self.tb.running = false;
                    self.crb &= !1;
                    self.tb.counter = self.tb.latch;
                } else {
                    let rem = (e_clocks - self.tb.counter as u32) % (self.tb.latch.max(1) as u32);
                    self.tb.counter = self.tb.latch.saturating_sub(rem as u16);
                }
                if (self.icr_mask & 0x02) != 0 {
                    self.icr_data |= 0x80;
                    fired = true;
                }
            } else {
                self.tb.counter -= e_clocks as u16;
            }
        }
        fired
    }

    pub fn read(&mut self, reg: u8) -> u8 {
        match reg & 0xF {
            0x0 => self.pra,
            0x1 => self.prb,
            0x2 => self.ddra,
            0x3 => self.ddrb,
            0x4 => (self.ta.counter & 0xFF) as u8,
            0x5 => (self.ta.counter >> 8) as u8,
            0x6 => (self.tb.counter & 0xFF) as u8,
            0x7 => (self.tb.counter >> 8) as u8,
            0x8 => (self.tod & 0xFF) as u8,
            0x9 => ((self.tod >> 8) & 0xFF) as u8,
            0xA => ((self.tod >> 16) & 0xFF) as u8,
            0xC => self.sdr,
            0xD => {
                let v = self.icr_data;
                self.icr_data = 0;
                v
            }
            0xE => self.cra,
            0xF => self.crb,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, reg: u8, val: u8) {
        match reg & 0xF {
            0x0 => self.pra = (self.pra & !self.ddra) | (val & self.ddra),
            0x1 => self.prb = val,
            0x2 => self.ddra = val,
            0x3 => self.ddrb = val,
            0x4 => self.ta.latch = (self.ta.latch & 0xFF00) | (val as u16),
            0x5 => {
                self.ta.latch = (self.ta.latch & 0x00FF) | ((val as u16) << 8);
                if !self.ta.running { self.ta.counter = self.ta.latch; }
                if self.ta.oneshot {
                    self.ta.counter = self.ta.latch;
                    self.ta.running = true;
                    self.cra |= 1;
                }
            }
            0x6 => self.tb.latch = (self.tb.latch & 0xFF00) | (val as u16),
            0x7 => {
                self.tb.latch = (self.tb.latch & 0x00FF) | ((val as u16) << 8);
                if !self.tb.running { self.tb.counter = self.tb.latch; }
                if self.tb.oneshot {
                    self.tb.counter = self.tb.latch;
                    self.tb.running = true;
                    self.crb |= 1;
                }
            }
            0xC => self.sdr = val,
            0xD => {
                if (val & 0x80) != 0 {
                    self.icr_mask |= val & 0x7F;
                } else {
                    self.icr_mask &= !(val & 0x7F);
                }
            }
            0xE => {
                self.cra = val & !0x10;
                self.ta.running = (val & 0x01) != 0;
                self.ta.oneshot = (val & 0x08) != 0;
                if (val & 0x10) != 0 { self.ta.counter = self.ta.latch; }
            }
            0xF => {
                self.crb = val & !0x10;
                self.tb.running = (val & 0x01) != 0;
                self.tb.oneshot = (val & 0x08) != 0;
                if (val & 0x10) != 0 { self.tb.counter = self.tb.latch; }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct JoyState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub fire: bool,
    pub fire2: bool,
}

impl JoyState {
    pub fn to_joydat(&self) -> u16 {
        let mut x1 = self.right as u16;
        let x0 = (self.down ^ self.right) as u16;
        let mut y1 = self.left as u16;
        let y0 = (self.up ^ self.left) as u16;
        // Standard Amiga JOYxDAT encoding:
        // bit 9 = left, bit 8 = up^left, bit 1 = right, bit 0 = down^right
        // Wait: if both left & right aren't pressed simultaneously:
        x1 &= 1;
        y1 &= 1;
        (y1 << 9) | (y0 << 8) | (x1 << 1) | x0
    }
}

#[derive(Clone, Copy)]
struct ManualSpriteStamp {
    hstart: u16,
    dat_a: u16,
    dat_b: u16,
    col1: u16,
    col3: u16,
}

pub struct Ocs {
    pub dmacon: u16,
    pub intena: u16,
    pub intreq: u16,
    pub adkcon: u16,

    // Copper
    pub copcon: u16,
    pub cop1lc: u32,
    pub cop2lc: u32,
    pub cop_pc: u32,
    pub cop_waiting: bool,
    pub cop_wait_vp: u16,
    pub cop_wait_hp: u16,
    pub cop_wait_ve: u16,
    pub cop_wait_he: u16,
    pub cop_stopped: bool,

    // Display
    pub diwstrt: u16,
    pub diwstop: u16,
    pub ddfstrt: u16,
    pub ddfstop: u16,
    pub bplcon0: u16,
    pub bplcon1: u16,
    pub bplcon2: u16,
    pub bpl1mod: i16,
    pub bpl2mod: i16,
    pub bplpt: [u32; 6],
    pub color: [u16; 32],
    pub sprites: [SpriteState; 8],
    manual_stamps: Vec<ManualSpriteStamp>,

    // Blitter
    pub bltcon0: u16,
    pub bltcon1: u16,
    pub bltafwm: u16,
    pub bltalwm: u16,
    pub bltapt: u32,
    pub bltbpt: u32,
    pub bltcpt: u32,
    pub bltdpt: u32,
    pub bltamod: i16,
    pub bltbmod: i16,
    pub bltcmod: i16,
    pub bltdmod: i16,
    pub bltadat: u16,
    pub bltbdat: u16,
    pub bltcdat: u16,
    pub blt_zero: bool,
    pub blit_count: u64,

    // Audio (Paula)
    pub aud: [AudioChannel; 4],
    pub audio_accum: f64,
    pub audio_buf: Vec<i16>,

    // CIAs & Input
    pub cia_a: Cia,
    pub cia_b: Cia,
    pub joy0: JoyState,
    pub joy1: JoyState,

    // Beam position
    pub vpos: u16,
    pub line_cycles: u32,
    pub frame_count: u64,
    pub frame_ready: bool,

    // Output framebuffer (320 x 256, 0x00RRGGBB)
    pub fb: Vec<u32>,
}

impl Ocs {
    pub fn new() -> Self {
        Self {
            dmacon: 0,
            intena: 0,
            intreq: 0,
            adkcon: 0,
            copcon: 0,
            cop1lc: 0,
            cop2lc: 0,
            cop_pc: 0,
            cop_waiting: false,
            cop_wait_vp: 0,
            cop_wait_hp: 0,
            cop_wait_ve: 0,
            cop_wait_he: 0,
            cop_stopped: true,
            diwstrt: 0x2C81,
            diwstop: 0x2CC1,
            ddfstrt: 0x0038,
            ddfstop: 0x00D0,
            bplcon0: 0,
            bplcon1: 0,
            bplcon2: 0x0024,
            bpl1mod: 0,
            bpl2mod: 0,
            bplpt: [0; 6],
            color: [0; 32],
            sprites: [SpriteState::default(); 8],
            manual_stamps: Vec::with_capacity(32),
            bltcon0: 0,
            bltcon1: 0,
            bltafwm: 0xFFFF,
            bltalwm: 0xFFFF,
            bltapt: 0,
            bltbpt: 0,
            bltcpt: 0,
            bltdpt: 0,
            bltamod: 0,
            bltbmod: 0,
            bltcmod: 0,
            bltdmod: 0,
            bltadat: 0,
            bltbdat: 0,
            bltcdat: 0,
            blt_zero: false,
            blit_count: 0,
            aud: [AudioChannel::default(); 4],
            audio_accum: 0.0,
            audio_buf: Vec::with_capacity(4096),
            cia_a: Cia::new(0xFC), // fire buttons high (not pressed), overlay off
            cia_b: Cia::new(0xFF),
            joy0: JoyState::default(),
            joy1: JoyState::default(),
            vpos: 0,
            line_cycles: 0,
            frame_count: 0,
            frame_ready: false,
            fb: vec![0; SCREEN_W * SCREEN_H],
        }
    }

    pub fn push_amiga_key(&mut self, scancode: u8, pressed: bool) {
        // Amiga keyboard SDR byte = ~((scancode << 1) | (released ? 1 : 0))
        let raw = (scancode << 1) | if pressed { 0 } else { 1 };
        let encoded = !raw;
        self.cia_a.key_queue.push_back(encoded);
    }

    pub fn pending_ipl(&self) -> u8 {
        if (self.intena & 0x4000) == 0 { return 0; }
        let active = self.intena & self.intreq;
        if active == 0 { return 0; }
        if (active & 0x2000) != 0 { return 6; } // EXTER (CIA-B)
        if (active & 0x1800) != 0 { return 5; } // DSKSYN / RBF
        if (active & 0x0780) != 0 { return 4; } // AUD0..3
        if (active & 0x0070) != 0 { return 3; } // BLIT / VERTB / COPER
        if (active & 0x0008) != 0 { return 2; } // PORTS (CIA-A)
        if (active & 0x0007) != 0 { return 1; } // TBE / DSKBLK / SOFT
        0
    }

    pub fn read_custom_w(&self, reg: u16) -> u16 {
        match reg & 0x1FE {
            0x002 => {
                // DMACONR: bit 14 = BBUSY (0 = finished), bit 13 = BZERO
                let mut v = self.dmacon & 0x07FF;
                if self.blt_zero { v |= 0x2000; }
                v
            }
            0x004 => {
                // VPOSR: bit 0 = V8, high byte = Agnus ID (0x20 = PAL OCS/ECS)
                0x2000 | ((self.vpos >> 8) & 1)
            }
            0x006 => {
                // VHPOSR: V7..0 in high byte, H8..1 in low byte
                let h = ((self.line_cycles * 227) / CYCLES_PER_LINE).min(226) as u16;
                ((self.vpos & 0xFF) << 8) | (h & 0xFF)
            }
            0x00A => self.joy0.to_joydat(),
            0x00C => self.joy1.to_joydat(),
            0x010 => self.adkcon,
            0x016 => {
                // POTGOR: fire2 buttons active low (bit 14 = port 2 RMB, bit 10 = port 1 RMB)
                let mut v = 0xFF00u16;
                if self.joy1.fire2 { v &= !0x4000; }
                if self.joy0.fire2 { v &= !0x0400; }
                v
            }
            0x01C => self.intena,
            0x01E => self.intreq,
            _ => 0,
        }
    }

    pub fn write_custom_w(&mut self, chip: &mut [u8], reg: u16, val: u16) {
        let r = reg & 0x1FE;
        match r {
            0x02E => self.copcon = val,
            0x040 => self.bltcon0 = val,
            0x042 => self.bltcon1 = val,
            0x044 => self.bltafwm = val,
            0x046 => self.bltalwm = val,
            0x048 => self.bltcpt = (self.bltcpt & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16),
            0x04A => self.bltcpt = (self.bltcpt & 0xFFFF_0000) | ((val as u32) & 0xFFFE),
            0x04C => self.bltbpt = (self.bltbpt & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16),
            0x04E => self.bltbpt = (self.bltbpt & 0xFFFF_0000) | ((val as u32) & 0xFFFE),
            0x050 => self.bltapt = (self.bltapt & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16),
            0x052 => self.bltapt = (self.bltapt & 0xFFFF_0000) | ((val as u32) & 0xFFFE),
            0x054 => self.bltdpt = (self.bltdpt & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16),
            0x056 => self.bltdpt = (self.bltdpt & 0xFFFF_0000) | ((val as u32) & 0xFFFE),
            0x058 => {
                let mut h = ((val >> 6) & 0x3FF) as u32;
                let mut w = (val & 0x3F) as u32;
                if h == 0 { h = 1024; }
                if w == 0 { w = 64; }
                self.run_blitter(chip, w, h);
            }
            0x060 => self.bltcmod = (val & 0xFFFE) as i16,
            0x062 => self.bltbmod = (val & 0xFFFE) as i16,
            0x064 => self.bltamod = (val & 0xFFFE) as i16,
            0x066 => self.bltdmod = (val & 0xFFFE) as i16,
            0x070 => self.bltcdat = val,
            0x072 => self.bltbdat = val,
            0x074 => self.bltadat = val,
            0x080 => self.cop1lc = (self.cop1lc & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16),
            0x082 => self.cop1lc = (self.cop1lc & 0xFFFF_0000) | ((val as u32) & 0xFFFE),
            0x084 => self.cop2lc = (self.cop2lc & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16),
            0x086 => self.cop2lc = (self.cop2lc & 0xFFFF_0000) | ((val as u32) & 0xFFFE),
            0x088 => {
                self.cop_pc = self.cop1lc;
                self.cop_waiting = false;
                self.cop_stopped = false;
            }
            0x08A => {
                self.cop_pc = self.cop2lc;
                self.cop_waiting = false;
                self.cop_stopped = false;
            }
            0x08E => self.diwstrt = val,
            0x090 => self.diwstop = val,
            0x092 => self.ddfstrt = val & 0x00FC,
            0x094 => self.ddfstop = val & 0x00FC,
            0x096 => {
                let old = self.dmacon;
                if (val & 0x8000) != 0 {
                    self.dmacon |= val & 0x07FF;
                } else {
                    self.dmacon &= !(val & 0x07FF);
                }
                let master_on = (self.dmacon & 0x0200) != 0;
                for ch in 0..4 {
                    let bit = 1u16 << ch;
                    let was_on = (old & 0x0200) != 0 && (old & bit) != 0;
                    let now_on = master_on && (self.dmacon & bit) != 0;
                    if now_on && !was_on {
                        self.aud[ch].dma_active = true;
                        self.aud[ch].cur_ptr = self.aud[ch].lc;
                        self.aud[ch].cur_len_bytes = (self.aud[ch].len as u32).max(1) * 2;
                        self.aud[ch].phase = 0.0;
                    } else if !now_on {
                        self.aud[ch].dma_active = false;
                    }
                }
            }
            0x09A => {
                if (val & 0x8000) != 0 {
                    self.intena |= val & 0x7FFF;
                } else {
                    self.intena &= !(val & 0x7FFF);
                }
            }
            0x09C => {
                if (val & 0x8000) != 0 {
                    self.intreq |= val & 0x7FFF;
                } else {
                    self.intreq &= !(val & 0x7FFF);
                }
            }
            0x09E => {
                if (val & 0x8000) != 0 {
                    self.adkcon |= val & 0x7FFF;
                } else {
                    self.adkcon &= !(val & 0x7FFF);
                }
            }
            // Paula audio channels 0..3 ($0A0..$0DF)
            0x0A0..=0x0DE => {
                let ch = ((r - 0x0A0) >> 4) as usize;
                match r & 0x0E {
                    0x0 => self.aud[ch].lc = (self.aud[ch].lc & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16),
                    0x2 => self.aud[ch].lc = (self.aud[ch].lc & 0xFFFF_0000) | ((val as u32) & 0xFFFE),
                    0x4 => self.aud[ch].len = val,
                    0x6 => self.aud[ch].per = val,
                    0x8 => self.aud[ch].vol = (val & 0x7F).min(64),
                    _ => {}
                }
            }
            // Bitplane pointers ($0E0..$0F6)
            0x0E0..=0x0F6 => {
                let p = ((r - 0x0E0) >> 2) as usize;
                if (r & 2) == 0 {
                    self.bplpt[p] = (self.bplpt[p] & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16);
                } else {
                    self.bplpt[p] = (self.bplpt[p] & 0xFFFF_0000) | ((val as u32) & 0xFFFE);
                }
            }
            0x100 => self.bplcon0 = val,
            0x102 => self.bplcon1 = val,
            0x104 => self.bplcon2 = val,
            0x108 => self.bpl1mod = (val & 0xFFFE) as i16,
            0x10A => self.bpl2mod = (val & 0xFFFE) as i16,
            // Sprite pointers ($120..$13E)
            0x120..=0x13E => {
                let s = ((r - 0x120) >> 2) as usize;
                if (r & 2) == 0 {
                    self.sprites[s].ptr = (self.sprites[s].ptr & 0x0000_FFFF) | (((val as u32) & 0x1F) << 16);
                } else {
                    self.sprites[s].ptr = (self.sprites[s].ptr & 0xFFFF_0000) | ((val as u32) & 0xFFFE);
                }
            }
            // Sprite direct POS/CTL/DATA ($140..$17E)
            0x140..=0x17E => {
                let s = ((r - 0x140) >> 3) as usize;
                match r & 6 {
                    0 => {
                        self.sprites[s].vstart = (self.sprites[s].vstart & 0x100) | (val >> 8);
                        self.sprites[s].hstart = (self.sprites[s].hstart & 1) | ((val & 0xFF) << 1);
                    }
                    2 => {
                        self.sprites[s].vstop = (val >> 8) | (((val >> 1) & 1) << 8);
                        self.sprites[s].vstart = (self.sprites[s].vstart & 0xFF) | (((val >> 2) & 1) << 8);
                        self.sprites[s].hstart = (self.sprites[s].hstart & 0x1FE) | (val & 1);
                        self.sprites[s].attached = (val & 0x80) != 0;
                        self.sprites[s].armed = false;
                    }
                    4 => {
                        self.sprites[s].dat_a = val;
                        self.sprites[s].armed = true;
                        // Record manual Copper-multiplexed sprite writes (used by Parasol Stars HUD!)
                        if val != 0 && self.vpos >= 44 && self.vpos < 44 + SCREEN_H as u16 {
                            let pair = s / 2;
                            self.manual_stamps.push(ManualSpriteStamp {
                                hstart: self.sprites[s].hstart,
                                dat_a: val,
                                dat_b: self.sprites[s].dat_b,
                                col1: self.color[16 + pair * 4 + 1],
                                col3: self.color[16 + pair * 4 + 3],
                            });
                        }
                    }
                    6 => { self.sprites[s].dat_b = val; }
                    _ => {}
                }
            }
            // Palette COLOR00..COLOR31 ($180..$1BE)
            0x180..=0x1BE => {
                let idx = ((r - 0x180) >> 1) as usize;
                self.color[idx] = val & 0x0FFF;
            }
            _ => {}
        }
    }

    #[inline(always)]
    fn chip_r16(chip: &[u8], addr: u32) -> u16 {
        let a = (addr as usize) & (chip.len() - 2);
        u16::from_be_bytes([chip[a], chip[a + 1]])
    }

    #[inline(always)]
    fn chip_w16(chip: &mut [u8], addr: u32, val: u16) {
        let a = (addr as usize) & (chip.len() - 2);
        let b = val.to_be_bytes();
        chip[a] = b[0];
        chip[a + 1] = b[1];
    }

    fn run_blitter(&mut self, chip: &mut [u8], width: u32, height: u32) {
        self.blit_count += 1;
        // Line mode?
        if (self.bltcon1 & 1) != 0 {
            self.run_blitter_line(chip, height);
            self.intreq |= 0x0040;
            return;
        }

        let ash = ((self.bltcon0 >> 12) & 0xF) as u32;
        let bsh = ((self.bltcon1 >> 12) & 0xF) as u32;
        let use_a = (self.bltcon0 & 0x0800) != 0;
        let use_b = (self.bltcon0 & 0x0400) != 0;
        let use_c = (self.bltcon0 & 0x0200) != 0;
        let use_d = (self.bltcon0 & 0x0100) != 0;
        let lf = (self.bltcon0 & 0xFF) as u8;
        let desc = (self.bltcon1 & 0x0002) != 0;
        let fill_ife = (self.bltcon1 & 0x0008) != 0;
        let fill_efe = (self.bltcon1 & 0x0010) != 0;
        let fill_mode = fill_ife || fill_efe;
        let mut fill_carry_init = (self.bltcon1 & 0x0004) != 0;

        let mut a_prev = 0u16;
        let mut b_prev = 0u16;
        let mut all_zero = true;

        let step: i32 = if desc { -2 } else { 2 };
        let amod = if desc { -(self.bltamod as i32) } else { self.bltamod as i32 };
        let bmod = if desc { -(self.bltbmod as i32) } else { self.bltbmod as i32 };
        let cmod = if desc { -(self.bltcmod as i32) } else { self.bltcmod as i32 };
        let dmod = if desc { -(self.bltdmod as i32) } else { self.bltdmod as i32 };

        for _y in 0..height {
            let mut fill_carry = fill_carry_init;
            for x in 0..width {
                let mut a_raw = if use_a {
                    let v = Self::chip_r16(chip, self.bltapt);
                    self.bltapt = self.bltapt.wrapping_add(step as u32);
                    v
                } else {
                    self.bltadat
                };
                if x == 0 { a_raw &= self.bltafwm; }
                if x == width - 1 { a_raw &= self.bltalwm; }

                let a_val = if ash == 0 {
                    a_raw
                } else if desc {
                    (a_raw << ash) | (a_prev >> (16 - ash))
                } else {
                    (a_prev << (16 - ash)) | (a_raw >> ash)
                };
                a_prev = a_raw;

                let b_raw = if use_b {
                    let v = Self::chip_r16(chip, self.bltbpt);
                    self.bltbpt = self.bltbpt.wrapping_add(step as u32);
                    v
                } else {
                    self.bltbdat
                };
                let b_val = if bsh == 0 {
                    b_raw
                } else if desc {
                    (b_raw << bsh) | (b_prev >> (16 - bsh))
                } else {
                    (b_prev << (16 - bsh)) | (b_raw >> bsh)
                };
                b_prev = b_raw;

                let c_val = if use_c {
                    let v = Self::chip_r16(chip, self.bltcpt);
                    self.bltcpt = self.bltcpt.wrapping_add(step as u32);
                    v
                } else {
                    self.bltcdat
                };

                let mut d_val = eval_minterm(lf, a_val, b_val, c_val);

                if fill_mode && desc {
                    let mut filled = 0u16;
                    for bit in 0..16 {
                        let b = ((d_val >> bit) & 1) != 0;
                        let out_bit = if fill_ife {
                            fill_carry || b
                        } else {
                            fill_carry ^ b
                        };
                        if out_bit { filled |= 1 << bit; }
                        if b { fill_carry = !fill_carry; }
                    }
                    d_val = filled;
                }

                if d_val != 0 { all_zero = false; }
                if use_d {
                    Self::chip_w16(chip, self.bltdpt, d_val);
                    self.bltdpt = self.bltdpt.wrapping_add(step as u32);
                }
            }
            if use_a { self.bltapt = self.bltapt.wrapping_add(amod as u32); }
            if use_b { self.bltbpt = self.bltbpt.wrapping_add(bmod as u32); }
            if use_c { self.bltcpt = self.bltcpt.wrapping_add(cmod as u32); }
            if use_d { self.bltdpt = self.bltdpt.wrapping_add(dmod as u32); }
            fill_carry_init = (self.bltcon1 & 0x0004) != 0;
        }

        self.blt_zero = all_zero;
        self.intreq |= 0x0040; // BLIT interrupt flag
    }

    fn run_blitter_line(&mut self, chip: &mut [u8], height: u32) {
        // Basic OCS Blitter line draw
        let mut x_shift = ((self.bltcon0 >> 12) & 0xF) as i32;
        let lf = (self.bltcon0 & 0xFF) as u8;
        let octant = (self.bltcon1 >> 2) & 7;
        let mut err = self.bltapt as i16 as i32;
        let mut sign = (self.bltcon1 & 0x40) != 0;
        let onedot = (self.bltcon1 & 0x02) != 0;
        let mut plotted_this_line = false;
        let bsh = ((self.bltcon1 >> 12) & 0xF) as u32;
        let mut b_rot = bsh;

        for _ in 0..height {
            let a_bit = (self.bltadat & self.bltafwm).rotate_right(x_shift as u32);
            let b_bit = if ((self.bltbdat >> b_rot) & 1) != 0 { 0xFFFFu16 } else { 0 };
            let c_val = Self::chip_r16(chip, self.bltcpt);
            if !(onedot && plotted_this_line) {
                let d_val = eval_minterm(lf, a_bit, b_bit, c_val);
                Self::chip_w16(chip, self.bltdpt, d_val);
                plotted_this_line = true;
            }
            self.bltdpt = self.bltcpt;
            b_rot = (b_rot.wrapping_sub(1)) & 15;

            let sud = (octant & 4) != 0;
            let aul = (octant & 2) != 0;
            let sul = (octant & 1) != 0;
            if sud {
                // Major axis is X
                if aul { x_shift -= 1; if x_shift < 0 { x_shift = 15; self.bltcpt = self.bltcpt.wrapping_sub(2); } }
                else { x_shift += 1; if x_shift > 15 { x_shift = 0; self.bltcpt = self.bltcpt.wrapping_add(2); } }
                if !sign {
                    if sul { self.bltcpt = self.bltcpt.wrapping_sub(self.bltcmod as i32 as u32); }
                    else { self.bltcpt = self.bltcpt.wrapping_add(self.bltcmod as i32 as u32); }
                    plotted_this_line = false;
                }
            } else {
                // Major axis is Y
                if aul { self.bltcpt = self.bltcpt.wrapping_sub(self.bltcmod as i32 as u32); }
                else { self.bltcpt = self.bltcpt.wrapping_add(self.bltcmod as i32 as u32); }
                plotted_this_line = false;
                if !sign {
                    if sul { x_shift -= 1; if x_shift < 0 { x_shift = 15; self.bltcpt = self.bltcpt.wrapping_sub(2); } }
                    else { x_shift += 1; if x_shift > 15 { x_shift = 0; self.bltcpt = self.bltcpt.wrapping_add(2); } }
                }
            }
            if !sign {
                err += self.bltamod as i32;
            } else {
                err += self.bltbmod as i32;
            }
            sign = err < 0;
        }
        self.bltapt = err as u32;
        self.bltcon0 = (self.bltcon0 & 0x0FFF) | (((x_shift & 0xF) as u16) << 12);
    }

    pub fn advance_cycles(&mut self, chip: &mut [u8], cycles: u32) {
        // Update CIA-A fire buttons (active low: bit 6 = port 0, bit 7 = port 1)
        self.cia_a.pra = (self.cia_a.pra & 0x3F)
            | if self.joy0.fire { 0 } else { 0x40 }
            | if self.joy1.fire { 0 } else { 0x80 };

        // Deliver queued keyboard scancodes when CIA-A SP interrupt is clear
        if (self.cia_a.icr_data & 0x08) == 0 {
            if let Some(code) = self.cia_a.key_queue.pop_front() {
                self.cia_a.sdr = code;
                self.cia_a.icr_data |= 0x08;
                if (self.cia_a.icr_mask & 0x08) != 0 {
                    self.cia_a.icr_data |= 0x80;
                    self.intreq |= 0x0008; // PORTS (Level 2)
                }
            }
        }

        // Tick CIA timers (E-clock = CPU / 10)
        let e_clks = (cycles + 9) / 10;
        if self.cia_a.tick(e_clks) { self.intreq |= 0x0008; }
        if self.cia_b.tick(e_clks) { self.intreq |= 0x2000; }

        self.line_cycles += cycles;
        while self.line_cycles >= CYCLES_PER_LINE {
            self.line_cycles -= CYCLES_PER_LINE;
            self.step_scanline(chip);
        }
    }

    fn step_scanline(&mut self, chip: &mut [u8]) {
        let y = self.vpos;

        // VBlank at scanline 0
        if y == 0 {
            self.intreq |= 0x0020; // VERTB (Level 3)
            self.cia_a.tod = self.cia_a.tod.wrapping_add(1);
            // Restart Copper at COP1LC if Copper DMA is enabled
            if (self.dmacon & 0x0280) == 0x0280 {
                self.cop_pc = self.cop1lc;
                self.cop_waiting = false;
                self.cop_stopped = false;
            }
            for s in 0..8 {
                self.sprites[s].armed = false;
                self.sprites[s].active_line = false;
                self.sprites[s].dma_stopped = true;
                self.sprites[s].vstart = 0;
                self.sprites[s].vstop = 0;
            }
        }

        // Run Copper for early horizontal beam on this scanline (before display fetch)
        self.manual_stamps.clear();
        self.run_copper(chip, y, 0x38);

        // Fetch initial sprite control words around scanline 25
        if y == 25 && (self.dmacon & 0x0220) == 0x0220 {
            for s in 0..8 {
                self.sprites[s].dma_stopped = false;
                self.fetch_sprite_ctrl(chip, s);
            }
        }

        // Render visible PAL lines 44..300 (256 lines)
        if y >= 44 && y < 44 + SCREEN_H as u16 {
            self.render_scanline(chip, y);
        } else if y > 25 && (self.dmacon & 0x0220) == 0x0220 {
            // Still advance sprite DMA state outside visible window
            self.step_sprites_line(chip, y);
        }

        // Run Copper for remainder of scanline (hpos up to 0xE2)
        self.run_copper(chip, y, 0xE2);

        // Draw any manual Copper-multiplexed sprite stamps written during this scanline
        if y >= 44 && y < 44 + SCREEN_H as u16 && !self.manual_stamps.is_empty() {
            let sy = (y - 44) as usize;
            let row_off = sy * SCREEN_W;
            for st in &self.manual_stamps {
                let hx0 = st.hstart as i32;
                for bit in 0..16 {
                    let sx = hx0 + bit - 129;
                    if sx < 0 || sx >= SCREEN_W as i32 { continue; }
                    let sh = 15 - bit;
                    let ba = (st.dat_a >> sh) & 1;
                    let bb = (st.dat_b >> sh) & 1;
                    if ba != 0 {
                        let col = if bb != 0 { st.col3 } else { st.col1 };
                        self.fb[row_off + sx as usize] = ocs_rgb(col);
                    }
                }
            }
        }

        // Mix Paula audio for this scanline
        self.mix_audio_scanline(chip);

        self.vpos += 1;
        if self.vpos >= PAL_LINES {
            self.vpos = 0;
            self.frame_count += 1;
            self.frame_ready = true;
        }
    }

    fn run_copper(&mut self, chip: &mut [u8], vpos: u16, max_hpos: u16) {
        if (self.dmacon & 0x0280) != 0x0280 || self.cop_stopped {
            return;
        }
        let v_masked = vpos & 0xFF;
        // Safety bound: max 256 copper instructions per scanline
        for _ in 0..256 {
            if self.cop_waiting {
                let beam_cmp = ((v_masked & self.cop_wait_ve) << 8) | (max_hpos & self.cop_wait_he);
                let wait_cmp = ((self.cop_wait_vp & self.cop_wait_ve) << 8) | (self.cop_wait_hp & self.cop_wait_he);
                // Special case: WAIT $FFFF, $FFFE = halt until next VBlank
                if self.cop_wait_vp == 0xFF && self.cop_wait_hp == 0xFE {
                    self.cop_stopped = true;
                    return;
                }
                // Check if beam reached wait position!
                // Note: if vpos >= 256 and wait_vp < 0x80, or v_masked matches:
                if beam_cmp >= wait_cmp {
                    self.cop_waiting = false;
                } else {
                    return;
                }
            }

            let w1 = Self::chip_r16(chip, self.cop_pc);
            let w2 = Self::chip_r16(chip, self.cop_pc.wrapping_add(2));
            self.cop_pc = self.cop_pc.wrapping_add(4);

            if (w1 & 1) == 0 {
                // MOVE
                let reg = w1 & 0x01FE;
                if reg < 0x0040 || (reg < 0x0080 && (self.copcon & 2) == 0) {
                    self.cop_stopped = true;
                    return;
                }
                self.write_custom_w(chip, reg, w2);
            } else if (w2 & 1) == 0 {
                // WAIT
                let vp = (w1 >> 8) & 0xFF;
                let hp = w1 & 0xFE;
                let ve = ((w2 >> 8) & 0x7F) | 0x80;
                let he = w2 & 0xFE;
                if w1 == 0xFFFF && w2 == 0xFFFE {
                    self.cop_stopped = true;
                    return;
                }
                self.cop_wait_vp = vp;
                self.cop_wait_hp = hp;
                self.cop_wait_ve = ve;
                self.cop_wait_he = he;
                self.cop_waiting = true;
                // Re-check immediately in case beam is already past wait target
                let beam_cmp = ((v_masked & ve) << 8) | (max_hpos & he);
                let wait_cmp = ((vp & ve) << 8) | (hp & he);
                if beam_cmp >= wait_cmp {
                    self.cop_waiting = false;
                } else {
                    return;
                }
            } else {
                // SKIP
                let vp = (w1 >> 8) & 0xFF;
                let hp = w1 & 0xFE;
                let ve = ((w2 >> 8) & 0x7F) | 0x80;
                let he = w2 & 0xFE;
                let beam_cmp = ((v_masked & ve) << 8) | (max_hpos & he);
                let skip_cmp = ((vp & ve) << 8) | (hp & he);
                if beam_cmp >= skip_cmp {
                    self.cop_pc = self.cop_pc.wrapping_add(4);
                }
            }
        }
    }

    fn fetch_sprite_ctrl(&mut self, chip: &mut [u8], s: usize) {
        let p = self.sprites[s].ptr;
        if p == 0 {
            self.sprites[s].dma_stopped = true;
            self.sprites[s].active_line = false;
            return;
        }
        let pos = Self::chip_r16(chip, p);
        let ctl = Self::chip_r16(chip, p.wrapping_add(2));
        self.sprites[s].ptr = p.wrapping_add(4);
        if pos == 0 && ctl == 0 {
            self.sprites[s].dma_stopped = true;
            self.sprites[s].vstart = 0x3FF;
            self.sprites[s].vstop = 0x3FF;
            self.sprites[s].active_line = false;
            return;
        }
        self.sprites[s].vstart = (pos >> 8) | (((ctl >> 2) & 1) << 8);
        self.sprites[s].vstop = (ctl >> 8) | (((ctl >> 1) & 1) << 8);
        self.sprites[s].hstart = ((pos & 0xFF) << 1) | (ctl & 1);
        self.sprites[s].attached = (ctl & 0x80) != 0;
        self.sprites[s].active_line = false;
    }

    fn step_sprites_line(&mut self, chip: &mut [u8], y: u16) {
        for s in 0..8 {
            if self.sprites[s].dma_stopped {
                self.sprites[s].active_line = false;
                continue;
            }
            if y == self.sprites[s].vstop {
                self.fetch_sprite_ctrl(chip, s);
                if self.sprites[s].dma_stopped {
                    continue;
                }
            }
            if y >= self.sprites[s].vstart && y < self.sprites[s].vstop {
                let p = self.sprites[s].ptr;
                self.sprites[s].dat_a = Self::chip_r16(chip, p);
                self.sprites[s].dat_b = Self::chip_r16(chip, p.wrapping_add(2));
                self.sprites[s].ptr = p.wrapping_add(4);
                self.sprites[s].active_line = true;
            } else {
                self.sprites[s].active_line = false;
            }
        }
    }

    fn render_scanline(&mut self, chip: &mut [u8], y: u16) {
        let sy = (y - 44) as usize;
        let row_off = sy * SCREEN_W;
        let bg_rgb = ocs_rgb(self.color[0]);
        for x in 0..SCREEN_W {
            self.fb[row_off + x] = bg_rgb;
        }

        // Update sprites for this scanline
        if (self.dmacon & 0x0220) == 0x0220 {
            self.step_sprites_line(chip, y);
        }

        // Display window bounds
        let diw_vstart = (self.diwstrt >> 8) as u16;
        let mut diw_vstop = (self.diwstop >> 8) as u16;
        if (diw_vstop & 0x80) == 0 { diw_vstop |= 0x100; }
        let diw_hstart = (self.diwstrt & 0xFF) as i32;
        let diw_hstop = ((self.diwstop & 0xFF) as i32) + 256;

        let bpl_en = (self.dmacon & 0x0300) == 0x0300;
        let bpu = ((self.bplcon0 >> 12) & 7) as usize;
        let dpf = (self.bplcon0 & 0x0400) != 0;
        let ehb = bpu == 6 && !dpf && (self.bplcon0 & 0x0800) == 0;

        let mut pf_idx = [0u8; SCREEN_W];
        let mut pf1_nz = [false; SCREEN_W];
        let mut pf2_nz = [false; SCREEN_W];

        if bpl_en && bpu > 0 && bpu <= 6 && y >= diw_vstart && y < diw_vstop {
            let ddf_s = self.ddfstrt as i32;
            let ddf_e = self.ddfstop as i32;
            let words = if ddf_e >= ddf_s { ((ddf_e - ddf_s) / 8 + 1).min(24) as usize } else { 0 };
            let mut plane_words = [[0u16; 24]; 6];
            for p in 0..bpu {
                let mut ptr = self.bplpt[p];
                for w in 0..words {
                    plane_words[p][w] = Self::chip_r16(chip, ptr);
                    ptr = ptr.wrapping_add(2);
                }
                let modulo = if (p & 1) == 0 { self.bpl1mod } else { self.bpl2mod };
                self.bplpt[p] = ptr.wrapping_add(modulo as i32 as u32);
            }

            let scroll1 = (self.bplcon1 & 0xF) as i32;
            let scroll2 = ((self.bplcon1 >> 4) & 0xF) as i32;
            // Hardware X of first fetched pixel (relative to screen x=0 at hardware H=$81=129)
            let base_hx = ddf_s * 2 + 17;

            let pf2_pri = (self.bplcon2 & 0x0040) != 0;

            for sx in 0..SCREEN_W {
                let hx = 129 + sx as i32;
                if hx < diw_hstart || hx >= diw_hstop { continue; }
                let rel1 = hx - (base_hx + scroll1);
                let rel2 = hx - (base_hx + scroll2);

                if dpf {
                    let mut c1 = 0u8;
                    let mut c2 = 0u8;
                    if rel1 >= 0 && (rel1 as usize) < words * 16 {
                        let wi = (rel1 as usize) >> 4;
                        let bi = 15 - ((rel1 as usize) & 15);
                        if bpu >= 1 { c1 |= (((plane_words[0][wi] >> bi) & 1) as u8) << 0; }
                        if bpu >= 3 { c1 |= (((plane_words[2][wi] >> bi) & 1) as u8) << 1; }
                        if bpu >= 5 { c1 |= (((plane_words[4][wi] >> bi) & 1) as u8) << 2; }
                    }
                    if rel2 >= 0 && (rel2 as usize) < words * 16 {
                        let wi = (rel2 as usize) >> 4;
                        let bi = 15 - ((rel2 as usize) & 15);
                        if bpu >= 2 { c2 |= (((plane_words[1][wi] >> bi) & 1) as u8) << 0; }
                        if bpu >= 4 { c2 |= (((plane_words[3][wi] >> bi) & 1) as u8) << 1; }
                        if bpu >= 6 { c2 |= (((plane_words[5][wi] >> bi) & 1) as u8) << 2; }
                    }
                    pf1_nz[sx] = c1 != 0;
                    pf2_nz[sx] = c2 != 0;
                    let ci = if pf2_pri {
                        if c2 != 0 { 8 + c2 } else { c1 }
                    } else {
                        if c1 != 0 { c1 } else if c2 != 0 { 8 + c2 } else { 0 }
                    };
                    pf_idx[sx] = ci;
                    self.fb[row_off + sx] = ocs_rgb(self.color[ci as usize]);
                } else {
                    let mut ci = 0u8;
                    for p in 0..bpu {
                        let rel = if (p & 1) == 0 { rel1 } else { rel2 };
                        if rel >= 0 && (rel as usize) < words * 16 {
                            let wi = (rel as usize) >> 4;
                            let bi = 15 - ((rel as usize) & 15);
                            if ((plane_words[p][wi] >> bi) & 1) != 0 {
                                ci |= 1 << p;
                            }
                        }
                    }
                    pf_idx[sx] = ci;
                    pf1_nz[sx] = ci != 0;
                    let rgb = if ehb && ci >= 32 {
                        let base = ocs_rgb(self.color[(ci & 31) as usize]);
                        (base >> 1) & 0x007F_7F7F
                    } else {
                        ocs_rgb(self.color[(ci & 31) as usize])
                    };
                    self.fb[row_off + sx] = rgb;
                }
            }
        }

        // Composite hardware sprites (back-to-front: 7 down to 0)
        if (self.dmacon & 0x0220) == 0x0220 && y >= diw_vstart && y < diw_vstop {
            let pf1p = (self.bplcon2 & 7) as usize;
            let pf2p = ((self.bplcon2 >> 3) & 7) as usize;
            let mut s = 7i32;
            while s >= 0 {
                let si = s as usize;
                let pair = si / 2;
                let attached = (si & 1) == 1 && self.sprites[si].attached;
                if attached {
                    let sp0 = self.sprites[si - 1];
                    let sp1 = self.sprites[si];
                    if sp0.active_line || sp1.active_line {
                        let hx0 = sp0.hstart as i32;
                        for bit in 0..16 {
                            let sx = hx0 + bit - 129;
                            if sx < 0 || sx >= SCREEN_W as i32 { continue; }
                            let ux = sx as usize;
                            let sh = 15 - bit;
                            let c = (if sp0.active_line { ((sp0.dat_a >> sh) & 1) | (((sp0.dat_b >> sh) & 1) << 1) } else { 0 })
                                  | (if sp1.active_line { (((sp1.dat_a >> sh) & 1) << 2) | (((sp1.dat_b >> sh) & 1) << 3) } else { 0 });
                            if c != 0 {
                                let behind_pf1 = pf1_nz[ux] && pair >= pf1p;
                                let behind_pf2 = dpf && pf2_nz[ux] && pair >= pf2p;
                                if !behind_pf1 && !behind_pf2 {
                                    self.fb[row_off + ux] = ocs_rgb(self.color[16 + c as usize]);
                                }
                            }
                        }
                    }
                    s -= 2;
                    continue;
                }
                let sp = self.sprites[si];
                if sp.active_line {
                    let hx0 = sp.hstart as i32;
                    for bit in 0..16 {
                        let sx = hx0 + bit - 129;
                        if sx < 0 || sx >= SCREEN_W as i32 { continue; }
                        let ux = sx as usize;
                        let sh = 15 - bit;
                        let c = ((sp.dat_a >> sh) & 1) | (((sp.dat_b >> sh) & 1) << 1);
                        if c != 0 {
                            let behind_pf1 = pf1_nz[ux] && pair >= pf1p;
                            let behind_pf2 = dpf && pf2_nz[ux] && pair >= pf2p;
                            if !behind_pf1 && !behind_pf2 {
                                self.fb[row_off + ux] = ocs_rgb(self.color[16 + pair * 4 + c as usize]);
                            }
                        }
                    }
                }
                s -= 1;
            }
        }
    }

    fn mix_audio_scanline(&mut self, chip: &[u8]) {
        // 44100 Hz / (50 Hz * 312 lines) = 2.826923 samples/line
        const SAMPLES_PER_LINE: f64 = (AUDIO_RATE as f64) / (50.0 * PAL_LINES as f64);
        self.audio_accum += SAMPLES_PER_LINE;
        let count = self.audio_accum as usize;
        self.audio_accum -= count as f64;
        if count == 0 { return; }

        let mask = chip.len() - 1;
        for _ in 0..count {
            let mut ch_out = [0i32; 4];
            for ch in 0..4 {
                let a = &mut self.aud[ch];
                if a.dma_active && a.per >= 64 && a.vol > 0 {
                    let step = PAL_AUDIO_CLK / ((a.per as f64) * (AUDIO_RATE as f64));
                    a.phase += step;
                    while a.phase >= 1.0 {
                        a.phase -= 1.0;
                        a.sample = chip[(a.cur_ptr as usize) & mask] as i8;
                        a.cur_ptr = a.cur_ptr.wrapping_add(1);
                        if a.cur_len_bytes > 1 {
                            a.cur_len_bytes -= 1;
                        } else {
                            a.cur_ptr = a.lc;
                            a.cur_len_bytes = (a.len as u32).max(1) * 2;
                            self.intreq |= 1 << (7 + ch);
                        }
                    }
                    ch_out[ch] = (a.sample as i32) * (a.vol as i32); // range: -8192..8128
                }
            }
            // Amiga stereo: ch0 + ch3 = Left, ch1 + ch2 = Right (with 25% crossfeed for pleasant stereo)
            let raw_l = ch_out[0] + ch_out[3];
            let raw_r = ch_out[1] + ch_out[2];
            let l = ((raw_l * 3 + raw_r) / 2).clamp(-32767, 32767) as i16;
            let r = ((raw_r * 3 + raw_l) / 2).clamp(-32767, 32767) as i16;
            self.audio_buf.push(l);
            self.audio_buf.push(r);
        }
    }
}

#[inline(always)]
fn eval_minterm(lf: u8, a: u16, b: u16, c: u16) -> u16 {
    let mut r = 0u16;
    if (lf & 0x80) != 0 { r |= a & b & c; }
    if (lf & 0x40) != 0 { r |= a & b & !c; }
    if (lf & 0x20) != 0 { r |= a & !b & c; }
    if (lf & 0x10) != 0 { r |= a & !b & !c; }
    if (lf & 0x08) != 0 { r |= !a & b & c; }
    if (lf & 0x04) != 0 { r |= !a & b & !c; }
    if (lf & 0x02) != 0 { r |= !a & !b & c; }
    if (lf & 0x01) != 0 { r |= !a & !b & !c; }
    r
}

#[inline(always)]
fn ocs_rgb(c: u16) -> u32 {
    let r = (((c >> 8) & 0xF) * 17) as u32;
    let g = (((c >> 4) & 0xF) * 17) as u32;
    let b = ((c & 0xF) * 17) as u32;
    (r << 16) | (g << 8) | b
}
