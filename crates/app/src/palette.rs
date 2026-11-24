//! Matching for the command palette: a query matches an entry when its
//! letters appear in order (case-insensitive); word starts and runs of
//! consecutive letters score higher.

/// Score of `query` against `label`, or `None` when it does not match.
/// An empty query matches everything with score 0.
pub fn score(query: &str, label: &str) -> Option<i64> {
    let query: Vec<char> = query.trim().chars().map(lower).collect();
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = label.chars().collect();
    let lowered: Vec<char> = text.iter().copied().map(lower).collect();
    // best[j]: best score with the current query letter matched at j.
    let mut best: Vec<Option<i64>> = vec![None; text.len()];
    for (i, &wanted) in query.iter().enumerate() {
        let mut next = vec![None; text.len()];
        // Best score of the previous letter at any position before j.
        let mut earlier: Option<i64> = None;
        for j in 0..text.len() {
            if lowered[j] == wanted {
                let bonus = letter_bonus(&text, j);
                next[j] = if i == 0 {
                    Some(bonus + if j == 0 { FIRST } else { 0 })
                } else {
                    let run = j
                        .checked_sub(1)
                        .and_then(|k| best[k])
                        .map(|prev| prev + bonus + RUN);
                    let gap = earlier.map(|prev| prev + bonus);
                    run.max(gap)
                };
            }
            earlier = earlier.max(best[j]);
        }
        best = next;
    }
    best.into_iter().flatten().max()
}

/// Every matched letter.
const LETTER: i64 = 1;
/// A letter right after the previous matched one.
const RUN: i64 = 8;
/// A letter that starts a word.
const WORD_START: i64 = 6;
/// The query starts at the start of the label.
const FIRST: i64 = 10;

fn letter_bonus(text: &[char], j: usize) -> i64 {
    let starts_word = match j.checked_sub(1).map(|k| text[k]) {
        None => true,
        Some(prev) => !prev.is_alphanumeric() || (prev.is_lowercase() && text[j].is_uppercase()),
    };
    LETTER + if starts_word { WORD_START } else { 0 }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Indexes of `labels` that match `query`, best first; ties keep the
/// original order.
pub fn filter<'a>(query: &str, labels: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    let mut found: Vec<(usize, i64)> = labels
        .into_iter()
        .enumerate()
        .filter_map(|(index, label)| score(query, label).map(|score| (index, score)))
        .collect();
    // The sort is stable: equal scores keep their order.
    found.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
    found.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_must_appear_in_order() {
        assert!(score("gp", "Toggle git panel").is_some());
        assert!(
            score("GIT", "Toggle git panel").is_some(),
            "case-insensitive"
        );
        assert!(score("pg", "Toggle git panel").is_none(), "order matters");
        assert!(score("gitx", "Toggle git panel").is_none());
        assert_eq!(score("", "Anything"), Some(0));
        assert_eq!(score("  ", "Anything"), Some(0), "blank is empty");
    }

    #[test]
    fn word_starts_and_runs_rank_higher() {
        // "gp" on word starts beats letters buried inside words.
        assert!(score("gp", "Toggle git panel") > score("gp", "Big apple pie"));
        // A run of consecutive letters beats scattered ones.
        assert!(score("split", "Split right") > score("split", "Settings plus little"));
        // A prefix of the label beats the same letters later.
        assert!(score("new", "New session") > score("new", "Open new tab"));
    }

    #[test]
    fn filter_orders_by_score_and_keeps_ties_stable() {
        let labels = ["Close pane", "Split right", "Settings", "Remove from split"];
        let found = filter("spl", labels);
        assert_eq!(found, [1, 3]);
        assert_eq!(filter("", labels), [0, 1, 2, 3]);
        assert_eq!(filter("zzz", labels), Vec::<usize>::new());
    }

    #[test]
    fn non_ascii_text_matches() {
        assert!(
            score("sesion", "Reanudar: Sesión de prueba").is_none(),
            "accents differ"
        );
        assert!(score("sesión", "Reanudar: Sesión de prueba").is_some());
        assert!(score("ñ", "Diseño").is_some());
    }
}
