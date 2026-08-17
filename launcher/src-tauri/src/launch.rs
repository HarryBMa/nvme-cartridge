//! Starting the game.

use std::path::Path;
use std::process::Command;

use crate::manifest::LaunchTarget;
use crate::trust::Trust;

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("this cartridge is not on the trust list, so it will not be started")]
    NotTrusted,
    #[error("auto-launch is switched off in settings.conf")]
    Blocked,
    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
}

/// Run the cartridge.
///
/// `trust` is passed in rather than recomputed so that the decision the user saw
/// on screen is the decision that is enforced here.
pub fn launch(target: &LaunchTarget, cwd: &Path, trust: &Trust) -> Result<(), LaunchError> {
    if target.requires_trust() && !trust.allows_launch() {
        return Err(LaunchError::NotTrusted);
    }

    match target {
        LaunchTarget::Steam {
            app_id,
            big_picture,
        } => {
            if *big_picture {
                // Best effort: if Big Picture is already up this is a no-op, and
                // a failure here should not stop the game from launching.
                let _ = open_url("steam://open/bigpicture");
                std::thread::sleep(std::time::Duration::from_millis(1200));
            }
            open_url(&format!("steam://rungameid/{app_id}"))
        }
        LaunchTarget::Script(script) => run_script(script, cwd),
        LaunchTarget::Command(argv) => {
            let (program, args) = argv.split_first().expect("validated non-empty argv");
            spawn(Command::new(program).args(args).current_dir(cwd), program)
        }
    }
}

/// Hand a URL to the desktop's protocol handler.
fn open_url(url: &str) -> Result<(), LaunchError> {
    #[cfg(target_os = "windows")]
    {
        // `explorer.exe <url>` is the least surprising way to trigger a protocol
        // handler without a shell, and avoids `cmd /c start` quoting rules.
        spawn(Command::new("explorer.exe").arg(url), "explorer.exe")
    }
    #[cfg(target_os = "macos")]
    {
        spawn(Command::new("/usr/bin/open").arg(url), "open")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        spawn(Command::new("xdg-open").arg(url), "xdg-open")
    }
}

fn run_script(script: &Path, cwd: &Path) -> Result<(), LaunchError> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .current_dir(cwd);
        spawn(&mut cmd, "powershell.exe")
    }
    #[cfg(unix)]
    {
        // Matches cartridge-launcher-helper.sh, which runs the script through
        // bash rather than relying on the exec bit surviving a FAT/exFAT mount.
        let mut cmd = Command::new("bash");
        cmd.arg(script).current_dir(cwd);
        spawn(&mut cmd, "bash")
    }
}

/// Spawn and detach. The launcher must not hold the game as a child, or quitting
/// the tray icon would take the game with it.
fn spawn(cmd: &mut Command, program: &str) -> Result<(), LaunchError> {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    let mut child = cmd.spawn().map_err(|source| LaunchError::Spawn {
        program: program.to_string(),
        source,
    })?;

    // Reap on a detached thread so the launcher does not accumulate zombies on
    // Unix, without ever blocking the UI thread on the game's lifetime.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn untrusted_scripts_are_refused_before_anything_spawns() {
        let target = LaunchTarget::Script(PathBuf::from("/media/x/launch.sh"));
        let err = launch(
            &target,
            Path::new("/media/x"),
            &Trust::Untrusted {
                digest: "0".repeat(64),
            },
        )
        .unwrap_err();
        assert!(matches!(err, LaunchError::NotTrusted));
    }

    #[test]
    fn untrusted_commands_are_refused_too() {
        // If this ever regresses, an unreadable cartridge could run argv.
        let target = LaunchTarget::Command(vec!["/bin/false".into()]);
        let err = launch(
            &target,
            Path::new("/tmp"),
            &Trust::Unreadable {
                reason: "gone".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, LaunchError::NotTrusted));
    }

    #[test]
    fn trusted_commands_spawn() {
        let target = LaunchTarget::Command(vec!["/bin/sh".into(), "-c".into(), "exit 0".into()]);
        let trust = Trust::Verified {
            digest: "a".repeat(64),
        };
        assert!(launch(&target, Path::new("/tmp"), &trust).is_ok());
    }

    #[test]
    fn a_missing_program_reports_which_one() {
        let target = LaunchTarget::Command(vec!["/nonexistent/program".into()]);
        let trust = Trust::Verified {
            digest: "a".repeat(64),
        };
        let err = launch(&target, Path::new("/tmp"), &trust).unwrap_err();
        match err {
            LaunchError::Spawn { program, .. } => assert_eq!(program, "/nonexistent/program"),
            other => panic!("expected a spawn error, got {other:?}"),
        }
    }
}
