use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::parse_command::ParsedCommandActionKind;

use crate::parse_command::shlex_join;

#[derive(Clone, Copy)]
enum VerbSource {
    FirstArg,
    PackageScript,
    Cargo,
    Cmake,
    Docker,
    Go,
    Gradle,
    Kubectl,
    Npm,
    Terraform,
}

#[derive(Clone, Copy)]
struct CommandSpec {
    executable: &'static str,
    source: VerbSource,
}

const COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        executable: "bun",
        source: VerbSource::Npm,
    },
    CommandSpec {
        executable: "cargo",
        source: VerbSource::Cargo,
    },
    CommandSpec {
        executable: "cmake",
        source: VerbSource::Cmake,
    },
    CommandSpec {
        executable: "composer",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "ctest",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "dart",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "deno",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "django-admin",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "docker",
        source: VerbSource::Docker,
    },
    CommandSpec {
        executable: "dotnet",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "expo",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "flutter",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "gh",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "go",
        source: VerbSource::Go,
    },
    CommandSpec {
        executable: "gradle",
        source: VerbSource::Gradle,
    },
    CommandSpec {
        executable: "gradlew",
        source: VerbSource::Gradle,
    },
    CommandSpec {
        executable: "hatch",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "just",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "kubectl",
        source: VerbSource::Kubectl,
    },
    CommandSpec {
        executable: "make",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "meson",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "mix",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "mvn",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "mvnw",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "ng",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "next",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "ninja",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "nox",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "npm",
        source: VerbSource::Npm,
    },
    CommandSpec {
        executable: "nx",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "phpunit",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "pip",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "pip3",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "pnpm",
        source: VerbSource::Npm,
    },
    CommandSpec {
        executable: "poetry",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "pytest",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "rake",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "rails",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "rspec",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "ruff",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "svelte-kit",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "swift",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "task",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "terraform",
        source: VerbSource::Terraform,
    },
    CommandSpec {
        executable: "tox",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "turbo",
        source: VerbSource::PackageScript,
    },
    CommandSpec {
        executable: "uv",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "vite",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "vue-cli-service",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "xcodebuild",
        source: VerbSource::FirstArg,
    },
    CommandSpec {
        executable: "yarn",
        source: VerbSource::Npm,
    },
];

pub(crate) fn action_from_tokens(tokens: &[String]) -> Option<ParsedCommand> {
    classify_tokens(tokens).map(|kind| {
        let command = shlex_join(tokens);
        ParsedCommand::Action {
            cmd: command.clone(),
            kind,
            detail: Some(command),
        }
    })
}

pub(crate) fn action_from_script(script: &str) -> Option<ParsedCommand> {
    let first_line = script.lines().next().unwrap_or(script).trim();
    if first_line.is_empty() {
        return None;
    }
    let lower_script = script.to_ascii_lowercase();
    if is_obvious_edit_script(&lower_script) {
        return Some(ParsedCommand::Action {
            cmd: script.to_string(),
            kind: ParsedCommandActionKind::Edit,
            detail: Some(first_line.to_string()),
        });
    }
    if lower_script.starts_with("$env:") || lower_script.starts_with("$psversiontable") {
        return Some(ParsedCommand::Action {
            cmd: script.to_string(),
            kind: ParsedCommandActionKind::Inspect,
            detail: Some(first_line.to_string()),
        });
    }
    if !is_single_simple_script(script, first_line) || has_unsupported_shell_syntax(first_line) {
        return None;
    }
    let tokens = shlex::split(first_line)
        .unwrap_or_else(|| first_line.split_whitespace().map(str::to_string).collect());
    classify_tokens(&tokens).map(|kind| ParsedCommand::Action {
        cmd: script.to_string(),
        kind,
        detail: Some(first_line.to_string()),
    })
}

