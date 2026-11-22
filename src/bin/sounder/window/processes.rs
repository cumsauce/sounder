//! The sounder processes on this machine, for the window's Servers panel:
//! daemons (`mcp_driver.py --daemon`), servers (the `sounder` binary, windowed
//! or headless) and the agent proxies. Found with `ps`, so a server an agent
//! started by itself (an MCP config that runs the bare binary) shows up too.

use std::process::{Command, Stdio};

use serde_json::{Value, json};

struct Row {
    pid: u32,
    ppid: u32,
    age: String,
    argv: Vec<String>,
}

fn base(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

fn arg_after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    let i = argv.iter().position(|a| a == flag)?;
    argv.get(i + 1).map(String::as_str)
}

fn rows() -> Result<Vec<Row>, String> {
    let out = Command::new("ps")
        .args(["-axww", "-o", "pid=,ppid=,etime=,command="])
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("ps: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut rows = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(age)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let (Ok(pid), Ok(ppid)) = (pid.parse(), ppid.parse()) else {
            continue;
        };
        rows.push(Row {
            pid,
            ppid,
            age: age.to_string(),
            argv: it.map(str::to_string).collect(),
        });
    }
    Ok(rows)
}

enum Kind {
    Daemon,
    Server,
    Proxy,
}

fn classify(r: &Row) -> Option<Kind> {
    let exe = base(r.argv.first()?);
    // The sandbox wrapper an MCP client puts around a command repeats it.
    if exe == "disclaimer" {
        return None;
    }
    // modeler ships a driver of the same name; its path says whose it is.
    if r.argv
        .iter()
        .any(|a| base(a) == "mcp_driver.py" && !a.to_lowercase().contains("modeler"))
    {
        if r.argv.iter().any(|a| a == "--daemon") {
            return Some(Kind::Daemon);
        }
        if r.argv.iter().any(|a| a == "--proxy") {
            return Some(Kind::Proxy);
        }
        return None;
    }
    // Spaces in a path split a command in two; a binary's own name is enough.
    if matches!(exe, "sounder" | "sounder-server") {
        return Some(Kind::Server);
    }
    None
}

fn socket_of(r: &Row) -> String {
    arg_after(&r.argv, "--socket")
        .unwrap_or("/tmp/sounder-mcp.sock")
        .to_string()
}

fn label_of(r: &Row) -> String {
    // The binary or script name; the launcher of an unknown parent.
    r.argv
        .iter()
        .find(|a| !a.starts_with('-') && (base(a).ends_with(".py") || !a.contains("python")))
        .map(|a| base(a).to_string())
        .unwrap_or_default()
}

/// Every sounder daemon, server and proxy, with who started what.
pub fn list() -> Result<Value, String> {
    let me = std::process::id();
    let rows = rows()?;
    let mut daemons = Vec::new();
    let mut servers = Vec::new();
    let mut proxies = 0;
    for r in &rows {
        match classify(r) {
            Some(Kind::Daemon) => daemons.push(json!({
                "pid": r.pid, "age": r.age, "socket": socket_of(r),
                "windowed": r.argv.iter().any(|a| a == "--windowed"),
            })),
            Some(Kind::Server) => {
                let parent = rows.iter().find(|p| p.pid == r.ppid);
                let via = match parent {
                    Some(p) if matches!(classify(p), Some(Kind::Daemon)) => {
                        format!("daemon {}", p.pid)
                    }
                    Some(p) => label_of(p),
                    None => String::new(),
                };
                servers.push(json!({
                    "pid": r.pid, "age": r.age, "parent": r.ppid, "via": via,
                    "windowed": !r.argv.iter().any(|a| a == "--headless"),
                    "project": arg_after(&r.argv, "--project"),
                    "binary": r.argv.first(),
                    "self": r.pid == me,
                }));
            }
            Some(Kind::Proxy) => proxies += 1,
            None => {}
        }
    }
    Ok(json!({"self": me, "daemons": daemons, "servers": servers, "proxies": proxies}))
}

/// Stops a daemon or server from the listing (SIGTERM; a daemon also takes
/// the server it owns down with it, which the daemon's own handler does).
pub fn stop(pid: u32) -> Result<Value, String> {
    let rows = rows()?;
    let ok = rows
        .iter()
        .find(|r| r.pid == pid)
        .is_some_and(|r| matches!(classify(r), Some(Kind::Daemon | Kind::Server)));
    if !ok {
        return Err(format!("pid {pid} is not a sounder daemon or server"));
    }
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(|e| format!("kill: {e}"))?;
    if !status.success() {
        return Err(format!("could not stop pid {pid}"));
    }
    list()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cmd: &str) -> Row {
        Row {
            pid: 1,
            ppid: 0,
            age: "00:01".into(),
            argv: cmd.split_whitespace().map(str::to_string).collect(),
        }
    }

    #[test]
    fn classifies_daemons_servers_and_proxies() {
        let d = row("/usr/bin/python3 /x/mcp_driver.py --daemon --windowed --socket /tmp/a.sock");
        assert!(matches!(classify(&d), Some(Kind::Daemon)));
        assert_eq!(socket_of(&d), "/tmp/a.sock");
        assert!(matches!(
            classify(&row("python3 /x/mcp_driver.py --proxy")),
            Some(Kind::Proxy)
        ));
        assert!(matches!(
            classify(&row("/Users/me/.cargo/bin/sounder --project /p")),
            Some(Kind::Server)
        ));
        // The sandbox wrapper repeats the command: not counted twice.
        assert!(
            classify(&row(
                "/A/disclaimer --pgroup -- python3 /x/mcp_driver.py --proxy"
            ))
            .is_none()
        );
        assert!(classify(&row("vim sounder.rs")).is_none());
        assert!(classify(&row("python3 /code/modeler/mcp_driver.py --proxy")).is_none());
    }

    #[test]
    fn stop_refuses_other_processes() {
        assert!(stop(std::process::id()).is_err());
    }
}
