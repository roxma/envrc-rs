# envrc -- Auto source .envrc of your workspace

## Motivation

Firstly, [direnv](https://github.com/direnv/direnv) doesn't officially support
alias at the moment.

Secondly, according to direnv documentation, 

> direnv is actually creating a new bash process to load the stdlib, direnvrc
> and .envrc, and only exports the environment diff back to the original shell

On the contrast, envrc is simpler. It starts a new interactive bash shell and
load the `.envrc` for you. The shell exits and returns your terminal back to
the parent shell after you `cd` out of the directory.

## Usage

- Build the program from source with `cargo build`
- Copy the executable `envrc` into your `$PATH`
- Add `PROMPT_COMMAND='eval "$(envrc bash)"'` to the end of your bashrc

Note: Take care of your background jobs before getting out of `.envrc`.

```
$ mkdir foo
$ echo 'echo in foo directory' > foo/.envrc
$ cd foo
  envrc: spawning new /bin/bash
  envrc: loading [/home/roxma/test/envrc/foo/.envrc]
  in foo directory
$ cd ..
  envrc: exit [/home/roxma/test/envrc/foo/.envrc]
$
```

```
$ envrc
  envrc 0.2
  Rox Ma roxma@qq.com
  auto source .envrc of your workspace

  USAGE:
      envrc [SUBCOMMAND]

  FLAGS:
      -h, --help       Prints help information
      -V, --version    Prints version information

  SUBCOMMANDS:
      allow    Grant permission to envrc to load the .envrc
      bash     for bashrc: PROMPT_COMMAND='eval "$(envrc bash)"'
      deny     Remove the permission
      help     Prints this message or the help of the given subcommand(s)
      prune    Remove expired or non-existing-file permissions
```

## Config

```bash
# If the `.envrc` is allowed, but not sourced for 1d since last unload, It
# will be considered expired
export ENVRC_ALLOW_DURATION=$((60*60*24))
```

## Why not bash/python?

The first working commit is written in python. But there's noticeable time lag
with the python version on my PC. Rewriting it with perl doesn't help either.
Then I decided to switch to rust.

```
$ time envrc.py bash-prompt-command >/dev/null
real    0m0.079s
user    0m0.044s
sys     0m0.004s
```

I have also tried a pure bash implementation. It works better than the python
implementation, since most of the python overhead is its startup time.  Most
of the bash overhead is fork/exec of sub-processes and it's way slower than
the rust implementation. Read [#1](https://github.com/roxma/envrc-rs/issues/1)
for more information.

