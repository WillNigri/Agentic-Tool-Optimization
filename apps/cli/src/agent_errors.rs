// Agent-robust CLI surface.
//
// The primary consumer of this CLI is an LLM agent shelling out from
// another process (see output.rs — JSON is the default for that reason).
// But clap's *errors* (missing subcommand, missing required arg, unknown
// flag) print human usage text to stderr and exit 2 — a dead end for a
// caller parsing JSON. An agent that types `ato optimize` (a command group
// that needs a subcommand) gets prose it can't act on, and gives up.
//
// This module closes that gap two ways:
//   1. handle_parse_error — on a clap parse failure, emit a STRUCTURED JSON
//      error envelope (problem, command path, available subcommands,
//      required args, a ready-to-run example) so a wrong call self-corrects
//      in one retry. Falls back to clap's native rendering under --human.
//   2. command_schema_json — `ato schema` dumps the whole command tree
//      (every command, subcommand, required arg) as JSON so an agent can
//      introspect the surface once instead of guessing.

use clap::error::ErrorKind;
use clap::Command;
use serde_json::{json, Value};

/// Long/short forms of every flag declared on a command.
fn all_flags(node: &Command) -> std::collections::HashSet<String> {
    node.get_arguments()
        .flat_map(|a| {
            let mut v = Vec::new();
            if let Some(l) = a.get_long() {
                v.push(format!("--{}", l));
            }
            if let Some(s) = a.get_short() {
                v.push(format!("-{}", s));
            }
            v
        })
        .collect()
}

/// The flags on a command that consume a following value (ArgAction::Set/
/// Append). Used so the tree-walk doesn't mistake a flag's VALUE for a
/// subcommand (e.g. `ato --db optimize` — "optimize" is the --db value).
fn value_flags(node: &Command) -> std::collections::HashSet<String> {
    node.get_arguments()
        .filter(|a| matches!(a.get_action(), clap::ArgAction::Set | clap::ArgAction::Append))
        .flat_map(|a| {
            let mut v = Vec::new();
            if let Some(l) = a.get_long() {
                v.push(format!("--{}", l));
            }
            if let Some(s) = a.get_short() {
                v.push(format!("-{}", s));
            }
            v
        })
        .collect()
}

/// Walk the command tree along the user's tokens to find where parsing got
/// stuck, and report what was actually available there. Returns
/// (command_path, available_subcommands, required_args, example).
fn analyze(root: &Command, raw_args: &[String]) -> (String, Vec<String>, Vec<String>, Option<String>) {
    let mut node = root;
    let mut path = vec![root.get_name().to_string()];
    // Root globals (e.g. --db value, --human/--quiet bool) apply at every
    // node, so seed the known/value flag sets with them.
    let global_all = all_flags(root);
    let global_value = value_flags(root);
    let mut expect_value = false;

    // Skip arg0 (the program name); follow tokens that match subcommands.
    for tok in raw_args.iter().skip(1) {
        if expect_value {
            expect_value = false; // this token is the previous flag's value
            continue;
        }
        if tok.starts_with('-') {
            let key = tok.split('=').next().unwrap_or(tok); // strip =value form
            let mut known = all_flags(node);
            known.extend(global_all.iter().cloned());
            if !known.contains(key) {
                // Unknown flag at this node — that's where parsing failed.
                // Stop here rather than letting a later token be mistaken for
                // a subcommand (`ato loop --foo share` must report `ato loop`,
                // not `ato loop share`).
                break;
            }
            if !tok.contains('=') {
                let mut vf = value_flags(node);
                vf.extend(global_value.iter().cloned());
                if vf.contains(key) {
                    expect_value = true; // next token is this flag's value
                }
            }
            continue;
        }
        match node.find_subcommand(tok) {
            Some(sub) => {
                node = sub;
                path.push(tok.clone());
            }
            None => break,
        }
    }

    let available: Vec<String> = node
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .filter(|n| n != "help")
        .collect();

    // Positionals render as <id>; options render as --long. Preserve clap's
    // declaration order so a multi-positional example reads correctly.
    let required: Vec<String> = node
        .get_arguments()
        .filter(|a| a.is_required_set())
        .map(|a| match a.get_long() {
            Some(l) => format!("--{}", l),
            None => format!("<{}>", a.get_id()),
        })
        .collect();

    let example = if let Some(first) = available.first() {
        Some(format!("{} {}", path.join(" "), first))
    } else if !required.is_empty() {
        // Include ALL required args. Options need a trailing <value>;
        // positionals already read as <name>.
        let parts: Vec<String> = required
            .iter()
            .map(|r| {
                if r.starts_with("--") {
                    format!("{} <value>", r)
                } else {
                    r.clone()
                }
            })
            .collect();
        Some(format!("{} {}", path.join(" "), parts.join(" ")))
    } else {
        None
    };

    (path.join(" "), available, required, example)
}

