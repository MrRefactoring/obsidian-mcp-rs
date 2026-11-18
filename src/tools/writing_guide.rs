use std::fmt::Write;

use rmcp::ErrorData;
use serde::Deserialize;

const MAX_TOPICS: usize = 4;
const MAX_RESPONSE_BYTES: usize = 8192;
const STYLE: &str = "Follow the user's instructions first. For edits, keep the note's style. Next use vault templates and similar notes. Use this guide when there is no local pattern.";
const SKILL: &str = "Use the official obsidian-markdown skill if it is installed.";
const GUIDE: &str = include_str!("../../docs/writing-guide.md");
const UPSTREAM_URL: &str = "https://github.com/obsidianmd/obsidian-skills";
const UPSTREAM_REV: &str = "caf3e98cd22d810db164f26c9a442f0a280daa43";
const UPSTREAM_DIR: &str = "plugins/obsidian/skills/obsidian-markdown";
const LICENSE: &str = include_str!("../../vendor/obsidian-markdown/LICENSE");

struct UpstreamFile {
    path: &'static str,
    content: &'static str,
}

const MARKDOWN: UpstreamFile = UpstreamFile {
    path: "SKILL.md",
    content: include_str!("../../vendor/obsidian-markdown/SKILL.md"),
};
const CALLOUTS: UpstreamFile = UpstreamFile {
    path: "references/CALLOUTS.md",
    content: include_str!("../../vendor/obsidian-markdown/references/CALLOUTS.md"),
};
const EMBEDS: UpstreamFile = UpstreamFile {
    path: "references/EMBEDS.md",
    content: include_str!("../../vendor/obsidian-markdown/references/EMBEDS.md"),
};
const PROPERTIES: UpstreamFile = UpstreamFile {
    path: "references/PROPERTIES.md",
    content: include_str!("../../vendor/obsidian-markdown/references/PROPERTIES.md"),
};

struct Topic {
    name: &'static str,
    summary: &'static str,
    excerpt: Option<(&'static UpstreamFile, &'static str)>,
}

impl Topic {
    const fn new(name: &'static str, summary: &'static str) -> Self {
        Self {
            name,
            summary,
            excerpt: None,
        }
    }

    const fn with_excerpt(
        name: &'static str,
        summary: &'static str,
        file: &'static UpstreamFile,
        section: &'static str,
    ) -> Self {
        Self {
            name,
            summary,
            excerpt: Some((file, section)),
        }
    }
}

const SYNTAX: &[Topic] = &[
    Topic::new("formatting", "Headings, lists, quotes, and text styles"),
    Topic::with_excerpt(
        "links",
        "Wikilinks, aliases, headings, and block IDs",
        &MARKDOWN,
        "Internal Links (Wikilinks)",
    ),
    Topic::with_excerpt(
        "embeds",
        "Notes, images, audio, and PDFs",
        &EMBEDS,
        "Embed Images",
    ),
    Topic::with_excerpt(
        "callouts",
        "Types, nesting, and folding",
        &CALLOUTS,
        "Foldable Callouts",
    ),
    Topic::with_excerpt(
        "mermaid",
        "Flowcharts, sequences, and note links",
        &MARKDOWN,
        "Diagrams (Mermaid)",
    ),
    Topic::new("tables", "Markdown tables and alias pipes"),
    Topic::with_excerpt(
        "properties",
        "YAML frontmatter and types",
        &PROPERTIES,
        "Property Types",
    ),
    Topic::with_excerpt("tags", "Inline and nested tags", &PROPERTIES, "Tags"),
    Topic::with_excerpt(
        "math",
        "Inline and display formulas",
        &MARKDOWN,
        "Math (LaTeX)",
    ),
    Topic::new("code", "Inline code and fenced code blocks"),
    Topic::with_excerpt(
        "footnotes",
        "Numbered and inline footnotes",
        &MARKDOWN,
        "Footnotes",
    ),
    Topic::new("comments", "Hidden text in Reading view"),
    Topic::new("checkboxes", "Open, done, and nested tasks"),
];
const RECIPES: &[Topic] = &[
    Topic::new("meeting", "Decisions, actions, and questions"),
    Topic::new("decision", "Context, decision, and consequences"),
    Topic::new("runbook", "Steps, checks, and rollback"),
    Topic::new("daily", "Daily priorities and follow-up"),
    Topic::new("research", "Findings, evidence, and limitations"),
    Topic::new("project", "Outcomes, actions, and references"),
    Topic::new("process", "Triggers, steps, and decisions"),
    Topic::new("task_overview", "Priorities, blockers, and source tasks"),
];

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WritingGuideParams {
    #[doc = "Up to four different topic names from the index. Omit to get the index."]
    #[serde(default)]
    pub topics: Vec<String>,
}

