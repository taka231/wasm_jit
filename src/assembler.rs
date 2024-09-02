#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Register64 {
    Rax,
    Rcx,
    Rdx,
    Rbx,
    Rsp,
    Rbp,
    Rsi,
    Rdi,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Register32 {
    Eax,
    Ecx,
    Edx,
    Ebx,
    Esp,
    Ebp,
    Esi,
    Edi,
    R8d,
    R9d,
    R10d,
    R11d,
    R12d,
    R13d,
    R14d,
    R15d,
}

#[derive(Debug, Clone)]
pub struct Addressing<Reg> {
    pub base: Reg,
    pub offset: i32,
}

impl Addressing<Register64> {
    pub fn with_offset(self, offset: i32) -> Self {
        Self { offset, ..self }
    }

    fn to_code(&self, reg_opcode: u8) -> Vec<u8> {
        use Register64::*;
        let mut code = vec![];
        let number = self.base.number();
        if self.offset == 0 {
            match self.base {
                Rsp | R12 => {
                    code.push(mod_rm(0, reg_opcode, 4));
                    code.push(sib(0, 4, 4))
                }
                Rbp | R13 => {
                    code.push(mod_rm(1, reg_opcode, number));
                    code.push(0);
                }
                _ => code.push(mod_rm(0, reg_opcode, number)),
            }
        } else if (i8::MIN as i32..=i8::MAX as i32).contains(&self.offset) {
            match self.base {
                Rsp | R12 => {
                    code.push(mod_rm(1, reg_opcode, 4));
                    code.push(sib(0, 4, 4));
                    code.push(self.offset as u8);
                }
                _ => {
                    code.push(mod_rm(1, reg_opcode, number));
                    code.push(self.offset as u8);
                }
            }
        } else {
            match self.base {
                Rsp | R12 => {
                    code.push(mod_rm(2, reg_opcode, 4));
                    code.push(sib(0, 4, 4));
                    code.extend_from_slice(&self.offset.to_le_bytes());
                }
                _ => {
                    code.push(mod_rm(2, reg_opcode, number));
                    code.extend_from_slice(&self.offset.to_le_bytes());
                }
            }
        }
        code
    }
}

fn rex(w: bool, r: bool, x: bool, b: bool) -> u8 {
    0x40 | (w as u8) << 3 | (r as u8) << 2 | (x as u8) << 1 | b as u8
}

fn mod_rm(mod_: u8, reg: u8, rm: u8) -> u8 {
    ((mod_ & 3) << 6) | ((reg & 7) << 3) | (rm & 7)
}

fn sib(scale: u8, index: u8, base: u8) -> u8 {
    ((scale & 3) << 6) | ((index & 7) << 3) | (base & 7)
}

impl Register64 {
    pub fn number(&self) -> u8 {
        use Register64::*;
        match self {
            Rax => 0,
            Rcx => 1,
            Rdx => 2,
            Rbx => 3,
            Rsp => 4,
            Rbp => 5,
            Rsi => 6,
            Rdi => 7,
            R8 => 8,
            R9 => 9,
            R10 => 10,
            R11 => 11,
            R12 => 12,
            R13 => 13,
            R14 => 14,
            R15 => 15,
        }
    }

    pub fn with_offset(self, offset: i32) -> Addressing<Self> {
        Addressing { base: self, offset }
    }

    pub fn to_mem(self) -> Addressing<Self> {
        Addressing {
            base: self,
            offset: 0,
        }
    }
}

impl Register32 {
    pub fn number(&self) -> u8 {
        use Register32::*;
        match self {
            Eax => 0,
            Ecx => 1,
            Edx => 2,
            Ebx => 3,
            Esp => 4,
            Ebp => 5,
            Esi => 6,
            Edi => 7,
            R8d => 8,
            R9d => 9,
            R10d => 10,
            R11d => 11,
            R12d => 12,
            R13d => 13,
            R14d => 14,
            R15d => 15,
        }
    }
}

pub trait Push {
    fn push(self) -> Vec<u8>;
}

impl Push for Register64 {
    fn push(self) -> Vec<u8> {
        let mut code = vec![];
        let number = self.number();
        if number < 8 {
            code.push(0x50 + number);
        } else {
            code.push(0x41);
            code.push(0x50 + number - 8);
        }
        code
    }
}

