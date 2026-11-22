//! A blocked agent's open prompt, read from its pane and answered with one of its own choices.
//!
//! Claude's "trust this folder?" and permission questions (and Codex's approvals) show a
//! numbered list with a cursor on one row:
//!
//! ```text
//!  Do you trust the files in this folder?
//!  ❯ 1. Yes, proceed
//!    2. No, exit
//! ```
//!
//! [`read`] turns the bottom of the pane into an [`AgentPrompt`]: the question's text and its
//! choices. A choice is answered by moving the cursor to it (`up` / `down`) and pressing
//! `enter`, computed from where the cursor is now, so no key is ever guessed. A screen the
//! reader cannot turn into choices (no numbered list, no cursor, numbers out of order) is
//! given as text with one choice, [`ANSWERED_IN_PANE`], which types nothing.
//!
//! A choice's id names its number *and* the question it belongs to, so an id taken from an
//! earlier question never answers a newer one: [`answer`] reads the pane again and types
//! only a choice the pane offers at that moment, into the one pane it was given.

use crate::adapter::RuntimeAdapter;
use crate::error::HerdrError;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How many recent lines of the pane a prompt is looked for in.
pub const PROMPT_READ_LINES: u32 = 40;
/// The id of the one choice of a prompt the app cannot read into choices: the developer says
/// they answered it themselves; nothing is typed.
pub const ANSWERED_IN_PANE: &str = "answered_in_pane";
/// The label of [`ANSWERED_IN_PANE`].
pub const ANSWERED_IN_PANE_LABEL: &str = "I answered it in the pane";
/// Lines of question text kept above the choices (and of screen for an unreadable prompt).
const TEXT_LINES: usize = 12;
/// The most choices a prompt is read with (they are picked by a single digit on screen).
const MAX_CHOICES: u32 = 9;
/// Lines that may separate two choices (wrapped labels, descriptions).
const MAX_GAP: usize = 4;
/// After a choice was typed into a pane, the same choice of the same question is refused for
/// this long: the pane may still show the old question for a moment, and a second click must
/// not answer the next question with it.
const SETTLE: Duration = Duration::from_secs(3);

/// Cursor marks agents put before the selected row.
const CURSORS: &[char] = &['❯', '›', '>', '▶', '➤', '▸', '→', '»'];
/// Box sides around a dialog, removed from both ends of a line.
const SIDES: &[char] = &['│', '┃', '║', '▌', '▐'];
/// Characters a box's top or bottom border is made of.
const BORDER: &str = "─━═╭╮╰╯┌┐└┘├┤┬┴┼╔╗╚╝-▔▁ ";

/// One choice of a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChoice {
    /// Opaque: the choice's number and a digest of the question it belongs to.
    pub id: String,
    /// The choice as the pane shows it.
    pub label: String,
    /// What selects it: arrow keys from the cursor's row, then `enter`. Empty for
    /// [`ANSWERED_IN_PANE`].
    pub keys: Vec<String>,
}

/// A blocked agent's open prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPrompt {
    /// The question (or, when it could not be read into choices, the bottom of the screen).
    pub text: String,
    /// The choices; exactly one, [`ANSWERED_IN_PANE`], when the screen could not be read.
    pub choices: Vec<PromptChoice>,
}

impl AgentPrompt {
    /// Whether the screen was read into real choices.
    pub fn readable(&self) -> bool {
        !self.choices.iter().any(|c| c.id == ANSWERED_IN_PANE)
    }

    /// The offered choice with `id`.
    pub fn choice(&self, id: &str) -> Option<&PromptChoice> {
        self.choices.iter().find(|c| c.id == id)
    }

    /// A prompt the app cannot read into choices: `text`, and the one hands-off choice.
    pub fn unreadable(text: impl Into<String>) -> AgentPrompt {
        AgentPrompt {
            text: text.into(),
            choices: vec![PromptChoice {
                id: ANSWERED_IN_PANE.into(),
                label: ANSWERED_IN_PANE_LABEL.into(),
                keys: vec![],
            }],
        }
    }
}

