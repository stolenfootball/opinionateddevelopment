//! Process-level acceptance tests for evidence bootstrap review.

use std::fs;
use std::process::Command;

use opdev_core::Evidence;
use opdev_project::{EVIDENCE_PATH, EvidenceBootstrap, ReviewDecision};

fn opdev() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opdev"))
}

#[test]
fn bootstrap_cli_requires_review_previews_and_writes_once() -> Result<(), Box<dyn std::error::Error>>
{
    let project = tempfile::tempdir()?;
    let root = project.path();
    assert!(
        Command::new("git")
            .arg("init")
            .arg(root)
            .status()?
            .success()
    );
    fs::write(
        root.join("package.json"),
        r#"{"name":"cli-bootstrap","scripts":{"test":"node --test"}}"#,
    )?;
    assert!(
        opdev()
            .args(["init", "--root"])
            .arg(root)
            .status()?
            .success()
    );
    fs::write(
        root.join("OPDEV_ADOPTION.md"),
        "Reviewed project policy, applicability, change scope, tests, and integration behavior.\n",
    )?;
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["add", "."])
            .status()?
            .success()
    );

    let generated = opdev()
        .args(["evidence", "bootstrap", "--root"])
        .arg(root)
        .output()?;
    assert!(generated.status.success());
    let yaml = String::from_utf8(generated.stdout)?;
    assert!(yaml.contains("review_required"));
    assert!(yaml.contains("# "));

    let answers_directory = tempfile::tempdir()?;
    let answers_path = answers_directory.path().join("review.yaml");
    fs::write(&answers_path, &yaml)?;
    let mut answers = EvidenceBootstrap::load(&answers_path)?;
    for decision in answers.project.decisions.values_mut() {
        *decision = ReviewDecision::Passed;
    }
    for decision in answers.change.decisions.values_mut() {
        *decision = ReviewDecision::Passed;
    }
    let evidence = Evidence {
        kind: "review".into(),
        summary: "The adoption review records the facts supporting these decisions.".into(),
        location: Some("OPDEV_ADOPTION.md".into()),
    };
    answers.project.evidence.push(evidence.clone());
    answers.change.evidence.push(evidence);
    answers.change.work = "OPDEV-15 CLI bootstrap acceptance".into();
    fs::write(&answers_path, answers.to_yaml()?)?;

    let preview = opdev()
        .args(["evidence", "bootstrap", "--root"])
        .arg(root)
        .arg("--answers")
        .arg(&answers_path)
        .output()?;
    assert!(preview.status.success());
    assert!(String::from_utf8(preview.stdout)?.contains("rule_id:"));
    assert!(!root.join(EVIDENCE_PATH).exists());

    assert!(
        opdev()
            .args(["evidence", "bootstrap", "--root"])
            .arg(root)
            .arg("--answers")
            .arg(&answers_path)
            .arg("--write")
            .status()?
            .success()
    );
    assert!(root.join(EVIDENCE_PATH).exists());
    assert!(
        !opdev()
            .args(["evidence", "bootstrap", "--root"])
            .arg(root)
            .status()?
            .success()
    );
    Ok(())
}
