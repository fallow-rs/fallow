//! Linux process state from `/proc`.
//!
//! `kill(pid, 0)` succeeds for a zombie: a process that has exited but that
//! its parent or the init process has not reaped yet. Cleanup waits must count
//! a zombie as exited, because nothing reaps a killed child while its owner is
//! not in `wait`, and some container init processes reap orphans only after
//! seconds. These helpers return `None` when `/proc` cannot answer, so callers
//! keep their `kill` fallback.

use std::fs;

/// Whether `pid` is running. A zombie or dead process is not running.
pub fn process_is_running(pid: u32) -> Option<bool> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (state, _) = state_and_process_group(&stat)?;
    Some(is_running_state(state))
}

/// Whether any member of process group `process_group_id` is running.
pub fn process_group_has_running_member(process_group_id: i32) -> Option<bool> {
    let entries = fs::read_dir("/proc").ok()?;
    let has_running_member = entries.flatten().any(|entry| {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str() else {
            return false;
        };
        if !pid.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        // A process can exit between the directory read and this read.
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        state_and_process_group(&stat)
            .is_some_and(|(state, group)| group == process_group_id && is_running_state(state))
    });
    Some(has_running_member)
}

fn is_running_state(state: &str) -> bool {
    !matches!(state, "Z" | "X")
}

/// Parse the state and process group ID from a `/proc/<pid>/stat` line.
///
/// The fields follow the parenthesized command name, which can itself contain
/// spaces and parentheses: `state ppid pgrp ...`.
fn state_and_process_group(stat: &str) -> Option<(&str, i32)> {
    let (_, fields) = stat.rsplit_once(')')?;
    let mut fields = fields.split_whitespace();
    let state = fields.next()?;
    let process_group = fields.nth(1)?.parse().ok()?;
    Some((state, process_group))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_state_and_process_group_after_command_name() {
        let stat = "4242 (a (b) c) Z 1 4200 4200 0 -1 4194560";
        assert_eq!(state_and_process_group(stat), Some(("Z", 4200)));
    }

    #[test]
    fn rejects_truncated_stat_line() {
        assert_eq!(state_and_process_group("4242 (sleep) S 1"), None);
        assert_eq!(state_and_process_group("4242 sleep S 1 4200"), None);
    }

    #[test]
    fn zombie_and_dead_states_are_not_running() {
        assert!(is_running_state("R"));
        assert!(is_running_state("S"));
        assert!(!is_running_state("Z"));
        assert!(!is_running_state("X"));
    }

    #[test]
    fn this_process_is_running() {
        assert_eq!(process_is_running(std::process::id()), Some(true));
    }
}