fn classify_tokens(tokens: &[String]) -> Option<ParsedCommandActionKind> {
    let tokens = strip_env_prefix(tokens)?;
    let tokens = unwrap_known_wrapper(tokens)?;
    let (head, args) = tokens.split_first()?;
    let executable = normalized_executable(head);

    if is_simple_version_probe(&executable, args) {
        return Some(ParsedCommandActionKind::Inspect);
    }
    if matches!(executable.as_str(), "sleep" | "start-sleep" | "timeout") {
        return Some(ParsedCommandActionKind::Wait);
    }
    if is_powershell_inspect_command(&executable) {
        return Some(ParsedCommandActionKind::Inspect);
    }
    if executable == "git" {
        return classify_git(args);
    }
    if is_direct_test_tool(&executable) {
        return Some(ParsedCommandActionKind::Test);
    }
    if is_direct_lint_tool(&executable) {
        return Some(ParsedCommandActionKind::Lint);
    }
    if matches!(executable.as_str(), "node" | "python" | "python3") {
        return Some(classify_python(args));
    }
    if executable == "php" && args.first().is_some_and(|arg| arg == "artisan") {
        return classify_first_arg(&args[1..]);
    }
    find_spec(&executable).and_then(|spec| classify_by_source(spec.source, args))
}

fn classify_by_source(source: VerbSource, args: &[String]) -> Option<ParsedCommandActionKind> {
    match source {
        VerbSource::FirstArg => classify_first_arg(args),
        VerbSource::PackageScript => classify_package_targets(args),
        VerbSource::Cargo => classify_cargo(args),
        VerbSource::Cmake => classify_cmake(args),
        VerbSource::Docker => classify_docker(args),
        VerbSource::Go => classify_go(args),
        VerbSource::Gradle => classify_package_targets(args),
        VerbSource::Kubectl => classify_kubectl(args),
        VerbSource::Npm => classify_npm(args),
        VerbSource::Terraform => classify_terraform(args),
    }
}

fn unwrap_known_wrapper(tokens: &[String]) -> Option<&[String]> {
    let tokens = strip_env_prefix(tokens)?;
    let (head, args) = tokens.split_first()?;
    let executable = normalized_executable(head);
    match executable.as_str() {
        "env" | "cross-env" => unwrap_known_wrapper(strip_env_prefix(args)?),
        "cross-env-shell" => None,
        "npx" | "pnpx" | "bunx" | "uvx" => first_command_after_options(args),
        "npm"
            if args
                .first()
                .is_some_and(|arg| matches!(arg.as_str(), "exec" | "x")) =>
        {
            if args.iter().any(|arg| arg == "-c" || arg == "--call") {
                None
            } else {
                first_command_after_options(&args[1..])
            }
        }
        "pnpm"
            if args
                .first()
                .is_some_and(|arg| matches!(arg.as_str(), "exec" | "dlx")) =>
        {
            first_command_after_options(&args[1..])
        }
        "yarn" if args.first().is_some_and(|arg| arg == "dlx") => {
            first_command_after_options(&args[1..])
        }
        "bun"
            if args
                .first()
                .is_some_and(|arg| matches!(arg.as_str(), "x" | "run")) =>
        {
            first_command_after_options(&args[1..])
        }
        "deno" if args.first().is_some_and(|arg| arg == "task") => Some(args),
        "python" | "python3" if args.first().is_some_and(|arg| arg == "-m") => {
            args.get(1..).filter(|rest| !rest.is_empty())
        }
        "uv" if args.first().is_some_and(|arg| arg == "run") => {
            first_command_after_options(&args[1..])
        }
        "poetry" | "pipenv" | "hatch" if args.first().is_some_and(|arg| arg == "run") => {
            first_command_after_options(&args[1..])
        }
        "bundle" if args.first().is_some_and(|arg| arg == "exec") => {
            first_command_after_options(&args[1..])
        }
        "lerna" if args.first().is_some_and(|arg| arg == "run") => Some(args),
        _ => Some(tokens),
    }
}

fn strip_env_prefix(tokens: &[String]) -> Option<&[String]> {
    let mut index = 0;
    while index < tokens.len() && is_env_assignment(&tokens[index]) {
        index += 1;
    }
    tokens.get(index..).filter(|rest| !rest.is_empty())
}

fn first_command_after_options(args: &[String]) -> Option<&[String]> {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return args.get(index + 1..).filter(|rest| !rest.is_empty());
        }
        if !arg.starts_with('-') {
            return args.get(index..).filter(|rest| !rest.is_empty());
        }
        if option_consumes_next(arg) {
            index += 2;
        } else {
            index += 1;
        }
    }
    None
}

