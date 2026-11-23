#[derive(Clone, Copy, PartialEq, Eq)]
enum SortedHint {
    Unknown,
    Increasing,
    Decreasing,
}

struct LessSwap<'a, T, F> {
    data: &'a mut [T],
    less: F,
}

impl<T, F: FnMut(&T, &T) -> bool> LessSwap<'_, T, F> {
    fn less(&mut self, i: usize, j: usize) -> bool {
        (self.less)(&self.data[i], &self.data[j])
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.data.swap(i, j);
    }
}

fn bits_len(x: usize) -> usize {
    (usize::BITS - x.leading_zeros()) as usize
}

pub fn slice<T, F: FnMut(&T, &T) -> bool>(x: &mut [T], less: F) {
    let length = x.len();
    let limit = bits_len(length);
    let mut data = LessSwap { data: x, less };
    pdqsort_func(&mut data, 0, length, limit);
}

pub fn slice_stable<T, F: FnMut(&T, &T) -> bool>(x: &mut [T], less: F) {
    let n = x.len();
    let mut data = LessSwap { data: x, less };
    stable_func(&mut data, n);
}

pub fn slice_is_sorted<T, F: FnMut(&T, &T) -> bool>(x: &[T], mut less: F) -> bool {
    (1..x.len()).all(|i| !less(&x[i], &x[i - 1]))
}

fn insertion_sort_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && data.less(j, j - 1) {
            data.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn sift_down_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    lo: usize,
    hi: usize,
    first: usize,
) {
    let mut root = lo;
    loop {
        let mut child = 2 * root + 1;
        if child >= hi {
            break;
        }
        if child + 1 < hi && data.less(first + child, first + child + 1) {
            child += 1;
        }
        if !data.less(first + root, first + child) {
            return;
        }
        data.swap(first + root, first + child);
        root = child;
    }
}

fn heap_sort_func<T, F: FnMut(&T, &T) -> bool>(data: &mut LessSwap<'_, T, F>, a: usize, b: usize) {
    let first = a;
    let lo = 0;
    let hi = b - a;
    let mut i = (hi as isize - 1) / 2;
    while i >= 0 {
        sift_down_func(data, i as usize, hi, first);
        i -= 1;
    }
    let mut i = hi as isize - 1;
    while i >= 0 {
        data.swap(first, first + i as usize);
        sift_down_func(data, lo, i as usize, first);
        i -= 1;
    }
}

fn pdqsort_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    mut a: usize,
    mut b: usize,
    mut limit: usize,
) {
    const MAX_INSERTION: usize = 12;
    let mut was_balanced = true;
    let mut was_partitioned = true;
    loop {
        let length = b - a;
        if length <= MAX_INSERTION {
            insertion_sort_func(data, a, b);
            return;
        }
        if limit == 0 {
            heap_sort_func(data, a, b);
            return;
        }
        if !was_balanced {
            break_patterns_func(data, a, b);
            limit -= 1;
        }
        let (mut pivot, mut hint) = choose_pivot_func(data, a, b);
        if hint == SortedHint::Decreasing {
            reverse_range_func(data, a, b);
            pivot = (b - 1) - (pivot - a);
            hint = SortedHint::Increasing;
        }
        if was_balanced
            && was_partitioned
            && hint == SortedHint::Increasing
            && partial_insertion_sort_func(data, a, b)
        {
            return;
        }
        if a > 0 && !data.less(a - 1, pivot) {
            let mid = partition_equal_func(data, a, b, pivot);
            a = mid;
            continue;
        }
        let (mid, already_partitioned) = partition_func(data, a, b, pivot);
        was_partitioned = already_partitioned;
        let (left_len, right_len) = (mid - a, b - mid);
        let balance_threshold = length / 8;
        if left_len < right_len {
            was_balanced = left_len >= balance_threshold;
            pdqsort_func(data, a, mid, limit);
            a = mid + 1;
        } else {
            was_balanced = right_len >= balance_threshold;
            pdqsort_func(data, mid + 1, b, limit);
            b = mid;
        }
    }
}

fn partition_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
    pivot: usize,
) -> (usize, bool) {
    data.swap(a, pivot);
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    while i <= j && data.less(i as usize, a) {
        i += 1;
    }
    while i <= j && !data.less(j as usize, a) {
        j -= 1;
    }
    if i > j {
        data.swap(j as usize, a);
        return (j as usize, true);
    }
    data.swap(i as usize, j as usize);
    i += 1;
    j -= 1;
    loop {
        while i <= j && data.less(i as usize, a) {
            i += 1;
        }
        while i <= j && !data.less(j as usize, a) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    data.swap(j as usize, a);
    (j as usize, false)
}

fn partition_equal_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
    pivot: usize,
) -> usize {
    data.swap(a, pivot);
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    loop {
        while i <= j && !data.less(a, i as usize) {
            i += 1;
        }
        while i <= j && data.less(a, j as usize) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    i as usize
}

fn partial_insertion_sort_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
) -> bool {
    const MAX_STEPS: usize = 5;
    const SHORTEST_SHIFTING: usize = 50;
    let mut i = a + 1;
    for _ in 0..MAX_STEPS {
        while i < b && !data.less(i, i - 1) {
            i += 1;
        }
        if i == b {
            return true;
        }
        if b - a < SHORTEST_SHIFTING {
            return false;
        }
        data.swap(i, i - 1);
        if i - a >= 2 {
            let mut j = i - 1;
            while j >= 1 {
                if !data.less(j, j - 1) {
                    break;
                }
                data.swap(j, j - 1);
                j -= 1;
            }
        }
        if b - i >= 2 {
            let mut j = i + 1;
            while j < b {
                if !data.less(j, j - 1) {
                    break;
                }
                data.swap(j, j - 1);
                j += 1;
            }
        }
    }
    false
}

