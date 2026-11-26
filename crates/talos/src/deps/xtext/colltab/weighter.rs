use super::collelem::Elem;

pub trait Weighter {
    fn start(&self, p: usize, b: &[u8]) -> usize;

    fn start_string(&self, p: usize, s: &[u8]) -> usize;

    fn append_next(&self, buf: &mut Vec<Elem>, s: &[u8]) -> usize;

    fn append_next_string(&self, buf: &mut Vec<Elem>, s: &[u8]) -> usize;

    fn domain(&self) -> Vec<String>;

    fn top(&self) -> u32;
}
