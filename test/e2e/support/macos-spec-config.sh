#!/usr/bin/env bash

# The GUI runtime keeps the login home even when SSH redirects the desktop home.
# The runner links the login home paths the runtime and Pi read to the spec home.
save_macos_spec_config() {
  [[ ${macos_config_saved:-0} != 1 && $HOME == /* ]] || return 1
  local config_root="$HOME/Library/Application Support"
  macos_runtime_config="$config_root/ai.muniment.desktop"
  mkdir -p "$config_root" || return 1
  saved_macos_config=$(mktemp -d "$config_root/.muniment-wdio-config.XXXXXX") || return 1
  macos_config_saved=1
  macos_spec_config_target=
  if [[ -e $macos_runtime_config || -L $macos_runtime_config ]]; then
    if ! mv -- "$macos_runtime_config" "$saved_macos_config/original"; then
      macos_config_saved=0
      rmdir "$saved_macos_config"
      return 1
    fi
  fi
  # Pi reads its agent directory under the runtime's login home too.
  macos_login_pi_agent="$HOME/.pi/agent"
  macos_spec_pi_target=
  if [[ -e $macos_login_pi_agent || -L $macos_login_pi_agent ]]; then
    if ! mv -- "$macos_login_pi_agent" "$saved_macos_config/pi-agent"; then
      restore_macos_spec_config
      return 1
    fi
  fi
}

remove_macos_spec_pi_link() {
  [[ -n ${macos_login_pi_agent:-} ]] || return 0
  if [[ -e $macos_login_pi_agent || -L $macos_login_pi_agent ]]; then
    [[ -L $macos_login_pi_agent && -n ${macos_spec_pi_target:-} ]] || return 1
    [[ $(readlink "$macos_login_pi_agent") == "$macos_spec_pi_target" ]] || return 1
    rm -- "$macos_login_pi_agent" || return 1
  fi
}

remove_macos_spec_config_link() {
  if [[ -e $macos_runtime_config || -L $macos_runtime_config ]]; then
    # Refuse to remove a path that another process created or replaced.
    [[ -L $macos_runtime_config && -n $macos_spec_config_target ]] || return 1
    [[ $(readlink "$macos_runtime_config") == "$macos_spec_config_target" ]] || return 1
    rm -- "$macos_runtime_config" || return 1
  fi
}

set_macos_spec_config() {
  [[ ${macos_config_saved:-0} == 1 && $HOME == /* ]] || return 1
  export XDG_CONFIG_HOME="$HOME/Library/Application Support"
  local target="$XDG_CONFIG_HOME/ai.muniment.desktop"
  [[ $target != "$macos_runtime_config" && $target != "$macos_runtime_config/"* ]] || return 1
  mkdir -p "$target" || return 1
  # The runner stops the runtime before it changes this link.
  remove_macos_spec_config_link || return 1
  macos_spec_config_target=$target
  ln -s "$target" "$macos_runtime_config" || return 1
  local pi_target="$HOME/.pi/agent"
  [[ $pi_target != "$macos_login_pi_agent" && $pi_target != "$macos_login_pi_agent/"* ]] || return 1
  mkdir -p "$pi_target" "${macos_login_pi_agent%/*}" || return 1
  remove_macos_spec_pi_link || return 1
  macos_spec_pi_target=$pi_target
  ln -s "$pi_target" "$macos_login_pi_agent"
}

restore_macos_spec_config() {
  [[ ${macos_config_saved:-0} == 1 ]] || return 0
  remove_macos_spec_config_link || return 1
  if [[ -e $saved_macos_config/original || -L $saved_macos_config/original ]]; then
    mv -- "$saved_macos_config/original" "$macos_runtime_config" || return 1
  fi
  remove_macos_spec_pi_link || return 1
  if [[ -n ${macos_login_pi_agent:-} && ( -e $saved_macos_config/pi-agent || -L $saved_macos_config/pi-agent ) ]]; then
    mv -- "$saved_macos_config/pi-agent" "$macos_login_pi_agent" || return 1
  fi
  macos_config_saved=0
  rmdir "$saved_macos_config"
}