fn option_consumes_next(arg: &str) -> bool {
    matches!(
        arg,
        "-c" | "--package"
            | "-p"
            | "--prefix"
            | "--cwd"
            | "--filter"
            | "--workspace"
            | "--project"
            | "--config"
    )
}

fn find_spec(executable: &str) -> Option<CommandSpec> {
    COMMAND_SPECS
        .iter()
        .copied()
        .find(|spec| spec.executable == executable)
}

fn normalized_executable(raw: &str) -> String {
    let normalized_path = raw.replace('\\', "/");
    let file = normalized_path
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .trim_start_matches("./")
        .to_ascii_lowercase();
    file.strip_suffix(".exe")
        .or_else(|| file.strip_suffix(".cmd"))
        .or_else(|| file.strip_suffix(".bat"))
        .unwrap_or(&file)
        .to_string()
}

fn classify_first_arg(args: &[String]) -> Option<ParsedCommandActionKind> {
    let verb = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| arg.to_ascii_lowercase())?;
    classify_verb(&verb)
}

fn classify_package_targets(args: &[String]) -> Option<ParsedCommandActionKind> {
    let targets = package_targets(args);
    classify_target_set(&targets)
}

fn package_targets(args: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--" {
            break;
        }
        if matches!(
            arg.as_str(),
            "run" | "affected" | "-t" | "--target" | "--targets"
        ) {
            continue;
        }
        if matches!(arg.as_str(), "--filter" | "--scope" | "-p" | "--project") {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        targets.extend(arg.split(',').map(str::to_string));
    }
    targets
}

fn classify_target_set(targets: &[String]) -> Option<ParsedCommandActionKind> {
    let mut kinds = targets
        .iter()
        .filter_map(|target| classify_script_name(target))
        .collect::<Vec<_>>();
    kinds.dedup();
    match kinds.as_slice() {
        [kind] => Some(kind.clone()),
        [
            ParsedCommandActionKind::Test,
            ParsedCommandActionKind::Build,
            ParsedCommandActionKind::Lint,
        ]
        | [
            ParsedCommandActionKind::Build,
            ParsedCommandActionKind::Test,
            ParsedCommandActionKind::Lint,
        ] => Some(ParsedCommandActionKind::Run),
        [] => None,
        _ => Some(ParsedCommandActionKind::Run),
    }
}

fn classify_npm(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args {
        [cmd, rest @ ..] if matches!(cmd.as_str(), "run" | "run-script") => {
            classify_package_targets(rest)
        }
        [cmd, rest @ ..] if matches!(cmd.as_str(), "exec" | "x") => {
            classify_tokens(first_command_after_options(rest)?)
        }
        [cmd, ..] if matches!(cmd.as_str(), "ci" | "install") => {
            Some(ParsedCommandActionKind::Build)
        }
        [cmd, ..] => classify_script_name(cmd).or_else(|| classify_verb(cmd)),
        [] => None,
    }
}

fn classify_cargo(args: &[String]) -> Option<ParsedCommandActionKind> {
    let verb = args.first()?.as_str();
    match verb {
        "nextest" => Some(ParsedCommandActionKind::Test),
        "clippy" | "fmt" | "fix" => Some(ParsedCommandActionKind::Lint),
        "metadata" => Some(ParsedCommandActionKind::Inspect),
        _ => classify_verb(verb),
    }
}

fn classify_cmake(args: &[String]) -> Option<ParsedCommandActionKind> {
    if args.first().is_some_and(|arg| arg == "--build") {
        Some(ParsedCommandActionKind::Build)
    } else {
        classify_first_arg(args)
    }
}

fn classify_docker(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args {
        [compose, verb, ..] if compose == "compose" => match verb.as_str() {
            "up" | "exec" | "run" | "start" | "restart" | "down" => {
                Some(ParsedCommandActionKind::Run)
            }
            "ps" | "images" | "logs" | "config" | "version" => {
                Some(ParsedCommandActionKind::Inspect)
            }
            "build" | "pull" => Some(ParsedCommandActionKind::Build),
            _ => None,
        },
        [verb, ..] => classify_verb(verb),
        [] => None,
    }
}

fn classify_go(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first().map(String::as_str)? {
        "fmt" | "vet" => Some(ParsedCommandActionKind::Lint),
        "env" | "list" | "version" => Some(ParsedCommandActionKind::Inspect),
        verb => classify_verb(verb),
    }
}

