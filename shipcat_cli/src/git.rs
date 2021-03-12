use super::Result;
use std::process::Command;

// Dumb git wrapper that validates output or bails
fn exec(args: &[&str]) -> Result<String> {
    debug!("git {}", args.join(" "));
    let s = Command::new("git").args(args).output()?;
    if !s.status.success() {
        bail!("Subprocess failure from git: {}", s.status.code().unwrap_or(1001))
    }
    let out: String = String::from_utf8_lossy(&s.stdout).into();
    let err: String = String::from_utf8_lossy(&s.stderr).into();
    if !err.is_empty() {
        warn!("{} stderr: {}", args.join(" "), err);
    }
    debug!("{}", out);
    Ok(out)
}

// Common command for determining the merge-base of the current head with origin/master
pub fn merge_base() -> Result<String> {
    let out = exec(&["merge-base", "origin/master", "HEAD"])?;
    Ok(out.trim().to_string())
}

// Are there local changes in the index or working copy?
pub fn needs_stash() -> bool {
    exec(&["diff", "--quiet", "--exit-code"]).is_err()
        || exec(&["diff", "--cached", "--quiet", "--exit-code"]).is_err()
}

// git stash
pub fn stash_push() -> Result<String> {
    exec(&["stash", "--quiet"])
}

// git stash pop
pub fn stash_pop() -> Result<String> {
    exec(&["stash", "pop", "--quiet"])
}

// git checkout <ref>
pub fn checkout(reference: &str) -> Result<String> {
    exec(&["checkout", reference, "--quiet"])
}

// git diff --name-only <ref>
pub fn diff_filenames(reference: &str) -> Result<String> {
    exec(&["diff", "--name-only", reference])
}
