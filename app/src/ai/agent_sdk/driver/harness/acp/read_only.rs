use warp_completer::parsers::simple::all_parsed_commands;
use warp_util::path::EscapeChar;

pub(super) fn is_read_only(command: &str, escape_char: EscapeChar) -> bool {
    if !has_literal_arguments(command, escape_char) {
        return false;
    }
    let commands: Vec<_> = all_parsed_commands(command, escape_char).collect();
    !commands.is_empty()
        && commands.iter().all(|command| {
            let arguments: Vec<_> = command
                .parts
                .iter()
                .map(|part| part.item.as_str())
                .collect();
            arguments_are_read_only(&arguments, matches!(escape_char, EscapeChar::Backtick))
        })
}

fn has_literal_arguments(command: &str, escape_char: EscapeChar) -> bool {
    let mut quote = None;
    let mut characters = command.chars();
    while let Some(character) = characters.next() {
        if quote == Some('\'') {
            if character == '\'' {
                quote = None;
            }
            continue;
        }
        if character == '\\' && matches!(escape_char, EscapeChar::Backslash) {
            if characters.next().is_none() {
                return false;
            }
            continue;
        }
        if character == '"' {
            quote = if quote == Some('"') { None } else { Some('"') };
            continue;
        }
        if character == '\'' && quote.is_none() {
            quote = Some('\'');
            continue;
        }
        // Expansions can execute commands, and assignments can install executable helpers such as
        // GIT_EXTERNAL_DIFF. The completion parser deliberately discards leading assignments.
        if matches!(character, '$' | '`' | '\0')
            || (quote.is_none()
                && matches!(character, '(' | ')' | '{' | '}' | '>' | '=' | '^' | '%'))
        {
            return false;
        }
    }
    quote.is_none()
}

fn arguments_are_read_only(arguments: &[&str], powershell: bool) -> bool {
    let Some((name, arguments)) = arguments.split_first() else {
        return false;
    };
    let normalized = if powershell {
        name.to_ascii_lowercase()
    } else {
        (*name).to_owned()
    };
    let name = normalized.strip_suffix(".exe").unwrap_or(&normalized);
    match name {
        "git" => git_is_read_only(arguments),
        "sort" => !arguments.iter().any(|argument| {
            argument.starts_with("--output")
                || argument.starts_with("--compress-program")
                || (argument.starts_with('-')
                    && !argument.starts_with("--")
                    && argument.contains('o'))
        }),
        "rg" => !arguments.iter().any(|argument| {
            argument.starts_with("--pre") || argument.starts_with("--hostname-bin")
        }),
        "find" if !powershell => !arguments.iter().any(|argument| {
            matches!(
                *argument,
                "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir"
            ) || argument.starts_with("-fprint")
                || *argument == "-fprintf"
                || *argument == "-fls"
        }),
        "tree" => !arguments.iter().any(|argument| {
            argument.starts_with('-') && !argument.starts_with("--") && argument.contains('o')
        }),
        "file" => !arguments.iter().any(|argument| {
            argument.starts_with("--compile")
                || (argument.starts_with('-')
                    && !argument.starts_with("--")
                    && argument.contains('C'))
        }),
        "hostname" => arguments.iter().all(|argument| {
            matches!(
                *argument,
                "-f" | "--fqdn" | "-s" | "--short" | "-I" | "--all-ip-addresses"
            )
        }),
        "ls" | "pwd" | "cat" | "head" | "tail" | "wc" | "grep" | "egrep" | "fgrep" | "stat"
        | "which" | "whoami" | "uname" | "id" | "du" | "df" | "ps" | "readlink" | "realpath"
        | "cut" | "paste" | "comm" | "diff" | "echo" => true,
        "dir"
        | "gci"
        | "get-childitem"
        | "gc"
        | "type"
        | "get-content"
        | "gl"
        | "get-location"
        | "get-item"
        | "get-itemproperty"
        | "get-itempropertyvalue"
        | "get-filehash"
        | "test-path"
        | "resolve-path"
        | "split-path"
        | "join-path"
        | "get-process"
        | "get-service"
        | "get-date"
        | "get-host"
        | "get-computerinfo"
        | "get-ciminstance"
        | "get-command"
        | "get-help"
        | "select-string"
        | "sls"
        | "select-object"
        | "select"
        | "measure-object"
        | "group-object"
        | "sort-object"
        | "format-table"
        | "format-list"
        | "format-wide"
        | "out-string"
        | "convertto-json"
        | "convertfrom-json"
        | "convertto-csv"
        | "convertfrom-csv"
        | "write-output"
        | "write-host"
            if powershell =>
        {
            true
        }
        _ => false,
    }
}

fn git_is_read_only(mut arguments: &[&str]) -> bool {
    if arguments == ["--version"] {
        return true;
    }
    while let Some((argument, rest)) = arguments.split_first() {
        match *argument {
            "--no-pager" | "--no-optional-locks" => arguments = rest,
            "-C" if !rest.is_empty() => arguments = &rest[1..],
            _ => break,
        }
    }
    let Some((subcommand, arguments)) = arguments.split_first() else {
        return false;
    };
    if arguments.iter().any(|argument| {
        argument.starts_with("--output")
            || argument.starts_with("--ext-diff")
            || argument.starts_with("--textconv")
            || (argument.starts_with('-') && !argument.starts_with("--") && argument.contains('o'))
            || argument.starts_with("--filters")
            || argument.starts_with("--open-files-in-pager")
            || argument.starts_with("-O")
    }) {
        return false;
    }
    match *subcommand {
        "status" | "diff" | "log" | "show" | "ls-files" | "ls-tree" | "grep" | "rev-parse"
        | "describe" | "shortlog" | "blame" | "cat-file" | "merge-base" | "rev-list"
        | "for-each-ref" => true,
        "branch" | "tag" => arguments.iter().all(|argument| {
            matches!(
                *argument,
                "--list"
                    | "--show-current"
                    | "-a"
                    | "--all"
                    | "-r"
                    | "--remotes"
                    | "-v"
                    | "-vv"
                    | "--verbose"
                    | "--no-color"
            )
        }),
        _ => false,
    }
}

#[cfg(test)]
#[path = "read_only_tests.rs"]
mod tests;
