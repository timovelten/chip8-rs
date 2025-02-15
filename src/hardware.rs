use std::ops::{Index, IndexMut, Range};

pub struct Registers {
    /// General purpose 16-bit register, usually used to store memory addresses.
    pub i: u16,
    /// Instruction pointer. This is incremented when an instruction is fetched, and then points to
    /// the *next* instruction.
    pub pc: u16,
    /// General purpose 8-bit registers. There are 16 such registers.
    regs: [u8; 16],
    /// Call stack
    stack: Vec<u16>,
    // Delay timer. If not zero, should be decremented by one with a rate of 60 Hz;
    pub delay_timer: u8,
    // Sound timer. If not zero, should be decremented by one with a rate of 60 Hz;
    pub sound_timer: u8,
}

impl Registers {
    pub fn advance_pc(&mut self) {
        self.pc += 2;
    }

    pub fn push_addr(&mut self, addr: u16) {
        self.stack.push(addr);
    }

    pub fn pop_addr(&mut self) -> u16 {
        self.stack.pop().unwrap()
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            i: 0,
            pc: 0x200,
            stack: Default::default(),
            regs: Default::default(),
            delay_timer: 0,
            sound_timer: 0,
        }
    }
}

impl Index<u8> for Registers {
    type Output = u8;

    fn index(&self, index: u8) -> &Self::Output {
        &self.regs[index as usize]
    }
}

impl Index<usize> for Registers {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.regs[index]
    }
}

impl IndexMut<usize> for Registers {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.regs[index]
    }
}

impl IndexMut<u8> for Registers {
    fn index_mut(&mut self, index: u8) -> &mut Self::Output {
        &mut self.regs[index as usize]
    }
}

pub struct Memory {
    inner: [u8; Self::SIZE],
}

impl Memory {
    pub const PROGRAM_OFFSET: u16 = 0x200;
    pub const FONT_OFFSET: u16 = 0x100;
    pub const SIZE: usize = 4096;

    pub fn load_program(&mut self, data: &[u8]) {
        for (mem, &v) in self.inner[Self::PROGRAM_OFFSET as usize..]
            .iter_mut()
            .zip(data)
        {
            *mem = v
        }
    }

    pub fn sprite_location(&self, key: u8) -> u16 {
        Self::FONT_OFFSET + key as u16 * 5
    }

    pub fn len(&self) -> u16 {
        self.inner.len() as u16
    }
}

impl Default for Memory {
    fn default() -> Self {
        // Programs expect to find the font data at a specific point in memory, so put it there
        const CHARS: [u8; 80] = [
            0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
            0x20, 0x60, 0x20, 0x20, 0x70, // 1
            0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
            0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
            0x90, 0x90, 0xF0, 0x10, 0x10, // 4
            0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
            0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
            0xF0, 0x10, 0x20, 0x40, 0x40, // 7
            0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
            0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
            0xF0, 0x90, 0xF0, 0x90, 0x90, // A
            0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
            0xF0, 0x80, 0x80, 0x80, 0xF0, // C
            0xE0, 0x90, 0x90, 0x90, 0xE0, // D
            0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
            0xF0, 0x80, 0xF0, 0x80, 0x80, // F
        ];

        let mut mem = [0; 4096];
        mem[0x100..0x100 + CHARS.len()].copy_from_slice(&CHARS);

        // Preload a valid program (one that just loops in place): JP 0x200
        mem[0x200] = 0x12;
        mem[0x201] = 0x00;

        Self { inner: mem }
    }
}

impl Index<u16> for Memory {
    type Output = u8;

    fn index(&self, index: u16) -> &Self::Output {
        &self.inner[index as usize]
    }
}

impl IndexMut<u16> for Memory {
    fn index_mut(&mut self, index: u16) -> &mut Self::Output {
        &mut self.inner[index as usize]
    }
}

impl Index<Range<u16>> for Memory {
    type Output = [u8];

    fn index(&self, index: Range<u16>) -> &Self::Output {
        &self.inner[index.start as usize..index.end as usize]
    }
}

#[derive(Clone)]
pub struct Screen {
    pixels: [u8; Self::RESOLUTION],
}

impl Screen {
    pub const WIDTH: usize = 64;
    pub const HEIGHT: usize = 32;
    pub const RESOLUTION: usize = Self::WIDTH * Self::HEIGHT;

    pub fn new() -> Self {
        Screen {
            pixels: [0x0; Self::RESOLUTION],
        }
    }

    pub fn frame(&self) -> &[u8] {
        &self.pixels
    }

    /// We assume the given bits data represent an image of width width, and some height.
    /// Draw this image at (x, y) (so that the last bit is drawn at (x + width, y + height))
    /// by xoring the image data with the current data.
    ///
    /// Return if any pixel that was previously set, is now unset.
    pub fn draw_sprite(&mut self, x0: usize, y0: usize, sprites: &[u8]) -> bool {
        let mut ret = false;

        for (dy, sprite) in sprites.iter().enumerate() {
            let row = (0..8).rev().map(move |k: u8| (sprite >> k) & 0b1_u8);

            for (dx, p) in row.enumerate() {
                let pixel = p * 0xff;

                let (draw_x, draw_y) = ((x0 + dx) % Self::WIDTH, (y0 + dy) % Self::HEIGHT);
                let draw_at = draw_x + Self::WIDTH * draw_y;

                let prev = self.pixels[draw_at];
                self.pixels[draw_at] ^= pixel;

                ret |= prev != 0 && prev == pixel;
            }
        }

        ret
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }
}

impl Default for Screen {
    fn default() -> Self {
        Screen::new()
    }
}
