//! "Did you mean" suggestions for misspelled names.

/// Returns a help line suggesting the candidates closest to `name`, or
/// `None` when none is close.
///
/// A candidate is close when its Damerau-Levenshtein distance to `name`,
/// with transpositions of adjacent characters, is at most a third of the
/// length of `name`, and at least one. At most three candidates are
/// suggested, the closest first and in alphabetical order on ties.
pub(crate) fn did_you_mean<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let limit = (name.chars().count() / 3).max(1);
    let mut found: Vec<(usize, &str)> = candidates
        .into_iter()
        .filter(|c| *c != name)
        .filter_map(|c| distance(name, c, limit).map(|d| (d, c)))
        .collect();
    found.sort_unstable();
    found.dedup();
    found.truncate(3);
    let names: Vec<String> = found.iter().map(|(_, c)| format!("`{c}`")).collect();
    match names.as_slice() {
        [] => None,
        [one] => Some(format!("did you mean {one}?")),
        [rest @ .., last] => Some(format!("did you mean {} or {last}?", rest.join(", "))),
    }
}

/// Returns the distance between `a` and `b`, or `None` when it exceeds
/// `limit`.
fn distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }
    let width = b.len() + 1;
    let mut rows = vec![vec![0usize; width]; 3];
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        let (cur, prev, prev2) = (i % 3, (i + 2) % 3, (i + 1) % 3);
        rows[cur][0] = i;
        let mut best = i;
        for j in 1..width {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (rows[prev][j] + 1)
                .min(rows[cur][j - 1] + 1)
                .min(rows[prev][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(rows[prev2][j - 2] + 1);
            }
            rows[cur][j] = d;
            best = best.min(d);
        }
        if best > limit {
            return None;
        }
    }
    let d = rows[a.len() % 3][b.len()];
    (d <= limit).then_some(d)
}

#[cfg(test)]
#[path = "suggest_tests.rs"]
mod tests;
