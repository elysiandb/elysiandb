pub fn clean_path(path: &[u8]) -> &[u8] {
    path.strip_suffix(b".").unwrap_or(path)
}

pub fn get_optional_paths(path: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut path = path.to_string();
    let mut start = 0usize;
    'walk: loop {
        if start >= path.len() {
            return paths;
        }
        let c = path.as_bytes()[start];
        start += 1;
        if c != b'{' {
            continue;
        }

        let mut new_path = String::new();
        let mut has_regex = false;
        let mut question_mark_index: Option<usize> = None;
        let mut brackets = 0;

        let snapshot = path.as_bytes()[start..].to_vec();
        for (end, &c) in snapshot.iter().enumerate() {
            match c {
                b'{' => brackets += 1,
                b'}' => {
                    if brackets > 0 {
                        brackets -= 1;
                        continue;
                    }
                    let Some(q) = question_mark_index else {
                        continue 'walk;
                    };
                    let end = end + 1;
                    new_path.push_str(&path[q + 1..start + end]);
                    path = format!("{}{}", &path[..q], &path[q + 1..]);
                    paths.push(new_path);
                    start += end - 1;
                    continue 'walk;
                }
                b':' => has_regex = true,
                b'?' => {
                    if has_regex {
                        continue;
                    }
                    let q = start + end;
                    question_mark_index = Some(q);
                    new_path.push_str(&path[..q]);
                    let prefix = &path[..start - 2];
                    if prefix.is_empty() {
                        paths.push("/".to_string());
                    } else if !paths.iter().any(|p| p == prefix) {
                        paths.push(prefix.to_string());
                    }
                }
                _ => {}
            }
        }
    }
}
