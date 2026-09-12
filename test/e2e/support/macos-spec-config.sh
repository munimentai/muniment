#!/usr/bin/env bash

# launchd does not inherit the spec environment from the runner.
save_macos_spec_config() {
  local launchctl_command=${MUNIMENT_E2E_LAUNCHCTL:-/bin/launchctl}
  saved_macos_config=$("$launchctl_command" getenv XDG_CONFIG_HOME) || saved_macos_config=
  macos_config_saved=1
}

set_macos_spec_config() {
  local launchctl_command=${MUNIMENT_E2E_LAUNCHCTL:-/bin/launchctl}
  [[ $HOME == /* ]] || return 1
  export XDG_CONFIG_HOME="$HOME/Library/Application Support"
  "$launchctl_command" setenv XDG_CONFIG_HOME "$XDG_CONFIG_HOME"
}

restore_macos_spec_config() {
  local launchctl_command=${MUNIMENT_E2E_LAUNCHCTL:-/bin/launchctl}
  [[ ${macos_config_saved:-0} == 1 ]] || return 0
  if [[ -n $saved_macos_config ]]; then
    "$launchctl_command" setenv XDG_CONFIG_HOME "$saved_macos_config"
  else
    "$launchctl_command" unsetenv XDG_CONFIG_HOME
  fi
}
