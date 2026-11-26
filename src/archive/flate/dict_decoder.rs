#[doc(hidden)]
#[derive(Default)]
pub struct DictDecoder {
    hist: Vec<u8>,
    wr_pos: usize,
    rd_pos: usize,
    full: bool,
}

impl DictDecoder {
    pub fn init(&mut self, size: usize, dict: Option<&[u8]>) {
        let mut hist = std::mem::take(&mut self.hist);
        *self = DictDecoder::default();
        hist.resize(size, 0);
        self.hist = hist;

        let mut dict = dict.unwrap_or(&[]);
        if dict.len() > self.hist.len() {
            dict = &dict[dict.len() - self.hist.len()..];
        }
        self.hist[..dict.len()].copy_from_slice(dict);
        self.wr_pos = dict.len();
        if self.wr_pos == self.hist.len() {
            self.wr_pos = 0;
            self.full = true;
        }
        self.rd_pos = self.wr_pos;
    }

    pub fn hist_size(&self) -> usize {
        if self.full {
            return self.hist.len();
        }
        self.wr_pos
    }

    pub fn avail_read(&self) -> usize {
        self.wr_pos - self.rd_pos
    }

    pub fn avail_write(&self) -> usize {
        self.hist.len() - self.wr_pos
    }

    pub fn write_slice(&mut self) -> &mut [u8] {
        &mut self.hist[self.wr_pos..]
    }

    pub fn write_mark(&mut self, cnt: usize) {
        self.wr_pos += cnt;
    }

    pub fn write_byte(&mut self, c: u8) {
        self.hist[self.wr_pos] = c;
        self.wr_pos += 1;
    }

    pub fn write_copy(&mut self, dist: usize, length: usize) -> usize {
        let dst_base = self.wr_pos;
        let mut dst_pos = dst_base;
        let mut src_pos = dst_pos as isize - dist as isize;
        let mut end_pos = dst_pos + length;
        if end_pos > self.hist.len() {
            end_pos = self.hist.len();
        }

        if src_pos < 0 {
            let src = (src_pos + self.hist.len() as isize) as usize;
            let n = (end_pos - dst_pos).min(self.hist.len() - src);
            self.hist.copy_within(src..src + n, dst_pos);
            dst_pos += n;
            src_pos = 0;
        }

        while dst_pos < end_pos {
            let src = src_pos as usize;
            let n = (end_pos - dst_pos).min(dst_pos - src);
            self.hist.copy_within(src..src + n, dst_pos);
            dst_pos += n;
        }

        self.wr_pos = dst_pos;
        dst_pos - dst_base
    }

    pub fn try_write_copy(&mut self, dist: usize, length: usize) -> usize {
        let mut dst_pos = self.wr_pos;
        let end_pos = dst_pos + length;
        if dst_pos < dist || end_pos > self.hist.len() {
            return 0;
        }
        let dst_base = dst_pos;
        let src_pos = dst_pos - dist;

        while dst_pos < end_pos {
            let n = (end_pos - dst_pos).min(dst_pos - src_pos);
            self.hist.copy_within(src_pos..src_pos + n, dst_pos);
            dst_pos += n;
        }

        self.wr_pos = dst_pos;
        dst_pos - dst_base
    }

    pub fn read_flush(&mut self) -> (usize, usize) {
        let to_read = (self.rd_pos, self.wr_pos);
        self.rd_pos = self.wr_pos;
        if self.wr_pos == self.hist.len() {
            self.wr_pos = 0;
            self.rd_pos = 0;
            self.full = true;
        }
        to_read
    }

    pub fn bytes(&self, range: (usize, usize)) -> &[u8] {
        &self.hist[range.0..range.1]
    }
}
