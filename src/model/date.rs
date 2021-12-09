pub struct Date {
    d: u32, // days since UNIX epoch (0 = 1970-01-01)
}

impl From<u32> for Date {
    fn from(d: u32) -> Self {
        Self { d: d }
    }
}

impl From<Date> for u32 {
    fn from(inst: Date) -> u32 {
        inst.d
    }
}