export GIT_COMMIT_HASH := `git rev-parse --short=12 HEAD`
export BUILDX_BAKE_ENTITLEMENTS_FS := "0"

[private]
default: help

build:
    docker buildx bake --push

build-stable:
    TAG=stable docker buildx bake --push

clean:
    docker builder prune --filter type=exec.cachemount

doctor:
  #!/usr/bin/env bash
    command -v docker >/dev/null 2>&1 && printf "docker = \033[1;32myes\033[0m\n" || printf "docker = \033[1;31mno\033[0m\n"
    command -v docker >/dev/null 2>&1 && printf "cargo = \033[1;32myes\033[0m\n" || printf "cargo = \033[1;31mno\033[0m\n"

help:
    @just --list
