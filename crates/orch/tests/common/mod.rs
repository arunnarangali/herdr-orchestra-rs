//! Test helper: write a fake executable that can be run straight away.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// Write `#!/bin/sh` + `body` to `path` as an executable and return only once it can be exec'd.
///
/// Another test thread may fork while the write handle is open; that child holds the handle
/// until it execs, and exec'ing the script meanwhile fails with ETXTBSY ("Text file busy").
/// A child forked after our handle is closed cannot hold it, so one successful probe exec
/// (the script exits at once when `ORCH_TEST_PROBE` is set) proves the file is safe for good.
pub fn write_executable(path: &Path, body: &str) {
    let text = format!("#!/bin/sh\n[ -n \"$ORCH_TEST_PROBE\" ] && exit 0\n{body}\n");
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match Command::new(path).env("ORCH_TEST_PROBE", "1").output() {
            Ok(_) => return,
            Err(e) if e.raw_os_error() == Some(26) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => panic!("cannot exec {}: {e}", path.display()),
        }
    }
}
