extern crate clap;
extern crate mkstemp;

use clap::{App, SubCommand};
use std::env::{current_dir, current_exe, var};
use mkstemp::TempFile;
use std::io::{Write, BufReader, BufRead};
use std::path::{PathBuf};
use std::fs::{create_dir_all, OpenOptions};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let bash = SubCommand::with_name("bash")
        .about("for bashrc: PROMPT_COMMAND='eval \"$(envrc bash)\"'");

    let allow = SubCommand::with_name("allow")
        .about("Allow envrc to load the .envrc");

    let matches = App::new("envrc")
        .version("0.1")
        .author("Rox Ma roxma@qq.com")
        .about("auto source .envrc of your workspace")
        .subcommand(bash)
        .subcommand(allow)
        .get_matches();

    if let Some(_) = matches.subcommand_matches("bash") {
        do_bash();
    }
    else if let Some(_) = matches.subcommand_matches("allow") {
        let rc_found = find_envrc().unwrap();
        add_allow(&rc_found);
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
  unset ENVRC_LOADED
  unset ENVRC_PPID
  unset ENVRC_TMP
  unset ENVRC_NOT_ALLOWED
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

    if rc_found == rc_cur {
        let p = format!(r#"
if [ -n "$ENVRC_LOAD" -a -z "$ENVRC_LOADED" ]
then
    ENVRC_LOADED=1
    . "$ENVRC_LOAD"
fi
ENVRC_NOT_ALLOWED=
            "#);

        println!("{}", p);
        return
    }

    if rc_cur.is_some() && is_out_of_scope(rc_cur.unwrap()) {
        update_if_allowed(rc_cur.as_ref().unwrap());
        return back_to_parent()
    }

    if rc_found.is_some() {
        let allow_err = check_allow(rc_found.unwrap());

        if allow_err.is_some() {
            let allow_err = allow_err.unwrap();

            let allow_err = match allow_err {
                AllowError::AllowDenied => "NOT ALLOWED.",
                AllowError::AllowExpired => "PERMISSION EXPIRED."
            };

            // found an .envrc, but it's not allowed to be loaded
            let p = format!(r#"
if [ "$ENVRC_NOT_ALLOWED" != "{rc_found}" ]
then
    tput setaf 3
    tput bold
    echo "envrc: [{rc_found}] {allow_err}"
    echo '       try execute "envrc allow"'
    tput sgr0
    ENVRC_NOT_ALLOWED="{rc_found}"
fi
             "#,
             rc_found = rc_found.unwrap(),
             allow_err = allow_err);

            println!("{}", p);
            return
        }
    }

    if rc_cur.is_some() {
        // we're in an .envrc scope, but need to load another one
        return back_to_parent()
    }


    // we're in parent shell, ENVRC_LOAD is empty
    // now we're going to load rc_found
    let rc_found = rc_found.unwrap();

    let mut tmp_file = TempFile::new("/tmp/envrc_XXXXXX", false).unwrap();
    tmp_file.write("exit 0".as_bytes()).unwrap();

    let p = format!(r#"
echo "envrc: spwan for [{rc_found}]"
ENVRC_TMP="{tmp_name}" ENVRC_LOAD="{rc_found}" ENVRC_PPID=$$ $BASH
eval "$(cat {tmp_name}; rm {tmp_name})"
eval "$({exe} bash)" 
        "#,
        rc_found = rc_found,
        exe = exe,
        tmp_name = String::from(tmp_file.path()));

    println!("{}", p);
}

fn back_to_parent() {
    // let the parent shell to take over
    println!(r#"
    echo "cd '$PWD'
    export OLDPWD='$OLDPWD'" > $ENVRC_TMP
    echo "envrc: exit [$ENVRC_LOAD]"
    exit 0
        "#);
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
        eprintln!("not out of scope {:?} {:?}", rc_path, dir);
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


fn update_if_allowed(rc: &String) {
    let now = SystemTime::now();
    let now = now.duration_since(UNIX_EPOCH).unwrap().as_secs();

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

fn check_allow(rc: &String) -> Option<AllowError> {
    let now = SystemTime::now();
    let now = now.duration_since(UNIX_EPOCH).unwrap().as_secs();

    let duration = match var("ENVRC_ALLOW_DURATION") {
        Ok(val) => val.parse::<u64>().unwrap(),
        Err(_) => 60 * 60 * 24
    };

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

        if d.parent().is_none() {
            return None
        }

        d = d.parent().unwrap().to_path_buf();
    }
}