/// A screen line without escapes, control characters and box sides.
fn clean(line: &str) -> String {
    orch_core::attention::strip_escapes(line)
        .replace(['\r', '\t'], " ")
        .trim()
        .trim_matches(SIDES)
        .trim()
        .to_string()
}

fn is_border(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| BORDER.contains(c)) && line.chars().any(|c| c != ' ')
}

/// `❯ 1. Yes, proceed` → `(1, "Yes, proceed", true)`.
fn option_line(line: &str) -> Option<(u32, String, bool)> {
    let (cursor, rest) = match line.chars().next() {
        Some(c) if CURSORS.contains(&c) => (true, line[c.len_utf8()..].trim_start()),
        _ => (false, line),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 1 {
        return None;
    }
    let after = &rest[digits.len()..];
    let after = after
        .strip_prefix('.')
        .or_else(|| after.strip_prefix(')'))?;
    if !after.starts_with(' ') {
        return None;
    }
    let label = after.trim();
    if label.is_empty() {
        return None;
    }
    Some((digits.parse().ok()?, label.to_string(), cursor))
}

/// FNV-1a over the question and its labels: the same question gives the same ids.
fn digest(text: &str, labels: &[String]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in text.bytes().chain(
        labels
            .iter()
            .flat_map(|l| std::iter::once(b'\n').chain(l.bytes())),
    ) {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h
}

/// The last `TEXT_LINES` non-empty lines, as the unreadable prompt's text.
fn tail_text(lines: &[String]) -> String {
    let kept: Vec<&str> = lines
        .iter()
        .filter(|l| !l.is_empty() && !is_border(l))
        .map(String::as_str)
        .collect();
    kept[kept.len().saturating_sub(TEXT_LINES)..].join("\n")
}

/// Read a pane's screen (its recent output) into the prompt it shows.
pub fn parse(screen: &str) -> AgentPrompt {
    let lines: Vec<String> = screen.lines().map(clean).collect();
    let fallback = || {
        let t = tail_text(&lines);
        AgentPrompt::unreadable(if t.is_empty() {
            "The agent is waiting for an answer in its pane, but its screen shows no question."
                .to_string()
        } else {
            t
        })
    };
    // the last numbered list: walk up from its last row to `1.`
    let opts: Vec<(usize, u32, String, bool)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| option_line(l).map(|(n, label, c)| (i, n, label, c)))
        .collect();
    let Some(last) = opts.last() else {
        return fallback();
    };
    // a list followed by more than a few lines of other output is old output, not the prompt
    let trailing = lines[last.0 + 1..].iter().filter(|l| !l.is_empty()).count();
    if trailing > MAX_GAP || last.1 < 2 || last.1 > MAX_CHOICES {
        return fallback();
    }
    let mut block = vec![last.clone()];
    for o in opts.iter().rev().skip(1) {
        let prev = block.last().unwrap();
        if o.1 + 1 != prev.1 || prev.0 - o.0 > MAX_GAP + 1 {
            break;
        }
        block.push(o.clone());
        if o.1 == 1 {
            break;
        }
    }
    block.reverse();
    if block[0].1 != 1 || block.len() < 2 {
        return fallback();
    }
    let cursors: Vec<usize> = block
        .iter()
        .enumerate()
        .filter(|(_, o)| o.3)
        .map(|(i, _)| i)
        .collect();
    let [at] = cursors[..] else {
        // no cursor (or more than one): which row is selected is unknown
        return fallback();
    };
    // the question: the lines above `1.`, up to a box border, the last TEXT_LINES of them
    let mut above: Vec<&str> = Vec::new();
    for l in lines[..block[0].0].iter().rev() {
        if is_border(l) || above.len() >= TEXT_LINES {
            break;
        }
        above.push(l);
    }
    above.reverse();
    // no blank lines at the ends, and never two in a row
    let mut kept: Vec<&str> = Vec::new();
    for l in above {
        if l.is_empty() && kept.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        kept.push(l);
    }
    while kept.last().is_some_and(|l| l.is_empty()) {
        kept.pop();
    }
    let mut text = kept.join("\n");
    if text.is_empty() {
        text = "The agent asks you to choose:".into();
    }
    let labels: Vec<String> = block.iter().map(|o| o.2.clone()).collect();
    let d = digest(&text, &labels);
    let choices = block
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let step = if i >= at { "down" } else { "up" };
            let mut keys = vec![step.to_string(); i.abs_diff(at)];
            keys.push("enter".into());
            PromptChoice {
                id: format!("{}-{d:08x}", o.1),
                label: o.2.clone(),
                keys,
            }
        })
        .collect();
    AgentPrompt { text, choices }
}