fn break_patterns_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
) {
    let length = b - a;
    if length >= 8 {
        let mut random = length as u64;
        let modulus = 1usize << bits_len(length);
        let mut idx = a + (length / 4) * 2 - 1;
        while idx <= a + (length / 4) * 2 + 1 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let mut other = (random as usize) & (modulus - 1);
            if other >= length {
                other -= length;
            }
            data.swap(idx, a + other);
            idx += 1;
        }
    }
}

fn choose_pivot_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
) -> (usize, SortedHint) {
    const SHORTEST_NINTHER: usize = 50;
    const MAX_SWAPS: usize = 4 * 3;
    let l = b - a;
    let mut swaps = 0;
    let mut i = a + l / 4;
    let mut j = a + l / 4 * 2;
    let mut k = a + l / 4 * 3;
    if l >= 8 {
        if l >= SHORTEST_NINTHER {
            i = median_adjacent_func(data, i, &mut swaps);
            j = median_adjacent_func(data, j, &mut swaps);
            k = median_adjacent_func(data, k, &mut swaps);
        }
        j = median_func(data, i, j, k, &mut swaps);
    }
    match swaps {
        0 => (j, SortedHint::Increasing),
        MAX_SWAPS => (j, SortedHint::Decreasing),
        _ => (j, SortedHint::Unknown),
    }
}

fn order2_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
    swaps: &mut usize,
) -> (usize, usize) {
    if data.less(b, a) {
        *swaps += 1;
        return (b, a);
    }
    (a, b)
}

fn median_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
    c: usize,
    swaps: &mut usize,
) -> usize {
    let (a, b) = order2_func(data, a, b, swaps);
    let (b, _c) = order2_func(data, b, c, swaps);
    let (_a, b) = order2_func(data, a, b, swaps);
    b
}

fn median_adjacent_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    swaps: &mut usize,
) -> usize {
    median_func(data, a - 1, a, a + 1, swaps)
}

fn reverse_range_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
) {
    let mut i = a;
    let mut j = b - 1;
    while i < j {
        data.swap(i, j);
        i += 1;
        j -= 1;
    }
}

fn swap_range_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    b: usize,
    n: usize,
) {
    for i in 0..n {
        data.swap(a + i, b + i);
    }
}

fn stable_func<T, F: FnMut(&T, &T) -> bool>(data: &mut LessSwap<'_, T, F>, n: usize) {
    let mut block_size = 20;
    let (mut a, mut b) = (0, block_size);
    while b <= n {
        insertion_sort_func(data, a, b);
        a = b;
        b += block_size;
    }
    insertion_sort_func(data, a, n);
    while block_size < n {
        a = 0;
        b = 2 * block_size;
        while b <= n {
            sym_merge_func(data, a, a + block_size, b);
            a = b;
            b += 2 * block_size;
        }
        let m = a + block_size;
        if m < n {
            sym_merge_func(data, a, m, n);
        }
        block_size *= 2;
    }
}

fn sym_merge_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    m: usize,
    b: usize,
) {
    if m - a == 1 {
        let mut i = m;
        let mut j = b;
        while i < j {
            let h = (i + j) >> 1;
            if data.less(h, a) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        let mut k = a;
        while k + 1 < i {
            data.swap(k, k + 1);
            k += 1;
        }
        return;
    }
    if b - m == 1 {
        let mut i = a;
        let mut j = m;
        while i < j {
            let h = (i + j) >> 1;
            if !data.less(m, h) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        let mut k = m;
        while k > i {
            data.swap(k, k - 1);
            k -= 1;
        }
        return;
    }
    let mid = (a + b) >> 1;
    let n = mid + m;
    let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
    let p = n - 1;
    while start < r {
        let c = (start + r) >> 1;
        if !data.less(p - c, c) {
            start = c + 1;
        } else {
            r = c;
        }
    }
    let end = n - start;
    if start < m && m < end {
        rotate_func(data, start, m, end);
    }
    if a < start && start < mid {
        sym_merge_func(data, a, start, mid);
    }
    if mid < end && end < b {
        sym_merge_func(data, mid, end, b);
    }
}

fn rotate_func<T, F: FnMut(&T, &T) -> bool>(
    data: &mut LessSwap<'_, T, F>,
    a: usize,
    m: usize,
    b: usize,
) {
    let mut i = m - a;
    let mut j = b - m;
    while i != j {
        if i > j {
            swap_range_func(data, m - i, m, j);
            i -= j;
        } else {
            swap_range_func(data, m - i, m + j - i, i);
            j -= i;
        }
    }
    swap_range_func(data, m - i, m, i);
}
