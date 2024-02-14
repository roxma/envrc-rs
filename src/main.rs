extern crate clap;

use clap::{Parser, Subcommand, ValueEnum};
use std::env::{current_dir, current_exe, var};
use std::fs::{canonicalize, create_dir_all, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Shell {
    Bash,
    Zsh,
}

#[derive(Subcommand)]
enum Commands {
    /// Init the prompt command
    Init {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Called for each prompt
    Hook,
    /// Grant permission to envrc to load the .envrc
    Allow,
    /// Remove the permission
    Deny {
        /// .envrc files to be denied
        file: Option<PathBuf>,
    },
    /// Remove expired or non-existing-file permissions
    Prune,
}

fn main() {
    let args = Cli::parse();
    match args.command {
        Commands::Init { shell } => {
            println!(
                "{}",
                match shell {
                    Shell::Bash => "PROMPT_COMMAND='eval \"$(envrc hook)\"'",
                    Shell::Zsh => "precmd() { eval \"$(envrc hook)\"; }",
                }
            );
        }
        Commands::Hook => {
            do_hook();
        }
        Commands::Allow => {
            let cur_dir = current_dir().unwrap();
            let rc_found = find_envrc(cur_dir).unwrap();
            add_allow(&rc_found);
        }
        Commands::Deny { file } => {
            let rc_found = if let Some(file) = file {
                let mut path = canonicalize(file).unwrap();
                if path.is_dir() {
                    let dir = PathBuf::from(path.clone());
                    path = PathBuf::from(find_envrc(dir).unwrap());
                }
                String::from(path.to_str().unwrap())
            } else {
                let cur_dir = current_dir().unwrap();
                find_envrc(cur_dir).unwrap()
            };
            remove_allow(&rc_found);
            println!("{} is denied", rc_found);
        }
        Commands::Prune => {
            prune_allow();
        }
    }
}

fn do_hook() {
    let exe = current_exe()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap();

    let begin = format!(
        r#"
{{
while :
do
 if [ -n "$ENVRC_PPID" -a "$ENVRC_PPID" != "$PPID" ]
 then
  unset ENVRC_LOAD
  unset ENVRC_PPID
  unset ENVRC_TMP
  unset envrc_loaded
  unset envrc_not_allowed
  eval "$({exe} hook)"
  break
 fi
"#,
        exe = exe
    );
    println!("{}", begin);

    do_hook_wrapped();

    let end = format!(
        r#"
break
done
}}"#
    );
    println!("{}", end);
}

fn do_hook_wrapped() {
    let rc_cur = current_envrc();
    let cur_dir = current_dir().unwrap();
    let rc_found = find_envrc(cur_dir);

    let rc_found = rc_found.as_ref();
    let rc_cur = rc_cur.as_ref();

    let exe = current_exe()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap();

    if rc_cur.is_some() {
        let rc_cur = rc_cur.unwrap();

        update_if_allowed(rc_cur);

        if is_out_of_scope(rc_cur) {
            return shell_to_parent();
        }
    }

    let allow_err = check_allow(rc_found);

    if rc_found == rc_cur {
        if allow_err.is_some() {
            return shell_to_parent_eval(format!(
                r#"
                    envrc_not_allowed={}
                    "#,
                rc_cur.unwrap()
            ));
        }

        let p = format!(
            r#"
if [ -n "$ENVRC_LOAD" -a -z "$envrc_loaded" ]
then
    envrc_loaded=1
    echo "envrc: loading [$ENVRC_LOAD]"
    if [ -f "$ENVRC_LOAD" ]
    then
        . "$ENVRC_LOAD"
    else
        . "$ENVRC_LOAD/envrc"
    fi
fi
envrc_not_allowed=
            "#
        );

        println!("{}", p);
        return;
    }

    if allow_err.is_some() {
        let allow_err = match allow_err.unwrap() {
            AllowError::AllowDenied => "NOT ALLOWED.",
            AllowError::AllowExpired => "PERMISSION EXPIRED.",
        };

        // found an .envrc, but it's not allowed to be loaded
        let p = format!(
            r#"
if [ "$envrc_not_allowed" != "{rc_found}" ]
then
    tput setaf 3
    tput bold
    echo "envrc: [{rc_found}] {allow_err}"
    echo '       try execute "envrc allow"'
    tput sgr0
    envrc_not_allowed="{rc_found}"
fi
             "#,
            rc_found = rc_found.unwrap(),
            allow_err = allow_err
        );

        println!("{}", p);
        return;
    }

    if rc_cur.is_some() {
        // we're in an .envrc scope, but need to load another one
        return shell_to_parent();
    }

    // we're in parent shell, ENVRC_LOAD is empty
    // now we're going to load rc_found
    let rc_found = rc_found.unwrap();

    let p = format!(
        r#"
if [[ "$(jobs)" = "" ]]
then
    echo "envrc: spwan $SHELL"
    export ENVRC_TMP="$(mktemp "${{TMPDIR-/tmp}}/envrc.XXXXXXXXXX")"
    ENVRC_LOAD="{rc_found}" ENVRC_PPID=$$ $SHELL
    eval "$(if [ -s $ENVRC_TMP ]; then cat $ENVRC_TMP; else echo exit 0; fi; rm $ENVRC_TMP)"
    unset ENVRC_TMP
    eval "$({exe} hook)"
else
    echo "envrc: you have jobs, cannot load envrc"
fi
        "#,
        rc_found = rc_found,
        exe = exe
    );

    println!("{}", p);
}

fn shell_to_parent() {
    shell_to_parent_eval(String::new())
}

fn shell_to_parent_eval(extra: String) {
    // let the parent shell to take over
    println!(
        r#"
    echo "cd '$PWD'
    export OLDPWD='$OLDPWD'
    {}
    " > $ENVRC_TMP
    echo "envrc: exit [$ENVRC_LOAD]"
    exit 0
        "#,
        extra
    );
}

fn is_out_of_scope(rc: &String) -> bool {
    let dir = current_dir();
    if dir.is_err() {
        return true;
    }
    let dir = dir.unwrap();

    let rc_path = PathBuf::from(rc);
    let rc_dir = rc_path.parent().unwrap();

    if dir.strip_prefix(rc_dir).is_ok() {
        return false;
    }

    return true;
}

fn add_allow(rc: &String) {
    let now = SystemTime::now();
    let now = now.duration_since(UNIX_EPOCH).unwrap().as_secs();

    let dir = get_config_dir();
    let _ = create_dir_all(dir.clone());

    let mut allow_list = dir;
    allow_list.push("allow.list");

    let list = load_allow_list();

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(allow_list.to_str().unwrap())
        .unwrap();

    for (name, ts) in &list {
        if name == rc {
            continue;
        }
        file.write_fmt(format_args!("{} {}\n", name, ts)).unwrap();
    }

    file.write_fmt(format_args!("{} {}\n", rc, now)).unwrap();
}

fn remove_allow(rc: &String) {
    let dir = get_config_dir();
    let _ = create_dir_all(dir.clone());

    let mut allow_list = dir;
    allow_list.push("allow.list");

    let list = load_allow_list();

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(allow_list.to_str().unwrap())
        .unwrap();

    for (name, ts) in &list {
        if name == rc {
            continue;
        }
        file.write_fmt(format_args!("{} {}\n", name, ts)).unwrap();
    }
}

fn prune_allow() {
    let now = timestamp();
    let duration = get_allow_duration();

    let dir = get_config_dir();
    let _ = create_dir_all(dir.clone());

    let mut allow_list = dir;
    allow_list.push("allow.list");

    let list = load_allow_list();

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(allow_list.to_str().unwrap())
        .unwrap();

    for (name, ts) in &list {
        if now >= ts + duration {
            println!("envrc: filter expired [{}]", name);
            continue;
        }
        let path = PathBuf::from(name);
        if path.is_file() == false && path.is_dir() == false {
            println!("envrc: filter non-existing [{}]", name);
            continue;
        }
        file.write_fmt(format_args!("{} {}\n", name, ts)).unwrap();
    }
}

fn timestamp() -> u64 {
    let now = SystemTime::now();
    now.duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn update_if_allowed(rc: &String) {
    let now = timestamp();
    // let duration = get_allow_duration();

    let dir = get_config_dir();
    let _ = create_dir_all(dir.clone());

    let mut allow_list = dir;
    allow_list.push("allow.list");

    let list = load_allow_list();
    let mut allowed = false;

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(allow_list.to_str().unwrap())
        .unwrap();

    for (name, ts) in &list {
        if name == rc {
            allowed = true;
            continue;
        }
        file.write_fmt(format_args!("{} {}\n", name, ts)).unwrap();
    }

    if allowed {
        file.write_fmt(format_args!("{} {}\n", rc, now)).unwrap();
    }
}

fn get_config_dir() -> PathBuf {
    let home = var("HOME").unwrap();
    let mut dir = PathBuf::from(home);

    let dirs = vec![".config", "envrc"];

    for (_, e) in dirs.iter().enumerate() {
        dir.push(e);
    }
    dir
}

enum AllowError {
    AllowDenied,
    AllowExpired,
}

fn get_allow_duration() -> u64 {
    match var("ENVRC_ALLOW_DURATION") {
        Ok(val) => val.parse::<u64>().unwrap(),
        Err(_) => 60 * 60 * 24,
    }
}

fn check_allow(rc: Option<&String>) -> Option<AllowError> {
    if rc.is_none() {
        return None;
    }
    let rc = rc.unwrap();

    let now = timestamp();

    let duration = get_allow_duration();

    let list = load_allow_list();

    for (name, ts) in &list {
        if name == rc {
            if now >= ts + duration {
                return Some(AllowError::AllowExpired);
            } else {
                return None;
            }
        }
    }

    return Some(AllowError::AllowDenied);
}

fn load_allow_list() -> Vec<(String, u64)> {
    let dir = get_config_dir();

    let mut allow_list = dir;
    allow_list.push("allow.list");

    let file = OpenOptions::new()
        .read(true)
        .open(allow_list.to_str().unwrap());
    if file.is_err() {
        return Vec::new();
    }
    let file = file.unwrap();

    let mut ret: Vec<(String, u64)> = Vec::new();

    for line in BufReader::new(file).lines() {
        let line = line.unwrap();
        let fields = line.split(" ");
        let fields = fields.collect::<Vec<&str>>();
        let mut ts = 0u64;
        let name = String::from(fields[0]);
        if fields.len() > 1 {
            let tmp = String::from(fields[1]);
            ts = tmp.parse::<u64>().unwrap();
        }
        ret.push((name, ts))
    }

    return ret;
}

fn current_envrc() -> Option<String> {
    let key = "ENVRC_LOAD";
    match var(key) {
        Ok(val) => Some(val),
        Err(_) => None,
    }
}

fn find_envrc(mut d: PathBuf) -> Option<String> {
    loop {
        let mut rc = d.clone();
        rc.push(".envrc");

        if rc.is_file() || rc.is_dir() {
            return match rc.into_os_string().into_string() {
                Ok(s) => Some(s),
                Err(_) => None,
            };
        }

        d = d.parent()?.to_path_buf();
    }
}
