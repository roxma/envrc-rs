extern crate clap;

use clap::{App, AppSettings, SubCommand, Arg};
use std::env::{current_dir, current_exe, var};
use std::io::{Write, BufReader, BufRead};
use std::path::{PathBuf};
use std::fs::{create_dir_all, OpenOptions, canonicalize};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let bash = SubCommand::with_name("bash")
        .about("for bashrc: PROMPT_COMMAND='eval \"$(envrc bash)\"'");

    let allow = SubCommand::with_name("allow")
        .about("Grant permission to envrc to load the .envrc");

    let deny = SubCommand::with_name("deny")
        .arg(Arg::with_name("envrc-file")
             .required(false)
             .help(".envrc files to be denied"))
        .about("Remove the permission");

    let prune = SubCommand::with_name("prune")
        .about("Remove expired or non-existing-file permissions");

    let matches = App::new("envrc")
        .version("0.2")
        .author("Rox Ma roxma@qq.com")
        .setting(AppSettings::ArgRequiredElseHelp)
        .about("auto source .envrc of your workspace")
        .subcommand(bash)
        .subcommand(allow)
        .subcommand(deny)
        .subcommand(prune)
        .get_matches();

    if let Some(_) = matches.subcommand_matches("bash") {
        do_bash();
    }
    else if let Some(_) = matches.subcommand_matches("allow") {
        let rc_found = find_envrc().unwrap();
        add_allow(&rc_found);
    }
    else if let Some(matches) = matches.subcommand_matches("deny") {
        if let Some(file) = matches.value_of("envrc-file") {
            let mut path = canonicalize(file).unwrap();
            if path.is_dir() {
                path.push(".envrc");
            }
            let path = String::from(path.to_str().unwrap());
            remove_allow(&path);
            println!("{} is denied", path);
        } else {
            let rc_found = find_envrc().unwrap();
            remove_allow(&rc_found);
            println!("{} is denied", rc_found);
        }
    }
    else if let Some(_) = matches.subcommand_matches("prune") {
        prune_allow();
    }
}

fn do_bash() {
    let exe = current_exe()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap();

    let begin = format!(r#"
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
  eval "$({exe} bash)"
  break
 fi
"#, exe=exe);
    println!("{}", begin);

    do_bash_wrapped();

    let end = format!(r#"
break
done
}}"#);
    println!("{}", end);
}

fn do_bash_wrapped() {
    let rc_cur = current_envrc();
    let rc_found = find_envrc();

    let rc_found = rc_found.as_ref();
    let rc_cur = rc_cur.as_ref();

    let exe = current_exe().unwrap().into_os_string().into_string().unwrap();

    if rc_cur.is_some() {
        let rc_cur = rc_cur.unwrap();

        update_if_allowed(rc_cur);

        if is_out_of_scope(rc_cur) {
            return bash_to_parent()
        }
    }

    let allow_err = check_allow(rc_found);

    if rc_found == rc_cur {
        if allow_err.is_some() {
            return bash_to_parent_eval(format!(r#"
                    envrc_not_allowed={}
                    "#, rc_cur.unwrap()))
        }

        let p = format!(r#"
if [ -n "$ENVRC_LOAD" -a -z "$envrc_loaded" ]
then
    envrc_loaded=1
    echo "envrc: loading [$ENVRC_LOAD]"
    . "$ENVRC_LOAD"
fi
envrc_not_allowed=
            "#);

        println!("{}", p);
        return
    }

    if allow_err.is_some() {
        let allow_err = match allow_err.unwrap() {
            AllowError::AllowDenied => "NOT ALLOWED.",
            AllowError::AllowExpired => "PERMISSION EXPIRED."
        };

        // found an .envrc, but it's not allowed to be loaded
        let p = format!(r#"
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
             allow_err = allow_err);

        println!("{}", p);
        return
    }

    if rc_cur.is_some() {
        // we're in an .envrc scope, but need to load another one
        return bash_to_parent()
    }

    // we're in parent shell, ENVRC_LOAD is empty
    // now we're going to load rc_found
    let rc_found = rc_found.unwrap();

    let p = format!(r#"
echo "envrc: spwan $BASH"
export ENVRC_TMP="$(mktemp "${{TMPDIR-/tmp}}/envrc.XXXXXXXXXX")"
ENVRC_LOAD="{rc_found}" ENVRC_PPID=$$ $BASH
eval "$(if [ -s $ENVRC_TMP ]; then cat $ENVRC_TMP; else echo exit 0; fi; rm $ENVRC_TMP)"
unset ENVRC_TMP
eval "$({exe} bash)" 
        "#,
        rc_found = rc_found,
        exe = exe);

    println!("{}", p);
}

fn bash_to_parent() {
    bash_to_parent_eval(String::new())
}

fn bash_to_parent_eval(extra: String) {
    // let the parent shell to take over
    println!(r#"
    echo "cd '$PWD'
    export OLDPWD='$OLDPWD'
    {}
    " > $ENVRC_TMP
    echo "envrc: exit [$ENVRC_LOAD]"
    exit 0
        "#, extra);
}

fn is_out_of_scope(rc: &String) -> bool {
    let dir = current_dir();
    if dir.is_err() {
        return true
    }
    let dir = dir.unwrap();

    let rc_path = PathBuf::from(rc);
    let rc_dir = rc_path.parent().unwrap();

    if dir.strip_prefix(rc_dir).is_ok() {
        return false
    }

    return true
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
        if path.is_file() == false {
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

    let dirs = vec![".cache", "envrc"];

    for (_, e) in dirs.iter().enumerate() {
        dir.push(e);
    }
    dir
}

enum AllowError {
    AllowDenied,
    AllowExpired
}

fn get_allow_duration() -> u64 {
    match var("ENVRC_ALLOW_DURATION") {
        Ok(val) => val.parse::<u64>().unwrap(),
        Err(_) => 60 * 60 * 24
    }
}

fn check_allow(rc: Option<&String>) -> Option<AllowError> {
    if rc.is_none() {
        return None
    }
    let rc = rc.unwrap();

    let now = timestamp();

    let duration = get_allow_duration();

    let list = load_allow_list();

    for (name, ts) in &list {
        if name == rc {
            if now >= ts + duration {
                return Some(AllowError::AllowExpired)
            } else {
                return None
            }
        }
    }

    return Some(AllowError::AllowDenied)
}

fn load_allow_list() -> Vec<(String, u64)> {
    let dir = get_config_dir();

    let mut allow_list = dir;
    allow_list.push("allow.list");

    let file = OpenOptions::new()
                    .read(true)
                    .open(allow_list.to_str().unwrap());
    if file.is_err() {
        return Vec::new()
    }
    let file = file.unwrap();

    let mut ret :Vec<(String, u64)> = Vec::new();

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

    return ret
}

fn current_envrc() -> Option<String> {
    let key = "ENVRC_LOAD";
    match var(key) {
        Ok(val) => Some(val),
        Err(_) => None
    }
}

fn find_envrc() -> Option<String> {
    let d = current_dir();
    if d.is_err() {
        return None
    }

    let mut d = d.unwrap();

    loop {
        let mut rc = d.clone();
        rc.push(".envrc");

        if rc.is_file() {
            return match rc.into_os_string().into_string() {
                Ok(s) => Some(s),
                Err(_) => None
            }
        }

        d = d.parent()?.to_path_buf();
    }
}