/// Handle a clap parse error. Never returns — prints and exits.
///
/// Help / version requests aren't errors: render them clap's way and exit 0.
/// Everything else becomes a structured JSON envelope on stderr (or clap's
/// native text under --human), exiting with clap's code (2 for usage errors).
pub fn handle_parse_error(err: clap::Error, raw_args: &[String], root: Command) -> ! {
    match err.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = err.print();
            std::process::exit(0);
        }
        _ => {}
    }

    let human = raw_args.iter().any(|a| a == "--human");
    let code = err.exit_code();
    if human {
        let _ = err.print();
        std::process::exit(code);
    }

    let (path, available, required, example) = analyze(&root, raw_args);

    let mut problem = match err.kind() {
        ErrorKind::MissingSubcommand | ErrorKind::InvalidSubcommand => {
            "missing_or_invalid_subcommand"
        }
        ErrorKind::MissingRequiredArgument => "missing_required_argument",
        ErrorKind::UnknownArgument => "unknown_argument",
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => "invalid_value",
        _ => "invalid_invocation",
    };
    // Some clap versions don't surface MissingSubcommand for a bare command
    // group; if we're parked at a node that has subcommands, that's what it
    // is — classify it accordingly so the label matches the payload.
    if problem == "invalid_invocation" && !available.is_empty() {
        problem = "missing_or_invalid_subcommand";
    }

    // First line of clap's message, stripped of the "error: " prefix —
    // concise and names the specific problem for InvalidSubcommand /
    // MissingRequiredArgument. For the bare-command-group case some clap
    // versions surface the command's `about` text here instead, which is
    // misleading — synthesize a clear message when there are subcommands to
    // pick and clap didn't already mention one.
    let raw_first = err
        .to_string()
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches("error: ")
        .to_string();
    let mentions_usage = {
        let l = raw_first.to_lowercase();
        l.contains("subcommand") || l.contains("argument") || l.contains("unexpected") || l.contains("value")
    };
    let message = if !available.is_empty() && !mentions_usage {
        "a subcommand is required".to_string()
    } else {
        raw_first
    };

    let envelope: Value = json!({
        "error": problem,
        "command": path,
        "message": message,
        "available_subcommands": available,
        "required_args": required,
        "example": example,
        "hint": "Run `ato schema` for the full command tree as JSON, or `ato <command> --help`.",
    });

    eprintln!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| message.clone()));
    std::process::exit(code);
}

