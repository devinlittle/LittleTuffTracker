variable "REGISTRY" { default = "git.home.devinlittle.net/devin" }
variable "TAG" { default = "dev" }
variable "GIT_COMMIT_HASH" {
  default = "unknown"
}

group "default" {
    targets = ["LittleTuffTracker"]
}

target "settings" {
    context = "."
    output = ["type=registry"]
    args = {
      GIT_COMMIT_HASH = GIT_COMMIT_HASH
    }
}

target "LittleTuffTracker" {
    inherits = ["settings"]
    dockerfile = "./Dockerfile"
    tags = ["${REGISTRY}/littletufftracker:${TAG}"]
}