/// The prompt `agent`'s pane shows now. A pane that cannot be read is an unreadable prompt
/// that says so (the developer can still answer it in the pane).
pub fn read(runtime: &dyn RuntimeAdapter, agent: &str) -> AgentPrompt {
    match runtime.read_output(agent, PROMPT_READ_LINES) {
        Ok(screen) => parse(&screen),
        Err(e) => AgentPrompt::unreadable(format!(
            "The agent is waiting for an answer in its pane, but the app could not read its \
screen ({e})."
        )),
    }
}

/// Why [`answer`] typed nothing.
#[derive(Debug)]
pub enum AnswerError {
    /// The id is not one of the choices the pane offers now.
    NotOffered(AgentPrompt),
    /// That choice of that question was typed into this pane a moment ago.
    JustAnswered,
    /// Herdr failed to type it.
    Herdr(HerdrError),
}

/// A pane of one runtime: the runtime's address and the pane id.
type PaneKey = (usize, String);

/// What was answered recently, per pane: the choice id and when.
fn recent() -> &'static Mutex<HashMap<PaneKey, (String, Instant)>> {
    static RECENT: OnceLock<Mutex<HashMap<PaneKey, (String, Instant)>>> = OnceLock::new();
    RECENT.get_or_init(Default::default)
}

