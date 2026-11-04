use crate::tests::parse_ts as parse_source;

fn commands(source: &str) -> Vec<(String, bool)> {
    parse_source(source)
        .command_invocations
        .iter()
        .map(|invocation| (invocation.command.clone(), invocation.shell))
        .collect()
}

fn program(name: &str) -> (String, bool) {
    (name.to_string(), false)
}

fn shell(text: &str) -> (String, bool) {
    (text.to_string(), true)
}

#[test]
fn child_process_named_imports_record_program_and_shell_calls() {
    let found = commands(
        "import { spawn, spawnSync, execFile, execFileSync, exec, execSync } from 'node:child_process';\n\
         spawn('ffmpeg', ['-i']);\n\
         spawnSync('pgrep', []);\n\
         execFile('hyprctl', []);\n\
         execFileSync('tailscale', ['ip']);\n\
         exec('make build');\n\
         execSync(`git status`);",
    );
    assert_eq!(
        found,
        vec![
            program("ffmpeg"),
            program("pgrep"),
            program("hyprctl"),
            program("tailscale"),
            shell("make build"),
            shell("git status"),
        ]
    );
}

#[test]
fn child_process_namespace_default_and_require_bindings_record_calls() {
    let found = commands(
        "import * as NodeChildProcess from 'node:child_process';\n\
         import cp from 'child_process';\n\
         const { spawnSync } = require('child_process');\n\
         const childProcess = require('node:child_process');\n\
         NodeChildProcess.spawnSync('tailscale', []);\n\
         cp.execSync('hyprctl reload');\n\
         spawnSync('gnome-extensions', []);\n\
         childProcess.spawn('sips', []);",
    );
    assert_eq!(
        found,
        vec![
            program("tailscale"),
            shell("hyprctl reload"),
            program("gnome-extensions"),
            program("sips"),
        ]
    );
}

#[test]
fn execa_zx_and_bun_forms_record_calls() {
    let found = commands(
        "import { execa, execaCommand, $ } from 'execa';\n\
         import { $ as zx } from 'zx';\n\
         await execa('ffmpeg', ['-y']);\n\
         await execaCommand('pkg-config --libs x');\n\
         await $`tailscale status`;\n\
         await $({ cwd: 'a' })`hyprctl ${arg}`;\n\
         await zx`gnome-extensions pack ${dir}`;\n\
         Bun.spawn(['sips', '-z']);\n\
         Bun.spawnSync({ cmd: ['plutil', '-p'] });\n\
         await Bun.$`mkfifo ${path}`;",
    );
    assert_eq!(
        found,
        vec![
            program("ffmpeg"),
            shell("pkg-config --libs x"),
            shell("tailscale status"),
            shell("hyprctl "),
            shell("gnome-extensions pack "),
            program("sips"),
            program("plutil"),
            shell("mkfifo "),
        ]
    );
}

#[test]
fn program_name_with_whitespace_runs_through_a_shell() {
    let found = commands(
        "import { spawn } from 'node:child_process';\nspawn('git status', { shell: true });",
    );
    assert_eq!(found, vec![shell("git status")]);
}

#[test]
fn unknown_callee_dynamic_command_and_shadowed_binding_are_not_recorded() {
    let found = commands(
        "import { spawn } from 'node:child_process';\n\
         import { spawn as other } from './local';\n\
         other('not-a-process');\n\
         spawn(command, []);\n\
         spawn(`${bin} --flag`);\n\
         $`not-imported`;\n\
         import { $ as zx2 } from 'zx';\n\
         await zx2`tool${suffix} --flag`;\n\
         function f(spawn: (cmd: string) => void) { spawn('shadowed'); }\n\
         function g(Bun: { spawn: (x: string[]) => void }) { Bun.spawn(['local-bun']); }",
    );
    assert!(found.is_empty(), "unexpected commands: {found:?}");
}
