use super::*;

#[test]
fn allows_powershell_listing_and_inspection() {
    assert!(is_read_only(
        "Get-ChildItem -Force -Path .",
        EscapeChar::Backtick
    ));
    assert!(is_read_only(
        "Get-Content -LiteralPath 'my file.txt'",
        EscapeChar::Backtick
    ));
    assert!(is_read_only(
        "gci -Recurse | Select-Object FullName",
        EscapeChar::Backtick
    ));
    assert!(is_read_only(
        "Get-Location; Get-FileHash file.txt",
        EscapeChar::Backtick
    ));
    assert!(is_read_only(
        "Get-Process | Sort-Object CPU",
        EscapeChar::Backtick
    ));
    assert!(is_read_only("GET-CHILDITEM -Force", EscapeChar::Backtick));
}

#[test]
fn allows_posix_listing_and_searches() {
    assert!(is_read_only("ls -la", EscapeChar::Backslash));
    assert!(is_read_only(
        "pwd && cat README.md | head -n 20",
        EscapeChar::Backslash
    ));
    assert!(is_read_only("rg -n 'pattern$' src", EscapeChar::Backslash));
    assert!(is_read_only(
        "find . -type f -name '*.rs'",
        EscapeChar::Backslash
    ));
    assert!(is_read_only(
        "stat 'file with spaces.txt'",
        EscapeChar::Backslash
    ));
    assert!(is_read_only(
        "cat 'literal>filename'",
        EscapeChar::Backslash
    ));
    assert!(is_read_only("wc -l < README.md", EscapeChar::Backslash));
    assert!(is_read_only("cat README.md | sort", EscapeChar::Backslash));
}

#[test]
fn allows_git_inspection() {
    assert!(is_read_only("git status --short", EscapeChar::Backslash));
    assert!(is_read_only(
        "git --no-pager diff --stat",
        EscapeChar::Backslash
    ));
    assert!(is_read_only(
        "git -C 'my repo' log -n 5 --oneline",
        EscapeChar::Backslash
    ));
    assert!(is_read_only(
        "git branch --show-current",
        EscapeChar::Backslash
    ));
    assert!(is_read_only("git tag --list", EscapeChar::Backslash));
}

#[test]
fn asks_for_file_modifications() {
    assert!(!is_read_only(
        "Set-Content file.txt changed",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only(
        "Add-Content file.txt changed",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only("Remove-Item file.txt", EscapeChar::Backtick));
    assert!(!is_read_only("rm file.txt", EscapeChar::Backslash));
    assert!(!is_read_only("cp old.txt new.txt", EscapeChar::Backslash));
    assert!(!is_read_only("git checkout main", EscapeChar::Backslash));
    assert!(!is_read_only(
        "git branch new-branch",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only("git tag v1", EscapeChar::Backslash));
}

#[test]
fn asks_for_mixed_commands_and_redirections() {
    assert!(!is_read_only(
        "Get-ChildItem; Remove-Item file.txt",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only(
        "Get-Content a.txt | Set-Content b.txt",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only("ls && touch file.txt", EscapeChar::Backslash));
    assert!(!is_read_only("cat a.txt > b.txt", EscapeChar::Backslash));
    assert!(!is_read_only(
        "Get-ChildItem 2> errors.txt",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only(
        "echo changed >> file.txt",
        EscapeChar::Backslash
    ));
}

#[test]
fn asks_for_executable_expansions_and_helpers() {
    assert!(!is_read_only("ls $(touch file.txt)", EscapeChar::Backslash));
    assert!(!is_read_only(
        "echo \"$(Remove-Item file.txt)\"",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only("ls `touch file.txt`", EscapeChar::Backslash));
    assert!(!is_read_only(
        "Get-ChildItem | Select-Object { Remove-Item file.txt }",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only(
        "rg --pre 'touch file.txt' pattern",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only(
        "find . -exec rm '{}' ';'",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only("find . -delete", EscapeChar::Backslash));
    assert!(!is_read_only(
        "find . -fls output.txt",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only("tree -ao output.txt", EscapeChar::Backslash));
    assert!(!is_read_only(
        "sort --output output.txt input.txt",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only(
        "sort --compress-program helper input.txt",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only(
        "git diff --output patch.txt",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only(
        "git grep --open-files-in-pager pattern",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only(
        "GIT_EXTERNAL_DIFF=helper git diff",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only(
        "ls; GIT_EXTERNAL_DIFF=helper git diff",
        EscapeChar::Backslash
    ));
}

#[test]
fn asks_when_the_command_cannot_be_classified() {
    assert!(!is_read_only("", EscapeChar::Backslash));
    assert!(!is_read_only("unknown-command", EscapeChar::Backslash));
    assert!(!is_read_only(
        "powershell -Command 'Remove-Item file.txt'",
        EscapeChar::Backtick
    ));
    assert!(!is_read_only(
        "python -c 'open(\"file.txt\", \"w\")'",
        EscapeChar::Backslash
    ));
    assert!(!is_read_only("cat 'unterminated", EscapeChar::Backslash));
    assert!(!is_read_only("LS", EscapeChar::Backslash));
}