/// Answer the prompt `agent` shows in `pane` with the choice `option`: read the pane again,
/// and type that choice's keys into `pane` only when the pane offers it now. Answers are one
/// at a time, and the same choice of the same question is not typed twice within a moment.
/// [`ANSWERED_IN_PANE`] (offered only for an unreadable prompt) types nothing. The caller has
/// checked that `agent` is blocked and that `pane` is its pane. Gives the prompt answered.
pub fn answer(
    runtime: &dyn RuntimeAdapter,
    agent: &str,
    pane: &str,
    option: &str,
) -> Result<AgentPrompt, AnswerError> {
    let mut recent = recent().lock().unwrap_or_else(|p| p.into_inner());
    let prompt = read(runtime, agent);
    let Some(choice) = prompt.choice(option) else {
        return Err(AnswerError::NotOffered(prompt));
    };
    if choice.keys.is_empty() {
        return Ok(prompt);
    }
    recent.retain(|_, (_, at)| at.elapsed() < SETTLE);
    let key: PaneKey = (
        runtime as *const dyn RuntimeAdapter as *const () as usize,
        pane.to_string(),
    );
    if recent.get(&key).is_some_and(|(id, _)| id == option) {
        return Err(AnswerError::JustAnswered);
    }
    let keys: Vec<&str> = choice.keys.iter().map(String::as_str).collect();
    runtime.send_keys(pane, &keys).map_err(AnswerError::Herdr)?;
    recent.insert(key, (option.to_string(), Instant::now()));
    drop(recent);
    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeHerdr, Sent};
    use crate::model::{AgentInfo, PaneInfo};
    use crate::status::AgentStatus;

    const TRUST: &str = "\
╭──────────────────────────────────────────╮
│ Do you trust the files in this folder?   │
│                                          │
│ /home/dev/project                        │
│                                          │
│ \u{1b}[36m❯ 1. Yes, proceed\u{1b}[0m                        │
│   2. No, exit                            │
╰──────────────────────────────────────────╯
   Enter to confirm · Esc to exit
";

    const PERMISSION: &str = "\
⏺ Bash(cargo test)
Do you want to proceed?
  1. Yes
❯ 2. Yes, and don't ask again for cargo test commands in /home/dev/project
  3. No, and tell Claude what to do differently (esc)
";

    #[test]
    fn a_trust_question_is_read_into_its_text_and_choices() {
        let p = parse(TRUST);
        assert!(p.readable());
        assert_eq!(
            p.text,
            "Do you trust the files in this folder?\n\n/home/dev/project"
        );
        let labels: Vec<&str> = p.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Yes, proceed", "No, exit"]);
        assert_eq!(p.choices[0].keys, ["enter"]);
        assert_eq!(p.choices[1].keys, ["down", "enter"]);
        assert!(p.choices[0].id.starts_with("1-") && p.choices[1].id.starts_with("2-"));
    }

    #[test]
    fn keys_move_from_where_the_cursor_is() {
        let p = parse(PERMISSION);
        assert_eq!(p.text, "⏺ Bash(cargo test)\nDo you want to proceed?");
        assert_eq!(p.choices[0].keys, ["up", "enter"]);
        assert_eq!(p.choices[1].keys, ["enter"]);
        assert_eq!(p.choices[2].keys, ["down", "enter"]);
        assert_eq!(
            p.choices[2].label,
            "No, and tell Claude what to do differently (esc)"
        );
    }

    #[test]
    fn ids_belong_to_their_question() {
        let a = parse(PERMISSION);
        let b = parse(&PERMISSION.replace("cargo test)", "rm -rf target)"));
        assert_eq!(parse(PERMISSION), a, "the same screen reads the same");
        assert_ne!(a.choices[0].id, b.choices[0].id);
    }

    #[test]
    fn screens_without_a_clear_choice_list_are_unreadable() {
        for screen in [
            "",
            "Allow this command? (y/n)",
            // a numbered list in prose: no cursor
            "Plan:\n1. read\n2. write\n",
            // two cursors
            "❯ 1. a\n❯ 2. b\n",
            // numbers do not start at 1
            "❯ 2. a\n  3. b\n",
            // a single choice
            "❯ 1. ok\n",
            // an old list with much output after it
            "❯ 1. a\n  2. b\nx\ny\nz\nw\nv\n",
        ] {
            let p = parse(screen);
            assert!(!p.readable(), "{screen:?} read as {p:?}");
            assert_eq!(p.choices.len(), 1);
            assert_eq!(p.choices[0].id, ANSWERED_IN_PANE);
            assert_eq!(p.choices[0].label, ANSWERED_IN_PANE_LABEL);
            assert!(p.choices[0].keys.is_empty());
            assert!(!p.text.is_empty());
        }
        assert_eq!(
            parse("Allow this command? (y/n)").text,
            "Allow this command? (y/n)"
        );
    }

    fn fake(screen: &str) -> FakeHerdr {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_pane(PaneInfo::new("w1:p2", "w1", "w1:t1"));
        f.add_agent(AgentInfo::new("a", "claude", "w1:p1", AgentStatus::Blocked));
        f.add_agent(AgentInfo::new("b", "claude", "w1:p2", AgentStatus::Blocked));
        f.set_output("a", screen);
        f
    }

    #[test]
    fn answering_types_only_an_offered_choice_into_the_given_pane_once() {
        let f = fake(PERMISSION);
        let id = parse(PERMISSION).choices[2].id.clone();
        answer(&f, "a", "w1:p1", &id).unwrap();
        assert_eq!(
            f.sent(),
            vec![Sent::Keys {
                pane: "w1:p1".into(),
                keys: vec!["down".into(), "enter".into()]
            }]
        );
        // a second click on the same choice while the pane still shows the question
        assert!(matches!(
            answer(&f, "a", "w1:p1", &id),
            Err(AnswerError::JustAnswered)
        ));
        // not offered: a bare number, raw keys, an id of another question
        for bad in ["1", "enter", "3-00000000", ANSWERED_IN_PANE] {
            assert!(matches!(
                answer(&f, "a", "w1:p1", bad),
                Err(AnswerError::NotOffered(_))
            ));
        }
        assert_eq!(f.sent().len(), 1, "nothing else typed: {:?}", f.sent());
    }

    #[test]
    fn answered_in_pane_types_nothing() {
        let f = fake("Allow? (y/n)");
        // `b` shows nothing readable either
        let p = answer(&f, "b", "w1:p2", ANSWERED_IN_PANE).unwrap();
        assert!(!p.readable());
        assert!(answer(&f, "a", "w1:p1", ANSWERED_IN_PANE).is_ok());
        assert!(f.sent().is_empty());
    }
}
