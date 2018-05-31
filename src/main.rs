extern crate clap;
extern crate mkstemp;

use clap::{App, SubCommand};
use std::env::{current_dir, current_exe, var};
use mkstemp::TempFile;
use std::io::Write;

fn main() {
    let bash = SubCommand::with_name("bash")
        .about("for bashrc: PROMPT_COMMAND='eval \"$(envrc bash)\"'");

    let matches = App::new("envrc")
        .version("0.1")
        .author("Rox Ma roxma@qq.com")
        .about("auto source .envrc of your workspace")
        .subcommand(bash)
        .get_matches();

    if let Some(_) = matches.subcommand_matches("bash") {
        do_bash();
    }
}

fn do_bash() {
    let found_rc = find_envrc();
    let cur_rc = current_envrc();
    let exe = current_exe().unwrap().into_os_string().into_string().unwrap();

    if found_rc == cur_rc {
        let p = format!(r#"
        if [ -n "$ENVRC_LOAD" -a -z "$ENVRC_LOADED" -a "$ENVRC_PPID" == "$PPID" ]
        then
            ENVRC_LOADED=1
            . "$ENVRC_LOAD"
        elif [ -n "$ENVRC_LOAD" -a "$ENVRC_PPID" != "$PPID" ]
        then
            unset ENVRC_LOAD
            unset ENVRC_LOADED
            unset ENVRC_PPID
            unset ENVRC_TMP
            unset ENVRC_DIR
            eval "$({exe} bash)"
        fi
            "#, exe = exe);

        println!("{}", p);
        return
    }

    if cur_rc == "" {
        let mut tmp_file = TempFile::new("/tmp/envrc_XXXXXX", false).unwrap();
        tmp_file.write("exit 0".as_bytes()).unwrap();

        let p = format!(r#"
echo "envrc: spwan for [{found_rc}]"
ENVRC_TMP="{tmp_name}" ENVRC_LOAD="{found_rc}" ENVRC_PPID=$$ ENVRC_DIR="$PWD" $BASH
eval "$(cat {tmp_name}; rm {tmp_name})"
eval "$({exe} bash)" "#,
            found_rc = found_rc,
            exe = exe,
            tmp_name = String::from(tmp_file.path()));

        println!("{}", p);
    } else {
        // let the parent shell to take over
        println!(r#"
echo "cd '$PWD'
export OLDPWD='$OLDPWD'" > $ENVRC_TMP
echo "envrc: exit [$ENVRC_LOAD]"
exit 0
        "#)
    }
}

fn current_envrc() -> String {
    let key = "ENVRC_LOAD";
    match var(key) {
        Ok(val) => val,
        Err(_) => String::new()
    }
}

fn find_envrc() -> String {
    let d = current_dir();
    if d.is_err() {
        return String::new()
    }

    let mut d = d.unwrap();

    loop {
        let mut rc = d.clone();
        rc.push(".envrc");

        if rc.is_file() {
            return match rc.into_os_string().into_string() {
                Ok(s) => s,
                Err(_) => String::new()
            }
        }

        if d.parent().is_none() {
            return String::new()
        }

        d = d.parent().unwrap().to_path_buf();
    }
}
