# Homebrew

Muniment targets the official `homebrew/cask` catalog. The install command is
`brew install --cask muniment` once Homebrew accepts the cask. Until acceptance,
use the [install guide](https://muniment.ai/docs/install/) for direct downloads.
Do not advertise the Homebrew command as available before the cask is merged.

## Release requirements

The cask uses the public stable macOS archive from `munimentai/muniment`.
It pins the version and SHA-256 checksum. The app must pass Gatekeeper with
its Developer ID signature and notarization intact. Installation must not
remove quarantine attributes or require a security bypass.

Validate the downloaded archive, test installation and removal, and run
Homebrew's cask audit before submitting a pull request to `Homebrew/homebrew-cask`.
The cask must work on every architecture it declares.

Homebrew reviews public interest, maintenance and security as well as packaging.
Owner submissions normally require 225 stars, 90 forks or 90 watchers.
A maintainer can consider a documented exception. Submission does not guarantee acceptance.

See the [cask requirements](https://docs.brew.sh/Acceptable-Casks),
[package acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy), and
[submission guide](https://docs.brew.sh/How-To-Open-a-Homebrew-Pull-Request).
