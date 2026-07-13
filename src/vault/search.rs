use std::{collections::HashMap, fs, path::Path};

use rayon::prelude::*;
use regex::RegexBuilder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::frontmatter::{content_has_tag, extract_frontmatter, find_closing_fm};
use super::walk::md_files;
use crate::error::VaultError;

/// BM25 saturation and length-normalisation constants — the standard defaults.
const K1: f32 = 1.2;
const B: f32 = 0.75;

/// How much a term is worth depending on where in the note it occurs. A query
/// word in the title says far more about what a note is *about* than the same
/// word buried in a paragraph.
const W_FILENAME: f32 = 5.0;
const W_TAG: f32 = 4.0;
const W_HEADING: f32 = 3.0;
const W_FRONTMATTER: f32 = 2.0;
const W_BODY: f32 = 1.0;

/// Defaults chosen so a careless query can't flood the model's context.
pub const DEFAULT_LIMIT: usize = 20;
pub const DEFAULT_MAX_MATCHES_PER_FILE: usize = 3;
/// Longest snippet we emit, in characters.
const SNIPPET_CHARS: usize = 200;

/// One matching line, with its 1-based line number.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Snippet {
    pub line: usize,
    pub text: String,
}

// There used to be a `filename` field here alongside `path` — the bare stem, no
// folder and no extension. It was a trap: the model reached for the field called
// `filename`, handed it to `read-note`, and `read-note` couldn't find the note,
// because the folder had been thrown away. `path` is now the only identifier, and
// every note tool takes it verbatim.
//
// Doc comments on this struct reach the model: `#[derive(JsonSchema)]` promotes
// them into the tool's `outputSchema`, which every conversation pays for. Keep
// them short and actionable; put the reasoning in `//` comments like this one.
/// One file's search hit.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchResult {
    /// Vault-relative path. Pass straight back as any note tool's `filename`.
    pub path: String,
    /// Relevance; results come back best-first.
    pub score: f32,
    /// Lines that matched, before `maxMatchesPerFile` clipped them.
    pub match_count: usize,
    pub snippets: Vec<Snippet>,
    /// Whether this file has more matches than are shown.
    pub truncated: bool,
}

// `total`/`truncated` are what let the model see that more hits exist without us
// shipping them — the whole point of the cap.
/// Ranked search results.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchOutput {
    pub results: Vec<SearchResult>,
    /// Files that matched, before `offset`/`limit`.
    pub total: usize,
    pub offset: usize,
    /// Whether more matches exist past this page.
    pub truncated: bool,
}

// Typed here, in the domain that owns the vocabulary, so an unknown value is
// rejected as INVALID_PARAMS instead of silently degrading to Content — and the
// legal values reach the model as an enum in the schema rather than as prose.
//
// The `schemars(description)` is deliberate: `#[derive(JsonSchema)]` promotes
// rustdoc straight onto the wire, and the paragraph above is for maintainers, not
// for the model, which pays for it in context on every conversation.
/// What the query is matched against.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    #[default]
    Content,
    Filename,
    Both,
}

/// Required frontmatter fields. A note has to carry every one of them.
pub type MetaFilter = std::collections::BTreeMap<String, serde_json::Value>;

/// Everything about *what* to look for. Grouped so the arity of `search` doesn't
/// grow every time the query language does.
pub struct SearchQuery<'a> {
    pub query: &'a str,
    pub case_sensitive: bool,
    pub search_type: &'a SearchType,
    /// Read `query` as a regular expression instead of a bag of words.
    pub regex: bool,
    /// Frontmatter fields the note must carry. Usable on its own, with an empty
    /// `query`, as a pure metadata filter.
    pub frontmatter: &'a MetaFilter,
}

/// Does the note carry every field the filter asks for?
fn matches_meta(content: &str, filter: &MetaFilter) -> bool {
    if filter.is_empty() {
        return true;
    }
    let Ok(fields) = super::frontmatter::parse_fields(content) else {
        // Malformed frontmatter carries nothing, so it satisfies nothing.
        return false;
    };
    filter
        .iter()
        .all(|(key, want)| fields.get(key).is_some_and(|got| value_matches(got, want)))
}

