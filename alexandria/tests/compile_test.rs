//! End-to-end tests running the compiler binary on the fixtures in `tests/fixtures`.

use std::{iter::once, path::Path, process::Command};

datatest_stable::harness! {
    {
        test = harness,
        root = "tests/fixtures",
        pattern = r"^(pass|fail)/([^/]+|[^/]+/main)\.aa$",
    }
}

/// The test harness. This collects the tags of a test and applies them. The two types of tags are:
/// 1. Error tags. Defined by `//~`. These are only respected in `fail/` tests, and each line is supposed to be a section of the
///    expected error output.
/// 2. Argument tags. Defined by `//=`. These are always respected and are passed as arguments to the compiler *without any modifications*.
fn harness(path: &Path) -> datatest_stable::Result<()> {
    let tags = extract_tags(path)?;

    if let Some(ref error_tags) = tags.error_tags
        && error_tags.is_empty()
    {
        return Err(Box::from("'fail' integrations tests must have error tags"));
    }

    let path_as_str = path.to_string_lossy();
    let args: Vec<&str> = once(&path_as_str as &str)
        .chain(tags.args.split(' ').filter(|x| !x.is_empty()))
        .collect();

    let command = Command::new(env!("CARGO_BIN_EXE_alexandria"))
        .args(&args)
        .output()?;

    let test_exit = command.status.success();
    let expected_exit = tags.error_tags.is_none();
    if test_exit != expected_exit {
        return Err(Box::from(format!(
            "integration test status does not match expected status (success: {}, expected_success: {})",
            test_exit, expected_exit
        )));
    } else if let Some(error_tags) = tags.error_tags
        && let cmd_output = String::from_utf8_lossy(&command.stderr)
        && let Some(missing_tag) = error_tags.iter().find(|tag| !cmd_output.contains(*tag))
    {
        return Err(Box::from(format!(
            "error output did not match expected error output, mismatched tag: {missing_tag}",
        )));
    }

    Ok(())
}

struct IntegrationTestTags {
    error_tags: Option<Vec<String>>,
    args: String,
}

fn extract_tags(path: &Path) -> datatest_stable::Result<IntegrationTestTags> {
    const ERROR_TAG: &str = "//~ ";
    const ARG_TAG: &str = "//= ";

    let mut is_pass = None;

    let mut trimmed_path = path;
    while let Some(parent) = trimmed_path.parent() {
        trimmed_path = parent;
        if parent.ends_with("pass") {
            is_pass = Some(true);
            break;
        } else if parent.ends_with("fail") {
            is_pass = Some(false);
            break;
        }
    }

    let Some(is_pass) = is_pass else {
        return Err(Box::from(
            "test must be in tests/fixtures/{pass,fail} folder",
        ));
    };

    let file_contents = std::fs::read_to_string(path)?;
    let error_tags: Option<Vec<String>> = (!is_pass).then(|| {
        file_contents
            .lines()
            .filter_map(|x| x.strip_prefix(ERROR_TAG).map(str::to_owned))
            .collect::<Vec<String>>()
    });

    let args: String = file_contents
        .lines()
        .filter_map(|x| x.strip_prefix(ARG_TAG))
        .collect();

    Ok(IntegrationTestTags { error_tags, args })
}
