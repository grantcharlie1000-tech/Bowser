//! Command chains: a named, ordered list of registry commands run one after another with a delay in
//! milliseconds before each. A chain is itself a registry command (`chain:<n>`), so Quick Commands,
//! a keyboard shortcut, a mouse gesture or a toolbar button runs it like any other.
//!
//! `chains.txt` (per profile): `bowser-chains-v1`, then for each chain a `chain<TAB>id<TAB>name` line
//! followed by its `step<TAB>delay_ms<TAB>command_id` lines. Steps name built-in commands only, so a
//! chain can never run itself or another chain.

use std::{
    sync::mpsc,
    thread,
    time::Duration,
};

pub const MAX_CHAINS: usize = 64;
pub const MAX_STEPS: usize = 32;
pub const MAX_DELAY_MS: u32 = 60_000;
pub const MAX_NAME_CHARS: usize = 48;
const MAX_FILE_BYTES: usize = 128 * 1024;
const HEADER: &str = "bowser-chains-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// A built-in command id.
    pub command: String,
    /// Wait this long before running the step (the first step's wait follows the chain's start).
    pub delay_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    pub id: u32,
    pub name: String,
    pub steps: Vec<Step>,
}

impl Chain {
    /// The registry id of the chain.
    pub fn command_id(&self) -> String {
        format!("chain:{}", self.id)
    }
}

/// The chain number in a `chain:<n>` command id.
pub fn chain_number(command_id: &str) -> Option<u32> {
    command_id.strip_prefix("chain:")?.parse().ok()
}

fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_owned()
}

fn valid_command_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn parse(text: &str) -> Vec<Chain> {
    if text.len() > MAX_FILE_BYTES {
        return Vec::new();
    }
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Vec::new();
    }
    let mut chains: Vec<Chain> = Vec::new();
    for line in lines {
        // `lines` keeps a lone trailing `\r` (a CRLF file whose last line has no `\n`, or a stray
        // CR), which would make the command id invalid and silently drop that step.
        let line = line.trim_end_matches('\r');
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.as_slice() {
            ["chain", id, name] => {
                let Ok(id) = id.parse::<u32>() else { continue };
                let name = clean_name(name);
                if name.is_empty() || chains.len() >= MAX_CHAINS || chains.iter().any(|c| c.id == id) {
                    continue;
                }
                chains.push(Chain { id, name: name.chars().take(MAX_NAME_CHARS).collect(), steps: Vec::new() });
            }
            ["step", delay, command] => {
                let (Ok(delay_ms), true) = (delay.parse::<u32>(), valid_command_id(command)) else { continue };
                if let Some(chain) = chains.last_mut()
                    && chain.steps.len() < MAX_STEPS
                {
                    chain.steps.push(Step { command: (*command).to_owned(), delay_ms: delay_ms.min(MAX_DELAY_MS) });
                }
            }
            _ => {}
        }
    }
    chains
}

pub fn serialize(chains: &[Chain]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for chain in chains {
        out.push_str(&format!("chain\t{}\t{}\n", chain.id, clean_name(&chain.name)));
        for step in &chain.steps {
            out.push_str(&format!("step\t{}\t{}\n", step.delay_ms, step.command));
        }
    }
    out
}

/// Whether `chain` may be saved. `others` are the other chains (a rename must stay unique); `known`
/// says whether a command id is a built-in.
pub fn validate(chain: &Chain, others: &[Chain], known: impl Fn(&str) -> bool) -> Result<(), String> {
    let name = clean_name(&chain.name);
    if name.is_empty() {
        return Err("Give the chain a name.".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("A chain name holds at most {MAX_NAME_CHARS} characters."));
    }
    if others.iter().any(|other| other.id != chain.id && clean_name(&other.name).eq_ignore_ascii_case(&name)) {
        return Err(format!("There is already a chain called {name}."));
    }
    if chain.steps.is_empty() {
        return Err("Add at least one command to the chain.".into());
    }
    if chain.steps.len() > MAX_STEPS {
        return Err(format!("A chain holds at most {MAX_STEPS} commands."));
    }
    for step in &chain.steps {
        if step.delay_ms > MAX_DELAY_MS {
            return Err(format!("A delay is at most {MAX_DELAY_MS} ms."));
        }
        if !known(&step.command) {
            return Err("A chain can only hold built-in commands.".into());
        }
    }
    Ok(())
}

