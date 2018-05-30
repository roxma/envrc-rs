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

    if found_rc == cur_rc {
        let p = r#"
        if [ -n "$ENVRC_LOAD" -a -z "$ENVRC_LOADED" ]
        then
            ENVRC_LOADED=1
            echo loading "$ENVRC_LOAD"
            . "$ENVRC_LOAD"
        fi
            "#;

        println!("{}", p);
        return
    }

    match cur_rc {
        None => {
            let found_rc = found_rc.unwrap();
            let exec = current_exe().unwrap().into_os_string().into_string().unwrap();

            let mut tmp_file = TempFile::new("/tmp/envrc_XXXXXX", false).unwrap();
            tmp_file.write("exit 0".as_bytes()).unwrap();

            let p = format!(r#"
ENVRC_TMP="{tmp_name}" ENVRC_LOAD="{found_rc}" $BASH
eval "$(cat {tmp_name}; rm {tmp_name})"
eval "$({envrc_path} bash)" "#,
                found_rc = found_rc,
                envrc_path = exec,
                tmp_name = String::from(tmp_file.path()));

            println!("{}", p);
        },
        Some(_) => {
            // let the parent shell to take over
            println!(r#"
echo "cd '$PWD'
export OLDPWD='$OLDPWD'" > $ENVRC_TMP
echo "unload $ENVRC_LOAD"
exit 0
            "#)
        }
    }
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
            return None;
        }

        d = d.parent().unwrap().to_path_buf();
    }
}
