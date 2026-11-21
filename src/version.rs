use std::cmp::Ordering;

/// Parsed Initializr version: numbers plus a qualifier rank (M < RC < SNAPSHOT < release).
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    numbers: [u64; 3],
    qualifier: (u8, u64),
}

fn parse(v: &str) -> Version {
    let v = v.trim();
    let split = v
        .char_indices()
        .filter(|&(_, c)| c == '-' || c == '.')
        .map(|(i, _)| i)
        .find(|&i| !v[i + 1..].starts_with(|c: char| c.is_ascii_digit()));
    let (nums, qual) = match split {
        Some(i) => (&v[..i], &v[i + 1..]),
        None => (v, ""),
    };
    let mut numbers = [0; 3];
    for (slot, n) in numbers.iter_mut().zip(nums.split('.')) {
        *slot = n.parse().unwrap_or(0);
    }
    let digits = |s: &str| s.parse().unwrap_or(0);
    let qualifier = if let Some(n) = qual.strip_prefix("RC") {
        (1, digits(n))
    } else if let Some(n) = qual.strip_prefix('M') {
        (0, digits(n))
    } else if qual.ends_with("SNAPSHOT") {
        (2, 0)
    } else {
        (3, 0)
    };
    Version { numbers, qualifier }
}

pub fn compare(a: &str, b: &str) -> Ordering {
    parse(a).cmp(&parse(b))
}

/// Initializr range syntax: a bare version means `>= version`; otherwise
/// `[`/`(` and `]`/`)` mark inclusive/exclusive bounds.
pub fn in_range(range: &str, version: &str) -> bool {
    let range = range.trim();
    let Some(open) = range.chars().next().filter(|c| *c == '[' || *c == '(') else {
        return compare(version, range) != Ordering::Less;
    };
    let close = range.chars().last().unwrap_or(')');
    let inner = &range[1..range.len() - 1];
    let (low, high) = inner.split_once(',').unwrap_or((inner, ""));
    let low_ok = match compare(version, low) {
        Ordering::Greater => true,
        Ordering::Equal => open == '[',
        Ordering::Less => false,
    };
    let high_ok = high.trim().is_empty()
        || match compare(version, high) {
            Ordering::Less => true,
            Ordering::Equal => close == ']',
            Ordering::Greater => false,
        };
    low_ok && high_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use Ordering::*;

    #[test]
    fn compare_orders_qualifiers() {
        assert_eq!(compare("4.2.0", "4.2.1-SNAPSHOT"), Less);
        assert_eq!(compare("4.3.0-M1", "4.3.0-RC1"), Less);
        assert_eq!(compare("4.3.0-RC1", "4.3.0-SNAPSHOT"), Less);
        assert_eq!(compare("4.3.0-SNAPSHOT", "4.3.0"), Less);
        assert_eq!(compare("3.3.4.RELEASE", "3.3.4"), Equal);
    }

    #[test]
    fn ranges() {
        assert!(in_range("4.0.0", "4.2.0"));
        assert!(!in_range("4.0.0", "3.5.0"));
        assert!(in_range("[4.0.0,4.3.0-M1)", "4.2.0"));
        assert!(!in_range("[4.0.0,4.3.0-M1)", "4.3.0"));
        assert!(!in_range("[4.0.0,4.1.0-M1)", "4.1.0-M1"));
        assert!(in_range("(4.0.0,4.2.0]", "4.2.0"));
    }
}
