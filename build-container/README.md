# Build Container

Docker container that builds a package

## Environment variables

| Name           | Description                          |
|----------------|--------------------------------------|
| AB_GITEA_USER  | Gitea User Name                      |
| AB_GITEA_TOKEN | Gitea Token                          |
| AB_GITEA_REPO  | Repo URL to push the package file to |
| AB_SOURCE      | Git URL to the source of the package |

## Exit codes

| Code | Description                  |
|------|------------------------------|
| 100  | Unable to change dir         |
| 101  | Environment Variable missing |
| 102  | Git clone failed             |
| 103  | Failed to run `yay -Syu`     |
| 104  | Failed to install dependency |
| 105  | Failed to build package      |
| 106  | Failed to copy result files  |
| 107  | Failed to upload pkg file    |