/// A field matches when it equals the wanted value — or, when it's a list, when
/// it *contains* it. That's what makes `{"tags": "work"}` find a note whose
/// frontmatter says `tags: [work, urgent]`, which is what anyone would expect.
fn value_matches(got: &serde_json::Value, want: &serde_json::Value) -> bool {
    match got {
        _ if got == want => true,
        serde_json::Value::Array(items) => items.contains(want),
        _ => false,
    }
}

/// How many results to return, and how much of each.
#[derive(Debug, Clone)]
pub struct SearchLimits {
    pub limit: usize,
    pub offset: usize,
    pub max_matches_per_file: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            offset: 0,
            max_matches_per_file: DEFAULT_MAX_MATCHES_PER_FILE,
        }
    }
}

/// Split text into lowercase-able word tokens. Tag characters (`-`, `_`, `/`)
/// break tokens, so `#status/active` contributes `status` and `active`.
fn tokenize(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
}

/// Which part of a note a line belongs to. Drives the field weights above.
fn line_weight(line: &str, in_frontmatter: bool) -> f32 {
    if in_frontmatter {
        W_FRONTMATTER
    } else if line.trim_start().starts_with('#') {
        W_HEADING
    } else {
        W_BODY
    }
}

/// A file that contains at least one query term. Non-candidates never get
/// tokenized — they only contribute their length to the corpus average.
struct Candidate {
    path: String,
    /// Weighted term frequency per query term.
    tf: Vec<f32>,
    /// Whether the term occurs at all, for the document-frequency count.
    present: Vec<bool>,
    /// Document length, in bytes. A consistent proxy for token count — BM25's
    /// length normalisation only needs the measure to be applied uniformly.
    len: f32,
    match_count: usize,
    snippets: Vec<Snippet>,
}

/// Score a candidate against the query. Terms the corpus has never seen
/// contribute nothing, so a stray word can't drag a good hit down.
fn bm25(cand: &Candidate, idf: &[f32], avgdl: f32) -> f32 {
    let norm = K1 * (1.0 - B + B * cand.len / avgdl);
    cand.tf
        .iter()
        .zip(idf)
        .map(|(&tf, &idf)| {
            if tf == 0.0 {
                0.0
            } else {
                idf * (tf * (K1 + 1.0)) / (tf + norm)
            }
        })
        .sum()
}

