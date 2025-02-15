use std::{fmt::Display, time::Instant};

use rand::{rngs::SmallRng, Rng, SeedableRng};

use crate::hardware::{Memory, Registers, Screen};

// References:
// - http://devernay.free.fr/hacks/chip8/C8TECH10.HTM
// - https://github.com/mattmikolay/chip-8/wiki/CHIP%E2%80%908-Instruction-Set

macro_rules! dbg_instruction {
    ($($t:tt)*) => {{
        if cfg!(debug_assertions) {
            eprintln!($($t)*);
        }
    }};
}

pub struct Config {
    /// See https://stackoverflow.com/a/60442588/3398054
    pub use_original_shift_semantics: bool,
    pub use_original_address_register_behavior: bool,
    /// Length of one cpu cycle, i.e. 1.0 / clockspeed is the CPU frequency
    pub cycle_length: f64,
    pub single_step: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            use_original_shift_semantics: false,
            use_original_address_register_behavior: false,
            cycle_length: 1.0 / 660.0,
            single_step: false,
        }
    }
}

pub struct Chip8Interpreter {
    regs: Registers,
    memory: Memory,
    screen: Screen,
    key_states: [bool; 16],

    /// If this is Some(x), then the interpreter will wait for some key to be pressed
    /// before executing any other instructions, and store its number in the register Vx.
    waiting_for_key: Option<u8>,

    rng: SmallRng,
    config: Config,

    /// How much time has passed since the last call to render started?
    previous_frame: Instant,
    /// Number of ticks (updates)
    ticks: u64,
    /// Number of rendered frames
    frames: u64,
    acc: f64,

    halted: bool,
}

#[derive(Debug)]
pub enum Error {
    InvalidInstruction,
    Fault,
    Blocked,
}

impl Chip8Interpreter {
    pub fn unhalt(&mut self) {
        self.halted = false;
    }

    pub fn load_program(&mut self, program: &[u8]) {
        self.memory.load_program(program);
    }

    pub fn should_play_sound(&self) -> bool {
        self.regs.sound_timer > 0
    }

    pub fn screen_size(&self) -> (u32, u32) {
        (Screen::WIDTH as u32, Screen::HEIGHT as u32)
    }

    pub fn with_config(config: Config) -> Self {
        Self {
            regs: Default::default(),
            memory: Default::default(),
            screen: Screen::default(),
            key_states: [false; 16],

            waiting_for_key: None,

            rng: SmallRng::from_os_rng(),
            config,

            previous_frame: Instant::now(),
            ticks: 0,
            frames: 0,
            acc: 0.0,

            halted: false,
        }
    }

    pub fn tick(&mut self, frame: &mut [u8]) {
        // This is based on https://gafferongames.com/post/fix_your_timestep/
        let dt = self.config.cycle_length;

        let now = Instant::now();
        let frame_time = (now - self.previous_frame).as_secs_f64();
        self.acc += frame_time;

        while self.acc >= dt {
            self.step().unwrap();
            self.ticks += 1;
            self.acc -= dt;
        }

        self.render(frame);
        self.frames += 1;
        self.previous_frame = now;
    }

    pub fn render(&self, frame: &mut [u8]) {
        let src = self.screen.frame();

        for (i, chunk) in frame.chunks_exact_mut(4).enumerate() {
            let gray = src[i];
            // R G B A
            chunk.copy_from_slice(&[gray, gray, gray, 0xff]);
        }
    }

    pub fn set_key(&mut self, key: u8, pressed: bool) {
        self.key_states[key as usize] = pressed;

        if pressed {
            if let Some(reg) = self.waiting_for_key.take() {
                self.regs[reg] = key;
            }
        }
    }

    pub fn step(&mut self) -> Result<(), Error> {
        if self.halted || self.waiting_for_key.is_some() {
            return Ok(());
        }

        if self.regs.pc + 1 >= self.memory.len() {
            return Err(Error::Fault);
        }

        // Fetch instruction
        let inst = u16::from_be_bytes([self.memory[self.regs.pc], self.memory[self.regs.pc + 1]]);

        // Instruction pointer always points at the next instruction...
        self.regs.advance_pc();

        if self.regs.sound_timer > 0 {
            self.regs.sound_timer -= 1;
        }

        if self.regs.delay_timer > 0 {
            self.regs.delay_timer -= 1;
        }

        self.execute_instruction(inst)?;

        if self.config.single_step {
            self.halted = true;
        }

        Ok(())
    }

