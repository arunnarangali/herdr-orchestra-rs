//! The driver lease: second taker refused, free again after the holder drops it, and the
//! liveness probe in both states. (`flock` conflicts even between two handles of the same
//! process, so no second process is needed.)

use orch_core::lease::{driver_alive, driver_info, driver_lock_path, take_lease};

#[test]
fn second_taker_is_refused_and_names_the_holder() {
    let t = tempfile::tempdir().unwrap();
    let p = driver_lock_path(&t.path().join(".orchestrate"));
    let lease = take_lease(&p).unwrap();
    let e = take_lease(&p).unwrap_err();
    assert!(
        e.to_string().contains("this run is already driven by"),
        "{e}"
    );
    assert!(
        e.to_string()
            .contains(&format!("pid {}", lease.info.pid.unwrap())),
        "{e}"
    );
    assert!(e.to_string().contains(lease.info.host.as_deref().unwrap()), "{e}");
    assert!(!e.to_string().contains("started ?"), "{e}");
    assert!(e.to_string().contains("started "), "{e}");
    assert!(driver_alive(&p));
}

#[test]
fn lease_is_free_again_after_the_holder_drops_it() {
    let t = tempfile::tempdir().unwrap();
    let p = driver_lock_path(&t.path().join(".orchestrate"));
    {
        let _lease = take_lease(&p).unwrap();
        assert!(driver_alive(&p));
        assert!(take_lease(&p).is_err());
    }
    assert!(!driver_alive(&p));
    let lease2 = take_lease(&p).unwrap();
    assert_ne!(lease2.token, "");
    let e = take_lease(&p).unwrap_err();
    assert!(
        e.to_string().contains(&format!("pid {}", lease2.info.pid.unwrap())),
        "{e}"
    );
}

#[test]
fn probe_reports_both_states() {
    let t = tempfile::tempdir().unwrap();
    let p = driver_lock_path(&t.path().join(".orchestrate"));
    assert!(!p.exists());
    assert!(!driver_alive(&p));
    let lease = take_lease(&p).unwrap();
    assert!(driver_alive(&p));
    let info = driver_info(&p);
    assert_eq!(info.pid, lease.info.pid);
    assert_eq!(info.host, lease.info.host);
    assert_eq!(info.started, lease.info.started);
    assert_eq!(info.token, lease.info.token);
    drop(lease);
    assert!(!driver_alive(&p));
    let missing = p.parent().unwrap().join("missing.lock");
    assert!(!driver_alive(&missing));
}

#[test]
fn an_empty_or_broken_file_counts_as_free_but_has_no_info() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("driver.lock");
    std::fs::write(&p, "leftover").unwrap();
    assert!(!driver_alive(&p));
    assert_eq!(driver_info(&p).pid, None);
    let lease = take_lease(&p).unwrap();
    assert!(lease.info.pid.is_some());
}
