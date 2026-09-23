#!/usr/bin/env bash

# The GUI runtime keeps the login home even when SSH redirects the desktop home.
# The runner links the login home's state root to the spec home's, so the
# launchd runtime, the desktop and the agent harness read one directory.
save_macos_spec_config() {
  [[ ${macos_config_saved:-0} != 1 && $HOME == /* ]] || return 1
  macos_login_state="$HOME/.muniment"
  saved_macos_config=$(mktemp -d "$HOME/.muniment-wdio-state.XXXXXX") || return 1
  macos_config_saved=1
  macos_spec_config_target=
  macos_default_home="$HOME/Documents/muniment"
  macos_default_home_saved=0
  macos_default_home_sequence=0
  if [[ -e $macos_login_state || -L $macos_login_state ]]; then
    if ! mv -- "$macos_login_state" "$saved_macos_config/original"; then
      macos_config_saved=0
      rmdir "$saved_macos_config"
      return 1
    fi
  fi
  if [[ -e $macos_default_home || -L $macos_default_home ]]; then
    mv -- "$macos_default_home" "$saved_macos_config/original-home" || return 1
  fi
  macos_default_home_saved=1
}

archive_macos_spec_home() {
  [[ ${macos_default_home_saved:-0} == 1 ]] || return 0
  if [[ -e $macos_default_home || -L $macos_default_home ]]; then
    # Keep test output inside the owned spec root until final diagnostics finish.
    [[ -n $macos_spec_config_target && -d $macos_spec_config_target ]] || return 1
    macos_default_home_sequence=$((macos_default_home_sequence + 1))
    local archive="$macos_spec_config_target/default-home-$macos_default_home_sequence"
    [[ ! -e $archive && ! -L $archive ]] || return 1
    mv -- "$macos_default_home" "$archive" || return 1
  fi
}

remove_macos_spec_config_link() {
  if [[ -e $macos_login_state || -L $macos_login_state ]]; then
    # Refuse to remove a path that another process created or replaced.
    [[ -L $macos_login_state && -n $macos_spec_config_target ]] || return 1
    [[ $(readlink "$macos_login_state") == "$macos_spec_config_target" ]] || return 1
    rm -- "$macos_login_state" || return 1
  fi
}

set_macos_spec_config() {
  [[ ${macos_config_saved:-0} == 1 && $HOME == /* ]] || return 1
  local target="$HOME/.muniment"
  [[ $target != "$macos_login_state" && $target != "$macos_login_state/"* ]] || return 1
  mkdir -p "$target" || return 1
  chmod 700 "$target" || return 1
  # The runner stops the runtime before it changes this link.
  archive_macos_spec_home || return 1
  remove_macos_spec_config_link || return 1
  macos_spec_config_target=$target
  ln -s "$target" "$macos_login_state"
}

restore_macos_spec_config() {
  [[ ${macos_config_saved:-0} == 1 ]] || return 0
  archive_macos_spec_home || return 1
  remove_macos_spec_config_link || return 1
  if [[ -e $saved_macos_config/original-home || -L $saved_macos_config/original-home ]]; then
    [[ ! -e $macos_default_home && ! -L $macos_default_home ]] || return 1
    mv -- "$saved_macos_config/original-home" "$macos_default_home" || return 1
  fi
  if [[ -e $saved_macos_config/original || -L $saved_macos_config/original ]]; then
    mv -- "$saved_macos_config/original" "$macos_login_state" || return 1
  fi
  macos_config_saved=0
  rmdir "$saved_macos_config"
}