    /// Execute an instruction. Note that this never modified the time based registers, i.e. sound
    /// and delay timers. They are modified elsewhere.
    fn execute_instruction(&mut self, inst: u16) -> Result<(), Error> {
        if self.waiting_for_key.is_some() {
            return Err(Error::Blocked);
        }

        // An instruction is 2 bytes, and consists of 4 nibbles. Deconstruct it first.
        // msn x y n
        //       n n
        //     n n n

        let msn = (inst >> (4 * 3) & 0xf) as u8; // most significant nibble
        let x = (inst >> (4 * 2) & 0xf) as u8; // second most significant nibble
        let y = (inst >> 4 & 0xf) as u8; // third
        let n = (inst & 0xf) as u8;

        let nn = (inst & 0xff) as u8; // lowest two nibbles
        let nnn = inst & 0xfff; // lowest three nibbles

        match msn {
            0x0 => match nnn {
                0xE0 => self.cls(),
                0xEE => self.ret(),
                _ => return Err(Error::InvalidInstruction),
            },
            0x1 => self.jp(nnn),
            0x2 => self.call(nnn),
            0x3 => self.se(x, nn),
            0x4 => self.sne(x, nn),
            0x5 if n == 0 => self.se_reg(x, y),
            0x6 => self.ld(x, nn),
            0x7 => self.add_literal(x, nn),
            0x8 => match n {
                0x0 => self.ld_reg(x, y),
                0x1 => self.or(x, y),
                0x2 => self.and(x, y),
                0x3 => self.xor(x, y),
                0x4 => self.add(x, y),
                0x5 => self.sub(x, y),
                0x6 => self.shr(x, y),
                0x7 => self.subn(x, y),
                0xE => self.shl(x, y),
                _ => return Err(Error::InvalidInstruction),
            },
            0x9 => self.sne_reg(x, y),
            0xA => self.ld_addr(nnn),
            0xB => self.jp0(nnn),
            0xC => self.rnd(x, nn),
            0xD => self.drw(x, y, n),
            0xE => match nn {
                0x9E => self.skp(x),
                0xA1 => self.skpn(x),
                _ => return Err(Error::InvalidInstruction),
            },
            0xF => match nn {
                0x07 => self.ld_dt(x),
                0x0A => self.ld_key(x),
                0x15 => self.set_dt(x),
                0x18 => self.set_st(x),
                0x1E => self.addi(x),
                0x29 => self.ld_sprite(x),
                0x33 => self.ld_bcd(x),
                0x55 => self.copy_regs_to_memory(x),
                0x65 => self.copy_memory_to_regs(x),
                _ => return Err(Error::InvalidInstruction),
            },
            _ => return Err(Error::InvalidInstruction),
        }

        Ok(())
    }

    fn cls(&mut self) {
        dbg_instruction!("CLS");
        self.screen.clear();
    }

    fn ret(&mut self) {
        dbg_instruction!("RET");
        let addr = self.regs.pop_addr();
        self.regs.pc = addr;
    }

    fn jp(&mut self, addr: u16) {
        dbg_instruction!("JP {}", Addr(addr));
        self.regs.pc = addr;
    }

    fn call(&mut self, addr: u16) {
        dbg_instruction!("CALL {}", Addr(addr));
        self.regs.push_addr(self.regs.pc);
        self.regs.pc = addr;
    }

    fn se(&mut self, vx: u8, byte: u8) {
        dbg_instruction!("SE {}, {}", Reg(vx), byte);
        if self.regs[vx] == byte {
            self.regs.advance_pc();
        }
    }

    fn sne(&mut self, vx: u8, byte: u8) {
        dbg_instruction!("SNE {}, {}", Reg(vx), byte);
        if self.regs[vx] != byte {
            self.regs.advance_pc();
        }
    }

    fn se_reg(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("SE {}, {}", Reg(vx), Reg(vy));
        if self.regs[vx] == self.regs[vy] {
            self.regs.advance_pc();
        }
    }

    fn ld(&mut self, vx: u8, byte: u8) {
        dbg_instruction!("LD {}, {}", Reg(vx), byte);
        self.regs[vx] = byte;
    }

    fn add_literal(&mut self, vx: u8, byte: u8) {
        dbg_instruction!("ADD {}, {}", Reg(vx), byte);
        self.regs[vx] = self.regs[vx].wrapping_add(byte);
    }

    fn ld_reg(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("LD {}, {}", Reg(vx), Reg(vy));
        self.regs[vx] = self.regs[vy];
    }

    fn or(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("OR {}, {}", Reg(vx), Reg(vy));
        self.regs[vx] |= self.regs[vy];
    }

    fn and(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("AND {}, {}", Reg(vx), Reg(vy));
        self.regs[vx] &= self.regs[vy];
    }

    fn xor(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("XOR {}, {}", Reg(vx), Reg(vy));
        self.regs[vx] ^= self.regs[vy];
    }

    fn add(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("ADD {}, {}", Reg(vx), Reg(vy));

        // Note that this also sets a carry flag
        let (res, carry) = self.regs[vx].overflowing_add(self.regs[vy]);
        self.regs[vx] = res;
        self.regs[0xf_u8] = carry as u8;
    }

    fn sub(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("SUB {}, {}", Reg(vx), Reg(vy));
        let (res, overflow) = self.regs[vx].overflowing_sub(self.regs[vy]);
        self.regs[vx] = res;
        self.regs[0xf_u8] = !overflow as u8;
    }

    fn shr(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("SHR {}, {}", Reg(vx), Reg(vy));
        if self.config.use_original_shift_semantics {
            self.regs[0xf_u8] = self.regs[vy] & 0b1;
            self.regs[vx] = self.regs[vy] >> 1;
        } else {
            self.regs[0xf_u8] = self.regs[vx] & 0b1;
            self.regs[vx] >>= 1;
        }
    }

