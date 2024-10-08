#!/bin/sh

check_dependency() {
  if ! [ -x "$(command -v $1)" ]; then
    echo "Error: $1 is not installed." >&2
    exit 1
  fi
}

check_dependency "ruby"
check_dependency "bundle"

bundle install