fn classify_git(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first()?.as_str() {
        "grep" | "ls-files" => None,
        _ => Some(ParsedCommandActionKind::Git),
    }
}

fn classify_kubectl(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first().map(String::as_str)? {
        "apply" | "delete" | "exec" | "port-forward" | "rollout" | "scale" => {
            Some(ParsedCommandActionKind::Run)
        }
        "get" | "describe" | "logs" | "config" | "version" | "api-resources" => {
            Some(ParsedCommandActionKind::Inspect)
        }
        _ => None,
    }
}

fn classify_python(args: &[String]) -> ParsedCommandActionKind {
    if args.first().is_some_and(|arg| arg == "-m")
        && let Some(kind) = classify_python_module(&args[1..])
    {
        return kind;
    }
    if args
        .iter()
        .any(|arg| arg.contains("write_text") || arg.contains("Path(") && arg.contains(".write"))
    {
        ParsedCommandActionKind::Edit
    } else {
        ParsedCommandActionKind::Run
    }
}

fn classify_python_module(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first().map(String::as_str)? {
        "pytest" | "unittest" => Some(ParsedCommandActionKind::Test),
        "pip" => args
            .get(1)
            .and_then(|verb| classify_verb(verb))
            .or(Some(ParsedCommandActionKind::Build)),
        module => classify_verb(module),
    }
}

fn classify_terraform(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first().map(String::as_str)? {
        "test" => Some(ParsedCommandActionKind::Test),
        "fmt" | "validate" => Some(ParsedCommandActionKind::Lint),
        "init" | "plan" | "apply" | "destroy" | "import" => Some(ParsedCommandActionKind::Run),
        "show" | "state" | "version" | "providers" | "workspace" | "output" => {
            Some(ParsedCommandActionKind::Inspect)
        }
        _ => None,
    }
}

fn classify_verb(verb: &str) -> Option<ParsedCommandActionKind> {
    let verb = verb.to_ascii_lowercase();
    if is_test_verb(&verb) {
        Some(ParsedCommandActionKind::Test)
    } else if is_build_verb(&verb) {
        Some(ParsedCommandActionKind::Build)
    } else if is_lint_verb(&verb) {
        Some(ParsedCommandActionKind::Lint)
    } else if is_run_verb(&verb) {
        Some(ParsedCommandActionKind::Run)
    } else if is_inspect_verb(&verb) {
        Some(ParsedCommandActionKind::Inspect)
    } else {
        None
    }
}

fn classify_script_name(script: &str) -> Option<ParsedCommandActionKind> {
    let script = script.to_ascii_lowercase();
    if script.contains("test") || script.contains("spec") || script.contains("e2e") {
        Some(ParsedCommandActionKind::Test)
    } else if script.contains("lint")
        || script.contains("typecheck")
        || script.contains("format")
        || script == "fmt"
        || script == "fix"
    {
        Some(ParsedCommandActionKind::Lint)
    } else if script.contains("build")
        || script.contains("compile")
        || script == "package"
        || script == "pack"
        || script == "publish"
        || script == "restore"
        || script == "install"
        || script == "ci"
        || script == "sync"
        || script == "fetch"
        || script == "generate"
        || script == "collectstatic"
    {
        Some(ParsedCommandActionKind::Build)
    } else if matches!(
        script.as_str(),
        "run"
            | "start"
            | "serve"
            | "server"
            | "dev"
            | "preview"
            | "watch"
            | "bootrun"
            | "runserver"
            | "migrate"
            | "migration"
            | "deploy"
    ) || script.contains("migrate")
        || script.contains("deploy")
    {
        Some(ParsedCommandActionKind::Run)
    } else {
        classify_verb(&script)
    }
}

fn is_test_verb(verb: &str) -> bool {
    matches!(
        verb,
        "test" | "spec" | "e2e" | "verify" | "phpunit" | "pest" | "ctest" | "rspec"
    )
}

fn is_build_verb(verb: &str) -> bool {
    matches!(
        verb,
        "build"
            | "compile"
            | "package"
            | "pack"
            | "publish"
            | "restore"
            | "install"
            | "ci"
            | "sync"
            | "fetch"
            | "generate"
            | "collectstatic"
            | "pull"
    )
}