/// Walk `search_root`, returning ranked matches. `root` is the vault root, used
/// to compute paths relative to the vault for display.
pub(crate) fn search(
    root: &Path,
    search_root: &Path,
    q: &SearchQuery<'_>,
    limits: &SearchLimits,
) -> Result<SearchOutput, VaultError> {
    let files = md_files(search_root);
    let (query, case_sensitive, search_type) = (q.query, q.case_sensitive, q.search_type);

    if q.regex {
        return regex_search(root, &files, q, limits);
    }

    // `tag:` is a filter, not a ranked query — a note either carries the tag or
    // it doesn't, so there is nothing to score.
    if let Some(tag) = query.strip_prefix("tag:") {
        return Ok(tag_search(root, &files, tag, q.frontmatter, limits));
    }

    let fold = |s: &str| {
        if case_sensitive {
            s.to_string()
        } else {
            s.to_lowercase()
        }
    };
    let terms: Vec<String> = tokenize(&fold(query)).map(String::from).collect();
    if terms.is_empty() {
        // No words to rank by. With a frontmatter filter that's still a real
        // question — "every note where status is active" — so answer it.
        if !q.frontmatter.is_empty() {
            return Ok(filter_only(root, &files, q.frontmatter, limits));
        }
        return Ok(empty(limits));
    }
    let index: HashMap<&str, usize> = terms
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), i))
        .collect();

    let match_filename = matches!(search_type, SearchType::Filename | SearchType::Both);
    let match_content = matches!(search_type, SearchType::Content | SearchType::Both);

    // One parallel pass over the vault: every file contributes its length to the
    // corpus average; only files carrying a query term are tokenized.
    let scanned: Vec<(f32, Option<Candidate>)> = files
        .par_iter()
        .filter_map(|path| {
            let content = fs::read_to_string(path).ok()?;
            let len = content.len() as f32;

            // A note the filter excludes is not a candidate — but it still counts
            // towards the corpus statistics, so scores don't shift when a filter
            // is added.
            if !matches_meta(&content, q.frontmatter) {
                return Some((len, None));
            }

            let filename = path.file_name()?.to_str()?.to_string();
            let haystack = fold(&content);
            let filename_folded = fold(&filename);

            // Cheap prefilter: skip tokenization unless some term is present.
            let hit = terms.iter().any(|t| {
                (match_content && haystack.contains(t.as_str()))
                    || (match_filename && filename_folded.contains(t.as_str()))
            });
            if !hit {
                return Some((len, None));
            }

            let mut tf = vec![0.0f32; terms.len()];
            let mut present = vec![false; terms.len()];
            let bump = |token: &str, weight: f32, tf: &mut Vec<f32>, present: &mut Vec<bool>| {
                if let Some(&i) = index.get(token) {
                    tf[i] += weight;
                    present[i] = true;
                }
            };

            if match_filename {
                for token in tokenize(&filename_folded) {
                    bump(token, W_FILENAME, &mut tf, &mut present);
                }
            }

            let mut match_count = 0;
            let mut snippets = Vec::new();

            if match_content {
                for tag in extract_frontmatter(&content)
                    .map(|fm| fm.tags)
                    .unwrap_or_default()
                {
                    for token in tokenize(&fold(&tag)) {
                        bump(token, W_TAG, &mut tf, &mut present);
                    }
                }

                // The frontmatter block ends at the standalone closing marker.
                let fm_end = content
                    .strip_prefix("---")
                    .and_then(find_closing_fm)
                    .map(|end| content[..3 + end].lines().count())
                    .unwrap_or(0);

                for (i, (line, folded)) in content.lines().zip(haystack.lines()).enumerate() {
                    let weight = line_weight(line, i < fm_end);
                    for token in tokenize(folded) {
                        bump(token, weight, &mut tf, &mut present);
                    }
                    if terms.iter().any(|t| folded.contains(t.as_str())) {
                        match_count += 1;
                        if snippets.len() < limits.max_matches_per_file {
                            snippets.push(Snippet {
                                line: i + 1,
                                text: clip(line.trim()),
                            });
                        }
                    }
                }
            }

            if !present.iter().any(|&p| p) {
                return Some((len, None));
            }

            // A filename-only hit has no matching line to quote.
            if snippets.is_empty() && match_filename {
                snippets.push(Snippet {
                    line: 0,
                    text: format!("filename: {}", filename),
                });
                match_count = match_count.max(1);
            }

            Some((
                len,
                Some(Candidate {
                    // `rel_path`, not `display()`: paths go out with forward
                    // slashes on every platform, the way the link graph and the
                    // other search paths already emit them. On Windows this was
                    // handing back `sub\deep.md` from ranked search alone.
                    path: super::rel_path(root, path),
                    tf,
                    present,
                    len,
                    match_count,
                    snippets,
                }),
            ))
        })
        .collect();

    let n = scanned.len() as f32;
    if n == 0.0 {
        return Ok(empty(limits));
    }
    let avgdl = (scanned.iter().map(|(len, _)| *len).sum::<f32>() / n).max(1.0);

    let mut candidates: Vec<Candidate> = scanned.into_iter().filter_map(|(_, c)| c).collect();

    // Document frequency, then inverse document frequency: a term that occurs in
    // nearly every note tells us almost nothing, so it is worth almost nothing.
    let idf: Vec<f32> = (0..terms.len())
        .map(|i| {
            let df = candidates.iter().filter(|c| c.present[i]).count() as f32;
            (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
        })
        .collect();

    let mut results: Vec<SearchResult> = candidates
        .iter_mut()
        .map(|c| SearchResult {
            score: bm25(c, &idf, avgdl),
            path: std::mem::take(&mut c.path),
            match_count: c.match_count,
            truncated: c.match_count > c.snippets.len(),
            snippets: std::mem::take(&mut c.snippets),
        })
        .collect();

    // Best first; ties broken by path so the order is stable across runs.
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });

    Ok(paginate(results, limits))
}