/// Recursively introspect a clap Command into a JSON tree an agent can read
/// to learn the exact surface (commands, subcommands, required args, types).
pub fn command_schema_json(root: &Command) -> Value {
    fn walk(cmd: &Command) -> Value {
        let subcommands: Vec<Value> = cmd
            .get_subcommands()
            .filter(|s| s.get_name() != "help")
            .map(walk)
            .collect();

        let args: Vec<Value> = cmd
            .get_arguments()
            .filter(|a| {
                let id = a.get_id().as_str();
                id != "help" && id != "version"
            })
            .map(|a| {
                let takes_value = matches!(
                    a.get_action(),
                    clap::ArgAction::Set | clap::ArgAction::Append
                );
                json!({
                    "name": a.get_id().as_str(),
                    "long": a.get_long(),
                    "short": a.get_short().map(|c| c.to_string()),
                    "required": a.is_required_set(),
                    "takes_value": takes_value,
                    "help": a.get_help().map(|h| h.to_string()),
                })
            })
            .collect();

        json!({
            "name": cmd.get_name(),
            "about": cmd.get_about().map(|a| a.to_string()),
            "args": args,
            "subcommands": subcommands,
        })
    }
    walk(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Arg, ArgAction, Command};

    fn sample() -> Command {
        Command::new("ato")
            .arg(
                // a global value-taking flag, like the real --db
                Arg::new("db").long("db").global(true).action(ArgAction::Set),
            )
            .subcommand(
                Command::new("optimize")
                    .about("optimize stuff")
                    .subcommand(Command::new("recommend"))
                    .subcommand(Command::new("run")),
            )
            .subcommand(
                Command::new("models").arg(
                    Arg::new("slug").long("slug").required(true).action(ArgAction::Set),
                ),
            )
            .subcommand(
                Command::new("dispatch")
                    .arg(Arg::new("runtime").required(true).action(ArgAction::Set))
                    .arg(Arg::new("prompt").required(true).action(ArgAction::Set)),
            )
    }

    #[test]
    fn analyze_reports_available_subcommands_for_bare_group() {
        let args = vec!["ato".into(), "optimize".into()];
        let (path, available, required, example) = analyze(&sample(), &args);
        assert_eq!(path, "ato optimize");
        assert_eq!(available, vec!["recommend", "run"]);
        assert!(required.is_empty());
        assert_eq!(example.as_deref(), Some("ato optimize recommend"));
    }

    #[test]
    fn analyze_reports_required_arg_when_no_subcommands() {
        let args = vec!["ato".into(), "models".into()];
        let (path, available, required, example) = analyze(&sample(), &args);
        assert_eq!(path, "ato models");
        assert!(available.is_empty());
        assert_eq!(required, vec!["--slug"]);
        assert_eq!(example.as_deref(), Some("ato models --slug <value>"));
    }

    #[test]
    fn analyze_stops_at_unknown_token() {
        // unknown subcommand → stay at the parent and list its options
        let args = vec!["ato".into(), "optimize".into(), "nope".into()];
        let (path, available, _required, _example) = analyze(&sample(), &args);
        assert_eq!(path, "ato optimize");
        assert_eq!(available, vec!["recommend", "run"]);
    }

    #[test]
    fn analyze_does_not_treat_a_flag_value_as_a_subcommand() {
        // `ato --db optimize` — "optimize" is the --db value, NOT a command.
        let args = vec!["ato".into(), "--db".into(), "optimize".into()];
        let (path, available, _required, _example) = analyze(&sample(), &args);
        assert_eq!(path, "ato"); // stayed at root
        assert!(available.contains(&"optimize".to_string()));
    }

    #[test]
    fn analyze_unknown_flag_stops_the_walk() {
        // `ato optimize --foo recommend` — --foo is unknown at `optimize`, so
        // parsing failed there; must NOT advance into `recommend`.
        let args = vec!["ato".into(), "optimize".into(), "--foo".into(), "recommend".into()];
        let (path, available, _required, _example) = analyze(&sample(), &args);
        assert_eq!(path, "ato optimize");
        assert!(available.contains(&"recommend".to_string()));
    }

    #[test]
    fn analyze_known_bool_flag_does_not_stop_the_walk() {
        // `ato --db x optimize` (value flag) already covered; here a known
        // global bool-ish flag must let the walk continue. Use --db=x form.
        let args = vec!["ato".into(), "--db=x".into(), "optimize".into()];
        let (path, _available, _required, _example) = analyze(&sample(), &args);
        assert_eq!(path, "ato optimize");
    }

    #[test]
    fn analyze_example_includes_all_required_positionals() {
        let args = vec!["ato".into(), "dispatch".into()];
        let (_path, available, required, example) = analyze(&sample(), &args);
        assert!(available.is_empty());
        assert_eq!(required, vec!["<runtime>", "<prompt>"]);
        assert_eq!(example.as_deref(), Some("ato dispatch <runtime> <prompt>"));
    }

    #[test]
    fn schema_json_is_recursive_and_marks_required() {
        let v = command_schema_json(&sample());
        assert_eq!(v["name"], "ato");
        let subs = v["subcommands"].as_array().unwrap();
        let models = subs.iter().find(|s| s["name"] == "models").unwrap();
        let slug = models["args"].as_array().unwrap()[0].clone();
        assert_eq!(slug["name"], "slug");
        assert_eq!(slug["required"], true);
        assert_eq!(slug["takes_value"], true);
    }
}