impl Push for i32 {
    fn push(self) -> Vec<u8> {
        if (i8::MIN as i32..=i8::MAX as i32).contains(&self) {
            vec![0x6a, self as u8]
        } else {
            [&[0x68], &self.to_le_bytes()[..]].concat()
        }
    }
}

impl Push for Addressing<Register64> {
    fn push(self) -> Vec<u8> {
        let mut code = vec![];
        let number = self.base.number();
        if number >= 8 {
            code.push(0x41);
        }
        code.push(0xff);
        code.extend_from_slice(&self.to_code(0b110));
        code
    }
}

pub trait Pop {
    fn pop(self) -> Vec<u8>;
}

impl Pop for Register64 {
    fn pop(self) -> Vec<u8> {
        let mut code = vec![];
        let number = self.number();
        if number < 8 {
            code.push(0x58 + number);
        } else {
            code.push(0x41);
            code.push(0x58 + number - 8);
        }
        code
    }
}

pub trait Mov<Src> {
    fn mov(self, src: Src) -> Vec<u8>;
}

impl Mov<Register64> for Register64 {
    fn mov(self, src: Register64) -> Vec<u8> {
        let mut code = vec![];
        let dest_number = self.number();
        let src_number = src.number();

        code.push(rex(true, src_number >= 8, false, dest_number >= 8));
        code.push(0x89);
        code.push(mod_rm(3, src_number, dest_number));
        code
    }
}

impl Mov<i32> for Register32 {
    fn mov(self, src: i32) -> Vec<u8> {
        let mut code = vec![];
        let number = self.number();
        if number < 8 {
            code.push(0xb8 + number);
        } else {
            code.push(0x41);
            code.push(0xb8 + number - 8);
        }
        code.extend_from_slice(&src.to_le_bytes());
        code
    }
}

impl Mov<i64> for Register64 {
    fn mov(self, src: i64) -> Vec<u8> {
        let mut code = vec![];
        let number = self.number();
        if number < 8 {
            code.push(0x48);
            code.push(0xb8 + number);
        } else {
            code.push(0x49);
            code.push(0xb8 + number - 8);
        }
        code.extend_from_slice(&src.to_le_bytes());
        code
    }
}

impl Mov<Addressing<Register64>> for Register64 {
    fn mov(self, src: Addressing<Register64>) -> Vec<u8> {
        let mut code = vec![];
        let dest_number = self.number();
        let src_number = src.base.number();
        code.push(rex(true, dest_number >= 8, false, src_number >= 8));
        code.push(0x8b);
        code.extend_from_slice(&src.to_code(dest_number));
        code
    }
}

impl Mov<Register64> for Addressing<Register64> {
    fn mov(self, src: Register64) -> Vec<u8> {
        let mut code = vec![];
        let dest_number = self.base.number();
        let src_number = src.number();
        code.push(rex(true, src_number >= 8, false, dest_number >= 8));
        code.push(0x89);
        code.extend_from_slice(&self.to_code(src_number));
        code
    }
}

pub trait Call {
    fn call(self) -> Vec<u8>;
}

impl Call for Register64 {
    fn call(self) -> Vec<u8> {
        let mut code = vec![];
        let number = self.number();
        if number < 8 {
            code.push(0xff);
            code.push(0xd0 + number);
        } else {
            code.push(0x41);
            code.push(0xff);
            code.push(0xd0 + number - 8);
        }
        code
    }
}

pub fn ret() -> Vec<u8> {
    vec![0xc3]
}

pub trait Add<Src> {
    fn add(self, src: Src) -> Vec<u8>;
}

impl Add<Register64> for Register64 {
    fn add(self, src: Register64) -> Vec<u8> {
        let mut code = vec![];
        let dest_number = self.number();
        let src_number = src.number();
        code.push(rex(true, src_number >= 8, false, dest_number >= 8));
        code.push(0x01);
        code.push(mod_rm(3, src_number, dest_number));
        code
    }
}

impl Add<Register32> for Register32 {
    fn add(self, src: Register32) -> Vec<u8> {
        let mut code = vec![];
        let dest_number = self.number();
        let src_number = src.number();
        code.push(rex(false, src_number >= 8, false, dest_number >= 8));
        code.push(0x01);
        code.push(mod_rm(3, src_number, dest_number));
        code
    }
}
