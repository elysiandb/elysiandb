use super::tables::EXCEPTIONS;
use super::trieval::{CCC_BREAK, CCC_MASK, CCC_ZERO, EXCEPTION_BIT, EXCEPTION_SHIFT, Info};

impl Info {
    pub(super) fn ccc_val(self) -> Info {
        if self.0 & EXCEPTION_BIT != 0 {
            return Info(u16::from(EXCEPTIONS[usize::from(self.0 >> EXCEPTION_SHIFT)]) & CCC_MASK);
        }
        Info(self.0 & CCC_MASK)
    }

    pub(super) fn ccc_type(self) -> Info {
        let ccc = self.ccc_val();
        if ccc <= CCC_ZERO {
            return CCC_ZERO;
        }
        ccc
    }

    pub(super) fn is_break(self) -> bool {
        self.ccc_val() == CCC_BREAK
    }
}
