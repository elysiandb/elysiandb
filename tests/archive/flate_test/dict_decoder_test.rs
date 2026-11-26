use elysiandb::archive::flate::DictDecoder;

const ABC: &str = "ABC\n";
const FOX: &str = "The quick brown fox jumped over the lazy dog!\n";
const POEM: &str = "The Road Not Taken\nRobert Frost\n\
\n\
Two roads diverged in a yellow wood,\n\
And sorry I could not travel both\n\
And be one traveler, long I stood\n\
And looked down one as far as I could\n\
To where it bent in the undergrowth;\n\
\n\
Then took the other, as just as fair,\n\
And having perhaps the better claim,\n\
Because it was grassy and wanted wear;\n\
Though as for that the passing there\n\
Had worn them really about the same,\n\
\n\
And both that morning equally lay\n\
In leaves no step had trodden black.\n\
Oh, I kept the first for another day!\n\
Yet knowing how way leads on to way,\n\
I doubted if I should ever come back.\n\
\n\
I shall be telling this with a sigh\n\
Somewhere ages and ages hence:\n\
Two roads diverged in a wood, and I-\n\
I took the one less traveled by,\n\
And that has made all the difference.\n";

const POEM_REFS: &[(usize, usize)] = &[
    (0, 38),
    (33, 3),
    (0, 48),
    (79, 3),
    (0, 11),
    (34, 5),
    (0, 6),
    (23, 7),
    (0, 8),
    (50, 3),
    (0, 2),
    (69, 3),
    (34, 5),
    (0, 4),
    (97, 3),
    (0, 4),
    (43, 5),
    (0, 6),
    (7, 4),
    (88, 7),
    (0, 12),
    (80, 3),
    (0, 2),
    (141, 4),
    (0, 1),
    (196, 3),
    (0, 3),
    (157, 3),
    (0, 6),
    (181, 3),
    (0, 2),
    (23, 3),
    (77, 3),
    (28, 5),
    (128, 3),
    (110, 4),
    (70, 3),
    (0, 4),
    (85, 6),
    (0, 2),
    (182, 6),
    (0, 4),
    (133, 3),
    (0, 7),
    (47, 5),
    (0, 20),
    (112, 5),
    (0, 1),
    (58, 3),
    (0, 8),
    (59, 3),
    (0, 4),
    (173, 3),
    (0, 5),
    (114, 3),
    (0, 4),
    (92, 5),
    (0, 2),
    (71, 3),
    (0, 2),
    (76, 5),
    (0, 1),
    (46, 3),
    (96, 4),
    (130, 4),
    (0, 3),
    (360, 3),
    (0, 3),
    (178, 5),
    (0, 7),
    (75, 3),
    (0, 3),
    (45, 6),
    (0, 6),
    (299, 6),
    (180, 3),
    (70, 6),
    (0, 1),
    (48, 3),
    (66, 4),
    (0, 3),
    (47, 5),
    (0, 9),
    (325, 3),
    (0, 1),
    (359, 3),
    (318, 3),
    (0, 2),
    (199, 3),
    (0, 1),
    (344, 3),
    (0, 3),
    (248, 3),
    (0, 10),
    (310, 3),
    (0, 3),
    (93, 6),
    (0, 3),
    (252, 3),
    (157, 4),
    (0, 2),
    (273, 5),
    (0, 14),
    (99, 4),
    (0, 1),
    (464, 4),
    (0, 2),
    (92, 4),
    (495, 3),
    (0, 1),
    (322, 4),
    (16, 4),
    (0, 3),
    (402, 3),
    (0, 2),
    (237, 4),
    (0, 2),
    (432, 4),
    (0, 1),
    (483, 5),
    (0, 2),
    (294, 4),
    (0, 2),
    (306, 3),
    (113, 5),
    (0, 1),
    (26, 4),
    (164, 3),
    (488, 4),
    (0, 1),
    (542, 3),
    (248, 6),
    (0, 5),
    (205, 3),
    (0, 8),
    (48, 3),
    (449, 6),
    (0, 2),
    (192, 3),
    (328, 4),
    (9, 5),
    (433, 3),
    (0, 3),
    (622, 25),
    (615, 5),
    (46, 5),
    (0, 2),
    (104, 3),
    (475, 10),
    (549, 3),
    (0, 4),
    (597, 8),
    (314, 3),
    (0, 1),
    (473, 6),
    (317, 5),
    (0, 1),
    (400, 3),
    (0, 3),
    (109, 3),
    (151, 3),
    (48, 4),
    (0, 4),
    (125, 3),
    (108, 3),
    (0, 2),
];

struct Harness {
    dd: DictDecoder,
    got: Vec<u8>,
}

impl Harness {
    fn flush(&mut self) {
        let range = self.dd.read_flush();
        self.got.extend_from_slice(self.dd.bytes(range));
    }

    fn write_copy(&mut self, dist: usize, mut length: usize) {
        while length > 0 {
            let mut cnt = self.dd.try_write_copy(dist, length);
            if cnt == 0 {
                cnt = self.dd.write_copy(dist, length);
            }

            length -= cnt;
            if self.dd.avail_write() == 0 {
                self.flush();
            }
        }
    }

    fn write_string(&mut self, mut s: &[u8]) {
        while !s.is_empty() {
            let slice = self.dd.write_slice();
            let cnt = slice.len().min(s.len());
            slice[..cnt].copy_from_slice(&s[..cnt]);
            s = &s[cnt..];
            self.dd.write_mark(cnt);
            if self.dd.avail_write() == 0 {
                self.flush();
            }
        }
    }
}

#[test]
fn dict_decoder() {
    let mut h = Harness {
        dd: DictDecoder::default(),
        got: Vec::new(),
    };
    let mut want: Vec<u8> = Vec::new();
    h.dd.init(1 << 11, None);

    h.write_string(b".");
    want.push(b'.');

    let mut s = POEM.as_bytes();
    for &(dist, length) in POEM_REFS {
        if dist == 0 {
            h.write_string(&s[..length]);
        } else {
            h.write_copy(dist, length);
        }
        s = &s[length..];
    }
    want.extend_from_slice(POEM.as_bytes());

    h.write_copy(h.dd.hist_size(), 33);
    let head = want[..33].to_vec();
    want.extend_from_slice(&head);

    h.write_string(ABC.as_bytes());
    h.write_copy(ABC.len(), 59 * ABC.len());
    want.extend_from_slice(ABC.repeat(60).as_bytes());

    h.write_string(FOX.as_bytes());
    h.write_copy(FOX.len(), 9 * FOX.len());
    want.extend_from_slice(FOX.repeat(10).as_bytes());

    h.write_string(b".");
    h.write_copy(1, 9);
    want.extend_from_slice(".".repeat(10).as_bytes());

    let upper = POEM.to_uppercase();
    h.write_string(upper.as_bytes());
    h.write_copy(POEM.len(), 7 * POEM.len());
    want.extend_from_slice(upper.repeat(8).as_bytes());

    let hist = h.dd.hist_size();
    h.write_copy(hist, 10);
    let tail = want[want.len() - hist..][..10].to_vec();
    want.extend_from_slice(&tail);

    h.flush();
    assert!(h.got == want, "final string mismatch");
}
