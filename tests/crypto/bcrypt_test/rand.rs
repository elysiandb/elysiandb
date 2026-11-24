use elysiandb::crypto::rand::os_random;

#[test]
fn os_random_fills_buffers_beyond_one_chunk() {
    for len in [0usize, 1, 16, 255, 256, 257, 512, 1000] {
        let mut a = vec![0u8; len];
        let mut b = vec![0u8; len];
        os_random(&mut a).unwrap();
        os_random(&mut b).unwrap();
        if len >= 16 {
            assert_ne!(a, b, "{len}");
            assert!(a.iter().any(|&x| x != 0), "{len}");
            let tail = &a[len - 16..];
            assert!(tail.iter().any(|&x| x != 0), "{len}");
        }
    }
}