/// Search by regular expression.
///
/// BM25 has nothing to say here: a pattern is not a bag of words, and there are
/// no terms whose rarity could be weighed. Ranking is by how many lines matched,
/// which is the only signal a pattern gives us.
fn regex_search(
    root: &Path,
    files: &[std::path::PathBuf],
    q: &SearchQuery<'_>,
    limits: &SearchLimits,
) -> Result<SearchOutput, VaultError> {
    let re = RegexBuilder::new(q.query)
        .case_insensitive(!q.case_sensitive)
        // A pathological pattern must not hang the server. The regex crate has no
        // backtracking, but a large enough compiled program can still eat memory.
        .size_limit(1 << 20)
        .build()
        .map_err(|e| VaultError::InvalidRegex(q.query.to_string(), e.to_string()))?;

    let match_filename = matches!(q.search_type, SearchType::Filename | SearchType::Both);
    let match_content = matches!(q.search_type, SearchType::Content | SearchType::Both);

    let mut results: Vec<SearchResult> = files
        .par_iter()
        .filter_map(|path| {
            let content = fs::read_to_string(path).ok()?;
            if !matches_meta(&content, q.frontmatter) {
                return None;
            }
            let filename = path.file_name()?.to_str()?.to_string();

            let mut snippets = Vec::new();
            let mut match_count = 0;

            if match_content {
                for (i, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        match_count += 1;
                        if snippets.len() < limits.max_matches_per_file {
                            snippets.push(Snippet {
                                line: i + 1,
                                text: clip(line.trim()),
                            });
                        }
                    }
                }
            }
            if match_filename && re.is_match(&filename) {
                match_count = match_count.max(1);
                if snippets.is_empty() {
                    snippets.push(Snippet {
                        line: 0,
                        text: format!("filename: {}", filename),
                    });
                }
            }
            if match_count == 0 {
                return None;
            }

            Some(SearchResult {
                path: super::rel_path(root, path),
                score: match_count as f32,
                match_count,
                truncated: match_count > snippets.len(),
                snippets,
            })
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(paginate(results, limits))
}

/// Notes matching a frontmatter filter and nothing else — "every note where
/// status is active". Unranked: there is no query to be relevant to.
fn filter_only(
    root: &Path,
    files: &[std::path::PathBuf],
    filter: &MetaFilter,
    limits: &SearchLimits,
) -> SearchOutput {
    let mut results: Vec<SearchResult> = files
        .par_iter()
        .filter_map(|path| {
            let content = fs::read_to_string(path).ok()?;
            if !matches_meta(&content, filter) {
                return None;
            }
            Some(SearchResult {
                path: super::rel_path(root, path),
                score: 1.0,
                match_count: 1,
                truncated: false,
                snippets: Vec::new(),
            })
        })
        .collect();

    results.sort_by(|a, b| a.path.cmp(&b.path));
    paginate(results, limits)
}

/// Files carrying `tag`. Unranked — tag membership is boolean.
fn tag_search(
    root: &Path,
    files: &[std::path::PathBuf],
    tag: &str,
    filter: &MetaFilter,
    limits: &SearchLimits,
) -> SearchOutput {
    let mut results: Vec<SearchResult> = files
        .par_iter()
        .filter_map(|path| {
            let content = fs::read_to_string(path).ok()?;
            if !content_has_tag(&content, tag) || !matches_meta(&content, filter) {
                return None;
            }
            Some(SearchResult {
                path: super::rel_path(root, path),
                score: 1.0,
                match_count: 1,
                snippets: vec![Snippet {
                    line: 0,
                    text: format!("tag: {}", tag),
                }],
                truncated: false,
            })
        })
        .collect();

    results.sort_by(|a, b| a.path.cmp(&b.path));
    paginate(results, limits)
}

fn paginate(results: Vec<SearchResult>, limits: &SearchLimits) -> SearchOutput {
    let total = results.len();
    let page: Vec<SearchResult> = results
        .into_iter()
        .skip(limits.offset)
        .take(limits.limit)
        .collect();
    SearchOutput {
        truncated: limits.offset + page.len() < total,
        results: page,
        total,
        offset: limits.offset,
    }
}

fn empty(limits: &SearchLimits) -> SearchOutput {
    SearchOutput {
        results: Vec::new(),
        total: 0,
        offset: limits.offset,
        truncated: false,
    }
}

/// Keep a snippet short enough that one match-heavy note can't dominate the
/// response.
fn clip(text: &str) -> String {
    if text.chars().count() <= SNIPPET_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(SNIPPET_CHARS).collect();
    format!("{}…", head.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    fn vault(notes: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in notes {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        dir
    }

    fn run(dir: &TempDir, query: &str, limits: SearchLimits) -> SearchOutput {
        search(
            dir.path(),
            dir.path(),
            &SearchQuery {
                query,
                case_sensitive: false,
                search_type: &SearchType::Both,
                regex: false,
                frontmatter: &MetaFilter::new(),
            },
            &limits,
        )
        .expect("a plain query cannot fail")
    }

    /// Run a regex query.
    fn run_regex(dir: &TempDir, pattern: &str) -> Result<SearchOutput, VaultError> {
        search(
            dir.path(),
            dir.path(),
            &SearchQuery {
                query: pattern,
                case_sensitive: false,
                search_type: &SearchType::Content,
                regex: true,
                frontmatter: &MetaFilter::new(),
            },
            &SearchLimits::default(),
        )
    }

    /// Run a frontmatter filter, optionally alongside a query.
    fn run_filter(dir: &TempDir, query: &str, filter: MetaFilter) -> SearchOutput {
        search(
            dir.path(),
            dir.path(),
            &SearchQuery {
                query,
                case_sensitive: false,
                search_type: &SearchType::Content,
                regex: false,
                frontmatter: &filter,
            },
            &SearchLimits::default(),
        )
        .expect("a plain query cannot fail")
    }

    fn filter(pairs: &[(&str, serde_json::Value)]) -> MetaFilter {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn paths(out: &SearchOutput) -> Vec<&str> {
        out.results.iter().map(|r| r.path.as_str()).collect()
    }

    // ── Regex ────────────────────────────────────────────────────────────────

    #[test]
    fn regex_matches_a_pattern_not_a_word() {
        let dir = vault(&[
            ("a.md", "call me on 555-1234 today\n"),
            ("b.md", "no digits here\n"),
        ]);
        let out = run_regex(&dir, r"\d{3}-\d{4}").unwrap();
        assert_eq!(paths(&out), vec!["a.md"]);
        assert_eq!(out.results[0].snippets[0].text, "call me on 555-1234 today");
    }

    #[test]
    fn regex_ranks_by_how_many_lines_matched() {
        let dir = vault(&[
            ("few.md", "TODO one\nplain\n"),
            ("many.md", "TODO one\nTODO two\nTODO three\n"),
        ]);
        let out = run_regex(&dir, "^TODO").unwrap();
        assert_eq!(paths(&out), vec!["many.md", "few.md"]);
    }

    #[test]
    fn an_invalid_regex_is_reported_not_swallowed() {
        let dir = vault(&[("a.md", "x\n")]);
        let err = run_regex(&dir, "([unclosed").unwrap_err();
        // The message hands the pattern back, so the model can see what it got
        // wrong instead of just "search failed".
        assert!(err.to_string().contains("([unclosed"), "{err}");
        assert!(matches!(err, VaultError::InvalidRegex(..)), "{err}");
    }

    // ── Frontmatter filter ───────────────────────────────────────────────────

    #[test]
    fn a_filter_alone_answers_which_notes_carry_a_field() {
        let dir = vault(&[
            ("a.md", "---\nstatus: active\n---\nbody\n"),
            ("b.md", "---\nstatus: done\n---\nbody\n"),
            ("c.md", "no frontmatter\n"),
        ]);
        let out = run_filter(&dir, "", filter(&[("status", json!("active"))]));
        assert_eq!(paths(&out), vec!["a.md"]);
    }

    #[test]
    fn a_list_field_matches_when_it_contains_the_value() {
        let dir = vault(&[
            ("a.md", "---\ntags: [work, urgent]\n---\nbody\n"),
            ("b.md", "---\ntags: [home]\n---\nbody\n"),
        ]);
        let out = run_filter(&dir, "", filter(&[("tags", json!("work"))]));
        assert_eq!(paths(&out), vec!["a.md"]);
    }

    #[test]
    fn a_filter_narrows_a_ranked_query() {
        let dir = vault(&[
            ("a.md", "---\nstatus: active\n---\nthe needle\n"),
            ("b.md", "---\nstatus: done\n---\nthe needle\n"),
        ]);
        let all = run_filter(&dir, "needle", MetaFilter::new());
        assert_eq!(all.results.len(), 2);

        let narrowed = run_filter(&dir, "needle", filter(&[("status", json!("active"))]));
        assert_eq!(paths(&narrowed), vec!["a.md"]);
    }

    #[test]
    fn every_field_in_the_filter_must_match() {
        let dir = vault(&[
            ("a.md", "---\nstatus: active\nowner: me\n---\nx\n"),
            ("b.md", "---\nstatus: active\nowner: you\n---\nx\n"),
        ]);
        let out = run_filter(
            &dir,
            "",
            filter(&[("status", json!("active")), ("owner", json!("me"))]),
        );
        assert_eq!(paths(&out), vec!["a.md"]);
    }

    #[test]
    fn ranks_a_title_match_above_a_body_mention() {
        let dir = vault(&[
            ("rust.md", "A note whose title is the term.\n"),
            (
                "other.md",
                "Filler filler filler. Somewhere in here we mention rust once.\n",
            ),
        ]);
        let out = run(&dir, "rust", SearchLimits::default());
        assert_eq!(out.results.len(), 2);
        assert_eq!(
            out.results[0].path, "rust.md",
            "the filename match must outrank the body mention"
        );
        assert!(out.results[0].score > out.results[1].score);
    }

    #[test]
    fn ranks_a_heading_match_above_a_body_match() {
        let dir = vault(&[
            ("a.md", "## Deploy\nsome text\n"),
            ("b.md", "text text\ndeploy appears in the body only\n"),
        ]);
        let out = run(&dir, "deploy", SearchLimits::default());
        assert_eq!(out.results[0].path, "a.md");
    }

    #[test]
    fn a_term_in_every_note_does_not_decide_the_ranking() {
        // "the" is in both, "kafka" in one — the rare term must dominate.
        let dir = vault(&[
            ("kafka.md", "the the the kafka\n"),
            ("plain.md", "the the the the the the the the\n"),
        ]);
        let out = run(&dir, "the kafka", SearchLimits::default());
        assert_eq!(out.results[0].path, "kafka.md");
    }

    #[test]
    fn limit_caps_the_files_returned_and_reports_the_total() {
        let notes: Vec<(String, String)> = (0..10)
            .map(|i| (format!("n{}.md", i), "needle\n".to_string()))
            .collect();
        let refs: Vec<(&str, &str)> = notes
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let dir = vault(&refs);

        let out = run(
            &dir,
            "needle",
            SearchLimits {
                limit: 3,
                ..Default::default()
            },
        );
        assert_eq!(out.results.len(), 3, "only the page is returned");
        assert_eq!(out.total, 10, "but the model is told how many matched");
        assert!(out.truncated);
    }

    #[test]
    fn offset_pages_through_results() {
        let dir = vault(&[
            ("a.md", "needle\n"),
            ("b.md", "needle\n"),
            ("c.md", "needle\n"),
        ]);
        let page2 = run(
            &dir,
            "needle",
            SearchLimits {
                limit: 2,
                offset: 2,
                ..Default::default()
            },
        );
        assert_eq!(page2.results.len(), 1);
        assert_eq!(page2.total, 3);
        assert_eq!(page2.offset, 2);
        assert!(!page2.truncated, "the last page is not truncated");
    }

    #[test]
    fn a_match_heavy_note_cannot_flood_the_response() {
        let body = "needle\n".repeat(500);
        let dir = vault(&[("noisy.md", body.as_str())]);
        let out = run(&dir, "needle", SearchLimits::default());
        let hit = &out.results[0];
        assert_eq!(hit.snippets.len(), DEFAULT_MAX_MATCHES_PER_FILE);
        assert_eq!(hit.match_count, 500, "the real count is still reported");
        assert!(hit.truncated);
    }

    #[test]
    fn snippets_carry_line_numbers_and_text() {
        let dir = vault(&[("n.md", "alpha\nbeta needle here\ngamma\n")]);
        let out = run(&dir, "needle", SearchLimits::default());
        let s = &out.results[0].snippets[0];
        assert_eq!(s.line, 2);
        assert_eq!(s.text, "beta needle here");
    }

    #[test]
    fn long_lines_are_clipped() {
        let long = "x".repeat(400);
        let dir = vault(&[("n.md", format!("needle {}\n", long).as_str())]);
        let out = run(&dir, "needle", SearchLimits::default());
        let text = &out.results[0].snippets[0].text;
        assert!(
            text.chars().count() <= SNIPPET_CHARS + 1,
            "got {} chars",
            text.chars().count()
        );
        assert!(text.ends_with('…'));
    }

    #[test]
    fn no_matches_gives_an_empty_page() {
        let dir = vault(&[("n.md", "nothing here\n")]);
        let out = run(&dir, "absent", SearchLimits::default());
        assert!(out.results.is_empty());
        assert_eq!(out.total, 0);
        assert!(!out.truncated);
    }

    #[test]
    fn tag_search_still_works_and_is_paginated() {
        let dir = vault(&[
            ("a.md", "---\ntags:\n  - target\n---\nbody\n"),
            ("b.md", "body with #target inline\n"),
            ("c.md", "unrelated\n"),
        ]);
        let out = run(&dir, "tag:target", SearchLimits::default());
        assert_eq!(out.total, 2);
        assert_eq!(out.results.len(), 2);
    }

    #[test]
    fn filename_search_ignores_body_matches() {
        let dir = vault(&[("report.md", "nothing\n"), ("other.md", "report\n")]);
        let out = search(
            dir.path(),
            dir.path(),
            &SearchQuery {
                query: "report",
                case_sensitive: false,
                search_type: &SearchType::Filename,
                regex: false,
                frontmatter: &MetaFilter::new(),
            },
            &SearchLimits::default(),
        )
        .unwrap();
        assert_eq!(out.total, 1);
        assert_eq!(out.results[0].path, "report.md");
    }

    #[test]
    fn content_search_ignores_filename_matches() {
        let dir = vault(&[("report.md", "nothing\n"), ("other.md", "report\n")]);
        let out = search(
            dir.path(),
            dir.path(),
            &SearchQuery {
                query: "report",
                case_sensitive: false,
                search_type: &SearchType::Content,
                regex: false,
                frontmatter: &MetaFilter::new(),
            },
            &SearchLimits::default(),
        )
        .unwrap();
        assert_eq!(out.total, 1);
        assert_eq!(out.results[0].path, "other.md");
    }

    #[test]
    fn case_sensitive_search_respects_case() {
        let dir = vault(&[("a.md", "Needle\n"), ("b.md", "needle\n")]);
        let out = search(
            dir.path(),
            dir.path(),
            &SearchQuery {
                query: "Needle",
                case_sensitive: true,
                search_type: &SearchType::Content,
                regex: false,
                frontmatter: &MetaFilter::new(),
            },
            &SearchLimits::default(),
        )
        .unwrap();
        assert_eq!(out.total, 1);
        assert_eq!(out.results[0].path, "a.md");
    }
}