pub(crate) fn render(topics: &[String]) -> Result<String, ErrorData> {
    if topics.is_empty() {
        return Ok(index());
    }

    let mut selected = Vec::new();
    for topic in topics {
        if selected.contains(&topic.as_str()) {
            continue;
        }
        if selected.len() == MAX_TOPICS {
            return Err(ErrorData::invalid_params(
                "Select up to four different writing guide topics per call.",
                None,
            ));
        }
        selected.push(topic.as_str());
    }

    let mut result = format!("{STYLE}\n\n{SKILL}");
    for name in selected {
        if name == "sources" {
            write!(
                result,
                "\n\n## sources\n\n[Official obsidian-markdown snapshot]({UPSTREAM_URL}/tree/{UPSTREAM_REV}/{UPSTREAM_DIR})\n\n{LICENSE}"
            ).unwrap();
            continue;
        }
        let topic = SYNTAX.iter().chain(RECIPES).find(|t| t.name == name).ok_or_else(|| {
            ErrorData::invalid_params(
                format!("Unknown writing guide topic '{name}'. Call get_writing_guide with no topics for the index."),
                None,
            )
        })?;
        write!(
            result,
            "\n\n## {name}\n\n{}",
            required_section(GUIDE, name)?
        )
        .unwrap();
        if let Some((file, heading)) = topic.excerpt {
            write!(
                result,
                "\n\n[Bundled Obsidian reference]({UPSTREAM_URL}/blob/{UPSTREAM_REV}/{UPSTREAM_DIR}/{})\n\n{}",
                file.path,
                required_section(file.content, heading)?
            ).unwrap();
        }
    }

    if result.len() > MAX_RESPONSE_BYTES {
        return Err(ErrorData::invalid_params(
            "The writing guide response is too long. Select fewer topics.",
            None,
        ));
    }
    Ok(result)
}

fn index() -> String {
    let mut result = format!(
        "# Obsidian writing guide\n\n{STYLE}\n\n{SKILL} [Official setup]({UPSTREAM_URL}#installation)\n\nSelect up to four topics with `topics`, for example `[\"links\", \"callouts\"]`."
    );
    for (title, topics) in [("Syntax", SYNTAX), ("Recipes", RECIPES)] {
        write!(result, "\n\n## {title}").unwrap();
        for topic in topics {
            write!(result, "\n- `{}` — {}", topic.name, topic.summary).unwrap();
        }
    }
    result.push_str("\n\n- `sources` — Official snapshot and MIT license");
    result
}

fn required_section(document: &'static str, name: &str) -> Result<&'static str, ErrorData> {
    section(document, name).ok_or_else(|| {
        ErrorData::internal_error(format!("Writing guide section '{name}' is missing."), None)
    })
}

fn section<'a>(document: &'a str, name: &str) -> Option<&'a str> {
    let heading = format!("## {name}");
    let mut start = None;
    let mut fence = 0;
    let mut end = 0;
    for line in document.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        let ticks = text.trim_start().bytes().take_while(|&b| b == b'`').count();
        if fence == 0 && ticks >= 3 {
            fence = ticks;
        } else if fence != 0 && ticks >= fence {
            fence = 0;
        } else if fence == 0 && text.starts_with("## ") {
            if start.is_some() {
                break;
            }
            if text == heading {
                start = Some(end + line.len());
            }
        }
        end += line.len();
    }
    start.map(|start| document[start..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_guide_index_is_short_and_lists_all_topics() {
        let index = render(&[]).unwrap();
        assert!(index.len() < 2000);
        assert!(!index.contains("```markdown"));
        for topic in SYNTAX.iter().chain(RECIPES) {
            assert!(index.contains(&format!("`{}`", topic.name)));
        }
        assert!(index.contains("`sources`"));
    }

    #[test]
    fn writing_guide_sections_include_examples_and_fit_the_limit() {
        let mut lengths = Vec::new();
        for topic in SYNTAX.iter().chain(RECIPES) {
            let text = render(&[topic.name.to_owned()]).unwrap();
            assert!(text.contains("````markdown"), "{}", topic.name);
            assert!(text.contains("https://obsidian.md/help/"), "{}", topic.name);
            if topic.excerpt.is_some() {
                assert!(text.contains(UPSTREAM_REV), "{}", topic.name);
            }
            lengths.push(text.len());
        }
        let sources = render(&["sources".to_string()]).unwrap();
        assert!(sources.contains(UPSTREAM_REV));
        assert!(sources.contains("MIT License"));
        assert!(sources.contains("Copyright (c) 2026 Steph Ango"));
        lengths.push(sources.len());
        lengths.sort_unstable();
        assert!(lengths.iter().rev().take(MAX_TOPICS).sum::<usize>() <= MAX_RESPONSE_BYTES);
    }

    #[test]
    fn writing_guide_deduplicates_topics_and_keeps_nested_example_headings() {
        let text = render(&["meeting".into(), "links".into(), "meeting".into()]).unwrap();
        assert_eq!(text.matches("## meeting\n").count(), 1);
        assert!(text.contains("## Open questions"));
        assert!(text.contains("## links\n"));
        assert!(!text.contains("## decision\n"));
    }

    #[test]
    fn writing_guide_extracts_crlf_sections_outside_fences() {
        let guide = GUIDE.replace("\r\n", "\n");
        let windows = guide.replace('\n', "\r\n");
        let meeting = section(&windows, "meeting").unwrap();
        assert_eq!(
            meeting.replace("\r\n", "\n"),
            section(&guide, "meeting").unwrap()
        );
        assert!(meeting.contains("## Decisions\r\n"));
        assert!(meeting.contains("## Actions\r\n"));
        assert!(!meeting.contains("## decision\r\n"));
    }

    #[test]
    fn writing_guide_rejects_unknown_and_too_many_topics() {
        assert!(render(&["unknown".into()]).is_err());
        let topics: Vec<_> = SYNTAX
            .iter()
            .take(MAX_TOPICS + 1)
            .map(|t| t.name.to_owned())
            .collect();
        assert!(render(&topics).is_err());
    }
}