fn is_lint_verb(verb: &str) -> bool {
    matches!(
        verb,
        "lint"
            | "fmt"
            | "format"
            | "fix"
            | "check"
            | "typecheck"
            | "validate"
            | "vet"
            | "analyze"
            | "clippy"
            | "mypy"
            | "pyright"
    )
}

fn is_run_verb(verb: &str) -> bool {
    matches!(
        verb,
        "run"
            | "start"
            | "serve"
            | "server"
            | "dev"
            | "preview"
            | "watch"
            | "bootrun"
            | "runserver"
            | "migrate"
            | "deploy"
            | "up"
            | "apply"
            | "exec"
            | "port-forward"
            | "delete"
    ) || verb.contains("migrate")
}

fn is_inspect_verb(verb: &str) -> bool {
    matches!(
        verb,
        "version"
            | "--version"
            | "-v"
            | "-version"
            | "--info"
            | "info"
            | "help"
            | "--help"
            | "-h"
            | "list"
            | "ls"
            | "show"
            | "status"
            | "env"
            | "doctor"
            | "config"
            | "outdated"
            | "audit"
            | "tree"
            | "metadata"
            | "logs"
            | "describe"
            | "get"
            | "ps"
            | "images"
    )
}

fn is_direct_test_tool(executable: &str) -> bool {
    matches!(
        executable,
        "pytest" | "rspec" | "phpunit" | "pest" | "ctest" | "tox" | "nox" | "vitest"
    )
}

fn is_direct_lint_tool(executable: &str) -> bool {
    matches!(executable, "mypy" | "pyright" | "eslint" | "tsc")
}

fn is_simple_version_probe(head: &str, tail: &[String]) -> bool {
    tail.first()
        .is_some_and(|arg| is_inspect_verb(&arg.to_ascii_lowercase()))
        && (find_spec(head).is_some()
            || matches!(
                head,
                "node" | "python" | "python3" | "ffmpeg" | "rg" | "mypy" | "pyright" | "eslint"
            ))
}

fn is_powershell_inspect_command(command: &str) -> bool {
    matches!(
        command,
        "test-path"
            | "resolve-path"
            | "get-item"
            | "gi"
            | "get-itemproperty"
            | "get-acl"
            | "get-filehash"
            | "get-process"
            | "get-service"
            | "get-command"
            | "where"
            | "where.exe"
    )
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_single_simple_script(script: &str, first_line: &str) -> bool {
    let mut non_empty_lines = script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    if non_empty_lines.next() != Some(first_line) || non_empty_lines.next().is_some() {
        return false;
    }
    !first_line.contains(';')
        && !first_line.contains('|')
        && !first_line.contains("&&")
        && !first_line.contains("||")
}

fn has_unsupported_shell_syntax(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    lower.contains("where-object")
        || lower.contains("foreach-object")
        || lower.contains("invoke-expression")
        || lower.contains("encodedcommand")
        || lower.contains("| ?")
        || lower.contains("| %")
}

fn is_obvious_edit_script(command: &str) -> bool {
    command.starts_with("apply_patch")
        || command.contains("apply_patch <<")
        || command.starts_with("python") && command.contains("write_text")
        || command.contains(" > ")
        || command.contains(" >> ")
        || command.contains(" 1> ")
        || command.contains(" 2> ")
        || command.starts_with("set-content ")
        || command.starts_with("add-content ")
        || command.starts_with("out-file ")
        || command.starts_with("new-item ")
        || command.starts_with("remove-item ")
        || command.starts_with("copy-item ")
        || command.starts_with("move-item ")
        || command.starts_with("rename-item ")
        || command.contains(" set-content ")
        || command.contains(" add-content ")
        || command.contains(" out-file ")
        || command.contains(" new-item ")
        || command.contains(" remove-item ")
        || command.contains(" copy-item ")
        || command.contains(" move-item ")
        || command.contains(" rename-item ")
        || command.contains("; set-content ")
        || command.contains("; add-content ")
        || command.contains("; out-file ")
        || command.contains("; new-item ")
        || command.contains("; remove-item ")
        || command.contains("; copy-item ")
        || command.contains("; move-item ")
        || command.contains("; rename-item ")
}
