/// 轻量语义化版本比较，覆盖 @deepseek-ai/dsh 实际使用的格式：0.1.5-rc.2 / 0.1.6-alpha.1
pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a_core, a_pre) = split_version(a);
    let (b_core, b_pre) = split_version(b);

    let ord = compare_numeric_list(&a_core, &b_core);
    if ord != Ordering::Equal {
        return ord;
    }

    match (a_pre.is_empty(), b_pre.is_empty()) {
        (true, true) => Ordering::Equal,
        // 无预发布段 > 有预发布段
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => compare_prerelease(&a_pre, &b_pre),
    }
}

fn split_version(v: &str) -> (Vec<u64>, String) {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, p),
        None => (v, ""),
    };
    let core = core
        .split('.')
        .map(|s| s.parse::<u64>().unwrap_or(0))
        .collect();
    (core, pre.to_string())
}

fn compare_numeric_list(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

fn compare_prerelease(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut as_ = a.split('.');
    let mut bs = b.split('.');
    loop {
        match (as_.next(), bs.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let xn = x.parse::<u64>();
                let yn = y.parse::<u64>();
                let ord = match (xn, yn) {
                    // 数字段 < 字母段（semver 规则），例如 2 < alpha
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::compare;
    use std::cmp::Ordering::*;

    #[test]
    fn ordering() {
        assert_eq!(compare("0.1.5-rc.2", "0.1.5-rc.1"), Greater);
        assert_eq!(compare("0.1.6-alpha.1", "0.1.5-rc.2"), Greater);
        assert_eq!(compare("0.1.5", "0.1.5-rc.2"), Greater);
        assert_eq!(compare("0.1.10", "0.1.9"), Greater);
        assert_eq!(compare("0.1.5-rc.2", "0.1.5-alpha.2"), Greater);
        assert_eq!(compare("1.0.0", "0.9.9"), Greater);
        assert_eq!(compare("0.1.5-rc.2", "0.1.5-rc.2"), Equal);
    }
}
