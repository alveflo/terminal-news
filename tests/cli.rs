use std::process::Command;

#[test]
fn configure_list_and_remove_feeds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("feeds.json");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_terminapp"))
            .arg("--config")
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    };
    let result = run(&["--list"]);
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("SVT Nyheter"));
    assert!(
        run(&["--add", "Example", "https://example.com/rss"])
            .status
            .success()
    );
    assert!(
        !run(&["--add", "Duplicate", "https://example.com/rss"])
            .status
            .success()
    );
    assert!(
        run(&["--remove", "https://example.com/rss"])
            .status
            .success()
    );
    assert!(
        !run(&["--remove", "https://example.com/rss"])
            .status
            .success()
    );
    assert!(!run(&["--list", "--check"]).status.success());
    assert!(!run(&[]).status.success());
    assert!(
        run(&[
            "--add",
            "Morot HN",
            "https://morot.app/api/hn",
            "--api-key",
            "test-secret"
        ])
        .status
        .success()
    );
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        config["feeds"].as_array().unwrap().last().unwrap()["api_key"],
        "test-secret"
    );
    let listed = run(&["--list"]);
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("test-secret"));
    assert!(
        !run(&["--api-key", "test-secret", "--list"])
            .status
            .success()
    );
    assert!(
        !run(&[
            "--add",
            "Bad",
            "https://example.com/bad",
            "--api-key",
            "bad\nkey"
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "--add",
            "Private",
            "https://example.com/private",
            "--api-key-env",
            "TERMINAPP_TEST_MISSING_KEY"
        ])
        .status
        .success()
    );
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        config["feeds"].as_array().unwrap().last().unwrap()["api_key_env"],
        "TERMINAPP_TEST_MISSING_KEY"
    );
}