    fn subn(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("SUBN {}, {}", Reg(vx), Reg(vy));
        // Subtract with a weird carry flag?
        let (res, overflow) = self.regs[vy].overflowing_sub(self.regs[vx]);
        self.regs[vx] = res;
        self.regs[0xf_u8] = !overflow as u8;
    }

    fn shl(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("SHL {}, {}", Reg(vx), Reg(vy));
        if self.config.use_original_shift_semantics {
            self.regs[0xf_u8] = self.regs[vy] & 0x80;
            self.regs[vx] = self.regs[vy] << 1;
        } else {
            self.regs[0xf_u8] = self.regs[vx] & 0x80;
            self.regs[vx] <<= 1;
        }
    }

    fn sne_reg(&mut self, vx: u8, vy: u8) {
        dbg_instruction!("SNE {}, {}", Reg(vx), Reg(vy));
        if self.regs[vx] != self.regs[vy] {
            self.regs.pc += 2;
        }
    }

    fn ld_addr(&mut self, addr: u16) {
        dbg_instruction!("LD I, {}", Addr(addr));
        self.regs.i = addr;
    }

    fn jp0(&mut self, addr: u16) {
        dbg_instruction!("JP {}, {}", Reg(0), Addr(addr));
        self.regs.pc = self.regs[0u8] as u16 + addr;
    }

    fn rnd(&mut self, vx: u8, byte: u8) {
        dbg_instruction!("RND {}, {}", Reg(vx), byte);
        self.regs[vx] = self.rng.random::<u8>() & byte;
    }

    fn drw(&mut self, vx: u8, vy: u8, nibble: u8) {
        dbg_instruction!("DRW {}, {}, {}", Reg(vx), Reg(vy), nibble);
        let sprite = &self.memory[self.regs.i..self.regs.i + nibble as u16];

        let flag = self
            .screen
            .draw_sprite(self.regs[vx] as usize, self.regs[vy] as usize, sprite);

        self.regs[0xfu8] = flag as u8;
    }

    fn skp(&mut self, vx: u8) {
        dbg_instruction!("SKP {}", Reg(vx));
        // skip if key vx is pressed
        if self.key_states[self.regs[vx] as usize] {
            self.regs.advance_pc();
        }
    }

    fn skpn(&mut self, vx: u8) {
        dbg_instruction!("SKPN {}", Reg(vx));
        if !self.key_states[self.regs[vx] as usize] {
            self.regs.advance_pc();
        }
    }

    fn ld_dt(&mut self, vx: u8) {
        dbg_instruction!("LD {}, DT", Reg(vx));
        self.regs[vx] = self.regs.delay_timer;
    }

    fn ld_key(&mut self, vx: u8) {
        dbg_instruction!("LD {}, K", Reg(vx));
        // Wait for *any* key press, and store the pressed key in vx
        self.waiting_for_key = Some(vx);
    }

    fn set_dt(&mut self, vx: u8) {
        dbg_instruction!("LD DT, {}", Reg(vx));
        self.regs.delay_timer = self.regs[vx];
    }

    fn set_st(&mut self, vx: u8) {
        dbg_instruction!("LD ST, {}", Reg(vx));
        self.regs.sound_timer = self.regs[vx];
    }

    fn addi(&mut self, vx: u8) {
        dbg_instruction!("ADD I, {}", Reg(vx));
        self.regs.i += self.regs[vx] as u16;
    }

    fn ld_sprite(&mut self, vx: u8) {
        dbg_instruction!("LD F, {}", Reg(vx));
        self.regs.i = self.memory.sprite_location(self.regs[vx]);
    }

    fn ld_bcd(&mut self, vx: u8) {
        dbg_instruction!("LD B, {}", Reg(vx));
        // write v{x} as a 10**2 + b * 10 + c and store c, b, a at I, I + 1 and I + 2
        let mut number = self.regs[vx];
        let ones = number % 10;
        number /= 10;
        let tens = number % 10;
        number /= 10;
        let hundreds = number % 10;

        self.memory[self.regs.i] = hundreds;
        self.memory[self.regs.i + 1] = tens;
        self.memory[self.regs.i + 2] = ones;
    }

    fn copy_regs_to_memory(&mut self, vx: u8) {
        dbg_instruction!("LD [I], {}", Reg(vx));
        // store v0 up to (including) vx at I
        for i in 0..=vx {
            self.memory[self.regs.i + i as u16] = self.regs[i]
        }

        if self.config.use_original_address_register_behavior {
            self.regs.i += vx as u16 + 1;
        }
    }

    fn copy_memory_to_regs(&mut self, vx: u8) {
        dbg_instruction!("LD {}, [I]", Reg(vx));
        for i in 0..=vx {
            self.regs[i] = self.memory[self.regs.i + i as u16];
        }

        if self.config.use_original_address_register_behavior {
            self.regs.i += vx as u16 + 1;
        }
    }
}

// Debugging utils
struct Reg(u8);

impl Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "V{}", self.0)
    }
}

struct Addr(u16);

impl Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:0x}", self.0)
    }
}