/// One step of a run: wait `delay_ms`, then run `command`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub index: usize,
    pub delay_ms: u32,
    pub command: String,
}

pub fn plan(chain: &Chain) -> Vec<Planned> {
    chain
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| Planned { index, delay_ms: step.delay_ms, command: step.command.clone() })
        .collect()
}

/// Runs `plan` on a helper thread: it waits each step's delay and then calls `fire(step)`, in order.
/// `fire` only posts a message to the window thread (commands themselves always run there). The
/// thread ends after the last step, or as soon as `fire` returns false (the window is gone).
pub fn schedule(plan: Vec<Planned>, fire: impl Fn(Planned) -> bool + Send + 'static) -> mpsc::Receiver<()> {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        for step in plan {
            if step.delay_ms > 0 {
                thread::sleep(Duration::from_millis(u64::from(step.delay_ms)));
            }
            if !fire(step) {
                break;
            }
        }
        let _ = done.send(());
    });
    finished
}

/// The chain text the settings page sends for editing and testing: `id<TAB>name` then one
/// `delay<TAB>command` per line. Separate from the file so a draft can be tested before it is saved.
pub fn parse_draft(text: &str) -> Option<Chain> {
    let mut lines = text.lines();
    let head = lines.next()?;
    let (id, name) = head.split_once('\t')?;
    let mut chain = Chain { id: id.parse().ok()?, name: clean_name(name), steps: Vec::new() };
    for line in lines {
        let (delay, command) = line.split_once('\t')?;
        if !valid_command_id(command) {
            return None;
        }
        chain.steps.push(Step { command: command.to_owned(), delay_ms: delay.parse().ok()? });
        if chain.steps.len() > MAX_STEPS {
            return None;
        }
    }
    Some(chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn chain(id: u32, name: &str, steps: &[(u32, &str)]) -> Chain {
        Chain { id, name: name.into(), steps: steps.iter().map(|(delay_ms, command)| Step { command: (*command).into(), delay_ms: *delay_ms }).collect() }
    }

    #[test]
    fn a_chain_file_round_trips_and_ignores_damage() {
        let chains = vec![chain(1, "Morning", &[(0, "new-tab"), (250, "history")]), chain(7, "Tidy", &[(0, "close-tab")])];
        let text = serialize(&chains);
        assert_eq!(parse(&text), chains);
        assert_eq!(chains[0].command_id(), "chain:1");
        assert_eq!(chain_number("chain:7"), Some(7));
        assert_eq!(chain_number("new-tab"), None);
        let damaged = format!("{text}step\tnotanumber\tx\nstep\t5\tBad Id\nchain\tabc\tX\njunk\n");
        assert_eq!(parse(&damaged), chains, "unreadable lines are skipped");
        assert_eq!(parse("not the header\nchain\t1\tX\n"), Vec::new());
        assert_eq!(parse(&"x".repeat(MAX_FILE_BYTES + 1)), Vec::new());
    }

    #[test]
    fn a_crlf_chain_file_keeps_every_step_including_an_unterminated_last_line() {
        let chains = vec![chain(1, "Morning", &[(0, "new-tab"), (250, "history")])];
        let crlf = serialize(&chains).replace('\n', "\r\n");
        assert_eq!(parse(&crlf), chains);
        assert_eq!(parse(&format!("{}\r", crlf.trim_end_matches("\r\n"))), chains, "a lone final CR");
    }

    #[test]
    fn parsing_clamps_delays_steps_names_and_duplicate_ids() {
        let mut text = String::from("bowser-chains-v1\nchain\t1\tA\n");
        for _ in 0..40 {
            text.push_str("step\t999999\tnew-tab\n");
        }
        text.push_str("chain\t1\tDuplicate id\nchain\t2\t\nchain\t3\t");
        text.push_str(&"n".repeat(80));
        text.push('\n');
        let chains = parse(&text);
        assert_eq!(chains.len(), 2, "duplicate id and empty name dropped");
        assert_eq!(chains[0].steps.len(), MAX_STEPS);
        assert!(chains[0].steps.iter().all(|s| s.delay_ms == MAX_DELAY_MS));
        assert_eq!(chains[1].name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn validation_covers_names_steps_delays_and_known_commands() {
        let known = |id: &str| matches!(id, "new-tab" | "reload");
        let others = vec![chain(2, "Existing", &[(0, "reload")])];
        assert_eq!(validate(&chain(1, "Mine", &[(0, "new-tab")]), &others, known), Ok(()));
        assert!(validate(&chain(1, "  ", &[(0, "new-tab")]), &others, known).is_err());
        assert!(validate(&chain(1, "existing", &[(0, "new-tab")]), &others, known).is_err(), "names are unique, ignoring case");
        assert_eq!(validate(&chain(2, "Existing", &[(0, "new-tab")]), &others, known), Ok(()), "a chain keeps its own name");
        assert!(validate(&chain(1, "Empty", &[]), &others, known).is_err());
        assert!(validate(&chain(1, "Slow", &[(MAX_DELAY_MS + 1, "new-tab")]), &others, known).is_err());
        assert!(validate(&chain(1, "Nested", &[(0, "chain:2")]), &others, known).is_err());
        assert!(validate(&chain(1, "Unknown", &[(0, "nope")]), &others, known).is_err());
        let long = chain(1, "Long", &vec![(0, "new-tab"); MAX_STEPS + 1].to_vec());
        assert!(validate(&long, &others, known).is_err());
    }

    #[test]
    fn a_plan_keeps_order_and_per_step_delays() {
        let planned = plan(&chain(1, "X", &[(0, "new-tab"), (40, "reload"), (5, "back")]));
        assert_eq!(
            planned,
            vec![
                Planned { index: 0, delay_ms: 0, command: "new-tab".into() },
                Planned { index: 1, delay_ms: 40, command: "reload".into() },
                Planned { index: 2, delay_ms: 5, command: "back".into() },
            ]
        );
    }

    #[test]
    fn a_scheduled_run_fires_every_step_in_order_after_its_delay_and_then_finishes() {
        let fired = Arc::new(Mutex::new(Vec::new()));
        let log = fired.clone();
        let started = std::time::Instant::now();
        let planned = plan(&chain(1, "X", &[(0, "new-tab"), (60, "reload"), (60, "back")]));
        let finished = schedule(planned, move |step| {
            log.lock().unwrap().push((step.index, step.command, started.elapsed()));
            true
        });
        finished.recv_timeout(Duration::from_secs(5)).expect("the run finishes");
        let fired = fired.lock().unwrap();
        let order: Vec<_> = fired.iter().map(|(i, c, _)| (*i, c.as_str())).collect();
        assert_eq!(order, [(0, "new-tab"), (1, "reload"), (2, "back")]);
        assert!(fired[1].2 >= Duration::from_millis(55), "the second step waited its delay: {:?}", fired[1].2);
        assert!(fired[2].2 >= Duration::from_millis(115), "delays add up: {:?}", fired[2].2);
    }

    #[test]
    fn a_run_stops_when_the_window_is_gone() {
        let count = Arc::new(Mutex::new(0));
        let seen = count.clone();
        let finished = schedule(plan(&chain(1, "X", &[(0, "new-tab"), (0, "reload"), (0, "back")])), move |_| {
            *seen.lock().unwrap() += 1;
            false
        });
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[test]
    fn a_draft_from_the_page_parses_and_refuses_bad_lines() {
        let draft = parse_draft("3\tMy chain\n0\tnew-tab\n250\treload").unwrap();
        assert_eq!(draft, chain(3, "My chain", &[(0, "new-tab"), (250, "reload")]));
        assert_eq!(parse_draft("x\tName\n0\tnew-tab"), None);
        assert_eq!(parse_draft("3\tName\nnot-a-delay\tnew-tab"), None);
        assert_eq!(parse_draft("3\tName\n0\tBad Id"), None);
        assert_eq!(parse_draft("3 Name"), None);
    }
}
