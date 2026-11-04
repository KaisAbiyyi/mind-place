//! Owner-invoked offline maintenance only. Never run from startup or MCP.
#[cfg(windows)]
mod owner_console {
    use mind_place_index::vault_owner_migration::OfflineLegacyRoot;
    use mind_place_security::{vault_migration::ExplicitMigrationConsent, SecurityGate};
    use std::io::{self, Read, Write};
    use zeroize::Zeroizing;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(kind: u32) -> isize;
        fn GetConsoleMode(handle: isize, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: isize, mode: u32) -> i32;
    }
    struct EchoGuard {
        handle: isize,
        mode: u32,
    }
    impl Drop for EchoGuard {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.handle, self.mode);
            }
        }
    }
    fn console_mode(kind: u32) -> Result<(isize, u32), &'static str> {
        let handle = unsafe { GetStdHandle(kind) };
        let mut mode = 0;
        if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
            return Err("MP_MIGRATION_INTERACTIVE_CONSOLE_REQUIRED");
        }
        Ok((handle, mode))
    }
    fn input(prompt: &str, hidden: bool) -> Result<Zeroizing<String>, &'static str> {
        print!("{prompt}");
        io::stdout()
            .flush()
            .map_err(|_| "MP_MIGRATION_CONSOLE_IO")?;
        let (handle, mode) = console_mode((-10i32) as u32)?;
        let echo = if hidden {
            if unsafe { SetConsoleMode(handle, mode & !4) } == 0 {
                return Err("MP_MIGRATION_CONSOLE_IO");
            }
            Some(EchoGuard { handle, mode })
        } else {
            None
        };
        let mut bytes = Zeroizing::new(Vec::new());
        let mut reader = io::stdin().lock();
        loop {
            let mut byte = [0u8; 1];
            if reader
                .read(&mut byte)
                .map_err(|_| "MP_MIGRATION_CONSOLE_IO")?
                != 1
            {
                return Err("MP_MIGRATION_CONSOLE_EOF");
            }
            if byte[0] == b'\n' {
                break;
            }
            if bytes.len() >= 1024 {
                return Err("MP_MIGRATION_INPUT_TOO_LONG");
            }
            bytes.push(byte[0]);
        }
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        drop(echo);
        if hidden {
            println!();
        }
        String::from_utf8(bytes.to_vec())
            .map(Zeroizing::new)
            .map_err(|_| "MP_MIGRATION_INVALID_INPUT")
    }
    pub fn run(action: &str) -> Result<(), String> {
        // No redirected output or piped secrets: the independent recovery code
        // may only be shown by this explicit, interactive owner action.
        console_mode((-11i32) as u32).map_err(str::to_owned)?;
        console_mode((-10i32) as u32).map_err(str::to_owned)?;
        println!("Close Mind Place and every capture/daemon process before continuing.");
        println!(
            "Original plaintext is retained. This action publishes a separate encrypted vault."
        );
        let phrase = input(
            "Type ENCRYPT MY LEGACY VAULT to authorize this action: ",
            false,
        )?;
        let consent = ExplicitMigrationConsent::from_owner_typed_confirmation(&phrase)
            .map_err(|e| e.code().to_owned())?;
        let archive = mind_place_archive::data_root().map_err(|_| "MP_MIGRATION_APP_ROOT")?;
        if archive.file_name().and_then(|s| s.to_str()) != Some("archive-v1") {
            return Err("MP_MIGRATION_APP_ROOT".into());
        }
        let root = archive.parent().ok_or("MP_MIGRATION_APP_ROOT")?;
        let gate =
            SecurityGate::open(root.join("security-v1.json")).map_err(|e| e.code().to_owned())?;
        let secret = input("Existing Mind Place unlock password (hidden): ", true)?;
        gate.unlock(secret.as_bytes())
            .map_err(|e| e.code().to_owned())?;
        drop(secret);
        let result = (|| {
            let lease = gate.require_unlocked().map_err(|e| e.code().to_owned())?;
            let offline =
                OfflineLegacyRoot::acquire(root, &gate, lease).map_err(|e| e.code().to_owned())?;
            if action == "begin" {
                let prepared = offline
                    .prepare(&gate, lease)
                    .map_err(|e| e.code().to_owned())?;
                let code = prepared.recovery_code();
                println!(
                    "Save this independent recovery code OFFLINE. It is never saved in plaintext:"
                );
                println!("{}", code.as_str());
                drop(code);
                let entered = input("Re-enter the saved recovery code (hidden): ", true)?;
                prepared
                    .confirm(&offline, &gate, lease, &entered, &consent)
                    .map_err(|e| e.code().to_owned())?;
            } else {
                let entered = input("Previously saved recovery code (hidden): ", true)?;
                offline
                    .resume(&gate, lease, &entered, &consent, action == "rollback")
                    .map_err(|e| e.code().to_owned())?;
            }
            Ok::<(), String>(())
        })();
        gate.lock();
        result?;
        println!("Offline action complete. Source retained. Restart Mind Place; it starts locked.");
        Ok(())
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let action = args.first().and_then(|s| s.to_str()).unwrap_or("--help");
    if args.len() <= 1 && matches!(action, "--help" | "-h") {
        println!("mp-vault-migrate <begin|resume|rollback>\nWindows offline owner maintenance. Close Mind Place first. No secrets or paths are accepted as command arguments.");
        return;
    }
    if args.len() != 1 || !matches!(action, "begin" | "resume" | "rollback") {
        eprintln!("MP_MIGRATION_INVALID_ACTION: use --help");
        std::process::exit(2);
    }
    #[cfg(windows)]
    let result = owner_console::run(action);
    #[cfg(not(windows))]
    let result: Result<(), String> = Err("MP_MIGRATION_WINDOWS_OFFLINE_REQUIRED".into());
    if let Err(code) = result {
        eprintln!(
            "{code}: no automatic cleanup; retain the offline recovery code for resume/rollback."
        );
        std::process::exit(1);
    }
}
