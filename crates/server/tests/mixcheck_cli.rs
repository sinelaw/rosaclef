//! `rosaclef mixcheck` on the command line: its exit status (0 no warnings,
//! 1 warnings, 2 an error naming what is wrong), and that it never writes
//! the project.

use std::path::PathBuf;
use std::process::Command;

fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rosaclef-mixcheck-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.json"),
        include_str!("../../studio/tests/mixcheck/fixture.json"),
    )
    .unwrap();
    dir
}

fn mixcheck(dir: &PathBuf, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rosaclef"))
        .arg("mixcheck")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into(),
        String::from_utf8_lossy(&out.stderr).into(),
    )
}

#[test]
fn exit_status_and_errors() {
    let dir = project();
    let before = std::fs::read(dir.join("project.json")).unwrap();

    // The fixture's limiter is driven hard: warnings.
    let (code, out, _) = mixcheck(&dir, &["--range", "2:3"]);
    assert_eq!(code, 1, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["rule"] == "master-overload"));

    // Bar 1 alone, levels only, loose (its true peak, -0.4 dBTP, under
    // loose's 0): no warnings.
    let (code, out, _) = mixcheck(
        &dir,
        &[
            "--range",
            "1:1",
            "--checks",
            "levels",
            "--threshold",
            "loose",
            "--text",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.lines().count() <= 40);

    // Errors: exit 2, and the message says what.
    let (code, _, err) = mixcheck(&dir, &["--range", "NaN:3"]);
    assert_eq!(code, 2);
    assert!(err.contains("range:"), "{err}");
    let (code, _, err) = mixcheck(
        &dir,
        &[
            "--what-if",
            r#"[{"op":"replace","path":"/channels/9/volume","value":1}]"#,
        ],
    );
    assert_eq!(code, 2);
    assert!(
        err.contains("whatIf[0] replace /channels/9/volume: no element 9"),
        "{err}"
    );
    let (code, _, err) = mixcheck(&dir, &["--focus", "nobody"]);
    assert_eq!(code, 2);
    assert!(err.contains("focus:"), "{err}");

    // A what-if is never written.
    let (code, _, _) = mixcheck(
        &dir,
        &[
            "--range",
            "2:3",
            "--what-if",
            r#"[{"op":"replace","path":"/mixer/inserts/0/effects/1/params/gain","value":0}]"#,
        ],
    );
    assert!(code == 0 || code == 1);
    assert_eq!(std::fs::read(dir.join("project.json")).unwrap(), before);
}
